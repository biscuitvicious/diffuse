//! Unit tests for exact move detection and block move rules.

use super::*;
use crate::parse::{AddLine, DelLine, FileStatus, Hunk, LineNum, Row};

fn del(num: u32, text: &str) -> Row {
    Row::Del(DelLine::new(LineNum::new(num).unwrap(), text))
}

fn add(num: u32, text: &str) -> Row {
    Row::Add(AddLine::new(LineNum::new(num).unwrap(), text))
}

fn file(hunks: Vec<Hunk>) -> DiffFile {
    DiffFile {
        path: "m.rs".into(),
        old_path: None,
        status: FileStatus::Modified,
        new_line_count: 20,
        old_line_count: 20,
        new_content: Vec::new(),
        old_content: Vec::new(),
        hunks,
        raw_hunks: Vec::new(),
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
        del(1, "old one"),
        add(1, "new one"),
        add(2, "extra line"),
        add(3, ""),
        del(4, "moved line"),
        add(4, "moved line"),
        add(5, "after the blank"),
    ])]);
    mark_moves(&mut f);
    let moved_add = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(4) && r.text() == "moved line")
        .expect("add");
    assert!(
        moved_add.is_moved(),
        "exact-content pair marked moved (add)"
    );
    assert_eq!(moved_add.new_line(), Some(4), "target at new_line 4");

    let moved_del = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(4) && r.text() == "moved line")
        .expect("del");
    assert!(
        moved_del.is_moved(),
        "exact-content pair marked moved (del source)"
    );
}

// Port of the block-move rule test: the old find_mount chain
// verbatim-matching the NEW get method must NOT be marked moved.
#[test]
fn block_move_rule_interior_matches_are_not_moves() {
    let mut f = file(vec![
        hunk(vec![
            del(1019, "        self.mount_stack"),
            del(1020, "            .filesystems"),
            del(1021, "            .iter()"),
            del(
                1022,
                "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))",
            ),
            del(
                1023,
                "            .map(|fs| fs.as_ref() as &dyn Filesystem)",
            ),
            add(1060, "        self.mount_stack.get(guest_path)"),
            add(1061, "    }"),
        ]),
        hunk(vec![
            add(
                771,
                "    pub fn get(&self, guest_path: &Path) -> Option<&dyn Filesystem> {",
            ),
            add(772, "        self.filesystems"),
            add(773, "            .iter()"),
            add(
                774,
                "            .rfind(|fs| guest_path.starts_with(fs.mount_point()))",
            ),
            add(775, "            .map(|fs| fs.as_ref() as &dyn Filesystem)"),
            add(776, "    }"),
        ]),
    ]);
    mark_moves(&mut f);
    let del_iter = f.hunks[0]
        .rows
        .iter()
        .find(|r| matches!(r, Row::Del(d) if d.text.trim() == ".iter()"));
    assert!(
        del_iter.is_some() && !del_iter.unwrap().is_moved(),
        "the interior chain del is NOT a move (the run's first del didn't move)"
    );
    let add_iter = f.hunks[1]
        .rows
        .iter()
        .find(|r| matches!(r, Row::Add(a) if a.text.trim() == ".iter()"));
    assert!(
        add_iter.is_some() && !add_iter.unwrap().is_moved(),
        "the get method's chain add is NOT marked moved"
    );
}

// When the whole block moves (including the leading line), common method call
// lines like .rows, .iter(), .collect() must all be marked moved.
#[test]
fn moved_method_chain_block_marks_all_lines_moved() {
    let mut f = file(vec![
        hunk(vec![
            del(128, "    let del_ids: Vec<usize> = h"),
            del(129, "        .rows"),
            del(130, "        .iter()"),
            del(131, "        .collect();"),
        ]),
        hunk(vec![
            add(97, "    let del_ids: Vec<usize> = h"),
            add(98, "        .rows"),
            add(99, "        .iter()"),
            add(103, "        .collect();"),
            // Another addition in the diff that happens to contain substring `.iter()`
            add(200, "    tokens.iter().map(|t| t.id).collect()"),
        ]),
    ]);
    mark_moves(&mut f);

    for (old_n, txt) in [
        (128, "    let del_ids: Vec<usize> = h"),
        (129, "        .rows"),
        (130, "        .iter()"),
        (131, "        .collect();"),
    ] {
        let r = f.hunks[0].rows.iter().find(|r| r.old_line() == Some(old_n));
        assert!(
            r.is_some() && r.unwrap().is_moved(),
            "source line {old_n} ({txt}) must be marked moved"
        );
    }

    for (new_n, txt) in [
        (97, "    let del_ids: Vec<usize> = h"),
        (98, "        .rows"),
        (99, "        .iter()"),
        (103, "        .collect();"),
    ] {
        let r = f.hunks[1].rows.iter().find(|r| r.new_line() == Some(new_n));
        assert!(
            r.is_some() && r.unwrap().is_moved(),
            "target line {new_n} ({txt}) must be marked moved"
        );
    }
}

// When a trailing blank line exists at both source and target sites of a moved
// block, it moves with the block.
#[test]
fn trailing_blank_line_at_target_site_moves_with_block() {
    let mut f = file(vec![
        hunk(vec![
            del(10, "let a = 1;"),
            del(11, "let b = 2;"),
            del(12, ""), // trailing blank at source
        ]),
        hunk(vec![
            add(50, "let a = 1;"),
            add(51, "let b = 2;"),
            add(52, ""), // matching trailing blank at target
            add(70, ""), // another unrelated blank line
        ]),
    ]);
    mark_moves(&mut f);

    // Both line 12 and line 52 must be marked moved
    let d12 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(12))
        .unwrap();
    assert!(
        d12.is_moved(),
        "source trailing blank line 12 must be marked moved"
    );

    let a52 = f.hunks[1]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(52))
        .unwrap();
    assert!(
        a52.is_moved(),
        "target trailing blank line 52 must be marked moved"
    );

    let a70 = f.hunks[1]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(70))
        .unwrap();
    assert!(
        !a70.is_moved(),
        "unrelated blank line 70 must NOT be marked moved"
    );
}

// When a trailing blank line exists only at the source site (target site has
// another code line or ends without blank), the blank line does not move and
// does not steal an unrelated blank line elsewhere.
#[test]
fn trailing_blank_line_missing_at_target_site_does_not_move() {
    let mut f = file(vec![
        hunk(vec![
            del(10, "let a = 1;"),
            del(11, "let b = 2;"),
            del(12, ""), // trailing blank at source
        ]),
        hunk(vec![
            add(50, "let a = 1;"),
            add(51, "let b = 2;"),
            add(52, "let c = 3;"), // not a blank line
            add(70, ""),           // unrelated blank line far away
        ]),
    ]);
    mark_moves(&mut f);

    let d12 = f.hunks[0]
        .rows
        .iter()
        .find(|r| r.old_line() == Some(12))
        .unwrap();
    assert!(
        !d12.is_moved(),
        "source trailing blank line 12 must NOT be marked moved"
    );

    let a70 = f.hunks[1]
        .rows
        .iter()
        .find(|r| r.new_line() == Some(70))
        .unwrap();
    assert!(
        !a70.is_moved(),
        "unrelated blank line 70 must NOT be marked moved"
    );
}
