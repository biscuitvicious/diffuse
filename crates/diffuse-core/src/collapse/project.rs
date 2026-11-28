//! Stage 1: monotone 2D coordinate-aware multi-line slot projection.
//!
//! Projects a 2D slot-aware Gotoh LCS directly onto a peer-reflow window's
//! ordered [`WindowSlot`] list, emitting [`RunKind::Ctx`] for same-slot matches
//! and balanced [`RunKind::MovedSrc`] / [`RunKind::MovedTgt`] phrases for
//! monotone cross-slot matches.
//!
//! Guarantees by construction:
//! - Exact old/new text projection on every slot (`project_old == old_text`,
//!   `project_new == new_text`).
//! - Exact closed-economy conservation (`Σ MovedSrc ≡ Σ MovedTgt`, identical
//!   phrase multisets on both sides).

use std::ops::Range;

use diffuse_syntax::profile::LangProfile;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    collapse::{
        prose::{LineSpan, count_salient_words, is_prose_window, prose_move_allowed},
        slot::{Runs, WindowSlot},
    },
    lcs::{
        MultiLineAlignment, MultiLineConfig, Token2D, WindowSlotMap, align_lines_2d,
        line_has_salient_token, token_is_ws,
    },
    merge::RunKind,
    tape::spans::{LeadingIndentSplit, split_leading_indent},
};

/// Projects a 2D coordinate-aware multi-line alignment directly onto a window's
/// ordered slot list, emitting `Ctx` for same-slot matches and balanced
/// `MovedSrc`/`MovedTgt` phrases for cross-slot matches.
///
/// Returns `None` when the window has no non-moved lines on both sides, exceeds
/// `token_product_cap`, or when any `Mod` slot would degrade into a wholesale
/// replacement (allowing the caller to fall back to per-slot alignment).
pub(crate) fn align_window_2d_tape(
    window_slots: &[WindowSlot<'_>],
    profile: Option<&LangProfile>,
) -> Option<Vec<Runs>> {
    let window = extract_window_lines(window_slots)?;

    // In `1 × m` expansions and `n × 1` collapses, every row belongs to the
    // single opposite-side construct (bounded by `reflow.rs`), so cross-slot
    // matches must reach the terminal closing delimiter regardless of window
    // height (`m` or `n`). Multi-row `n × m` windows keep
    // `DEFAULT_MAX_SLOT_DISTANCE`.
    let is_single_construct = window.old_lines.len() == 1 || window.new_lines.len() == 1;
    let config = if is_single_construct {
        MultiLineConfig::single_construct()
    } else {
        MultiLineConfig::default()
    };

    let aln = align_lines_2d(
        &window.old_lines,
        &window.new_lines,
        WindowSlotMap {
            slot_of_old: &window.slot_of_old,
            slot_of_new: &window.slot_of_new,
        },
        profile,
        &config,
    )?;

    let kept_pairs = filter_cross_slot_punctuation(&aln, &window, is_single_construct, profile);
    let kept_pairs = filter_isolated_intraword_punct(&aln, &window, kept_pairs);
    let extended_pairs = extend_cross_slot_phrases(&aln, &window, &kept_pairs);
    let bridged_pairs = bridge_enclosed_cross_slot_spans(&aln, &window, extended_pairs);
    let clean_pairs = filter_weak_prose_cross_slot_phrases(&aln, &window, bridged_pairs, profile);
    let (old_matches, new_matches) = assign_phrase_matches(&aln, &window, &clean_pairs);

    let mut slot_runs: Vec<Runs> = Vec::with_capacity(window_slots.len());
    for (slot_idx, &slot) in window_slots.iter().enumerate() {
        match (
            window.old_line_of_slot[slot_idx],
            window.new_line_of_slot[slot_idx],
        ) {
            (Some(ol), Some(nl)) => {
                let old_pieces = build_line_pieces(
                    &aln.old.tokens,
                    aln.old.line_ranges[ol].clone(),
                    &old_matches,
                    RunKind::MovedSrc,
                    RunKind::Del,
                );
                let new_pieces = build_line_pieces(
                    &aln.new.tokens,
                    aln.new.line_ranges[nl].clone(),
                    &new_matches,
                    RunKind::MovedTgt,
                    RunKind::Add,
                );
                let runs = merge_mod_slot(&old_pieces, &new_pieces);
                if slot_runs_are_wholesale(&runs, window.old_lines[ol], window.new_lines[nl]) {
                    return None;
                }
                slot_runs.push(runs);
            }
            (Some(ol), None) => {
                let pieces = build_line_pieces(
                    &aln.old.tokens,
                    aln.old.line_ranges[ol].clone(),
                    &old_matches,
                    RunKind::MovedSrc,
                    RunKind::Del,
                );
                slot_runs.push(emit_single_side(&pieces));
            }
            (None, Some(nl)) => {
                let pieces = build_line_pieces(
                    &aln.new.tokens,
                    aln.new.line_ranges[nl].clone(),
                    &new_matches,
                    RunKind::MovedTgt,
                    RunKind::Add,
                );
                slot_runs.push(emit_single_side(&pieces));
            }
            (None, None) => {
                // Pre-moved `Row::MovSrc` / `Row::MovTgt` slot retains its
                // cached runs untouched.
                slot_runs.push(
                    slot.cached_moved_runs()
                        .expect("only pre-moved slots omit both old and new lines")
                        .clone(),
                );
            }
        }
    }

    Some(slot_runs)
}

/// Returns `true` when `runs` would turn a `Mod` slot into a wholesale
/// replacement (zero non-whitespace `Ctx` runs despite non-empty text).
#[inline]
pub(crate) fn slot_runs_are_wholesale(
    runs: &[(RunKind, String)],
    old_text: &str,
    new_text: &str,
) -> bool {
    let has_non_ws_ctx = runs
        .iter()
        .any(|(kind, text)| *kind == RunKind::Ctx && !text.trim().is_empty());
    !has_non_ws_ctx && (!old_text.trim().is_empty() || !new_text.trim().is_empty())
}

/// Non-moved old/new lines extracted from a window alongside bidirectional
/// line-to-slot coordinate maps.
struct WindowLines<'a> {
    old_lines: Vec<&'a str>,
    new_lines: Vec<&'a str>,
    slot_of_old: Vec<usize>,
    slot_of_new: Vec<usize>,
    old_line_of_slot: Vec<Option<usize>>,
    new_line_of_slot: Vec<Option<usize>>,
}

/// Match assignment for one 2D token after filtering, extension, and bridging.
#[derive(Clone, Copy)]
enum TokenMatch {
    Ctx,
    Moved(usize),
}

/// One classified token on an old or new line during 2D slot projection.
#[derive(Clone, Copy, Debug)]
enum ProjectedPiece<'a> {
    Ctx(&'a str),
    Edge {
        kind: RunKind,
        text: &'a str,
        phrase_id: Option<usize>,
        is_leading_ws: bool,
    },
}

#[derive(Clone, Copy)]
enum ExtendDir {
    Backward,
    Forward,
}

fn extract_window_lines<'a>(window_slots: &[WindowSlot<'a>]) -> Option<WindowLines<'a>> {
    let total_slots = window_slots.len();
    let mut old_lines: Vec<&'a str> = Vec::new();
    let mut new_lines: Vec<&'a str> = Vec::new();
    let mut slot_of_old: Vec<usize> = Vec::new();
    let mut slot_of_new: Vec<usize> = Vec::new();
    let mut old_line_of_slot: Vec<Option<usize>> = vec![None; total_slots];
    let mut new_line_of_slot: Vec<Option<usize>> = vec![None; total_slots];

    for (slot_idx, &slot) in window_slots.iter().enumerate() {
        match slot {
            WindowSlot::Mod(u, v) => {
                old_line_of_slot[slot_idx] = Some(old_lines.len());
                old_lines.push(u.text());
                slot_of_old.push(slot_idx);
                new_line_of_slot[slot_idx] = Some(new_lines.len());
                new_lines.push(v.text());
                slot_of_new.push(slot_idx);
            }
            WindowSlot::Del(u) => {
                old_line_of_slot[slot_idx] = Some(old_lines.len());
                old_lines.push(u.text());
                slot_of_old.push(slot_idx);
            }
            WindowSlot::Add(v) => {
                new_line_of_slot[slot_idx] = Some(new_lines.len());
                new_lines.push(v.text());
                slot_of_new.push(slot_idx);
            }
            WindowSlot::MovSrc(_) | WindowSlot::MovTgt(_) => {}
        }
    }

    if old_lines.is_empty() || new_lines.is_empty() {
        return None;
    }

    Some(WindowLines {
        old_lines,
        new_lines,
        slot_of_old,
        slot_of_new,
        old_line_of_slot,
        new_line_of_slot,
    })
}

/// Reconstructs the contiguous alphanumeric/underscore full word containing
/// `tokens[idx]` (matching `is_token_salient_in_context`'s subword grouping).
fn full_subword_at<'a>(tokens: &[&'a str], idx: usize) -> Option<&'a str> {
    let t = *tokens.get(idx)?;
    if !crate::lcs::token_is_alnum(t) {
        return None;
    }
    let mut start = idx;
    while start > 0
        && crate::lcs::tokenizer::are_contiguous_subwords(tokens[start - 1], tokens[start])
    {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < tokens.len()
        && crate::lcs::tokenizer::are_contiguous_subwords(tokens[end - 1], tokens[end])
    {
        end += 1;
    }
    let full_start = tokens[start].as_ptr();
    let full_len =
        (tokens[end - 1].as_ptr() as usize + tokens[end - 1].len()) - full_start as usize;
    // SAFETY: contiguous subwords come from the same source `&str` line slice.
    Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(full_start, full_len)) })
}

/// Counts occurrences of each distinct full alphanumeric word across all lines
/// in `side`, counting each contiguous multi-subword identifier once per
/// occurrence.
fn count_window_words<'a>(
    side: &crate::lcs::MultiLineTokens<'a>,
    texts: &[&'a str],
) -> FxHashMap<&'a str, usize> {
    let mut freq: FxHashMap<&'a str, usize> = FxHashMap::default();
    for range in &side.line_ranges {
        let line_toks = &texts[range.clone()];
        let mut i = 0;
        while i < line_toks.len() {
            if !crate::lcs::token_is_alnum(line_toks[i]) {
                i += 1;
                continue;
            }
            let mut end = i + 1;
            while end < line_toks.len()
                && crate::lcs::tokenizer::are_contiguous_subwords(
                    line_toks[end - 1],
                    line_toks[end],
                )
            {
                end += 1;
            }
            if let Some(word) = full_subword_at(line_toks, i) {
                *freq.entry(word).or_default() += 1;
            }
            i = end;
        }
    }
    freq
}

/// Demotes coincidental bare-punctuation, non-salient, and ambiguous-only
/// cross-slot matches between unrelated full lines back to `Del`/`Add`.
///
/// Preserves matches that:
/// - stay within the same slot (`so == sn`),
/// - belong to a slot pair linked by a genuine content anchor (a salient
///   identifier or unique multi-character literal, excluding single repeated
///   ambiguous words scavenging onto a pure-`Add`/`Del` line with its own
///   unmatched salient identifiers),
/// - land on a line with no salient identifiers of its own (e.g. `) -> usize {`
///   or `);`), or
/// - carry the construct-closing trailing suffix on the final line pair of a
///   single-construct (`1 × m` or `n × 1`) window that already carries a
///   salient cross-slot link (e.g. `=> {` on the last arm of a `1 × 3` match
///   arm expansion).
fn filter_cross_slot_punctuation(
    aln: &MultiLineAlignment<'_>,
    window: &WindowLines<'_>,
    is_single_construct: bool,
    profile: Option<&LangProfile>,
) -> Vec<(usize, usize)> {
    let old_line_has_salient: Vec<bool> = window
        .old_lines
        .iter()
        .map(|line| line_has_salient_token(line, profile))
        .collect();
    let new_line_has_salient: Vec<bool> = window
        .new_lines
        .iter()
        .map(|line| line_has_salient_token(line, profile))
        .collect();

    let old_texts: Vec<&str> = aln.old.tokens.iter().map(|t| t.text).collect();
    let new_texts: Vec<&str> = aln.new.tokens.iter().map(|t| t.text).collect();
    let old_word_freq = count_window_words(&aln.old, &old_texts);
    let new_word_freq = count_window_words(&aln.new, &new_texts);

    let mut matched_old = vec![false; aln.old.tokens.len()];
    let mut matched_new = vec![false; aln.new.tokens.len()];
    for &(oi, ni) in &aln.pairs {
        matched_old[oi] = true;
        matched_new[ni] = true;
    }

    let line_has_unmatched_salient = |side: &crate::lcs::MultiLineTokens<'_>,
                                      texts: &[&str],
                                      matched: &[bool],
                                      line_idx: usize| {
        let range = &side.line_ranges[line_idx];
        let line_toks = &texts[range.clone()];
        (range.start..range.end).any(|gi| {
            !matched[gi]
                && crate::lcs::is_token_salient_in_context(line_toks, gi - range.start, profile)
        })
    };

    // Track per cross-slot `(so, sn)` pair: distinct matched content words and
    // whether at least one matched content word is unambiguous (1-to-1 in the
    // window).
    let mut pair_words: FxHashMap<(usize, usize), FxHashSet<&str>> = FxHashMap::default();
    let mut pair_has_unique: FxHashSet<(usize, usize)> = FxHashSet::default();
    let mut pair_lines: FxHashMap<(usize, usize), (usize, usize)> = FxHashMap::default();

    for &(oi, ni) in &aln.pairs {
        let ol = aln.old.tokens[oi].line;
        let nl = aln.new.tokens[ni].line;
        let so = window.slot_of_old[ol];
        let sn = window.slot_of_new[nl];
        if so == sn {
            continue;
        }
        let o_range = &aln.old.line_ranges[ol];
        let o_line_toks = &old_texts[o_range.clone()];
        let local_oi = oi - o_range.start;
        let Some(word) = full_subword_at(o_line_toks, local_oi) else {
            continue;
        };
        let is_salient = crate::lcs::is_token_salient_in_context(o_line_toks, local_oi, profile);
        let is_unique = old_word_freq.get(word).copied().unwrap_or(0) == 1
            && new_word_freq.get(word).copied().unwrap_or(0) == 1;
        let is_unique_literal = is_unique
            && crate::lcs::char_len(word) >= 2
            && !crate::lcs::tokenizer::COMMON_KEYWORDS.contains(&word)
            && !profile.is_some_and(|p| p.is_non_salient(word));
        if is_salient || is_unique_literal {
            pair_words.entry((so, sn)).or_default().insert(word);
            pair_lines.insert((so, sn), (ol, nl));
            if is_unique {
                pair_has_unique.insert((so, sn));
            }
        }
    }

    let mut salient_linked: FxHashSet<(usize, usize)> = FxHashSet::default();
    for (&(so, sn), words) in &pair_words {
        let (ol, nl) = pair_lines[&(so, sn)];
        let has_unique = pair_has_unique.contains(&(so, sn));
        // Anti-scavenging guard: if a pure-Add (or pure-Del) line introduces
        // unmatched salient identifiers of its own, a single repeated/ambiguous
        // word (e.g. a user-defined repeated type `ScrollPx`) is insufficient
        // evidence of a cross-slot move.
        let single_ambiguous_word = !has_unique && words.len() <= 1;
        let pure_add_scavenge = single_ambiguous_word
            && window.old_line_of_slot[sn].is_none()
            && line_has_unmatched_salient(&aln.new, &new_texts, &matched_new, nl);
        let pure_del_scavenge = single_ambiguous_word
            && window.new_line_of_slot[so].is_none()
            && line_has_unmatched_salient(&aln.old, &old_texts, &matched_old, ol);
        if !pure_add_scavenge && !pure_del_scavenge {
            salient_linked.insert((so, sn));
        }
    }

    // In a single-construct expansion/collapse (`1 × m` or `n × 1`), preserve
    // the contiguous trailing construct-closing suffix on the final `(ol, nl)`
    // line pair when either the window already carries a salient cross-slot
    // move (e.g. `=> {` on a `1 × 3` match arm expansion) or the suffix has at
    // least two non-whitespace tokens ending in a statement/block delimiter
    // (e.g. `);` when the opening call frame relocates non-monotonically in
    // Stage 2).
    let mut terminal_suffix_pairs: FxHashSet<(usize, usize)> = FxHashSet::default();
    if is_single_construct && !aln.pairs.is_empty() {
        let last_ol = window.old_lines.len() - 1;
        let last_nl = window.new_lines.len() - 1;
        let last_non_ws = |side: &crate::lcs::MultiLineTokens<'_>, line: usize| {
            let r = &side.line_ranges[line];
            (r.start..r.end)
                .rev()
                .find(|&idx| !token_is_ws(side.tokens[idx].text))
        };
        if let (Some(end_o), Some(end_n)) = (
            last_non_ws(&aln.old, last_ol),
            last_non_ws(&aln.new, last_nl),
        ) && aln.pairs.last() == Some(&(end_o, end_n))
        {
            let mut suffix = Vec::new();
            let mut k = aln.pairs.len();
            let (mut exp_o, mut exp_n) = (end_o, end_n);
            while k > 0 {
                let (oi, ni) = aln.pairs[k - 1];
                if aln.old.tokens[oi].line != last_ol || aln.new.tokens[ni].line != last_nl {
                    break;
                }
                if oi == exp_o && ni == exp_n {
                    suffix.push((oi, ni));
                    k -= 1;
                    if exp_o == 0 || exp_n == 0 {
                        break;
                    }
                    exp_o -= 1;
                    exp_n -= 1;
                } else {
                    break;
                }
            }
            let non_ws_count = suffix
                .iter()
                .filter(|&&(oi, _)| !token_is_ws(aln.old.tokens[oi].text))
                .count();
            let ends_with_frame_delim =
                matches!(aln.old.tokens[end_o].text, ")" | "]" | "}" | ";" | "{");
            if !salient_linked.is_empty() || (non_ws_count >= 2 && ends_with_frame_delim) {
                terminal_suffix_pairs.extend(suffix);
            }
        }
    }

    aln.pairs
        .iter()
        .copied()
        .filter(|&(oi, ni)| {
            let ol = aln.old.tokens[oi].line;
            let nl = aln.new.tokens[ni].line;
            let so = window.slot_of_old[ol];
            let sn = window.slot_of_new[nl];
            so == sn
                || salient_linked.contains(&(so, sn))
                || !old_line_has_salient[ol]
                || !new_line_has_salient[nl]
                || terminal_suffix_pairs.contains(&(oi, ni))
        })
        .collect()
}

/// Drops isolated single-token same-slot punctuation (such as `-` in
/// `1-indexed` vs `re-anchor`) when both immediate neighbors on both sides are
/// unmatched alphanumeric tokens, preventing coincidental hyphen matches from
/// splitting a hyphenated word across a `Ctx` island.
fn filter_isolated_intraword_punct(
    aln: &MultiLineAlignment<'_>,
    window: &WindowLines<'_>,
    pairs: Vec<(usize, usize)>,
) -> Vec<(usize, usize)> {
    let is_alnum_tok = |tok: Token2D<'_>| tok.text.chars().any(char::is_alphanumeric);
    let mut out = Vec::with_capacity(pairs.len());
    for (idx, &(oi, ni)) in pairs.iter().enumerate() {
        let ot = aln.old.tokens[oi];
        let nt = aln.new.tokens[ni];
        let same_slot = window.slot_of_old[ot.line] == window.slot_of_new[nt.line];
        let is_punct = !token_is_ws(ot.text) && !is_alnum_tok(ot);
        if same_slot && is_punct {
            let prev_adj = idx > 0 && pairs[idx - 1] == (oi.wrapping_sub(1), ni.wrapping_sub(1));
            let next_adj = idx + 1 < pairs.len() && pairs[idx + 1] == (oi + 1, ni + 1);
            let o_range = &aln.old.line_ranges[ot.line];
            let n_range = &aln.new.line_ranges[nt.line];
            let o_between_alnum = oi > o_range.start
                && oi + 1 < o_range.end
                && is_alnum_tok(aln.old.tokens[oi - 1])
                && is_alnum_tok(aln.old.tokens[oi + 1]);
            let n_between_alnum = ni > n_range.start
                && ni + 1 < n_range.end
                && is_alnum_tok(aln.new.tokens[ni - 1])
                && is_alnum_tok(aln.new.tokens[ni + 1]);
            if !prev_adj && !next_adj && o_between_alnum && n_between_alnum {
                continue;
            }
        }
        out.push((oi, ni));
    }
    out
}

#[inline]
fn same_line_non_ws_match(o: Token2D<'_>, n: Token2D<'_>, ol: usize, nl: usize) -> bool {
    o.line == ol && n.line == nl && !token_is_ws(o.text) && o.text == n.text
}

#[inline]
fn same_line_interior_ws_match(o: Token2D<'_>, n: Token2D<'_>, ol: usize, nl: usize) -> bool {
    o.line == ol
        && n.line == nl
        && !o.is_leading_ws
        && !n.is_leading_ws
        && token_is_ws(o.text)
        && o.text == n.text
}

/// Extends a cross-slot moved anchor `(co, cn)` in `dir` across adjacent
/// identical non-whitespace tokens (optionally separated by one identical
/// non-leading interior space) on the same `(ol, nl)` line pair, stopping
/// strictly inside `(limit_o, limit_n)`. Returns the final `(co, cn)`.
fn extend_boundary(
    aln: &MultiLineAlignment<'_>,
    (ol, nl): (usize, usize),
    (mut co, mut cn): (usize, usize),
    (limit_o, limit_n): (usize, usize),
    dir: ExtendDir,
    out: &mut Vec<(usize, usize)>,
) -> (usize, usize) {
    let old_tokens = &aln.old.tokens;
    let new_tokens = &aln.new.tokens;
    let remaining = |cur: usize, limit: usize| match dir {
        ExtendDir::Backward => cur.saturating_sub(limit),
        ExtendDir::Forward => limit.saturating_sub(cur + 1),
    };
    let step = |cur: usize, delta: usize| match dir {
        ExtendDir::Backward => cur - delta,
        ExtendDir::Forward => cur + delta,
    };

    loop {
        let rem_o = remaining(co, limit_o);
        let rem_n = remaining(cn, limit_n);
        if rem_o >= 1 && rem_n >= 1 {
            let (no, nn) = (step(co, 1), step(cn, 1));
            if same_line_non_ws_match(old_tokens[no], new_tokens[nn], ol, nl) {
                out.push((no, nn));
                co = no;
                cn = nn;
                continue;
            }
            if rem_o >= 2 && rem_n >= 2 {
                let (to, tn) = (step(co, 2), step(cn, 2));
                if same_line_interior_ws_match(old_tokens[no], new_tokens[nn], ol, nl)
                    && same_line_non_ws_match(old_tokens[to], new_tokens[tn], ol, nl)
                {
                    out.push((no, nn));
                    out.push((to, tn));
                    co = to;
                    cn = tn;
                    continue;
                }
            }
        }
        break;
    }

    (co, cn)
}

/// Extends cross-slot moved phrases across adjacent unmatched identical
/// non-whitespace boundary tokens (optionally separated by one identical
/// non-leading interior space) on the same `(ol, nl)` line pair — recovering
/// isolated low-weight operators/punctuation such as `self.vis_line =` or
/// trailing `,`, plus a trailing interior separator space (such as
/// `|pat: &str| `) when both lines carry the exact same separator before a
/// following token (preventing stray red/green space flashes).
fn extend_cross_slot_phrases(
    aln: &MultiLineAlignment<'_>,
    window: &WindowLines<'_>,
    kept_pairs: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let mut extended: Vec<(usize, usize)> = Vec::with_capacity(kept_pairs.len());
    let mut prefix: Vec<(usize, usize)> = Vec::new();

    for (idx, &(oi, ni)) in kept_pairs.iter().enumerate() {
        let ol = aln.old.tokens[oi].line;
        let nl = aln.new.tokens[ni].line;
        let is_cross = window.slot_of_old[ol] != window.slot_of_new[nl];

        if is_cross {
            let min_bounds = extended.last().map_or((0, 0), |&(po, pn)| (po + 1, pn + 1));
            prefix.clear();
            extend_boundary(
                aln,
                (ol, nl),
                (oi, ni),
                min_bounds,
                ExtendDir::Backward,
                &mut prefix,
            );
            prefix.reverse();
            extended.extend(prefix.iter().copied());
        }

        extended.push((oi, ni));

        if is_cross {
            let max_bounds = kept_pairs
                .get(idx + 1)
                .copied()
                .unwrap_or((aln.old.tokens.len(), aln.new.tokens.len()));
            let (co, cn) = extend_boundary(
                aln,
                (ol, nl),
                (oi, ni),
                max_bounds,
                ExtendDir::Forward,
                &mut extended,
            );

            // Absorb a single identical interior separator space immediately
            // following a moved phrase when both lines have a subsequent token
            // on the same line (e.g. `|pat: &str| `), avoiding stray red/green
            // separator flashes.
            if co + 1 < max_bounds.0
                && cn + 1 < max_bounds.1
                && co + 2 < aln.old.line_ranges[ol].end
                && cn + 2 < aln.new.line_ranges[nl].end
                && same_line_interior_ws_match(
                    aln.old.tokens[co + 1],
                    aln.new.tokens[cn + 1],
                    ol,
                    nl,
                )
            {
                extended.push((co + 1, cn + 1));
            }
        }
    }

    extended
}

/// Bridges identical non-leading intervening token spans strictly enclosed
/// between two cross-slot moved tokens on the same `(ol, nl)` line pair so
/// relocated phrases like `tag = "kind",` or `the pi review extension` stay
/// one clean moved run. Runs after boundary extension so newly extended tokens
/// bridge cleanly with adjacent matches.
fn bridge_enclosed_cross_slot_spans(
    aln: &MultiLineAlignment<'_>,
    window: &WindowLines<'_>,
    extended_pairs: Vec<(usize, usize)>,
) -> Vec<(usize, usize)> {
    let mut bridged: Vec<(usize, usize)> = Vec::with_capacity(extended_pairs.len());
    for pair in extended_pairs {
        if let Some(&(prev_oi, prev_ni)) = bridged.last() {
            let (oi, ni) = pair;
            let gap = oi.saturating_sub(prev_oi);
            if gap > 1 && ni.saturating_sub(prev_ni) == gap {
                let prev_ol = aln.old.tokens[prev_oi].line;
                let prev_nl = aln.new.tokens[prev_ni].line;
                let cur_ol = aln.old.tokens[oi].line;
                let cur_nl = aln.new.tokens[ni].line;
                if prev_ol == cur_ol
                    && prev_nl == cur_nl
                    && window.slot_of_old[cur_ol] != window.slot_of_new[cur_nl]
                    && (1..gap).all(|d| {
                        let mo = aln.old.tokens[prev_oi + d];
                        let mn = aln.new.tokens[prev_ni + d];
                        mo.line == cur_ol
                            && mn.line == cur_nl
                            && !mo.is_leading_ws
                            && !mn.is_leading_ws
                            && mo.text == mn.text
                    })
                {
                    for d in 1..gap {
                        bridged.push((prev_oi + d, prev_ni + d));
                    }
                }
            }
        }
        bridged.push(pair);
    }
    bridged
}

/// Demotes weak cross-slot prose/comment matches (bare punctuation, stopwords,
/// and isolated mid-sentence single words) back to `Del`/`Add` before Stage 2
/// runs, so spurious single-word hits cannot steal tokens from longer wrapped
/// phrases or form noisy prose peer groups.
fn filter_weak_prose_cross_slot_phrases(
    aln: &MultiLineAlignment<'_>,
    window: &WindowLines<'_>,
    pairs: Vec<(usize, usize)>,
    profile: Option<&LangProfile>,
) -> Vec<(usize, usize)> {
    let all_lines = window
        .old_lines
        .iter()
        .chain(window.new_lines.iter())
        .copied();
    if !is_prose_window(all_lines, profile) {
        return pairs;
    }

    let mut pair_salient_words: FxHashMap<(usize, usize), usize> = FxHashMap::default();
    let mut spans: Vec<(usize, usize, bool)> = Vec::new();
    let mut i = 0usize;
    while i < pairs.len() {
        let (oi, ni) = pairs[i];
        let ol = aln.old.tokens[oi].line;
        let nl = aln.new.tokens[ni].line;
        let so = window.slot_of_old[ol];
        let sn = window.slot_of_new[nl];
        if so == sn {
            spans.push((i, i + 1, false));
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < pairs.len() {
            let (po, pn) = pairs[i - 1];
            let (co, cn) = pairs[i];
            if co == po + 1
                && cn == pn + 1
                && aln.old.tokens[co].line == ol
                && aln.new.tokens[cn].line == nl
                && window.slot_of_old[ol] != window.slot_of_new[nl]
            {
                i += 1;
            } else {
                break;
            }
        }
        let phrase: String = pairs[start..i]
            .iter()
            .map(|&(o, _)| aln.old.tokens[o].text)
            .collect();
        *pair_salient_words.entry((so, sn)).or_default() += count_salient_words(&phrase, profile);
        spans.push((start, i, true));
    }

    let mut out = Vec::with_capacity(pairs.len());
    for (start, end, is_cross) in spans {
        if !is_cross {
            out.push(pairs[start]);
            continue;
        }
        let run = &pairs[start..end];
        let first_o = aln.old.tokens[run[0].0];
        let last_o = aln.old.tokens[run[run.len() - 1].0];
        let first_n = aln.new.tokens[run[0].1];
        let last_n = aln.new.tokens[run[run.len() - 1].1];
        let ol = first_o.line;
        let nl = first_n.line;
        let so = window.slot_of_old[ol];
        let sn = window.slot_of_new[nl];
        let phrase: String = run.iter().map(|&(o, _)| aln.old.tokens[o].text).collect();
        let pair_sal = pair_salient_words.get(&(so, sn)).copied().unwrap_or(0);
        let old_span = LineSpan {
            line: window.old_lines[ol],
            slot: so,
            start: first_o.byte_start,
            end: last_o.byte_end,
        };
        let new_span = LineSpan {
            line: window.new_lines[nl],
            slot: sn,
            start: first_n.byte_start,
            end: last_n.byte_end,
        };
        if prose_move_allowed(&phrase, old_span, new_span, pair_sal, profile) {
            out.extend_from_slice(run);
        }
    }
    out
}

/// Assigns identical `phrase_id` to contiguous cross-slot runs adjacent on
/// BOTH old and new lines; same-slot matches map to `Ctx`.
fn assign_phrase_matches(
    aln: &MultiLineAlignment<'_>,
    window: &WindowLines<'_>,
    pairs: &[(usize, usize)],
) -> (Vec<Option<TokenMatch>>, Vec<Option<TokenMatch>>) {
    let mut old_matches: Vec<Option<TokenMatch>> = vec![None; aln.old.tokens.len()];
    let mut new_matches: Vec<Option<TokenMatch>> = vec![None; aln.new.tokens.len()];
    let mut next_phrase_id = 0usize;
    let mut prev_moved: Option<(usize, usize, usize, usize, usize)> = None;

    for &(oi, ni) in pairs {
        let ol = aln.old.tokens[oi].line;
        let nl = aln.new.tokens[ni].line;
        if window.slot_of_old[ol] == window.slot_of_new[nl] {
            old_matches[oi] = Some(TokenMatch::Ctx);
            new_matches[ni] = Some(TokenMatch::Ctx);
            prev_moved = None;
        } else {
            let pid = match prev_moved {
                Some((p_oi, p_ni, p_ol, p_nl, p_id))
                    if oi == p_oi + 1 && ni == p_ni + 1 && ol == p_ol && nl == p_nl =>
                {
                    p_id
                }
                _ => {
                    let id = next_phrase_id;
                    next_phrase_id += 1;
                    id
                }
            };
            old_matches[oi] = Some(TokenMatch::Moved(pid));
            new_matches[ni] = Some(TokenMatch::Moved(pid));
            prev_moved = Some((oi, ni, ol, nl, pid));
        }
    }

    (old_matches, new_matches)
}

fn build_line_pieces<'a>(
    tokens: &[Token2D<'a>],
    line_range: Range<usize>,
    matches: &[Option<TokenMatch>],
    moved_kind: RunKind,
    unmatched_kind: RunKind,
) -> Vec<ProjectedPiece<'a>> {
    line_range
        .map(|idx| {
            let tok = tokens[idx];
            match matches[idx] {
                Some(TokenMatch::Ctx) => ProjectedPiece::Ctx(tok.text),
                Some(TokenMatch::Moved(pid)) => ProjectedPiece::Edge {
                    kind: moved_kind,
                    text: tok.text,
                    phrase_id: Some(pid),
                    is_leading_ws: tok.is_leading_ws,
                },
                None => ProjectedPiece::Edge {
                    kind: unmatched_kind,
                    text: tok.text,
                    phrase_id: None,
                    is_leading_ws: tok.is_leading_ws,
                },
            }
        })
        .collect()
}

fn push_projected_run(
    out: &mut Runs,
    last_phrase: &mut Option<usize>,
    kind: RunKind,
    text: &str,
    phrase_id: Option<usize>,
) {
    if text.is_empty() {
        return;
    }
    let can_merge = match (out.last(), phrase_id, *last_phrase) {
        (Some((last_k, _)), None, None) => *last_k == kind,
        (Some((last_k, _)), Some(cur_pid), Some(prev_pid)) => {
            *last_k == kind && cur_pid == prev_pid
        }
        _ => false,
    };
    if can_merge {
        if let Some((_, last_t)) = out.last_mut() {
            last_t.push_str(text);
        }
    } else {
        out.push((kind, text.to_string()));
        *last_phrase = phrase_id;
    }
}

fn emit_single_side(pieces: &[ProjectedPiece<'_>]) -> Runs {
    let mut out = Vec::new();
    let mut last_phrase = None;
    for &piece in pieces {
        match piece {
            ProjectedPiece::Ctx(text) => {
                push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, text, None);
            }
            ProjectedPiece::Edge {
                kind,
                text,
                phrase_id,
                ..
            } => {
                push_projected_run(&mut out, &mut last_phrase, kind, text, phrase_id);
            }
        }
    }
    out
}

/// Lockstep `Ctx`-anchored merger for a paired `Mod(ol, nl)` slot, applying
/// per-line leading-indentation splitting on the initial leading whitespace
/// pair.
fn merge_mod_slot(old_pieces: &[ProjectedPiece<'_>], new_pieces: &[ProjectedPiece<'_>]) -> Runs {
    let mut out = Vec::new();
    let mut last_phrase = None;
    let (mut oi, mut ni) = (0usize, 0usize);
    let mut is_leading = true;

    loop {
        let old_gap_start = oi;
        while oi < old_pieces.len() && !matches!(old_pieces[oi], ProjectedPiece::Ctx(_)) {
            oi += 1;
        }
        let new_gap_start = ni;
        while ni < new_pieces.len() && !matches!(new_pieces[ni], ProjectedPiece::Ctx(_)) {
            ni += 1;
        }

        let mut old_gap = &old_pieces[old_gap_start..oi];
        let mut new_gap = &new_pieces[new_gap_start..ni];

        if is_leading
            && let Some(ProjectedPiece::Edge {
                kind: RunKind::Del,
                text: d_ws,
                is_leading_ws: true,
                ..
            }) = old_gap.first().copied()
            && let Some(ProjectedPiece::Edge {
                kind: RunKind::Add,
                text: a_ws,
                is_leading_ws: true,
                ..
            }) = new_gap.first().copied()
            && let Some(split) = split_leading_indent(d_ws, a_ws)
        {
            match split {
                LeadingIndentSplit::Same(shared) => {
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, shared, None);
                }
                LeadingIndentSplit::Indented { added, shared } => {
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Add, added, None);
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, shared, None);
                }
                LeadingIndentSplit::Dedented { removed, shared } => {
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Del, removed, None);
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, shared, None);
                }
            }
            old_gap = &old_gap[1..];
            new_gap = &new_gap[1..];
        }

        for &piece in old_gap {
            if let ProjectedPiece::Edge {
                kind,
                text,
                phrase_id,
                ..
            } = piece
            {
                if !text.trim().is_empty() {
                    is_leading = false;
                }
                push_projected_run(&mut out, &mut last_phrase, kind, text, phrase_id);
            }
        }
        for &piece in new_gap {
            if let ProjectedPiece::Edge {
                kind,
                text,
                phrase_id,
                ..
            } = piece
            {
                if !text.trim().is_empty() {
                    is_leading = false;
                }
                push_projected_run(&mut out, &mut last_phrase, kind, text, phrase_id);
            }
        }

        if oi < old_pieces.len() && ni < new_pieces.len() {
            if let ProjectedPiece::Ctx(text) = old_pieces[oi] {
                if !text.trim().is_empty() {
                    is_leading = false;
                }
                push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, text, None);
            }
            oi += 1;
            ni += 1;
        } else {
            break;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{AddLine, DelLine, Row, Spans};

    fn make_del(num: u32, text: &str) -> Row {
        Row::del(DelLine::new(num, text))
    }

    fn make_add(num: u32, text: &str) -> Row {
        Row::add(AddLine::new(num, text))
    }

    fn runs_project_to_text(
        runs: &[(RunKind, String)],
        old_text: Option<&str>,
        new_text: Option<&str>,
    ) -> bool {
        let old_proj: String = runs
            .iter()
            .filter(|(k, _)| !k.is_add_like())
            .map(|(_, t)| t.as_str())
            .collect();
        let new_proj: String = runs
            .iter()
            .filter(|(k, _)| !k.is_del_like())
            .map(|(_, t)| t.as_str())
            .collect();
        match (old_text, new_text) {
            (Some(o), Some(n)) => old_proj == o && new_proj == n,
            (Some(o), None) => old_proj == o,
            (None, Some(n)) => new_proj == n,
            (None, None) => true,
        }
    }

    fn assert_2d_conservation_and_projection(slots: &[WindowSlot<'_>], slot_runs: &[Runs]) {
        assert_eq!(slots.len(), slot_runs.len());
        let mut src: Vec<&str> = Vec::new();
        let mut tgt: Vec<&str> = Vec::new();
        for (&slot, runs) in slots.iter().zip(slot_runs) {
            assert!(
                runs_project_to_text(
                    runs,
                    slot.old_row().map(Row::text),
                    slot.new_row().map(Row::text)
                ),
                "slot projection mismatch: old={:?}, new={:?}, runs={:?}",
                slot.old_row().map(Row::text),
                slot.new_row().map(Row::text),
                runs
            );
            for (k, t) in runs {
                match k {
                    RunKind::MovedSrc => src.push(t.as_str()),
                    RunKind::MovedTgt => tgt.push(t.as_str()),
                    _ => {}
                }
            }
        }
        src.sort_unstable();
        tgt.sort_unstable();
        assert_eq!(src, tgt, "closed-economy conservation violated");
    }

    #[test]
    fn test_align_window_2d_tape_1x4_mod_at_slot_1_assert_eq_frame() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let old = make_del(
            134,
            "    assert_eq!(super::resolve_theme(Some(\"dark\")), DiffuseTheme::dark());",
        );
        let new0 = make_add(134, "    assert_eq!(");
        let new1 = make_add(135, "        super::resolve_theme(Some(\"dark\"), false),");
        let new2 = make_add(136, "        DiffuseTheme::dark()");
        let new3 = make_add(137, "    );");
        let slots = vec![
            WindowSlot::Add(&new0),
            WindowSlot::Mod(&old, &new1),
            WindowSlot::Add(&new2),
            WindowSlot::Add(&new3),
        ];

        let runs = align_window_2d_tape(&slots, rs).expect("2D tape succeeds");
        assert_2d_conservation_and_projection(&slots, &runs);

        // Slot 0 receives `assert_eq!(` as MovedTgt, Slot 2 receives
        // `DiffuseTheme::dark()`, Slot 3 receives `);`.
        assert!(
            runs[0]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t == "assert_eq!(")
        );
        assert!(
            runs[2]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t == "DiffuseTheme::dark()")
        );
        assert!(
            runs[3]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t == ");")
        );
        // Slot 1 Mod keeps `super::resolve_theme(Some("dark")` as Ctx and
        // `, false` as Add with leading-indent split (`Add("    ")`, `Ctx("
        // ")`).
        assert!(
            runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::Ctx && t.contains("resolve_theme"))
        );
        assert!(
            runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::Add && t.contains("false"))
        );
        assert_eq!(runs[1][0], (RunKind::Add, "    ".to_string()));
        assert_eq!(runs[1][1], (RunKind::Ctx, "    ".to_string()));
    }

    #[test]
    fn test_align_window_2d_tape_5x1_collapse_mod_at_slot_2_with_interior_ws_bridge() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let o0 = make_del(42, "#[serde(");
        let o1 = make_del(43, "    tag = \"kind\",");
        let o2 = make_del(44, "    rename_all = \"camelCase\",");
        let o3 = make_del(45, ")]");
        let n0 = make_add(97, "#[serde(tag = \"kind\", rename_all = \"camelCase\")]");
        let slots = vec![
            WindowSlot::Del(&o0),
            WindowSlot::Del(&o1),
            WindowSlot::Mod(&o2, &n0),
            WindowSlot::Del(&o3),
        ];

        let runs = align_window_2d_tape(&slots, rs).expect("2D collapse succeeds");
        assert_2d_conservation_and_projection(&slots, &runs);

        // Interior spaces inside `tag = "kind",` bridge into one clean moved
        // phrase rather than splitting into red/green space flashes.
        assert!(
            runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t == "tag = \"kind\","),
            "expected bridged phrase on slot 1, got: {:?}",
            runs[1]
        );
        assert!(
            runs[0]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t == "#[serde("),
            "expected moved opener on slot 0, got: {:?}",
            runs[0]
        );
        assert!(
            runs[3]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t == ")]"),
            "expected moved closer on slot 3, got: {:?}",
            runs[3]
        );
    }

    #[test]
    fn test_align_window_2d_tape_general_nxm_and_pre_moved_and_fallbacks() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // General 2x3 monotone reflow.
        let o0 = make_del(10, "        let alpha = compute(first, second);");
        let o1 = make_del(11, "        let beta = finish(alpha);");
        let n0 = make_add(10, "    let alpha = compute(");
        let n1 = make_add(11, "        first, second");
        let n2 = make_add(12, "    );");
        let n3 = make_add(13, "    let beta = finish(alpha);");
        let slots = vec![
            WindowSlot::Mod(&o0, &n0),
            WindowSlot::Add(&n1),
            WindowSlot::Add(&n2),
            WindowSlot::Mod(&o1, &n3),
        ];
        let runs = align_window_2d_tape(&slots, rs).expect("2x4 2D alignment succeeds");
        assert_2d_conservation_and_projection(&slots, &runs);
        // Slot 3 has dedented leading spaces (`Del("    ")` followed by
        // coalesced `Ctx("    let beta = finish(alpha);")`).
        assert_eq!(runs[3][0], (RunKind::Del, "    ".to_string()));
        assert_eq!(
            runs[3][1],
            (RunKind::Ctx, "    let beta = finish(alpha);".to_string())
        );
        // Slot 1 receives `first, second` as MovedTgt from slot 0.
        assert!(
            runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t == "first, second"),
            "expected bridged moved args on slot 1, got: {:?}",
            runs[1]
        );

        // Single-sided window returns None.
        let del_only = vec![WindowSlot::Del(&o0)];
        assert!(align_window_2d_tape(&del_only, rs).is_none());

        // Wholesale Mod slot triggers clean None fallback.
        let w_old = make_del(1, "    alpha_one");
        let w_new = make_add(1, "    beta_two");
        let wholesale_slots = vec![WindowSlot::Mod(&w_old, &w_new)];
        assert!(align_window_2d_tape(&wholesale_slots, rs).is_none());

        // Pre-moved slot retains cached_runs untouched alongside normal slots.
        let mov_src = Row::MovSrc {
            line: DelLine::new(20, "    moved_line();"),
            spans: Spans::default(),
            cached_runs: vec![(RunKind::Ctx, "    moved_line();".to_string())],
            group: None,
        };
        let with_moved = vec![WindowSlot::Mod(&o0, &n0), WindowSlot::MovSrc(&mov_src)];
        let moved_runs = align_window_2d_tape(&with_moved, rs).expect("preserves MovSrc slot");
        assert_eq!(
            moved_runs[1],
            vec![(RunKind::Ctx, "    moved_line();".to_string())]
        );
    }
}
