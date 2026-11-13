//! One-for-one port of `review.ts`'s `markMoves` (PLAN.md §3) — the
//! exact-content relocation marking plus the block-move rule. Comments
//! ported verbatim. The TS mutates rows via references; the port collects
//! the decisions first (row positions), then applies them.

use std::mem;

use diffuse_algo::bipartite_matching::{BipartiteMatcher, HunkGridNeighborhood};

use crate::{
    lcs::word_spans_and_runs_with_profile,
    parse::{DiffFile, Row, Spans},
};

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
) {
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
    mark_moves_internal(f, profile, can_match, false, true);
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
) {
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
    mark_moves_internal(f, profile, is_cross_cluster, true, false);
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
) where
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
        return;
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
            return;
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
}

#[cfg(test)]
mod tests;
