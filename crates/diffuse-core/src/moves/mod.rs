//! One-for-one port of `review.ts`'s `markMoves` (PLAN.md §3) — the
//! exact-content relocation marking plus the block-move rule. Comments
//! ported verbatim. The TS mutates rows via references; the port collects
//! the decisions first (row positions), then applies them.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::parse::{DiffFile, Row};

type RowPos = (usize, usize);

/// Mark exact-content del/add pairs as *moved*: the same line at a new
/// position (e.g. renumbered sections) — rendered muted and excluded from
/// the change counts. Matching is greedy per file (a multiset of add texts).
#[tracing::instrument(level = "trace", skip(f), fields(path = %f.path, hunk_count = f.hunks.len()))]
pub fn mark_moves(f: &mut DiffFile) {
    // Leading-indent-insensitive: a line reflowed to a different indentation
    // is still a move, not a change. Trailing whitespace is NOT stripped —
    // trimming the right side would cause trailing-space cleanup hunks (like
    // `/// ` → `///`) to match across hunks, producing false-positive ≈ moves.
    fn key(t: &str) -> &str {
        t.trim_start()
    }

    let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
    let mut add_by_key: FxHashMap<&str, Vec<RowPos>> = FxHashMap::default();
    let mut add_texts: Vec<&str> = Vec::new();

    for (hi, h) in f.hunks.iter().enumerate() {
        for (ri, r) in h.rows.iter().enumerate() {
            if let Row::Add(add_line) = r {
                let k = key(&add_line.text);
                *counts.entry(k).or_insert(0) += 1;
                add_by_key.entry(k).or_default().push((hi, ri));
                add_texts.push(&add_line.text);
            }
        }
    }

    if add_texts.is_empty() {
        return;
    }

    // Next unused index in add_by_key vector for each key
    let mut add_cursor: FxHashMap<&str, usize> = FxHashMap::default();
    let mut decisions: Vec<(RowPos, Option<RowPos>)> = Vec::new();
    let mut used_adds: FxHashSet<RowPos> = FxHashSet::default();

    for (hi, h) in f.hunks.iter().enumerate() {
        for (ri, r) in h.rows.iter().enumerate() {
            let del_line = match r {
                Row::Del(d) => d,
                _ => continue,
            };
            let t = key(&del_line.text);
            let n = counts.get(t).copied().unwrap_or(0);
            if n == 0 {
                continue;
            }

            // A del whose content survives INSIDE a different add (a
            // restructured line — the old `.iter()` of the `let up` chain
            // folded into the one-liner mod) is a reflow the collapse
            // absorbs, not a move: pairing it would mark an unrelated
            // same-text add (the new `below` block's `.iter()`) as relocated.
            let reflow = !t.is_empty()
                && add_texts
                    .iter()
                    .any(|&add_text| key(add_text) != t && add_text.contains(t));
            if reflow {
                continue;
            }

            counts.insert(t, n - 1);

            let cursor = add_cursor.entry(t).or_insert(0);
            let mut paired = None;
            if let Some(positions) = add_by_key.get(t) {
                while *cursor < positions.len() {
                    let pos = positions[*cursor];
                    *cursor += 1;
                    if !used_adds.contains(&pos) {
                        paired = Some(pos);
                        break;
                    }
                }
            }

            if let Some(pos) = paired {
                used_adds.insert(pos);
                decisions.push(((hi, ri), Some(pos)));
            } else {
                decisions.push(((hi, ri), None));
            }
        }
    }

    // Apply: convert paired adds into Row::Mov.
    let mut pair_of: FxHashMap<RowPos, RowPos> = FxHashMap::default(); // del -> paired add
    let mut moved_dels: FxHashSet<RowPos> = FxHashSet::default();
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
            if let Row::Del(d) = &mut f.hunks[hi].rows[ri] {
                d.moved = true;
            }
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
    // Un-mark a moved del's paired add (revert back to Row::Add) and reset
    // del.moved.
    for pos in unmark_positions {
        let (hi, ri) = pos;
        if let Row::Del(d) = &mut f.hunks[hi].rows[ri] {
            d.moved = false;
        }
        if let Some(&(ah, ar)) = pair_of.get(&pos)
            && let Row::Mov { new_line, .. } = &f.hunks[ah].rows[ar]
        {
            f.hunks[ah].rows[ar] = Row::Add(new_line.clone());
        }
    }

    crate::ensure!(
        f.hunks.iter().flat_map(|h| &h.rows).all(|r| {
            if let Row::Mov { old_line, new_line } = r {
                key(&old_line.text) == key(&new_line.text)
            } else {
                true
            }
        }),
        "Every Row::Mov must have matching trimmed text on old and new sides"
    );
}

#[cfg(test)]
mod tests;
