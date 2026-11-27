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
    pairs::{TokenContainerContext, expand_anchor_span, filter_container_and_subword_pairs},
    raw::{Side2D, SlotAffinity, raw_lcs_pairs_with_slots},
    tokenizer::{token_is_ws, tokenize_line, unique_token_anchors},
};

/// Bonus awarded to alphanumeric matches staying inside the same peer-group
/// slot. Sized above the maximum single-token match reward (`34` in
/// `lcs::raw`) so an adjacent `Del`/`Add` slot cannot steal a `Mod(i, j)`
/// slot's shared identifiers.
pub const SAME_SLOT_ALNUM_BONUS: i32 = 36;

/// Bonus awarded to non-alphanumeric (operator/delimiter) matches staying
/// inside the same peer-group slot — enough to anchor repeated punctuation
/// to its `Mod` partner without letting bare outer delimiters (`{`, `(`)
/// overpower a cross-slot identifier chain like `io::{`.
pub const SAME_SLOT_PUNCT_BONUS: i32 = 12;

/// Default maximum slot distance across which a cross-slot match or residual
/// relocation may travel within a multi-row `n × m` window.
pub const DEFAULT_MAX_SLOT_DISTANCE: usize = 4;

/// Scoring and search bounds for 2D coordinate-aware multi-line LCS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiLineConfig {
    pub max_slot_distance: usize,
    pub token_product_cap: usize,
    pub same_slot_alnum_bonus: i32,
    pub same_slot_punct_bonus: i32,
}

impl Default for MultiLineConfig {
    fn default() -> Self {
        Self {
            max_slot_distance: DEFAULT_MAX_SLOT_DISTANCE,
            token_product_cap: token_product_cap(),
            same_slot_alnum_bonus: SAME_SLOT_ALNUM_BONUS,
            same_slot_punct_bonus: SAME_SLOT_PUNCT_BONUS,
        }
    }
}

impl MultiLineConfig {
    /// Unbounded cross-slot reach for Dyck-bounded single-construct (`1 × m`
    /// and `n × 1`) windows whose closing delimiter may sit arbitrarily far
    /// from the single opposite-side row.
    pub fn single_construct() -> Self {
        Self {
            max_slot_distance: usize::MAX,
            ..Self::default()
        }
    }
}

/// One token in a multi-line slice with exact 2D line and byte provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token2D<'a> {
    pub line: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: &'a str,
    pub is_leading_ws: bool,
}

impl Token2D<'_> {
    /// Exact byte range of this token within its source line.
    #[inline]
    pub fn byte_range(&self) -> Range<usize> {
        self.byte_start..self.byte_end
    }
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
/// `new_lines` using `config`.
///
/// Returns `None` if `old_tokens.len() * new_tokens.len()` exceeds
/// `config.token_product_cap`, signaling the caller to fall back to per-slot
/// alignment.
pub fn align_lines_2d<'a>(
    old_lines: &[&'a str],
    new_lines: &[&'a str],
    slots: WindowSlotMap<'_>,
    profile: Option<&LangProfile>,
    config: &MultiLineConfig,
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
    if old.tokens.len().saturating_mul(new.tokens.len()) > config.token_product_cap {
        return None;
    }

    let ot: Vec<&str> = old.tokens.iter().map(|t| t.text).collect();
    let nt: Vec<&str> = new.tokens.iter().map(|t| t.text).collect();
    let line_a: Vec<usize> = old.tokens.iter().map(|t| t.line).collect();
    let line_b: Vec<usize> = new.tokens.iter().map(|t| t.line).collect();

    let classifier = UniversalDelimiterClassifier;
    let mut c_old = vec![0u8; ot.len()];
    let mut c_new = vec![0u8; nt.len()];
    let mut d_old = vec![0u16; ot.len()];
    let mut d_new = vec![0u16; nt.len()];
    diffuse_algo::track_enclosing_containers_into(&[], &ot, &classifier, &mut c_old);
    diffuse_algo::track_enclosing_containers_into(&[], &nt, &classifier, &mut c_new);
    diffuse_algo::track_nesting_depths_into(&[], &ot, &classifier, &mut d_old);
    diffuse_algo::track_nesting_depths_into(&[], &nt, &classifier, &mut d_new);

    // When the window contains at least one paired `Mod` slot (`slot_of_old`
    // and `slot_of_new` share a slot index), only hard-pin same-slot unique
    // anchors (`sa == sb`). Hard-pinning cross-slot anchors beforehand can trap
    // a `Mod` slot's shared prefix (`//`, `///`, `assert_eq!(`) on the wrong
    // side of a cross-slot word; letting Gotoh DP solve cross-slot intervals
    // with same-slot affinity preserves every `Mod` slot's `Ctx` while still
    // aligning all monotone cross-slot moves.
    let has_mod_slot = slots
        .slot_of_old
        .iter()
        .any(|so| slots.slot_of_new.contains(so));

    let unique_anchors: Vec<(usize, usize)> = unique_token_anchors(&ot, &nt, profile)
        .into_iter()
        .filter(|&(ai, bi)| {
            let sa = slots.slot_of_old[line_a[ai]];
            let sb = slots.slot_of_new[line_b[bi]];
            let ca = ContainerKind::from_id(c_old[ai]);
            let cb = ContainerKind::from_id(c_new[bi]);
            (!has_mod_slot || sa == sb)
                && sa.abs_diff(sb) <= config.max_slot_distance
                && ContainerKind::is_compatible_with(ca, cb)
        })
        .collect();

    let anchors = merge_mod_comment_prefix_anchors(
        old_lines,
        new_lines,
        &old,
        &new,
        slots,
        (&c_old, &c_new),
        profile,
        unique_anchors,
    );

    let old_side = Side2D {
        tokens: &ot,
        lines: &line_a,
        line_ranges: &old.line_ranges,
        slot_of_line: slots.slot_of_old,
        depths: &d_old,
        containers: &c_old,
    };
    let new_side = Side2D {
        tokens: &nt,
        lines: &line_b,
        line_ranges: &new.line_ranges,
        slot_of_line: slots.slot_of_new,
        depths: &d_new,
        containers: &c_new,
    };

    let pair_map = solve_anchored_2d(&old_side, &new_side, &anchors, config);

    let mut pairs: Vec<(usize, usize)> = pair_map.into_iter().collect();
    pairs.sort_by_key(|&(oi, _)| oi);

    // Defense-in-depth: container invariance & line-bounded subword cohesion.
    filter_container_and_subword_pairs(
        &mut pairs,
        &TokenContainerContext {
            tokens: &ot,
            containers: &c_old,
            depths: &d_old,
            lines: Some(&line_a),
        },
        &TokenContainerContext {
            tokens: &nt,
            containers: &c_new,
            depths: &d_new,
            lines: Some(&line_b),
        },
    );

    Some(MultiLineAlignment { old, new, pairs })
}

/// Pins identical leading comment-marker tokens (`///`, `//!`, `//`, `/*`, `*`,
/// `#`) on paired `Mod` slots as same-slot anchors so Stage 1 Gotoh never
/// sacrifices an unchanged comment gutter (`[-///-][+///+]`) to force a
/// downward cross-slot match; Stage 2 relocates wrapped prose cleanly across
/// the preserved gutter instead.
#[allow(clippy::too_many_arguments)]
fn merge_mod_comment_prefix_anchors(
    old_lines: &[&str],
    new_lines: &[&str],
    old: &MultiLineTokens<'_>,
    new: &MultiLineTokens<'_>,
    slots: WindowSlotMap<'_>,
    containers: (&[u8], &[u8]),
    profile: Option<&LangProfile>,
    unique_anchors: Vec<(usize, usize)>,
) -> Vec<(usize, usize)> {
    let Some(prof) = profile.filter(|p| !p.markdown) else {
        return unique_anchors;
    };
    let (c_old, c_new) = containers;

    let mut prefix_anchors: Vec<(usize, usize)> = Vec::new();
    let mut ni_line = 0usize;
    for (ol, &o_line) in old_lines.iter().enumerate() {
        let so = slots.slot_of_old[ol];
        while ni_line < new_lines.len() && slots.slot_of_new[ni_line] < so {
            ni_line += 1;
        }
        if ni_line >= new_lines.len() || slots.slot_of_new[ni_line] != so {
            continue;
        }
        let nl = ni_line;
        let n_line = new_lines[nl];
        let (Some(om), Some(nm)) = (
            prof.leading_comment_prefix(o_line),
            prof.leading_comment_prefix(n_line),
        ) else {
            continue;
        };
        if om != nm {
            continue;
        }
        let o_range = old.line_ranges[ol].clone();
        let n_range = new.line_ranges[nl].clone();
        let mut oi = o_range.start;
        let mut ni = n_range.start;
        if oi < o_range.end && old.tokens[oi].is_leading_ws {
            oi += 1;
        }
        if ni < n_range.end && new.tokens[ni].is_leading_ws {
            ni += 1;
        }
        let mut covered = 0usize;
        let mut line_pairs = Vec::new();
        while covered < om.len() && oi < o_range.end && ni < n_range.end {
            let ot = old.tokens[oi];
            let nt = new.tokens[ni];
            let ca = ContainerKind::from_id(c_old[oi]);
            let cb = ContainerKind::from_id(c_new[ni]);
            if ot.text != nt.text || !ContainerKind::is_compatible_with(ca, cb) {
                line_pairs.clear();
                break;
            }
            covered += ot.text.len();
            line_pairs.push((oi, ni));
            oi += 1;
            ni += 1;
        }
        if covered == om.len() {
            prefix_anchors.extend(line_pairs);
        }
    }

    if prefix_anchors.is_empty() {
        return unique_anchors;
    }

    // Merge strictly increasing `(ai, bi)` pairs, preferring comment-prefix
    // anchors whenever a unique anchor would cross or duplicate one.
    let mut merged: Vec<(usize, usize)> =
        Vec::with_capacity(prefix_anchors.len() + unique_anchors.len());
    let (mut pi, mut ui) = (0usize, 0usize);
    while pi < prefix_anchors.len() || ui < unique_anchors.len() {
        let take_prefix = match (prefix_anchors.get(pi), unique_anchors.get(ui)) {
            (Some(&(pa, _)), Some(&(ua, _))) => pa <= ua,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        if take_prefix {
            let pair = prefix_anchors[pi];
            pi += 1;
            // Prefix anchors are strictly monotonic by construction; pop any
            // previously pushed unique anchor that conflicts with this gutter.
            while let Some(&(la, lb)) = merged.last() {
                if la >= pair.0 || lb >= pair.1 {
                    merged.pop();
                } else {
                    break;
                }
            }
            merged.push(pair);
        } else {
            let pair = unique_anchors[ui];
            ui += 1;
            let fits_prev = merged
                .last()
                .is_none_or(|&(la, lb)| pair.0 > la && pair.1 > lb);
            let fits_next = prefix_anchors
                .get(pi)
                .is_none_or(|&(pa, pb)| pair.0 < pa && pair.1 < pb);
            if fits_prev && fits_next {
                merged.push(pair);
            }
        }
    }
    merged
}

fn solve_anchored_2d(
    old: &Side2D<'_>,
    new: &Side2D<'_>,
    anchors: &[(usize, usize)],
    config: &MultiLineConfig,
) -> FxHashMap<usize, usize> {
    let run_gap = |a_start: usize, a_end: usize, b_start: usize, b_end: usize| {
        let affinity = SlotAffinity {
            old,
            new,
            offset_a: a_start,
            offset_b: b_start,
            max_slot_distance: config.max_slot_distance,
            same_slot_alnum_bonus: config.same_slot_alnum_bonus,
            same_slot_punct_bonus: config.same_slot_punct_bonus,
        };
        raw_lcs_pairs_with_slots(
            &old.tokens[a_start..a_end],
            &new.tokens[b_start..b_end],
            &affinity,
        )
    };

    if anchors.is_empty() {
        return run_gap(0, old.tokens.len(), 0, new.tokens.len());
    }

    let mut pairs = FxHashMap::default();
    let mut a_prev = 0usize;
    let mut b_prev = 0usize;

    for (anchor_idx, &(ai, bi)) in anchors.iter().enumerate() {
        let next_bounds = anchors
            .get(anchor_idx + 1)
            .copied()
            .unwrap_or((old.tokens.len(), new.tokens.len()));
        let ((back_a, fwd_a), (back_b, fwd_b)) = expand_anchor_span(
            old.tokens,
            new.tokens,
            Some(old.lines),
            Some(new.lines),
            (ai, bi),
            (a_prev, b_prev),
            next_bounds,
        );

        if back_a > a_prev && back_b > b_prev {
            for (si, sj) in run_gap(a_prev, back_a, b_prev, back_b) {
                pairs.insert(a_prev + si, b_prev + sj);
            }
        }

        for (ka, kb) in (back_a..=fwd_a).zip(back_b..=fwd_b) {
            pairs.insert(ka, kb);
        }
        a_prev = fwd_a + 1;
        b_prev = fwd_b + 1;
    }

    if a_prev < old.tokens.len() && b_prev < new.tokens.len() {
        for (si, sj) in run_gap(a_prev, old.tokens.len(), b_prev, new.tokens.len()) {
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
                &lines[tok.line][tok.byte_range()],
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
        let default_cfg = MultiLineConfig::default();
        let empty = align_lines_2d(
            &[],
            &["let x = 1;"],
            WindowSlotMap {
                slot_of_old: &[],
                slot_of_new: &[0],
            },
            None,
            &default_cfg,
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
            &default_cfg,
        )
        .expect("empty lines succeed");
        assert!(blank_only.pairs.is_empty());

        let capped_cfg = MultiLineConfig {
            token_product_cap: 4,
            ..default_cfg
        };
        let capped = align_lines_2d(
            &["let alpha = compute(a, b);"],
            &["let beta = compute(a, c);"],
            WindowSlotMap {
                slot_of_old: &[0],
                slot_of_new: &[0],
            },
            None,
            &capped_cfg,
        );
        assert!(capped.is_none(), "exceeding token-product cap returns None");
    }

    #[test]
    fn align_lines_2d_forbids_cross_slot_whitespace_and_respects_slot_affinity() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let cfg = MultiLineConfig::default();
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
            &cfg,
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
            &cfg,
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
        let aln =
            align_lines_2d(&old, &new, slots, rs, &MultiLineConfig::default()).expect("within cap");

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
        let cfg = MultiLineConfig::default();
        // Unique salient token 6 slots apart must be rejected when cap is 4.
        let aln_far = align_lines_2d(
            &["    unique_payload_ident();"],
            &["    unique_payload_ident();"],
            WindowSlotMap {
                slot_of_old: &[0],
                slot_of_new: &[6],
            },
            rs,
            &cfg,
        )
        .expect("within cap");
        assert!(
            aln_far.pairs.is_empty(),
            "tokens beyond max_slot_distance must not match"
        );

        // Same token within max_slot_distance (3 <= 4) matches cleanly.
        let aln_near = align_lines_2d(
            &["    unique_payload_ident();"],
            &["    unique_payload_ident();"],
            WindowSlotMap {
                slot_of_old: &[0],
                slot_of_new: &[3],
            },
            rs,
            &cfg,
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
            &cfg,
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
            &MultiLineConfig::default(),
        )
        .expect("within cap");
        assert_eq!(aln.pairs.len(), 2);
        assert_eq!(aln.old.tokens[aln.pairs[0].0].line, 0);
        assert_eq!(aln.new.tokens[aln.pairs[0].1].line, 0);
        assert_eq!(aln.old.tokens[aln.pairs[1].0].line, 1);
        assert_eq!(aln.new.tokens[aln.pairs[1].1].line, 2);
    }

    #[test]
    fn align_lines_2d_pins_identical_mod_comment_prefixes_against_downward_wraps() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let old = [
            "/// left-edge stripe from amber to active cyan, and if `is_current_line` is",
            "/// true, highlights the line background with cursor",
        ];
        let new = [
            "/// left-edge stripe from the status color to active cyan, and if",
            "/// `is_current_line` is true, highlights the line background with cursor",
        ];
        let aln = align_lines_2d(
            &old,
            &new,
            WindowSlotMap {
                slot_of_old: &[0, 1],
                slot_of_new: &[0, 1],
            },
            rs,
            &MultiLineConfig::default(),
        )
        .expect("within cap");

        // The `///` marker on line 1 must stay matched same-slot (`Ctx`).
        assert!(
            aln.pairs.iter().any(|&(oi, ni)| {
                aln.old.tokens[oi].line == 1
                    && aln.new.tokens[ni].line == 1
                    && aln.old.tokens[oi].text == "///"
            }),
            "identical `///` prefix on paired Mod slot 1 must stay same-slot Ctx"
        );
    }
}
