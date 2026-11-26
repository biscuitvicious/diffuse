//! 2D coordinate-aware multi-line token stream and slot-aware Gotoh LCS.
//!
//! Aligns slices of old and new lines (`&[&str]`) directly without synthetic
//! `\n` concatenation, preserving per-token `(line, byte_range, is_leading_ws)`
//! provenance and incorporating window slot affinity (`WindowSlotMap`) directly
//! inside Gotoh scoring.

use std::ops::Range;

use diffuse_algo::UniversalDelimiterClassifier;
use diffuse_syntax::profile::LangProfile;
use rustc_hash::FxHashMap;

use crate::token::ContainerKind;

use super::{
    arena::token_product_cap,
    raw::{SlotAffinity, raw_lcs_pairs_with_slots_context},
    tokenizer::{
        are_contiguous_subwords, is_subword_token, token_is_ws, tokenize_line, unique_token_anchors,
    },
};

/// Bonus awarded to non-whitespace matches staying inside the same peer-group
/// slot, sized below identifier match rewards so genuine cross-slot moves win
/// while repeated delimiters and tokens stay anchored to their own `Mod` row.
pub const SAME_SLOT_AFFINITY_BONUS: i32 = 6;

/// Default maximum slot distance across which a 2D cross-slot match may reach
/// (matches `CROSS_SLOT_WINDOW` in `collapse::slot`).
pub const DEFAULT_MAX_SLOT_DISTANCE: usize = 4;

/// One token in a multi-line slice with exact 2D line and byte provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token2D<'a> {
    pub line: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: &'a str,
    pub is_leading_ws: bool,
}

/// Flattened 2D token stream for a slice of lines alongside each line's token
/// index range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiLineTokens<'a> {
    pub tokens: Vec<Token2D<'a>>,
    pub line_ranges: Vec<Range<usize>>,
}

/// Slot ownership for every old and new line in a segmented window.
#[derive(Clone, Copy, Debug)]
pub struct WindowSlotMap<'a> {
    pub slot_of_old: &'a [usize],
    pub slot_of_new: &'a [usize],
}

/// Output of a 2D multi-line alignment: both tokenized line streams plus the
/// monotone list of matched `(old_tok_idx, new_tok_idx)` pairs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiLineAlignment<'a> {
    pub old: MultiLineTokens<'a>,
    pub new: MultiLineTokens<'a>,
    pub pairs: Vec<(usize, usize)>,
}

/// Tokenizes a slice of lines into a coordinate-aware 2D token stream without
/// allocating composite strings or inserting synthetic newline tokens.
pub fn tokenize_lines_2d<'a>(lines: &[&'a str]) -> MultiLineTokens<'a> {
    let mut tokens = Vec::new();
    let mut line_ranges = Vec::with_capacity(lines.len());

    for (line_idx, &line) in lines.iter().enumerate() {
        let start_tok = tokens.len();
        let base_ptr = line.as_ptr() as usize;
        let mut seen_non_ws = false;

        for tok in tokenize_line(line) {
            let is_ws = token_is_ws(tok);
            let is_leading_ws = !seen_non_ws && is_ws;
            if !is_ws {
                seen_non_ws = true;
            }
            let byte_start = (tok.as_ptr() as usize).saturating_sub(base_ptr);
            tokens.push(Token2D {
                line: line_idx,
                byte_start,
                byte_end: byte_start + tok.len(),
                text: tok,
                is_leading_ws,
            });
        }

        line_ranges.push(start_tok..tokens.len());
    }

    MultiLineTokens {
        tokens,
        line_ranges,
    }
}

/// Runs 2D coordinate-aware, slot-aware Gotoh LCS over `old_lines` and
/// `new_lines` using default slot-distance and token-product bounds.
///
/// Returns `None` if `old_tokens.len() * new_tokens.len()` exceeds
/// [`token_product_cap`], signaling the caller to fall back to per-slot
/// alignment.
pub fn align_lines_2d<'a>(
    old_lines: &[&'a str],
    new_lines: &[&'a str],
    slots: WindowSlotMap<'_>,
    profile: Option<&LangProfile>,
) -> Option<MultiLineAlignment<'a>> {
    align_lines_2d_with_cap(
        old_lines,
        new_lines,
        slots,
        profile,
        DEFAULT_MAX_SLOT_DISTANCE,
        token_product_cap(),
    )
}

/// Runs 2D coordinate-aware, slot-aware Gotoh LCS with explicit
/// `max_slot_distance` and token-product `cap`.
pub fn align_lines_2d_with_cap<'a>(
    old_lines: &[&'a str],
    new_lines: &[&'a str],
    slots: WindowSlotMap<'_>,
    profile: Option<&LangProfile>,
    max_slot_distance: usize,
    cap: usize,
) -> Option<MultiLineAlignment<'a>> {
    crate::require!(
        slots.slot_of_old.len() == old_lines.len(),
        "slot_of_old length ({}) must match old_lines length ({})",
        slots.slot_of_old.len(),
        old_lines.len()
    );
    crate::require!(
        slots.slot_of_new.len() == new_lines.len(),
        "slot_of_new length ({}) must match new_lines length ({})",
        slots.slot_of_new.len(),
        new_lines.len()
    );

    let old = tokenize_lines_2d(old_lines);
    let new = tokenize_lines_2d(new_lines);
    if old.tokens.is_empty() || new.tokens.is_empty() {
        return Some(MultiLineAlignment {
            old,
            new,
            pairs: Vec::new(),
        });
    }
    if old.tokens.len().saturating_mul(new.tokens.len()) > cap {
        return None;
    }

    let ot: Vec<&str> = old.tokens.iter().map(|t| t.text).collect();
    let nt: Vec<&str> = new.tokens.iter().map(|t| t.text).collect();
    let line_a: Vec<usize> = old.tokens.iter().map(|t| t.line).collect();
    let line_b: Vec<usize> = new.tokens.iter().map(|t| t.line).collect();

    let classifier = UniversalDelimiterClassifier;
    let c_old = diffuse_algo::track_enclosing_containers(&ot, &classifier);
    let c_new = diffuse_algo::track_enclosing_containers(&nt, &classifier);
    let d_old = diffuse_algo::track_nesting_depths_with_prev(&[], &ot, &classifier);
    let d_new = diffuse_algo::track_nesting_depths_with_prev(&[], &nt, &classifier);

    let anchors: Vec<(usize, usize)> = unique_token_anchors(&ot, &nt, profile)
        .into_iter()
        .filter(|&(ai, bi)| {
            let sa = slots.slot_of_old[line_a[ai]];
            let sb = slots.slot_of_new[line_b[bi]];
            let ca = c_old[ai].and_then(ContainerKind::from_id);
            let cb = c_new[bi].and_then(ContainerKind::from_id);
            sa.abs_diff(sb) <= max_slot_distance && ContainerKind::is_compatible_with(ca, cb)
        })
        .collect();

    let pair_map = solve_anchored_2d(
        &ot,
        &nt,
        &line_a,
        &line_b,
        &old.line_ranges,
        &new.line_ranges,
        slots,
        &anchors,
        max_slot_distance,
    );

    let mut pairs: Vec<(usize, usize)> = pair_map.into_iter().collect();
    pairs.sort_by_key(|&(oi, _)| oi);

    // Defense-in-depth: container invariance & line-bounded subword cohesion.
    pairs.retain(|&(oi, ni)| {
        let ca = c_old[oi].and_then(ContainerKind::from_id);
        let cb = c_new[ni].and_then(ContainerKind::from_id);
        if !ContainerKind::is_compatible_with(ca, cb) {
            return false;
        }
        if is_subword_token(ot[oi]) {
            let la = oi > 0
                && line_a[oi - 1] == line_a[oi]
                && is_subword_token(ot[oi - 1])
                && are_contiguous_subwords(ot[oi - 1], ot[oi]);
            let ra = oi + 1 < ot.len()
                && line_a[oi + 1] == line_a[oi]
                && is_subword_token(ot[oi + 1])
                && are_contiguous_subwords(ot[oi], ot[oi + 1]);
            let lb = ni > 0
                && line_b[ni - 1] == line_b[ni]
                && is_subword_token(nt[ni - 1])
                && are_contiguous_subwords(nt[ni - 1], nt[ni]);
            let rb = ni + 1 < nt.len()
                && line_b[ni + 1] == line_b[ni]
                && is_subword_token(nt[ni + 1])
                && are_contiguous_subwords(nt[ni], nt[ni + 1]);
            if (la != lb || ra != rb) && (d_old[oi] != d_new[ni] || ca != cb) {
                return false;
            }
        }
        true
    });

    Some(MultiLineAlignment { old, new, pairs })
}

#[allow(clippy::too_many_arguments)]
fn solve_anchored_2d(
    ot: &[&str],
    nt: &[&str],
    line_a: &[usize],
    line_b: &[usize],
    line_ranges_a: &[Range<usize>],
    line_ranges_b: &[Range<usize>],
    slots: WindowSlotMap<'_>,
    anchors: &[(usize, usize)],
    max_slot_distance: usize,
) -> FxHashMap<usize, usize> {
    let run_gap = |a_start: usize,
                   a_end: usize,
                   b_start: usize,
                   b_end: usize,
                   next_a: Option<&str>,
                   next_b: Option<&str>| {
        let affinity = SlotAffinity {
            full_a: ot,
            full_b: nt,
            offset_a: a_start,
            offset_b: b_start,
            line_a,
            line_b,
            line_ranges_a,
            line_ranges_b,
            slot_of_old: slots.slot_of_old,
            slot_of_new: slots.slot_of_new,
            max_slot_distance,
            same_slot_bonus: SAME_SLOT_AFFINITY_BONUS,
        };
        raw_lcs_pairs_with_slots_context(
            &ot[a_start..a_end],
            &nt[b_start..b_end],
            &ot[..a_start],
            &nt[..b_start],
            next_a,
            next_b,
            &affinity,
        )
    };

    if anchors.is_empty() {
        return run_gap(0, ot.len(), 0, nt.len(), None, None);
    }

    let mut pairs = FxHashMap::default();
    let mut a_prev = 0usize;
    let mut b_prev = 0usize;

    for (anchor_idx, &(ai, bi)) in anchors.iter().enumerate() {
        let mut back_a = ai;
        let mut back_b = bi;
        while back_a > a_prev
            && back_b > b_prev
            && line_a[back_a - 1] == line_a[back_a]
            && line_b[back_b - 1] == line_b[back_b]
            && is_subword_token(ot[back_a - 1])
            && is_subword_token(nt[back_b - 1])
            && ot[back_a - 1] == nt[back_b - 1]
        {
            back_a -= 1;
            back_b -= 1;
        }

        let (next_limit_a, next_limit_b) = anchors
            .get(anchor_idx + 1)
            .copied()
            .unwrap_or((ot.len(), nt.len()));

        let mut fwd_a = ai;
        let mut fwd_b = bi;
        while fwd_a + 1 < next_limit_a
            && fwd_b + 1 < next_limit_b
            && line_a[fwd_a + 1] == line_a[fwd_a]
            && line_b[fwd_b + 1] == line_b[fwd_b]
            && are_contiguous_subwords(ot[fwd_a], ot[fwd_a + 1])
            && are_contiguous_subwords(nt[fwd_b], nt[fwd_b + 1])
            && ot[fwd_a + 1] == nt[fwd_b + 1]
        {
            fwd_a += 1;
            fwd_b += 1;
        }

        if back_a > a_prev && back_b > b_prev {
            for (si, sj) in run_gap(
                a_prev,
                back_a,
                b_prev,
                back_b,
                Some(ot[back_a]),
                Some(nt[back_b]),
            ) {
                pairs.insert(a_prev + si, b_prev + sj);
            }
        }

        for (ka, kb) in (back_a..=fwd_a).zip(back_b..=fwd_b) {
            pairs.insert(ka, kb);
        }
        a_prev = fwd_a + 1;
        b_prev = fwd_b + 1;
    }

    if a_prev < ot.len() && b_prev < nt.len() {
        for (si, sj) in run_gap(a_prev, ot.len(), b_prev, nt.len(), None, None) {
            pairs.insert(a_prev + si, b_prev + sj);
        }
    }

    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_lines_2d_preserves_utf8_bytes_empty_lines_and_leading_ws() {
        let lines = ["    let café = 1;", "", "   ", "\tfoo\r"];
        let res = tokenize_lines_2d(&lines);
        assert_eq!(res.line_ranges.len(), 4);
        assert!(res.line_ranges[1].is_empty(), "empty line has empty range");

        for tok in &res.tokens {
            assert_eq!(
                &lines[tok.line][tok.byte_start..tok.byte_end],
                tok.text,
                "byte range must slice back to exact token text"
            );
        }

        // Line 0: leading indent is leading_ws, interior spaces are not.
        let l0 = &res.tokens[res.line_ranges[0].clone()];
        assert!(l0[0].is_leading_ws);
        assert_eq!(l0[0].text, "    ");
        assert!(l0[1..].iter().all(|t| !t.is_leading_ws));

        // Line 2: whitespace-only line has leading_ws.
        let l2 = &res.tokens[res.line_ranges[2].clone()];
        assert_eq!(l2.len(), 1);
        assert!(l2[0].is_leading_ws);

        // Line 3: leading tab is leading_ws, trailing \r is not.
        let l3 = &res.tokens[res.line_ranges[3].clone()];
        assert!(l3[0].is_leading_ws);
        assert_eq!(l3[0].text, "\t");
        assert!(!l3.last().unwrap().is_leading_ws);
    }

    #[test]
    fn align_lines_2d_handles_empty_inputs_and_product_cap() {
        let empty = align_lines_2d(
            &[],
            &["let x = 1;"],
            WindowSlotMap {
                slot_of_old: &[],
                slot_of_new: &[0],
            },
            None,
        )
        .expect("empty old succeeds");
        assert!(empty.pairs.is_empty());

        let blank_only = align_lines_2d(
            &[""],
            &[""],
            WindowSlotMap {
                slot_of_old: &[0],
                slot_of_new: &[0],
            },
            None,
        )
        .expect("empty lines succeed");
        assert!(blank_only.pairs.is_empty());

        let capped = align_lines_2d_with_cap(
            &["let alpha = compute(a, b);"],
            &["let beta = compute(a, c);"],
            WindowSlotMap {
                slot_of_old: &[0],
                slot_of_new: &[0],
            },
            None,
            DEFAULT_MAX_SLOT_DISTANCE,
            4,
        );
        assert!(capped.is_none(), "exceeding token-product cap returns None");
    }

    #[test]
    fn align_lines_2d_forbids_cross_slot_whitespace_and_respects_slot_affinity() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let old = ["    alpha(x, y);", "    beta(z, w);"];
        let new = ["    alpha(x, y2);", "    beta(z, w2);"];
        let aln = align_lines_2d(
            &old,
            &new,
            WindowSlotMap {
                slot_of_old: &[0, 1],
                slot_of_new: &[0, 1],
            },
            rs,
        )
        .expect("within cap");

        for &(oi, ni) in &aln.pairs {
            assert_eq!(
                aln.old.tokens[oi].line, aln.new.tokens[ni].line,
                "same-slot affinity keeps matched token {:?} on its own Mod slot",
                aln.old.tokens[oi].text
            );
        }

        // Unanchored tie case (no unique salient tokens): repeated lines stay
        // locked to their own slot rather than drifting across slots.
        let dup_old = ["  (,", "  (,"];
        let dup_new = ["  (,", "  (,"];
        let dup_aln = align_lines_2d(
            &dup_old,
            &dup_new,
            WindowSlotMap {
                slot_of_old: &[0, 1],
                slot_of_new: &[0, 1],
            },
            None,
        )
        .expect("within cap");
        assert!(!dup_aln.pairs.is_empty());
        for &(oi, ni) in &dup_aln.pairs {
            assert_eq!(dup_aln.old.tokens[oi].line, dup_aln.new.tokens[ni].line);
        }
    }

    #[test]
    fn align_lines_2d_never_matches_whitespace_across_different_slots() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let old = ["    assert_eq!(super::resolve_theme(Some(\"dark\")), DiffuseTheme::dark());"];
        let new = [
            "    assert_eq!(",
            "        super::resolve_theme(Some(\"dark\"), false),",
            "        DiffuseTheme::dark()",
            "    );",
        ];
        let slots = WindowSlotMap {
            slot_of_old: &[1],
            slot_of_new: &[0, 1, 2, 3],
        };
        let aln = align_lines_2d(&old, &new, slots, rs).expect("within cap");

        // Zero whitespace tokens matched across different slots.
        for &(oi, ni) in &aln.pairs {
            let ot = aln.old.tokens[oi];
            let nt = aln.new.tokens[ni];
            if token_is_ws(ot.text) {
                assert_eq!(
                    slots.slot_of_old[ot.line], slots.slot_of_new[nt.line],
                    "whitespace {:?} must never match across distinct slots",
                    ot.text
                );
            }
        }

        // Key tokens land on the exact expected lines across all 4 slots.
        let matched_by_line = |target_new_line: usize, needle: &str| {
            aln.pairs.iter().any(|&(oi, ni)| {
                aln.old.tokens[oi].text == needle && aln.new.tokens[ni].line == target_new_line
            })
        };
        assert!(matched_by_line(0, "assert"));
        assert!(matched_by_line(1, "resolve"));
        assert!(matched_by_line(1, "dark"));
        assert!(matched_by_line(2, "Diffuse"));
        assert!(matched_by_line(3, ";"));
    }

    #[test]
    fn align_lines_2d_respects_max_slot_distance_and_container_invariance() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // Unique salient token 6 slots apart must be rejected when cap is 4.
        let aln_far = align_lines_2d_with_cap(
            &["    unique_payload_ident();"],
            &["    unique_payload_ident();"],
            WindowSlotMap {
                slot_of_old: &[0],
                slot_of_new: &[6],
            },
            rs,
            4,
            token_product_cap(),
        )
        .expect("within cap");
        assert!(
            aln_far.pairs.is_empty(),
            "tokens beyond max_slot_distance must not match"
        );

        // Same token within max_slot_distance (3 <= 4) matches cleanly.
        let aln_near = align_lines_2d_with_cap(
            &["    unique_payload_ident();"],
            &["    unique_payload_ident();"],
            WindowSlotMap {
                slot_of_old: &[0],
                slot_of_new: &[3],
            },
            rs,
            4,
            token_product_cap(),
        )
        .expect("within cap");
        assert!(
            aln_near
                .pairs
                .iter()
                .any(|&(oi, _)| aln_near.old.tokens[oi].text == "payload"),
            "tokens within max_slot_distance must match"
        );

        // Incompatible enclosing containers `(...)` vs `{...}` reject matches.
        let aln_container = align_lines_2d(
            &["    call(unique_inner_sym);"],
            &["    block { unique_inner_sym }"],
            WindowSlotMap {
                slot_of_old: &[0],
                slot_of_new: &[0],
            },
            rs,
        )
        .expect("within cap");
        assert!(
            !aln_container
                .pairs
                .iter()
                .any(|&(oi, _)| aln_container.old.tokens[oi].text == "inner"),
            "tokens inside incompatible containers must not match"
        );
    }

    #[test]
    fn align_lines_2d_line_boundaries_block_cross_line_subword_fusion() {
        // `foo_bar` across two lines (`foo` at end of line 0, `bar` at start of
        // line 1) must not treat `foo` and `bar` as contiguous subwords.
        let aln = align_lines_2d(
            &["alpha", "beta"],
            &["alpha", "gamma", "beta"],
            WindowSlotMap {
                slot_of_old: &[0, 2],
                slot_of_new: &[0, 1, 2],
            },
            None,
        )
        .expect("within cap");
        assert_eq!(aln.pairs.len(), 2);
        assert_eq!(aln.old.tokens[aln.pairs[0].0].line, 0);
        assert_eq!(aln.new.tokens[aln.pairs[0].1].line, 0);
        assert_eq!(aln.old.tokens[aln.pairs[1].0].line, 1);
        assert_eq!(aln.new.tokens[aln.pairs[1].1].line, 2);
    }
}
