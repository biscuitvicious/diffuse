//! Move marking and tail absorption for the diff analysis pipeline.
//!
//! This module implements two complementary passes:
//!
//! 1. **Move marking** (`mark_moves_with_profile`,
//!    `mark_cross_cluster_moves_with_profile`): identifies del/add pairs that
//!    represent relocations of identical (or uniformly re-indented) code and
//!    converts them to `Row::MovSrc`/`Row::MovTgt`. A bipartite matching
//!    heuristic (salient-aware cluster gating, contiguous-block enforcement)
//!    guards against false-positive relocation claims.
//!
//! 2. **Tail absorption** (`absorb_mov_tails`): after move pairs are locked,
//!    extends each move block's tail by absorbing subsequent rows that continue
//!    the block at a uniform indent delta. This reclaims trailing lines that
//!    the initial diff mis-assigned as `Ctx`/`Add` leftovers, stopping at the
//!    first genuine edit, delta change, `Mod` row, or already-moved row.
//!
//! The two passes run in sequence from `pipeline/file.rs`, with an additional
//! pre-collapse run of the cross-cluster shield to protect relocated blocks
//! from being consumed by intra-cluster hunk DP.

use std::mem;

use diffuse_algo::bipartite_matching::{BipartiteMatcher, HunkGridNeighborhood};

use crate::{
    lcs::word_spans_and_runs_with_profile,
    parse::{AddLine, CtxLine, DelLine, DiffFile, Row, Spans},
};

/// A locked move pairing: `(source, target)` row positions, each a
/// `(hunk index, row index)` pair. Returned by the marking passes and
/// consumed by [`absorb_mov_tails`].
pub type MovePair = ((usize, usize), (usize, usize));

/// Re-resolves locked move pairs by line number after an index-shifting
/// pass (hunk collapse merges `Del`/`Add` rows into `Mod`, so recorded
/// positions go stale). Line numbers are stable identifiers: each old/new
/// line occupies exactly one row, so the current `(source, target)`
/// positions are recovered unambiguously. Pairs whose rows are no longer
/// `MovSrc`/`MovTgt` are dropped.
pub fn resolve_mov_pairs_by_line(f: &DiffFile, lines: &[(u32, u32)]) -> Vec<MovePair> {
    let mut resolved = Vec::with_capacity(lines.len());
    for &(old_line, new_line) in lines {
        let mut src_pos = None;
        let mut tgt_pos = None;
        for (hi, hunk) in f.hunks.iter().enumerate() {
            for (ri, row) in hunk.rows.iter().enumerate() {
                if src_pos.is_none()
                    && matches!(row, Row::MovSrc { .. })
                    && row.old_line() == Some(old_line)
                {
                    src_pos = Some((hi, ri));
                }
                if tgt_pos.is_none()
                    && matches!(row, Row::MovTgt { .. })
                    && row.new_line() == Some(new_line)
                {
                    tgt_pos = Some((hi, ri));
                }
            }
        }
        if let (Some(s), Some(t)) = (src_pos, tgt_pos) {
            resolved.push((s, t));
        }
    }
    resolved
}

/// Mark exact-content del/add pairs as *moved*: the same line at a new
/// position (e.g. renumbered sections) — rendered muted and excluded from
/// the change counts. Matching is greedy per file (a multiset of add texts).
///
/// After marking, the source hunk gets `Row::MovSrc(del_line)` (its
/// deletion is absorbed in the counts), while the target hunk gets
/// `Row::MovTgt(add_line)`.
#[tracing::instrument(level = "trace", skip(f), fields(path = %f.path, hunk_count = f.hunks.len()))]
pub fn mark_moves(f: &mut DiffFile) {
    mark_moves_with_profile(f, None);
}

/// Mark exact-content del/add pairs as *moved*, using language profile syntax
/// boundaries to filter coincidental delimiter moves.
#[tracing::instrument(
    level = "debug",
    skip(f, profile),
    fields(path = %f.path, hunk_count = f.hunks.len())
)]
pub fn mark_moves_with_profile(
    f: &mut DiffFile,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) -> Vec<MovePair> {
    // Salient-aware cluster boundaries: a context run only separates clusters
    // when it contains substantive code or spans at least 3 lines. Trivial
    // delimiter context (like `};` or blank lines) keeps adjacent change groups
    // in the same cluster. Trimmed-only matches (indentation reflows) are only
    // accepted when the source and target are in different clusters, preventing
    // nearby indentation changes from being misclassified as relocations. Exact-
    // text matches are always allowed regardless of cluster membership (see the
    // bypass inside `mark_moves_internal`'s `full_can_match`).
    let row_clusters: Vec<Vec<usize>> = f
        .hunks
        .iter()
        .map(|h| hunk_row_clusters_salient(h, profile))
        .collect();
    let can_match = move |src: &(usize, usize), tgt: &(usize, usize)| -> bool {
        src.0 != tgt.0 || row_clusters[src.0][src.1] != row_clusters[tgt.0][tgt.1]
    };
    mark_moves_internal(f, profile, can_match, false, true)
}

/// Pre-pass: marks only strong cross-cluster contiguous moved blocks prior to
/// hunk collapse, preventing intra-cluster DP from stealing relocated lines
/// into spurious Mod rows.
#[tracing::instrument(
    level = "debug",
    skip(f, profile),
    fields(path = %f.path, hunk_count = f.hunks.len())
)]
pub fn mark_cross_cluster_moves_with_profile(
    f: &mut DiffFile,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) -> Vec<MovePair> {
    // Deliberately syntax-blind clusters (ANY context row splits): the
    // pre-pass is a shield, not a philosopher. It runs before hunk collapse,
    // and anything it does not lock in is exposed to collapse, which pairs
    // leftovers by content similarity — producing spurious Mods like a
    // reindented line paired with a content-similar but unrelated line.
    // A too-narrow (salient-aware) shield lets such pairs through to be
    // stolen; a locked Mov is muted and excluded from counts, while a
    // spurious Mod actively misclaims one line became another. The main pass
    // below still applies the precise salient-aware rule to leftovers, and
    // the exact-text bypass stays scoped to it (`allow_exact_bypass`).
    let row_clusters: Vec<Vec<usize>> = f.hunks.iter().map(hunk_row_clusters).collect();
    let is_cross_cluster = |src: &(usize, usize), tgt: &(usize, usize)| -> bool {
        src.0 != tgt.0 || row_clusters[src.0][src.1] != row_clusters[tgt.0][tgt.1]
    };
    mark_moves_internal(f, profile, is_cross_cluster, true, false)
}

/// Assigns a cluster ID to each row in a hunk. Any context row is treated as a
/// cluster boundary: consecutive Del/Add rows with no context between them
/// share the same cluster ID.
fn hunk_row_clusters(h: &crate::parse::Hunk) -> Vec<usize> {
    let mut clusters = Vec::with_capacity(h.rows.len());
    let mut cluster_id = 0;
    let mut prev_was_change = false;
    for r in &h.rows {
        if matches!(r, Row::Ctx(_)) {
            if prev_was_change {
                cluster_id += 1;
                prev_was_change = false;
            }
            clusters.push(usize::MAX);
        } else {
            prev_was_change = true;
            clusters.push(cluster_id);
        }
    }
    clusters
}

/// Assigns a cluster ID to each row in a hunk using salient-aware boundaries.
/// A context run only establishes a cluster boundary when it contains
/// substantive (salient) code or spans at least 3 lines. Trivial delimiter
/// context (like `};` or blank lines) keeps adjacent change groups in the
/// same cluster, preventing incidental indentation-reflow from being
/// misclassified as a cross-cluster relocation.
fn hunk_row_clusters_salient(
    h: &crate::parse::Hunk,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) -> Vec<usize> {
    let mut clusters = Vec::with_capacity(h.rows.len());
    let mut cluster_id = 0;
    let mut ctx_run: Vec<&Row> = Vec::new();
    for r in &h.rows {
        if matches!(r, Row::Ctx(_)) {
            ctx_run.push(r);
            clusters.push(usize::MAX);
        } else {
            if !ctx_run.is_empty() {
                let is_boundary = ctx_run.len() >= 3
                    || ctx_run
                        .iter()
                        .any(|cr| is_salient_move_line(cr.text(), profile));
                if is_boundary {
                    cluster_id += 1;
                }
                ctx_run.clear();
            }
            clusters.push(cluster_id);
        }
    }
    clusters
}

fn is_salient_move_line(
    text: &str,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if let Some(p) = profile {
        if p.classify_exact_close(trimmed) != diffuse_syntax::profile::CloseDelimiter::None
            || p.is_expression_open(trimmed)
            || p.is_block_close(trimmed)
        {
            return false;
        }
    } else {
        if diffuse_syntax::profile::CloseDelimiter::default_classify_exact(trimmed)
            != diffuse_syntax::profile::CloseDelimiter::None
            || trimmed == "{"
            || trimmed == "("
            || trimmed == "["
        {
            return false;
        }
    }

    let s = trimmed.trim_end_matches(';').trim();
    if matches!(
        s,
        "break"
            | "continue"
            | "return"
            | "else"
            | "else {"
            | "} else"
            | "} else {"
            | "do"
            | "do {"
            | "{"
            | "}"
            | "};"
    ) {
        return false;
    }

    crate::lcs::line_has_salient_token(trimmed, profile)
}

fn mark_moves_internal<F>(
    f: &mut DiffFile,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
    can_match: F,
    require_contiguous_group: bool,
    allow_exact_bypass: bool,
) -> Vec<MovePair>
where
    F: Fn(&(usize, usize), &(usize, usize)) -> bool,
{
    // Leading-indent-insensitive: a line reflowed to a different indentation
    // is still a move, not a change. Trailing whitespace is NOT stripped —
    // trimming the right side would cause trailing-space cleanup hunks (like
    // `/// ` → `///`) to match across hunks, producing false-positive ≈ moves.
    fn key(t: &str) -> &str {
        t.trim_start()
    }

    let mut matcher = BipartiteMatcher::new();

    for (hi, h) in f.hunks.iter().enumerate() {
        for (ri, r) in h.rows.iter().enumerate() {
            if let Row::Add(add_line) = r {
                let k = key(&add_line.text);
                if !k.is_empty() && is_salient_move_line(&add_line.text, profile) {
                    matcher.add_target(k, (hi, ri));
                }
            }
        }
    }

    if matcher.targets().is_empty() {
        return Vec::new();
    }

    let mut candidate_sources = Vec::new();
    let mut runs = Vec::new();

    for (hi, h) in f.hunks.iter().enumerate() {
        let mut run = Vec::new();
        for (ri, r) in h.rows.iter().enumerate() {
            if let Row::Del(del_line) = r {
                let k = key(&del_line.text);
                if !k.is_empty() && is_salient_move_line(&del_line.text, profile) {
                    matcher.add_source(k, (hi, ri));
                } else {
                    candidate_sources.push((hi, ri));
                }
                run.push((hi, ri));
            } else if !run.is_empty() {
                runs.push(mem::take(&mut run));
            }
        }
        if !run.is_empty() {
            runs.push(run);
        }
    }

    if require_contiguous_group {
        runs.retain(|r| r.len() >= 2);
        if runs.is_empty() {
            return Vec::new();
        }
    }

    let hunk_lens: Vec<usize> = f.hunks.iter().map(|h| h.rows.len()).collect();
    let grid = HunkGridNeighborhood::new(&hunk_lens);

    let is_trivial_line =
        |pos: &(usize, usize)| !is_salient_move_line(f.hunks[pos.0].rows[pos.1].text(), profile);

    let full_can_match = |src: &(usize, usize), tgt: &(usize, usize)| -> bool {
        let src_row = &f.hunks[src.0].rows[src.1];
        let tgt_row = &f.hunks[tgt.0].rows[tgt.1];
        // Exact-text matches bypass cluster gating (main pass only):
        // byte-identical lines are relocations regardless of cluster
        // membership. The cross-cluster pre-pass sets `allow_exact_bypass`
        // to false so it keeps its cross-cluster-only contract.
        if allow_exact_bypass && src_row.text() == tgt_row.text() {
            return true;
        }
        key(src_row.text()) == key(tgt_row.text()) && can_match(src, tgt)
    };

    let relocation_matches = matcher.solve_filtered(
        &candidate_sources,
        &grid,
        &grid,
        |pos| matches!(&f.hunks[pos.0].rows[pos.1], Row::Del(_)),
        |pos| matches!(&f.hunks[pos.0].rows[pos.1], Row::Add(_)),
        runs.clone(),
        is_trivial_line,
        full_can_match,
    );

    let mut matches_vec: Vec<_> = relocation_matches.into_iter().collect();

    if require_contiguous_group {
        // Enforce that both source AND target lines form a contiguous block
        // of >= 2 lines in the same target hunk, containing at least one salient line.
        let mut target_of: rustc_hash::FxHashMap<(usize, usize), (usize, usize)> =
            rustc_hash::FxHashMap::default();
        for m in &matches_vec {
            target_of.insert(m.source_pos, m.target_pos);
        }

        let mut accepted_sources: rustc_hash::FxHashSet<(usize, usize)> =
            rustc_hash::FxHashSet::default();
        for run in &runs {
            let mut i = 0;
            while i < run.len() {
                if let Some(&tgt_start) = target_of.get(&run[i]) {
                    let mut j = i + 1;
                    while j < run.len() {
                        if let Some(&tgt_next) = target_of.get(&run[j]) {
                            let expected_tgt = (tgt_start.0, tgt_start.1 + (j - i));
                            if tgt_next == expected_tgt {
                                j += 1;
                                continue;
                            }
                        }
                        break;
                    }
                    let has_salient = run[i..j].iter().any(|pos| {
                        is_salient_move_line(f.hunks[pos.0].rows[pos.1].text(), profile)
                    });
                    if j - i >= 2 && has_salient {
                        accepted_sources.extend(run[i..j].iter().copied());
                    }
                    i = j;
                } else {
                    i += 1;
                }
            }
        }
        matches_vec.retain(|m| accepted_sources.contains(&m.source_pos));
    }

    // Applied pairs are returned so a later tail-absorption pass can extend
    // locked blocks. Skipped rows (no longer Del/Add, e.g. consumed by an
    // earlier pass) are not reported.
    let mut applied = Vec::new();
    let mut move_count = 0;
    for m in matches_vec {
        let (hi, ri) = m.source_pos;
        let (ah, ar) = m.target_pos;
        let del_line = match &f.hunks[hi].rows[ri] {
            Row::Del(d) => d.clone(),
            _ => continue,
        };
        let add_line = match &f.hunks[ah].rows[ar] {
            Row::Add(a) => a.clone(),
            _ => continue,
        };
        let (spans, cached_runs) = if del_line.text != add_line.text {
            word_spans_and_runs_with_profile(&del_line.text, &add_line.text, profile)
        } else {
            (Spans::default(), Vec::new())
        };
        f.hunks[ah].rows[ar] = Row::MovTgt {
            line: add_line,
            spans: spans.clone(),
            cached_runs: cached_runs.clone(),
        };
        f.hunks[hi].rows[ri] = Row::MovSrc {
            line: del_line,
            spans,
            cached_runs,
        };
        move_count += 1;
        applied.push((m.source_pos, m.target_pos));
    }

    tracing::debug!(moves_count = move_count, "Moves marked");

    crate::ensure!(
        f.hunks.iter().flat_map(|h| &h.rows).all(|r| {
            match r {
                Row::MovTgt { line, .. } => r.text() == line.text,
                Row::MovSrc { line, .. } => r.text() == line.text,
                _ => true,
            }
        }),
        "Every Row::MovTgt and Row::MovSrc must have matching line text"
    );
    applied
}

/// Leading-whitespace width of a line, in characters. Used for indent deltas.
fn indent_width(text: &str) -> usize {
    text.chars().take_while(|c| c.is_whitespace()).count()
}

/// Indent delta of a locked move pair: new-side indent minus old-side indent.
/// Returns `None` when the pair is not a rigid translation: trimmed texts
/// differ, or either side is blank (blank lines carry no delta).
fn pair_delta(old_text: &str, new_text: &str) -> Option<isize> {
    if old_text.trim() != new_text.trim() {
        return None;
    }
    if old_text.trim().is_empty() {
        return None;
    }
    Some(indent_width(new_text) as isize - indent_width(old_text) as isize)
}

/// Whether an (old, new) trailing pair continues a tail run at the given
/// delta: exact equality after subtracting the delta from the new side.
/// Comparing `trim_start` slices (rather than trimmed text) means trailing
/// whitespace differences also break the match. A blank/blank pair matches
/// by position alone and is delta-transparent.
fn tail_pair_matches(old_text: &str, new_text: &str, delta: isize) -> bool {
    if old_text.trim().is_empty() && new_text.trim().is_empty() {
        return true;
    }
    if old_text.trim_start() != new_text.trim_start() {
        return false;
    }
    indent_width(new_text) as isize - indent_width(old_text) as isize == delta
}

/// Action recorded for one trailing row claimed by tail absorption. Each
/// variant carries the *counterpart* text (the other side of the absorbed
/// pair), needed to compute word spans when the indent differs. The row's
/// own line is taken from the drained row at apply time.
#[derive(Debug)]
enum TailAction {
    /// Convert a `Del` row into `MovSrc`.
    ClaimSrc { add_text: String },
    /// Convert an `Add` row into `MovTgt`.
    ClaimTgt { del_text: String },
    /// Break a `Ctx` row on the old side: absorb its old line as `MovSrc`
    /// and orphan its new line as a plain `Add` leftover emitted after.
    BreakSrc { add_text: String },
    /// Break a `Ctx` row on the new side: absorb its new line as `MovTgt`
    /// and orphan its old line as a plain `Del` leftover emitted after.
    BreakTgt { del_text: String },
}

/// Absorbs uniform-Δ trailing runs into locked move blocks ("Mov-Ctx").
///
/// Starting from each locked move block's tail, pairs successive old/new
/// trailing rows while [`tail_pair_matches`] holds at the block's delta,
/// converting them to `MovSrc`/`MovTgt` (breaking `Ctx` pairs where the
/// tail was misassigned, orphaning the far partners as plain leftovers)
/// and stopping at the first genuine edit, delta change, `Mod` row, or
/// already-moved row. Locked pairs that no longer resolve to
/// `MovSrc`/`MovTgt` (e.g. recorded before an index-shifting pass) are
/// skipped.
///
/// `locked` holds the applied `(source, target)` row positions from the
/// marking pass that ran immediately before, so positions are fresh.
/// `allow_ctx_break` is false for the pre-collapse call (claim free
/// `Del`/`Add` leftovers only, keeping collapse input stable and creating
/// no orphans for collapse to mis-pair) and true for the post-collapse
/// call (also break misassigned `Ctx` rows).
///
/// The outcome of one [`absorb_mov_tails`] call. Line numbers (not row
/// positions) are reported so a later stage can re-resolve absorbed pairs
/// after an index-shifting pass via [`resolve_mov_pairs_by_line`], and
/// relocate the Ctx-break orphans via [`relocate_tail_orphans`].
#[derive(Debug, Default)]
pub struct TailAbsorption {
    /// Absorbed `(old line, new line)` pairs.
    pub absorbed: Vec<(u32, u32)>,
    /// Old lines orphaned by breaking a `Ctx` row on the new side.
    pub del_orphans: Vec<u32>,
    /// New lines orphaned by breaking a `Ctx` row on the old side.
    pub add_orphans: Vec<u32>,
}

/// Absorbs uniform-Δ trailing runs into locked move blocks ("Mov-Ctx").
///
/// Starting from each locked move block's tail, pairs successive old/new
/// trailing rows while [`tail_pair_matches`] holds at the block's delta,
/// converting them to `MovSrc`/`MovTgt` (breaking `Ctx` pairs where the
/// tail was misassigned, orphaning the far partners as plain leftovers)
/// and stopping at the first genuine edit, delta change, `Mod` row, or
/// already-moved row. Locked pairs that no longer resolve to
/// `MovSrc`/`MovTgt` (e.g. recorded before an index-shifting pass) are
/// skipped.
///
/// `locked` holds the applied `(source, target)` row positions from the
/// marking pass that ran immediately before, so positions are fresh.
/// `allow_ctx_break` is false for the pre-collapse call (claim free
/// `Del`/`Add` leftovers only, keeping collapse input stable and creating
/// no orphans for collapse to mis-pair) and true for the post-collapse
/// call (also break misassigned `Ctx` rows).
pub fn absorb_mov_tails(
    f: &mut DiffFile,
    locked: &[MovePair],
    profile: Option<&diffuse_syntax::profile::LangProfile>,
    allow_ctx_break: bool,
) -> TailAbsorption {
    use rustc_hash::{FxHashMap, FxHashSet};

    // Validate locked pairs against current rows: only pairs that still
    // resolve to MovSrc/MovTgt seed absorption.
    let mut tgt_of: FxHashMap<(usize, usize), (usize, usize)> = FxHashMap::default();
    for &(s, t) in locked {
        let src_ok = matches!(
            f.hunks.get(s.0).and_then(|h| h.rows.get(s.1)),
            Some(Row::MovSrc { .. })
        );
        let tgt_ok = matches!(
            f.hunks.get(t.0).and_then(|h| h.rows.get(t.1)),
            Some(Row::MovTgt { .. })
        );
        if src_ok && tgt_ok {
            tgt_of.insert(s, t);
        }
    }
    if tgt_of.is_empty() {
        return TailAbsorption::default();
    }

    // Plan (read-only): one action per claimed row. `claimed` keeps runs
    // disjoint, so orphans and other blocks are never re-absorbed.
    // `absorbed_lines` collects the absorbed `(old line, new line)` pairs
    // for the caller to thread through index-shifting passes.
    let mut plan = PlanState {
        actions: FxHashMap::default(),
        claimed: FxHashSet::default(),
        absorbed_lines: Vec::new(),
    };
    for hi in 0..f.hunks.len() {
        let len = f.hunks[hi].rows.len();
        let mut run_start: Option<usize> = None;
        let mut ri = 0;
        while ri <= len {
            let is_src = ri < len && matches!(f.hunks[hi].rows[ri], Row::MovSrc { .. });
            if is_src && run_start.is_none() {
                run_start = Some(ri);
            }
            if !is_src && let Some(start) = run_start.take() {
                absorb_run_tail(f, hi, start, ri, &tgt_of, allow_ctx_break, &mut plan);
            }
            ri += 1;
        }
    }
    let PlanState {
        mut actions,
        absorbed_lines,
        ..
    } = plan;
    if actions.is_empty() {
        return TailAbsorption {
            absorbed: absorbed_lines,
            ..TailAbsorption::default()
        };
    }

    // Apply: rebuild each touched hunk, expanding broken Ctx rows into
    // [Mov, orphan-leftover] pairs. Planning and apply share the same
    // pre-edit rows, so kinds always match; anything unexpected is kept
    // as-is rather than dropped. Orphan line numbers are collected inline
    // so the caller can relocate them out of the move runs afterwards.
    let mut absorbed = 0;
    let mut del_orphans = Vec::new();
    let mut add_orphans = Vec::new();
    for (hi, hunk) in f.hunks.iter_mut().enumerate() {
        if !actions.keys().any(|&(h, _)| h == hi) {
            continue;
        }
        let mut rebuilt = Vec::with_capacity(hunk.rows.len() + 4);
        for (ri, row) in hunk.rows.drain(..).enumerate() {
            match actions.remove(&(hi, ri)) {
                None => rebuilt.push(row),
                Some(TailAction::ClaimSrc { add_text }) => match row {
                    Row::Del(del) => {
                        let (spans, cached_runs) = if del.text != add_text {
                            word_spans_and_runs_with_profile(&del.text, &add_text, profile)
                        } else {
                            (Spans::default(), Vec::new())
                        };
                        absorbed += 1;
                        rebuilt.push(Row::MovSrc {
                            line: del,
                            spans,
                            cached_runs,
                        });
                    }
                    other => rebuilt.push(other),
                },
                Some(TailAction::ClaimTgt { del_text }) => match row {
                    Row::Add(add) => {
                        let (spans, cached_runs) = if del_text != add.text {
                            word_spans_and_runs_with_profile(&del_text, &add.text, profile)
                        } else {
                            (Spans::default(), Vec::new())
                        };
                        absorbed += 1;
                        rebuilt.push(Row::MovTgt {
                            line: add,
                            spans,
                            cached_runs,
                        });
                    }
                    other => rebuilt.push(other),
                },
                Some(TailAction::BreakSrc { add_text }) => match row {
                    Row::Ctx(ctx) => {
                        use crate::parse::{AddLine, DelLine};
                        let (spans, cached_runs) = if ctx.text != add_text {
                            word_spans_and_runs_with_profile(&ctx.text, &add_text, profile)
                        } else {
                            (Spans::default(), Vec::new())
                        };
                        absorbed += 1;
                        let orphan_num = ctx.new_pos;
                        add_orphans.push(orphan_num);
                        let text = ctx.text.clone();
                        rebuilt.push(Row::MovSrc {
                            line: DelLine::new(ctx.old_pos, ctx.text),
                            spans,
                            cached_runs,
                        });
                        rebuilt.push(Row::Add(AddLine::new(orphan_num, text)));
                    }
                    other => rebuilt.push(other),
                },
                Some(TailAction::BreakTgt { del_text }) => match row {
                    Row::Ctx(ctx) => {
                        use crate::parse::{AddLine, DelLine};
                        let (spans, cached_runs) = if del_text != ctx.text {
                            word_spans_and_runs_with_profile(&del_text, &ctx.text, profile)
                        } else {
                            (Spans::default(), Vec::new())
                        };
                        absorbed += 1;
                        let orphan_num = ctx.old_pos;
                        del_orphans.push(orphan_num);
                        let text = ctx.text.clone();
                        rebuilt.push(Row::MovTgt {
                            line: AddLine::new(ctx.new_pos, ctx.text),
                            spans,
                            cached_runs,
                        });
                        rebuilt.push(Row::Del(DelLine::new(orphan_num, text)));
                    }
                    other => rebuilt.push(other),
                },
            }
        }
        hunk.rows = rebuilt;
    }
    tracing::debug!(absorbed, "Mov tails absorbed");

    crate::ensure!(
        f.hunks.iter().flat_map(|h| &h.rows).all(|r| {
            match r {
                Row::MovTgt { line, .. } => r.text() == line.text,
                Row::MovSrc { line, .. } => r.text() == line.text,
                _ => true,
            }
        }),
        "Every Row::MovTgt and Row::MovSrc must have matching line text"
    );
    // Without Ctx breaking only free Del/Add leftovers are claimed, so no
    // orphans can exist.
    crate::ensure!(
        allow_ctx_break || (del_orphans.is_empty() && add_orphans.is_empty()),
        "pre-collapse absorption must not orphan lines"
    );
    TailAbsorption {
        absorbed: absorbed_lines,
        del_orphans,
        add_orphans,
    }
}

/// Relocates Ctx-break orphans out of the move runs they were emitted
/// into, restoring change-cluster contiguity (a run of dels followed by
/// a run of adds, instead of alternating single-row micro-clusters).
///
/// Placement is by line contiguity, not block kind: a `Del` unit moves to
/// directly after the row carrying its preceding old line, and an `Add`
/// unit moves to directly before the first row below carrying a greater
/// new line (normally its following new line). Whatever kind those rows
/// are — `Del`, `Ctx`, `MovSrc`, even `Mod` — they are the orphan's file
/// neighbors, so adjacency restores file order. Units are the orphan plus
/// file-adjacent same-kind leftovers, which travel as one block. `Mov`
/// rows never move, so move pairing is unaffected; `Sep` rows (and `Mod`
/// rows, conservatively) are never crossed. New-line monotonicity is
/// preserved by construction on the add side (dels carry no new lines).
/// Del orphans are processed in ascending file order and add orphans in
/// descending file order, so previously placed units become merge targets
/// and relative file order is preserved. Orphans whose line — or whose
/// neighbor line — can no longer be found in the hunk are left in place.
pub fn relocate_tail_orphans(f: &mut DiffFile, del_orphans: &[u32], add_orphans: &[u32]) {
    use rustc_hash::FxHashSet;

    // Del orphans move up in file order; each placed unit becomes the
    // merge target for the next, so relative order is preserved.
    let mut dels: Vec<u32> = del_orphans.to_vec();
    dels.sort_unstable();
    dels.dedup();
    let mut placed: FxHashSet<u32> = FxHashSet::default();
    let mut moved: FxHashSet<u32> = FxHashSet::default();
    while let Some(&seed) = dels.iter().find(|l| !placed.contains(l)) {
        // Mark the seed first: a unit that aborts (boundary, vanished
        // line) still counts as handled, or this loop would spin.
        placed.insert(seed);
        relocate_del_unit(f, seed, &mut placed, &mut moved);
    }

    // Mirror image, bottom-up.
    let mut adds: Vec<u32> = add_orphans.to_vec();
    adds.sort_unstable();
    adds.dedup();
    let mut placed: FxHashSet<u32> = FxHashSet::default();
    let mut moved: FxHashSet<u32> = FxHashSet::default();
    while let Some(&seed) = adds.iter().rev().find(|l| !placed.contains(l)) {
        placed.insert(seed);
        relocate_add_unit(f, seed, &mut placed, &mut moved);
    }
}

/// Relocates one del-orphan unit: the seed orphan plus file-adjacent
/// pre-existing `Del` leftovers chained to it move as one file-ordered
/// block to directly after the seed's preceding old line. The anchor
/// line itself never joins (it stays put as the merge target), nor do
/// already-moved lines.
fn relocate_del_unit(
    f: &mut DiffFile,
    seed: u32,
    placed: &mut rustc_hash::FxHashSet<u32>,
    moved: &mut rustc_hash::FxHashSet<u32>,
) {
    let Some((hi, _ri)) = find_del_row(f, seed) else {
        return;
    };
    // Anchor: the row carrying the seed's preceding old line, whatever
    // kind it is. Without that neighbor row in this hunk there is no
    // line-contiguity target, so the unit stays in place.
    let Some(want) = seed.checked_sub(1) else {
        return;
    };
    let Some(anchor_pos) = f.hunks[hi]
        .rows
        .iter()
        .position(|r| r.old_line() == Some(want))
    else {
        return;
    };
    let anchor = anchor_pos + 1;
    // Expand the unit along file-adjacent Del leftovers; the walk stops
    // at the anchor line itself (it stays put as the merge target) and
    // at any cluster boundary. Chain membership needs no positional
    // bound: only lines adjacent through same-kind leftovers can join,
    // so the unit is always one file-contiguous run.
    let mut unit: Vec<u32> = vec![seed];
    let mut k = 0;
    while k < unit.len() {
        let line = unit[k];
        k += 1;
        let Some(front) = find_del_row_in(&f.hunks[hi], line) else {
            continue;
        };
        for next in [line.checked_sub(1), line.checked_add(1)]
            .into_iter()
            .flatten()
        {
            if unit.contains(&next) || moved.contains(&next) || next == want {
                continue;
            }
            let Some(pos) = find_del_row_in(&f.hunks[hi], next) else {
                continue;
            };
            if rows_between_are_boundary(&f.hunks[hi].rows, front, pos) {
                continue;
            }
            unit.push(next);
        }
    }
    // Never cross a file boundary or a paired Mod row to reach the anchor.
    let member_pos: Vec<usize> = unit
        .iter()
        .filter_map(|&l| find_del_row_in(&f.hunks[hi], l))
        .collect();
    if member_pos.len() != unit.len() {
        return;
    }
    let lo = member_pos.iter().copied().fold(anchor_pos, usize::min);
    let top = member_pos.iter().copied().fold(anchor_pos, usize::max);
    // Only rows strictly between the anchor row and the unit can be
    // crossed; the anchor row itself (any kind) is the target.
    if lo < top
        && f.hunks[hi].rows[lo + 1..top]
            .iter()
            .any(|r| matches!(r, Row::Mod { .. } | Row::Sep(_)))
    {
        return;
    }
    for line in &unit {
        placed.insert(*line);
        moved.insert(*line);
    }
    move_unit_within_hunk(&mut f.hunks[hi].rows, &unit, anchor, true);
}

/// Mirror image of [`relocate_del_unit`]: the add unit moves down to just
/// before the first row below carrying a greater new line — normally the
/// row of its following new line. Rows the unit slides past must sort
/// below all of it, so hunk row order stays monotonic in new-line numbers
/// (rows with no new line — dels, move sources — are always crossable).
/// A `Mod`/`Sep` boundary or the hunk end leaves the unit in place.
fn relocate_add_unit(
    f: &mut DiffFile,
    seed: u32,
    placed: &mut rustc_hash::FxHashSet<u32>,
    moved: &mut rustc_hash::FxHashSet<u32>,
) {
    let Some((hi, _ri)) = find_add_row(f, seed) else {
        return;
    };
    // Unit: the seed plus file-adjacent smaller `Add` leftovers, chaining
    // across the non-add rows (the MovSrc halves of the same broken Ctx
    // run). Larger lines never join: the first greater line below is the
    // anchor, and absorbing it would drag the unit past its own target.
    let mut unit: Vec<u32> = vec![seed];
    let mut k = 0;
    while k < unit.len() {
        let line = unit[k];
        k += 1;
        let Some(front) = find_add_row_in(&f.hunks[hi], line) else {
            continue;
        };
        for next in [line.checked_sub(1), line.checked_add(1)]
            .into_iter()
            .flatten()
        {
            if unit.contains(&next) || moved.contains(&next) || next > seed {
                continue;
            }
            let Some(pos) = find_add_row_in(&f.hunks[hi], next) else {
                continue;
            };
            if rows_between_are_boundary(&f.hunks[hi].rows, front, pos) {
                continue;
            }
            unit.push(next);
        }
    }
    let max_line = unit
        .iter()
        .copied()
        .max()
        .expect("unit always contains the seed");
    let Some(bottom) = unit
        .iter()
        .filter_map(|&l| find_add_row_in(&f.hunks[hi], l))
        .max()
    else {
        return;
    };
    let len = f.hunks[hi].rows.len();
    let mut anchor = None;
    let mut j = bottom;
    while j + 1 < len {
        j += 1;
        match &f.hunks[hi].rows[j] {
            Row::Mod { .. } | Row::Sep(_) => break,
            row if row.new_line().is_some_and(|n| n > max_line) => {
                anchor = Some(j);
                break;
            }
            _ => {}
        }
    }
    let Some(anchor) = anchor else {
        return;
    };
    for line in &unit {
        placed.insert(*line);
        moved.insert(*line);
    }
    move_unit_within_hunk(&mut f.hunks[hi].rows, &unit, anchor, false);
}

/// Whether any `Ctx`/`Mod`/`Sep` row sits strictly between `a` and `b`.
fn rows_between_are_boundary(rows: &[Row], a: usize, b: usize) -> bool {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    rows.get(lo + 1..hi).is_some_and(|between| {
        between
            .iter()
            .any(|r| matches!(r, Row::Ctx(_) | Row::Mod { .. } | Row::Sep(_)))
    })
}

/// Moves a file-ordered unit of same-kind leftover rows to `anchor`, the
/// index the unit's first row must end up at (pre-removal coordinates).
/// `del_lines` selects which row kinds the unit's line numbers refer to —
/// old lines for dels, new lines for adds — since the two number spaces
/// overlap. Moves happen within one hunk, so removals above the anchor
/// shift it down by one each; removals below it never move it. Members
/// always sit below the anchor under the placement rules above, so the
/// shift count is zero in practice; the arithmetic stays general.
fn move_unit_within_hunk(rows: &mut Vec<Row>, unit: &[u32], anchor: usize, del_lines: bool) {
    let mut idx: Vec<usize> = unit
        .iter()
        .filter_map(|&line| {
            rows.iter().position(|r| match r {
                Row::Del(d) => del_lines && d.num == line,
                Row::AuxDel { line: l, group, .. } => del_lines && group.is_none() && l.num == line,
                Row::Add(a) => !del_lines && a.num == line,
                Row::AuxAdd { line: l, group, .. } => {
                    !del_lines && group.is_none() && l.num == line
                }
                _ => false,
            })
        })
        .collect();
    if idx.len() != unit.len() {
        return;
    }
    // Remove descending so earlier indices stay valid; removals below the
    // anchor never shift it, removals above it shift it down by one each.
    idx.sort_unstable_by(|a, b| b.cmp(a));
    let mut removed: Vec<(usize, Row)> = Vec::with_capacity(idx.len());
    for i in idx {
        removed.push((i, rows.remove(i)));
    }
    let shift = removed.iter().filter(|(i, _)| *i < anchor).count();
    let at = (anchor - shift).min(rows.len());
    // Re-emit the unit file-ordered (all members are same-kind rows).
    removed.sort_by_key(|(_, r)| match r {
        Row::Del(d) => d.num,
        Row::AuxDel { line, .. } => line.num,
        Row::Add(a) => a.num,
        Row::AuxAdd { line, .. } => line.num,
        _ => unreachable!("unit members are always same-kind leftover rows"),
    });
    rows.splice(at..at, removed.into_iter().map(|(_, r)| r));
}

/// Position within one hunk of the `Del` row carrying `old_line`.
fn find_del_row_in(hunk: &crate::parse::Hunk, old_line: u32) -> Option<usize> {
    hunk.rows.iter().position(|r| match r {
        Row::Del(d) => d.num == old_line,
        Row::AuxDel { line, group, .. } => group.is_none() && line.num == old_line,
        _ => false,
    })
}

/// Position within one hunk of the `Add` row carrying `new_line`.
fn find_add_row_in(hunk: &crate::parse::Hunk, new_line: u32) -> Option<usize> {
    hunk.rows.iter().position(|r| match r {
        Row::Add(a) => a.num == new_line,
        Row::AuxAdd { line, group, .. } => group.is_none() && line.num == new_line,
        _ => false,
    })
}

/// Position of the `Del` row carrying `old_line`, if it still exists.
fn find_del_row(f: &DiffFile, old_line: u32) -> Option<(usize, usize)> {
    for (hi, h) in f.hunks.iter().enumerate() {
        if let Some(ri) = h.rows.iter().position(|r| match r {
            Row::Del(d) => d.num == old_line,
            Row::AuxDel { line, group, .. } => group.is_none() && line.num == old_line,
            _ => false,
        }) {
            return Some((hi, ri));
        }
    }
    None
}

/// Position of the `Add` row carrying `new_line`, if it still exists.
fn find_add_row(f: &DiffFile, new_line: u32) -> Option<(usize, usize)> {
    for (hi, h) in f.hunks.iter().enumerate() {
        if let Some(ri) = h.rows.iter().position(|r| match r {
            Row::Add(a) => a.num == new_line,
            Row::AuxAdd { line, group, .. } => group.is_none() && line.num == new_line,
            _ => false,
        }) {
            return Some((hi, ri));
        }
    }
    None
}

/// Mutable planning state threaded through [`absorb_run_tail`]. Bundled to
/// keep the helper's argument list within clippy's limit.
struct PlanState {
    /// One pending action per row position claimed by tail absorption.
    actions: rustc_hash::FxHashMap<(usize, usize), TailAction>,
    /// Positions already committed to a plan, preventing double-absorption.
    claimed: rustc_hash::FxHashSet<(usize, usize)>,
    /// Absorbed `(old line, new line)` pairs returned to the caller.
    absorbed_lines: Vec<(u32, u32)>,
}

/// Plans tail absorption for one maximal `MovSrc` run `[start, end)` in
/// hunk `hi`, recording actions into `plan`.
fn absorb_run_tail(
    f: &DiffFile,
    hi: usize,
    start: usize,
    end: usize,
    tgt_of: &rustc_hash::FxHashMap<(usize, usize), (usize, usize)>,
    allow_ctx_break: bool,
    plan: &mut PlanState,
) {
    // Resolve the target run: every member must map to a MovTgt row, all
    // in one hunk, contiguous with stride +1 and in order.
    let mut tgts = Vec::with_capacity(end - start);
    for ri in start..end {
        match tgt_of.get(&(hi, ri)) {
            Some(&t) => tgts.push(t),
            None => return,
        }
    }
    let ah = tgts[0].0;
    for (i, &(th, tr)) in tgts.iter().enumerate() {
        if th != ah || tr != tgts[0].1 + i {
            return;
        }
        if !matches!(
            f.hunks.get(ah).and_then(|h| h.rows.get(tr)),
            Some(Row::MovTgt { .. })
        ) {
            return;
        }
    }

    // Block delta from the last non-blank locked pair. A trimmed-differing
    // (non-blank) pair cannot establish a delta, so the run is skipped;
    // blank/blank pairs are transparent and walked past.
    let mut delta = None;
    for (i, &(_, tr)) in tgts.iter().enumerate().rev() {
        let src_text = f.hunks[hi].rows[start + i].text();
        let tgt_text = f.hunks[ah].rows[tr].text();
        if src_text.trim().is_empty() && tgt_text.trim().is_empty() {
            continue;
        }
        delta = pair_delta(src_text, tgt_text);
        break;
    }
    let delta: isize = match delta {
        Some(d) => d,
        None => return,
    };
    // Extend file-order successors on both sides while the pair matches at
    // the fixed block delta. Line numbers strictly increase, so the loop
    // always terminates.
    let mut s_line = match f.hunks[hi].rows[end - 1].old_line() {
        Some(n) => n,
        None => return,
    };
    let mut t_line = match f.hunks[ah].rows[tgts[tgts.len() - 1].1].new_line() {
        Some(n) => n,
        None => return,
    };
    loop {
        let o_idx = next_row_with_old_line(f, hi, s_line);
        let n_idx = next_row_with_new_line(f, ah, t_line);
        let (o_idx, n_idx) = match (o_idx, n_idx) {
            (Some(o), Some(n)) => (o, n),
            _ => break,
        };
        let o_row = &f.hunks[hi].rows[o_idx];
        let n_row = &f.hunks[ah].rows[n_idx];
        let o_take =
            matches!(o_row, Row::Del(_)) || (allow_ctx_break && matches!(o_row, Row::Ctx(_)));
        let n_take =
            matches!(n_row, Row::Add(_)) || (allow_ctx_break && matches!(n_row, Row::Ctx(_)));
        if !o_take || !n_take {
            break;
        }
        if plan.claimed.contains(&(hi, o_idx)) || plan.claimed.contains(&(ah, n_idx)) {
            break;
        }
        if !tail_pair_matches(o_row.text(), n_row.text(), delta) {
            break;
        }
        let o_action = match o_row {
            Row::Del(_) => TailAction::ClaimSrc {
                add_text: n_row.text().to_owned(),
            },
            Row::Ctx(_) => TailAction::BreakSrc {
                add_text: n_row.text().to_owned(),
            },
            _ => break,
        };
        let n_action = match n_row {
            Row::Add(_) => TailAction::ClaimTgt {
                del_text: o_row.text().to_owned(),
            },
            Row::Ctx(_) => TailAction::BreakTgt {
                del_text: o_row.text().to_owned(),
            },
            _ => break,
        };
        let o_line = match o_row.old_line() {
            Some(n) => n,
            None => break,
        };
        let n_line = match n_row.new_line() {
            Some(n) => n,
            None => break,
        };
        s_line = o_line;
        t_line = n_line;
        plan.absorbed_lines.push((o_line, n_line));
        plan.actions.insert((hi, o_idx), o_action);
        plan.actions.insert((ah, n_idx), n_action);
        plan.claimed.insert((hi, o_idx));
        plan.claimed.insert((ah, n_idx));
    }
}

/// Index of the row in hunk `hi` carrying the smallest old line number
/// greater than `after`, or `None` when the block tail is at the end.
fn next_row_with_old_line(f: &DiffFile, hi: usize, after: u32) -> Option<usize> {
    let mut best: Option<(u32, usize)> = None;
    for (ri, row) in f.hunks[hi].rows.iter().enumerate() {
        if let Some(n) = row.old_line()
            && n > after
            && best.is_none_or(|(bn, _)| n < bn)
        {
            best = Some((n, ri));
        }
    }
    best.map(|(_, ri)| ri)
}

/// Index of the row in hunk `ah` carrying the smallest new line number
/// greater than `after`, or `None` when the block tail is at the end.
fn next_row_with_new_line(f: &DiffFile, ah: usize, after: u32) -> Option<usize> {
    let mut best: Option<(u32, usize)> = None;
    for (ri, row) in f.hunks[ah].rows.iter().enumerate() {
        if let Some(n) = row.new_line()
            && n > after
            && best.is_none_or(|(bn, _)| n < bn)
        {
            best = Some((n, ri));
        }
    }
    best.map(|(_, ri)| ri)
}

/// Collapses touching corresponding move halves into direct `Mod`/`Ctx` rows.
///
/// Tail absorption can grow a move block until its target half ends up
/// immediately adjacent to its source half in row order (touching at the
/// top or the bottom edge). That relocation presentation is degenerate:
/// the lines are paired directly instead. This pass finds adjacent
/// `MovTgt`/`MovSrc` runs (in either order) within one hunk and collapses
/// the mutually-paired lines into `Mod` rows (differing text) or `Ctx`
/// rows (identical text), leaving non-corresponding rows as moves.
///
/// Correspondence comes from the recorded `(old line, new line)` move
/// pairs: a target row and a source row collapse only when each names the
/// other as its partner. From every touching mutually-paired seed the
/// collapse grows through line-adjacent mutual pairs, so with mixed
/// attribution it extends as far into the blocks as the mutual pairing
/// reaches — including pairs whose own runs never touch (tail-absorption
/// fragments one rigid shift across several runs). Growth never leaves
/// the seed hunk: halves split across a hunk boundary never touch in the
/// row stream, so cross-hunk pairs stay moves.
pub fn collapse_touching_moves(
    f: &mut DiffFile,
    pairs: &[(u32, u32)],
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) {
    use rustc_hash::{FxHashMap, FxHashSet};
    let mut old_to_new: FxHashMap<u32, u32> = FxHashMap::default();
    let mut new_to_old: FxHashMap<u32, u32> = FxHashMap::default();
    for &(o, n) in pairs {
        old_to_new.insert(o, n);
        new_to_old.insert(n, o);
    }
    if old_to_new.is_empty() {
        return;
    }
    // Row positions of every move row, for the same-hunk confinement.
    let mut old_pos: FxHashMap<u32, (usize, usize)> = FxHashMap::default();
    let mut new_pos: FxHashMap<u32, (usize, usize)> = FxHashMap::default();
    for (hi, hunk) in f.hunks.iter().enumerate() {
        for (ri, row) in hunk.rows.iter().enumerate() {
            match row {
                Row::MovSrc { line, .. } => {
                    old_pos.insert(line.num, (hi, ri));
                }
                Row::MovTgt { line, .. } => {
                    new_pos.insert(line.num, (hi, ri));
                }
                _ => {}
            }
        }
    }
    // Seed pairs: mutual pairs of row-adjacent (touching) tgt/src runs.
    // Runs touch only when row-adjacent: any intervening row (context,
    // dels, separators) breaks the touch. Each hunk's seeds grow into
    // regions below; seeds already visited belong to a grown region.
    let mut regions: Vec<(usize, Vec<(u32, u32)>)> = Vec::new();
    let mut visited: FxHashSet<(u32, u32)> = FxHashSet::default();
    for (hi, hunk) in f.hunks.iter().enumerate() {
        // Maximal MovTgt / MovSrc runs in row order.
        let mut runs: Vec<(usize, usize, bool)> = Vec::new();
        let mut ri = 0;
        while ri < hunk.rows.len() {
            let is_tgt = matches!(hunk.rows[ri], Row::MovTgt { .. });
            let is_src = !is_tgt && matches!(hunk.rows[ri], Row::MovSrc { .. });
            if !is_tgt && !is_src {
                ri += 1;
                continue;
            }
            let start = ri;
            ri += 1;
            while ri < hunk.rows.len()
                && matches!(
                    (&hunk.rows[ri], is_tgt),
                    (Row::MovTgt { .. }, true) | (Row::MovSrc { .. }, false)
                )
            {
                ri += 1;
            }
            runs.push((start, ri, is_tgt));
        }
        for w in runs.windows(2) {
            let [a, b] = w else { continue };
            let &(a_start, a_end, a_tgt) = a;
            let &(b_start, b_end, b_tgt) = b;
            if a_tgt == b_tgt || a_end != b_start {
                continue;
            }
            let (t_start, t_end, s_start, s_end) = if a_tgt {
                (a_start, a_end, b_start, b_end)
            } else {
                (b_start, b_end, a_start, a_end)
            };
            for ti in t_start..t_end {
                let Some(n) = hunk.rows[ti].new_line() else {
                    continue;
                };
                let Some(&o) = new_to_old.get(&n) else {
                    continue;
                };
                if old_to_new.get(&o) != Some(&n) {
                    continue;
                }
                if (s_start..s_end).any(|si| hunk.rows[si].old_line() == Some(o))
                    && visited.insert((o, n))
                {
                    // Grow the maximal line-adjacent mutual-pair region
                    // around the seed, confined to this hunk.
                    let mut region = vec![(o, n)];
                    let mut stack = vec![(o, n)];
                    while let Some((co, cn)) = stack.pop() {
                        for (do_, dn) in [(-1, -1), (1, 1)] {
                            let (Some(no), Some(nn)) =
                                (co.checked_add_signed(do_), cn.checked_add_signed(dn))
                            else {
                                continue;
                            };
                            if old_to_new.get(&no) != Some(&nn) || new_to_old.get(&nn) != Some(&no)
                            {
                                continue;
                            }
                            if old_pos.get(&no).map(|&(h, _)| h) != Some(hi) {
                                continue;
                            }
                            if new_pos.get(&nn).map(|&(h, _)| h) != Some(hi) {
                                continue;
                            }
                            if visited.insert((no, nn)) {
                                region.push((no, nn));
                                stack.push((no, nn));
                            }
                        }
                    }
                    regions.push((hi, region));
                }
            }
        }
    }
    // Rebuild each touched hunk: consumed Mov rows become Mod/Ctx rows in
    // old-line order, emitted at the first consumed row position.
    let mut by_hunk: FxHashMap<usize, Vec<Vec<(u32, u32)>>> = FxHashMap::default();
    for (hi, region) in regions {
        by_hunk.entry(hi).or_default().push(region);
    }
    for (hi, regs) in by_hunk {
        let hunk = &mut f.hunks[hi];
        // first consumed row index -> Mod/Ctx rows for the region.
        let mut emit_at: FxHashMap<usize, Vec<Row>> = FxHashMap::default();
        let mut consumed: FxHashSet<usize> = FxHashSet::default();
        for mut region in regs {
            region.sort_unstable();
            let mut out = Vec::with_capacity(region.len());
            let mut first = usize::MAX;
            for (o, n) in region {
                let &(oh, si) = old_pos.get(&o).expect("seed pair rows exist");
                let &(nh, ti) = new_pos.get(&n).expect("seed pair rows exist");
                debug_assert_eq!(oh, hi);
                debug_assert_eq!(nh, hi);
                first = first.min(si.min(ti));
                consumed.insert(si);
                consumed.insert(ti);
                let old_text = hunk.rows[si].text().to_owned();
                let new_text = hunk.rows[ti].text().to_owned();
                if old_text == new_text {
                    out.push(Row::Ctx(CtxLine::new(o, n, old_text)));
                } else {
                    let (spans, cached_runs) =
                        word_spans_and_runs_with_profile(&old_text, &new_text, profile);
                    out.push(Row::Mod {
                        old_line: DelLine::new(o, old_text),
                        new_line: AddLine::new(n, new_text),
                        spans,
                        cached_runs,
                        group: None,
                    });
                }
            }
            emit_at.insert(first, out);
        }
        let mut rebuilt = Vec::with_capacity(hunk.rows.len());
        for (idx, row) in hunk.rows.drain(..).enumerate() {
            if let Some(out) = emit_at.remove(&idx) {
                rebuilt.extend(out);
            } else if !consumed.contains(&idx) {
                rebuilt.push(row);
            }
        }
        hunk.rows = rebuilt;
    }
}

#[cfg(test)]
mod tests;
