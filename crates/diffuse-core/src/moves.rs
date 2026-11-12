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
    mark_moves_internal(f, profile, |_, _| true, false);
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
    let row_clusters: Vec<Vec<usize>> = f.hunks.iter().map(hunk_row_clusters).collect();
    let is_cross_cluster = |src: &(usize, usize), tgt: &(usize, usize)| -> bool {
        src.0 != tgt.0 || row_clusters[src.0][src.1] != row_clusters[tgt.0][tgt.1]
    };
    mark_moves_internal(f, profile, is_cross_cluster, true);
}

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

fn mark_moves_internal<F>(
    f: &mut DiffFile,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
    can_match: F,
    require_contiguous_group: bool,
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
                if !k.is_empty() {
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
                if k.is_empty() {
                    candidate_sources.push((hi, ri));
                } else {
                    matcher.add_source(k, (hi, ri));
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

    let is_trivial_line = |pos: &(usize, usize)| {
        let text = f.hunks[pos.0].rows[pos.1].text();
        let s = text.trim();
        if s.is_empty() {
            return true;
        }
        if let Some(p) = profile {
            p.classify_exact_close(s) != diffuse_syntax::profile::CloseDelimiter::None
                || p.is_expression_open(s)
                || p.is_block_close(s)
        } else {
            diffuse_syntax::profile::CloseDelimiter::default_classify_exact(s)
                != diffuse_syntax::profile::CloseDelimiter::None
                || s == "{"
                || s == "("
                || s == "["
        }
    };

    let relocation_matches = matcher.solve_filtered(
        &candidate_sources,
        &grid,
        &grid,
        |pos| matches!(&f.hunks[pos.0].rows[pos.1], Row::Del(d) if key(&d.text).is_empty()),
        |pos| matches!(&f.hunks[pos.0].rows[pos.1], Row::Add(a) if key(&a.text).is_empty()),
        runs.clone(),
        is_trivial_line,
        can_match,
    );

    let mut matches_vec: Vec<_> = relocation_matches.into_iter().collect();

    if require_contiguous_group {
        // Enforce that both source AND target lines form a contiguous block
        // of >= 2 lines in the same target hunk.
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
                    if j - i >= 2 {
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
