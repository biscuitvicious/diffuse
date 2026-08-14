//! One-for-one port of `review.ts`'s `markMoves` (PLAN.md §3) — the
//! exact-content relocation marking plus the block-move rule. Comments
//! ported verbatim. The TS mutates rows via references; the port collects
//! the decisions first (row positions), then applies them.

use crate::parse::{DiffFile, Row};
use std::collections::{HashMap, HashSet};

type RowPos = (usize, usize);

/// Mark exact-content del/add pairs as *moved*: the same line at a new
/// position (e.g. renumbered sections) — rendered muted and excluded from
/// the change counts. Matching is greedy per file (a multiset of add texts).
pub fn mark_moves(f: &mut DiffFile) {
    // Leading-indent-insensitive: a line reflowed to a different indentation
    // is still a move, not a change. Trailing whitespace is NOT stripped —
    // trimming the right side would cause trailing-space cleanup hunks (like
    // `/// ` → `///`) to match across hunks, producing false-positive ≈ moves.
    let key = |t: &str| -> String { t.trim_start().to_string() };

    let mut counts: HashMap<String, usize> = HashMap::new();
    for h in &f.hunks {
        for r in &h.rows {
            if let Row::Add(add_line) = r {
                *counts.entry(key(&add_line.text)).or_insert(0) += 1;
            }
        }
    }

    // All add positions in document order — the TS re-scans the hunks per
    // del; precomputing keeps the same "first un-moved add" semantics.
    let mut add_positions: Vec<RowPos> = Vec::new();
    for (hi, h) in f.hunks.iter().enumerate() {
        for (ri, r) in h.rows.iter().enumerate() {
            if matches!(r, Row::Add(_)) {
                add_positions.push((hi, ri));
            }
        }
    }

    // The pairing decisions: (del, paired add, moved). The reflow check and
    // the count consume read-only state; the mutation is applied after.
    let mut decisions: Vec<(RowPos, Option<RowPos>)> = Vec::new();
    let mut used_adds: HashSet<RowPos> = HashSet::new();
    for (hi, h) in f.hunks.iter().enumerate() {
        for (ri, r) in h.rows.iter().enumerate() {
            let del_line = match r {
                Row::Del(d) => d,
                _ => continue,
            };
            let t = key(&del_line.text);
            // A del whose content survives INSIDE a different add (a
            // restructured line — the old `.iter()` of the `let up` chain
            // folded into the one-liner mod) is a reflow the collapse
            // absorbs, not a move: pairing it would mark an unrelated
            // same-text add (the new `below` block's `.iter()`) as relocated.
            let reflow = !t.is_empty()
                && f.hunks.iter().any(|h2| {
                    h2.rows.iter().any(|a| {
                        matches!(a, Row::Add(add_line) if key(&add_line.text) != t && add_line.text.contains(&t))
                    })
                });
            if reflow {
                continue;
            }
            let n = counts.get(&t).copied().unwrap_or(0);
            if n == 0 {
                continue;
            }
            counts.insert(t.clone(), n - 1);
            // The first un-moved add with the same trimmed text, in document
            // order — the TS `outer:` scan.
            let paired = add_positions.iter().copied().find(|&pos| {
                !used_adds.contains(&pos) && {
                    let (ph, pr) = pos;
                    match &f.hunks[ph].rows[pr] {
                        Row::Add(add_line) => key(&add_line.text) == t,
                        _ => false,
                    }
                }
            });
            if let Some(pos) = paired {
                used_adds.insert(pos);
                decisions.push(((hi, ri), Some(pos)));
            } else {
                decisions.push(((hi, ri), None));
            }
        }
    }

    // Apply: convert paired adds into Row::Mov.
    let mut pair_of: HashMap<RowPos, RowPos> = HashMap::new(); // del -> paired add
    let mut moved_dels: HashSet<RowPos> = HashSet::new();
    for (del_pos, add_pos) in decisions {
        let (hi, ri) = del_pos;
        if let Some(pos) = add_pos {
            let (ah, ar) = pos;
            let del_line = match &f.hunks[hi].rows[ri] {
                Row::Del(d) => d.clone(),
                _ => continue,
            };
            let add_line = match &f.hunks[ah].rows[ar] {
                Row::Add(a) => a.clone(),
                Row::Mov { new_line, .. } => new_line.clone(),
                _ => continue,
            };
            f.hunks[ah].rows[ar] = Row::Mov {
                old_line: del_line,
                new_line: add_line,
            };
            pair_of.insert(del_pos, pos);
            moved_dels.insert(del_pos);
        }
    }

    // A del run whose FIRST line did not move is a deleted block, not a
    // relocation — the interior "moved" lines are coincidental same-text
    // matches (the old `find_mount`'s `.iter()/.rfind()/.map()` chain
    // verbatim-matching lines inside the NEW `get` method). The move would
    // render the new method's chain as ≈ inside a clearly-added block —
    // "`self.filesystems` added, the rest moved?" — misleading. The whole
    // deleted block reads as deleted; the new method reads as added.
    let mut unmark_positions: Vec<RowPos> = Vec::new();
    for (hi, h) in f.hunks.iter().enumerate() {
        let mut run: Vec<RowPos> = Vec::new();
        for (ri, r) in h.rows.iter().enumerate() {
            if matches!(r, Row::Del(_)) {
                run.push((hi, ri));
            } else {
                // flush the run
                if !run.is_empty() && !moved_dels.contains(&run[0]) {
                    unmark_positions
                        .extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));
                }
                run.clear();
            }
        }
        if !run.is_empty() && !moved_dels.contains(&run[0]) {
            unmark_positions.extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));
        }
    }
    // Un-mark a moved del's paired add (revert back to Row::Add).
    for pos in unmark_positions {
        if let Some(&(ah, ar)) = pair_of.get(&pos)
            && let Row::Mov { new_line, .. } = &f.hunks[ah].rows[ar]
        {
            f.hunks[ah].rows[ar] = Row::Add(new_line.clone());
        }
    }
}

#[cfg(test)]
mod tests;
