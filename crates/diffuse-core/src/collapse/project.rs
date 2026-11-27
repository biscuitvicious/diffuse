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
use rustc_hash::FxHashSet;

use crate::{
    collapse::slot::{Runs, WindowSlot},
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
    let extended_pairs = extend_cross_slot_phrases(&aln, &window, &kept_pairs);
    let bridged_pairs = bridge_enclosed_cross_slot_spans(&aln, &window, extended_pairs);
    let (old_matches, new_matches) = assign_phrase_matches(&aln, &window, &bridged_pairs);

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

/// Demotes coincidental bare-punctuation cross-slot matches between unrelated
/// full lines back to `Del`/`Add`, preserving matches inside the same slot,
/// single-construct windows, salient-linked slot pairs, or bare-punctuation
/// lines.
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

    let mut salient_linked: FxHashSet<(usize, usize)> = FxHashSet::default();
    for &(oi, ni) in &aln.pairs {
        let ol = aln.old.tokens[oi].line;
        let nl = aln.new.tokens[ni].line;
        let so = window.slot_of_old[ol];
        let sn = window.slot_of_new[nl];
        if so != sn && line_has_salient_token(aln.old.tokens[oi].text, profile) {
            salient_linked.insert((so, sn));
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
                || is_single_construct
                || salient_linked.contains(&(so, sn))
                || !old_line_has_salient[ol]
                || !new_line_has_salient[nl]
        })
        .collect()
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
