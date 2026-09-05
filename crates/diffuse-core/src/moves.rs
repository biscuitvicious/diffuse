//! One-for-one port of `review.ts`'s `markMoves` (PLAN.md §3) — the
//! exact-content relocation marking plus the block-move rule. Comments
//! ported verbatim. The TS mutates rows via references; the port collects
//! the decisions first (row positions), then applies them.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::parse::{AddLine, DiffFile, Row};

type RowPos = (usize, usize);

/// Mark exact-content del/add pairs as *moved*: the same line at a new
/// position (e.g. renumbered sections) — rendered muted and excluded from
/// the change counts. Matching is greedy per file (a multiset of add texts).
///
/// After marking, the source hunk gets `Row::MovSrc(del_line)` (its
/// deletion is absorbed in the counts), while the target hunk gets
/// `Row::MovTgt(add_line)`.
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

    for (hi, h) in f.hunks.iter().enumerate() {
        for (ri, r) in h.rows.iter().enumerate() {
            if let Row::Add(add_line) = r {
                let k = key(&add_line.text);
                if k.is_empty() {
                    continue;
                }
                *counts.entry(k).or_insert(0) += 1;
                add_by_key.entry(k).or_default().push((hi, ri));
            }
        }
    }

    if add_by_key.is_empty() {
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
            if t.is_empty() {
                continue;
            }
            let n = counts.get(t).copied().unwrap_or(0);
            if n == 0 {
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

    // Apply: convert paired dels/adds into Row::MovSrc and Row::MovTgt.
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
                Row::MovTgt(a) => a.clone(),
                _ => continue,
            };
            f.hunks[ah].rows[ar] = Row::MovTgt(AddLine::with_donor(
                add_line.num,
                &add_line.text,
                &del_line.text,
                add_line.spans.clone(),
            ));
            f.hunks[hi].rows[ri] = Row::MovSrc(del_line);
            pair_of.insert(del_pos, pos);
            moved_dels.insert(del_pos);
        }
    }

    // Expand pairings to adjacent blank lines: if an empty del line is
    // directly preceded or succeeded by a paired moved line, and the
    // corresponding adjacent target position is also an unused blank add line,
    // pair them as moved.
    for hi in 0..f.hunks.len() {
        for ri in 0..f.hunks[hi].rows.len() {
            if let Row::Del(d) = &f.hunks[hi].rows[ri] {
                if !key(&d.text).is_empty() {
                    continue;
                }
                // Check if preceded by a paired moved line
                let mut target_candidate: Option<RowPos> = None;
                if ri > 0
                    && let Some(&(ah, ar)) = pair_of.get(&(hi, ri - 1))
                    && ar + 1 < f.hunks[ah].rows.len()
                    && !used_adds.contains(&(ah, ar + 1))
                    && matches!(&f.hunks[ah].rows[ar + 1], Row::Add(a) if key(&a.text).is_empty())
                {
                    target_candidate = Some((ah, ar + 1));
                }
                // Check if succeeded by a paired moved line
                if target_candidate.is_none()
                    && let Some(&(ah, ar)) = pair_of.get(&(hi, ri + 1))
                    && ar > 0
                    && !used_adds.contains(&(ah, ar - 1))
                    && matches!(&f.hunks[ah].rows[ar - 1], Row::Add(a) if key(&a.text).is_empty())
                {
                    target_candidate = Some((ah, ar - 1));
                }

                if let Some((ah, ar)) = target_candidate {
                    let del_line = d.clone();
                    let add_line = match &f.hunks[ah].rows[ar] {
                        Row::Add(a) => a.clone(),
                        _ => continue,
                    };
                    used_adds.insert((ah, ar));
                    f.hunks[ah].rows[ar] = Row::MovTgt(AddLine::with_donor(
                        add_line.num,
                        &add_line.text,
                        &del_line.text,
                        add_line.spans.clone(),
                    ));
                    f.hunks[hi].rows[ri] = Row::MovSrc(del_line);
                    pair_of.insert((hi, ri), (ah, ar));
                    moved_dels.insert((hi, ri));
                }
            }
        }
    }

    fn is_trivial_line(t: &str) -> bool {
        let s = t.trim();
        s.is_empty()
            || s == "}"
            || s == "{"
            || s == "};"
            || s == ")"
            || s == ");"
            || s == "]"
            || s == "];"
            || s == "},"
            || s == "),"
    }

    // A del run whose FIRST line did not move is a deleted block, not a
    // relocation — the interior "moved" lines are coincidental same-text
    // matches (the old `find_mount`'s `.iter()`/`.rfind()`/`.map()` chain
    // verbatim-matching lines inside the NEW `get` method). The move would
    // render the new method's chain as ≈ inside a clearly-added block —
    // "`self.filesystems` added, the rest moved?" — misleading. The whole
    // deleted block reads as deleted; the new method reads as added.
    //
    // Similarly, a run containing only trivial delimiter moves (like `}` or
    // `);`) or starting with a trivial delimiter is coincidental delimiter
    // matching, not a relocated code block.
    let mut unmark_positions: Vec<RowPos> = Vec::new();
    for (hi, h) in f.hunks.iter().enumerate() {
        let mut run: Vec<RowPos> = Vec::new();
        let flush_run = |run: &[RowPos], unmark_positions: &mut Vec<RowPos>| {
            if run.is_empty() {
                return;
            }
            let first_pos = run[0];
            let first_text = f.hunks[first_pos.0].rows[first_pos.1].text();
            let first_moved = moved_dels.contains(&first_pos);
            let first_trivial = is_trivial_line(first_text);

            let has_nontrivial_move = run.iter().any(|pos| {
                if moved_dels.contains(pos) {
                    let text = f.hunks[pos.0].rows[pos.1].text();
                    !is_trivial_line(text)
                } else {
                    false
                }
            });

            if !first_moved || first_trivial || !has_nontrivial_move {
                unmark_positions.extend(run.iter().copied().filter(|pos| moved_dels.contains(pos)));
                return;
            }

            // Block Move Cohesion Rule:
            // In a multi-line deletion run (e.g. deleting a function), isolated
            // matches (e.g. single-line attributes like `#[test]`,
            // closing braces `}`, or interior method calls) are
            // coincidental boilerplate, NOT code relocations.
            //
            // A deletion run is only marked moved if:
            // - The ENTIRE deletion run moved, OR
            // - A contiguous prefix block of >= 2 lines starting at run[0]
            //   moved.
            // Otherwise, all lines in the deletion run remain normal deletions.
            if run.len() > 1 {
                let moved_in_run: Vec<RowPos> = run
                    .iter()
                    .copied()
                    .filter(|pos| moved_dels.contains(pos))
                    .collect();
                if moved_in_run.len() < 2
                    || !moved_dels.contains(&run[0])
                    || moved_in_run.len() < run.len()
                {
                    let prefix_len = run
                        .iter()
                        .take_while(|pos| moved_dels.contains(pos))
                        .count();
                    if prefix_len < 2 || prefix_len < moved_in_run.len() {
                        unmark_positions.extend(moved_in_run);
                        return;
                    } else {
                        unmark_positions.extend(
                            run.iter()
                                .skip(prefix_len)
                                .copied()
                                .filter(|pos| moved_dels.contains(pos)),
                        );
                    }
                }
            }

            // If only isolated trivial lines at the end of the run moved,
            // unmark them
            for i in 0..run.len() {
                let pos = run[i];
                if moved_dels.contains(&pos) {
                    let text = f.hunks[pos.0].rows[pos.1].text();
                    if is_trivial_line(text) && i > 0 && !moved_dels.contains(&run[i - 1]) {
                        unmark_positions.push(pos);
                    }
                }
            }
        };

        for (ri, r) in h.rows.iter().enumerate() {
            if matches!(r, Row::Del(_) | Row::MovSrc(_)) {
                run.push((hi, ri));
            } else {
                flush_run(&run, &mut unmark_positions);
                run.clear();
            }
        }
        flush_run(&run, &mut unmark_positions);
    }
    // Un-mark a moved del's paired add (revert back to Row::Add and Row::Del).
    for pos in &unmark_positions {
        let (hi, ri) = *pos;
        if let Row::MovSrc(d) = &f.hunks[hi].rows[ri] {
            f.hunks[hi].rows[ri] = Row::Del(d.clone());
        }
        if let Some(&(ah, ar)) = pair_of.get(pos)
            && let Row::MovTgt(a) = &f.hunks[ah].rows[ar]
        {
            f.hunks[ah].rows[ar] = Row::Add(a.clone());
        }
    }

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
