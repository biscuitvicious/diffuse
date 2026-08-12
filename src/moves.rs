//! One-for-one port of `review.ts`'s `markMoves` (PLAN.md §3) — the
//! exact-content relocation marking plus the block-move rule. Comments
//! ported verbatim. The TS mutates rows via references; the port collects
//! the decisions first (row positions), then applies them.

use crate::parse::{DiffFile, RowKind};
use std::collections::{HashMap, HashSet};

type RowPos = (usize, usize);

/// Mark exact-content del/add pairs as *moved*: the same line at a new
/// position (e.g. renumbered sections) — rendered muted and excluded from
/// the change counts. Matching is greedy per file (a multiset of add texts).
pub fn mark_moves(f: &mut DiffFile) {
    // Indent-insensitive: a line reflowed to a different indentation is still
    // a move, not a change (the pairing must not turn it into a `~` mod).
    let key = |t: &str| -> String { t.trim().to_string() };

    let mut counts: HashMap<String, usize> = HashMap::new();
    for h in &f.hunks {
        for r in &h.rows {
            if r.kind == RowKind::Add {
                *counts.entry(key(&r.text)).or_insert(0) += 1;
            }
        }
    }

    // All add positions in document order — the TS re-scans the hunks per
    // del; precomputing keeps the same "first un-moved add" semantics.
    let mut add_positions: Vec<RowPos> = Vec::new();
    for (hi, h) in f.hunks.iter().enumerate() {
        for (ri, r) in h.rows.iter().enumerate() {
            if r.kind == RowKind::Add {
                add_positions.push((hi, ri));
            }
        }
    }

    // The pairing decisions: (del, paired add, moved). The reflow check and
    // the count consume read-only state; the mutation is applied after.
    let mut decisions: Vec<(RowPos, Option<RowPos>, bool)> = Vec::new();
    let mut used_adds: HashSet<RowPos> = HashSet::new();
    for (hi, h) in f.hunks.iter().enumerate() {
        for (ri, r) in h.rows.iter().enumerate() {
            if r.kind != RowKind::Del || r.moved {
                continue;
            }
            let t = key(&r.text);
            // A del whose content survives INSIDE a different add (a
            // restructured line — the old `.iter()` of the `let up` chain
            // folded into the one-liner mod) is a reflow the collapse
            // absorbs, not a move: pairing it would mark an unrelated
            // same-text add (the new `below` block's `.iter()`) as relocated.
            let reflow = !t.is_empty()
                && f.hunks.iter().any(|h2| {
                    h2.rows
                        .iter()
                        .any(|a| a.kind == RowKind::Add && key(&a.text) != t && a.text.contains(&t))
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
                    f.hunks[ph].rows[pr].kind == RowKind::Add
                        && !f.hunks[ph].rows[pr].moved
                        && key(&f.hunks[ph].rows[pr].text) == t
                }
            });
            if let Some(pos) = paired {
                used_adds.insert(pos);
                decisions.push(((hi, ri), Some(pos), true));
            } else {
                decisions.push(((hi, ri), None, true));
            }
        }
    }

    // Apply: mark the dels and their paired adds moved, carry the del text.
    let mut pair_of: HashMap<RowPos, RowPos> = HashMap::new(); // del -> paired add
    for (del_pos, add_pos, moved) in decisions {
        let (hi, ri) = del_pos;
        f.hunks[hi].rows[ri].moved = moved;
        if let Some(pos) = add_pos {
            let (ah, ar) = pos;
            f.hunks[ah].rows[ar].moved = true;
            // The paired old text: the moved row can subword the
            // indentation change (8 → 12 spaces is a real semantic move)
            // instead of hiding it in the ≈.
            f.hunks[ah].rows[ar].del_text = Some(f.hunks[hi].rows[ri].text.clone());
            pair_of.insert(del_pos, pos);
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
            if r.kind == RowKind::Del {
                run.push((hi, ri));
            } else {
                // flush the run
                if !run.is_empty() && !h.rows[run[0].1].moved {
                    unmark_positions.extend(run.iter().copied().filter(|&pos| h.rows[pos.1].moved));
                }
                run.clear();
            }
        }
        if !run.is_empty() && !h.rows[run[0].1].moved {
            unmark_positions.extend(run.iter().copied().filter(|&pos| h.rows[pos.1].moved));
        }
    }
    // Un-mark a moved del AND its paired add (the TS clears both).
    for pos in unmark_positions {
        let (hi, ri) = pos;
        f.hunks[hi].rows[ri].moved = false;
        if let Some(&(ah, ar)) = pair_of.get(&pos) {
            f.hunks[ah].rows[ar].moved = false;
            f.hunks[ah].rows[ar].del_text = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Hunk, Row};

    fn row(kind: RowKind, text: &str, new_line: Option<u32>, old_line: Option<u32>) -> Row {
        Row::new(kind, text, new_line, old_line)
    }

    fn file(hunks: Vec<Hunk>) -> DiffFile {
        DiffFile {
            path: "m.rs".into(),
            old_path: None,
            status: crate::parse::FileStatus::Modified,
            new_line_count: 20,
            old_line_count: 20,
            new_content: Vec::new(),
            old_content: Vec::new(),
            hunks,
            no_content: false,
        }
    }

    fn hunk(rows: Vec<Row>) -> Hunk {
        Hunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 3,
            section: String::new(),
            rows,
            above_ctx: 0,
            below_ctx: 0,
        }
    }

    // Port of the moved-line test block from test/review.test.ts.
    #[test]
    fn exact_content_pair_marked_moved() {
        let mut f = file(vec![hunk(vec![
            row(RowKind::Del, "old one", None, Some(1)),
            row(RowKind::Add, "new one", Some(1), None),
            row(RowKind::Add, "extra line", Some(2), None),
            row(RowKind::Add, "", Some(3), None),
            row(RowKind::Del, "moved line", None, Some(4)),
            row(RowKind::Add, "moved line", Some(4), None),
            row(RowKind::Add, "after the blank", Some(5), None),
        ])]);
        mark_moves(&mut f);
        let moved_del = f.hunks[0]
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Del && r.text == "moved line")
            .expect("del");
        let moved_add = f.hunks[0]
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Add && r.text == "moved line")
            .expect("add");
        assert!(moved_del.moved, "exact-content pair marked moved (del)");
        assert!(moved_add.moved, "exact-content pair marked moved (add)");
    }

    // Port of the block-move rule test: the old find_mount chain
    // verbatim-matching the NEW get method must NOT be marked moved.
    #[test]
    fn block_move_rule_interior_matches_are_not_moves() {
        let mut f = file(vec![
            hunk(vec![
                row(RowKind::Del, "        self.mount_stack", None, Some(1019)),
                row(RowKind::Del, "            .filesystems", None, Some(1020)),
                row(RowKind::Del, "            .iter()", None, Some(1021)),
                row(
                    RowKind::Del,
                    "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))",
                    None,
                    Some(1022),
                ),
                row(
                    RowKind::Del,
                    "            .map(|fs| fs.as_ref() as &dyn Filesystem)",
                    None,
                    Some(1023),
                ),
                row(RowKind::Add, "        self.mount_stack.get(guest_path)", Some(1060), None),
                row(RowKind::Add, "    }", Some(1061), None),
            ]),
            hunk(vec![
                row(
                    RowKind::Add,
                    "    pub fn get(&self, guest_path: &Path) -> Option<&dyn Filesystem> {",
                    Some(771),
                    None,
                ),
                row(RowKind::Add, "        self.filesystems", Some(772), None),
                row(RowKind::Add, "            .iter()", Some(773), None),
                row(
                    RowKind::Add,
                    "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))",
                    Some(774),
                    None,
                ),
                row(
                    RowKind::Add,
                    "            .map(|fs| fs.as_ref() as &dyn Filesystem)",
                    Some(775),
                    None,
                ),
                row(RowKind::Add, "    }", Some(776), None),
            ]),
        ]);
        mark_moves(&mut f);
        let del_iter = f.hunks[0]
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Del && r.text.trim() == ".iter()");
        assert!(del_iter.is_none() || !del_iter.unwrap().moved,
            "the interior chain del is NOT a move (the run's first del didn't move)");
        let add_iter = f.hunks[1]
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Add && r.text.trim() == ".iter()");
        assert!(add_iter.is_none() || !add_iter.unwrap().moved,
            "the get method's chain add is NOT marked moved");
        if let Some(a) = add_iter {
            assert!(a.del_text.is_none(), "the paired delText is dropped with the move");
        }
    }
}
