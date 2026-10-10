//! One-for-one port of `review.ts`'s `markMoves` (PLAN.md §3) — the
//! exact-content relocation marking plus the block-move rule. Comments
//! ported verbatim. The TS mutates rows via references; the port collects
//! the decisions first (row positions), then applies them.

use std::mem;

use diffuse_algo::bipartite_matching::{BipartiteMatcher, HunkGridNeighborhood};

use crate::parse::{AddLine, DiffFile, Row};

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

    let relocation_matches = matcher.solve(
        &candidate_sources,
        &grid,
        &grid,
        |pos| matches!(&f.hunks[pos.0].rows[pos.1], Row::Del(d) if key(&d.text).is_empty()),
        |pos| matches!(&f.hunks[pos.0].rows[pos.1], Row::Add(a) if key(&a.text).is_empty()),
        runs,
        is_trivial_line,
    );

    let mut move_count = 0;
    for m in relocation_matches {
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
        f.hunks[ah].rows[ar] = Row::MovTgt(AddLine::with_donor(
            add_line.num,
            &add_line.text,
            &del_line.text,
            add_line.spans.clone(),
        ));
        f.hunks[hi].rows[ri] = Row::MovSrc(del_line);
        move_count += 1;
    }

    tracing::debug!(moves_count = move_count, "Moves marked");

    crate::ensure!(
        f.hunks.iter().flat_map(|h| &h.rows).all(|r| {
            if let Row::MovTgt(a) = r {
                // Verify any MovTgt with del_text has matching trimmed text.
                if let Some(dt) = a.del_text.as_deref() {
                    key(dt) == key(&a.text)
                } else {
                    true
                }
            } else {
                true
            }
        }),
        "Every Row::MovTgt must have matching trimmed text on old and new sides"
    );
}

#[cfg(test)]
mod tests;
