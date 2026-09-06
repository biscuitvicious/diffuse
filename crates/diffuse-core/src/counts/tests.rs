//! Unit tests for file and row count calculation.

use crate::{
    collapse::collapse_hunk_rows,
    counts::{FileCounts, file_counts},
    moves::mark_moves,
    parse::{AddLine, DelLine, DiffFile, FileStatus, Hunk, LineNum, Row},
};

fn del(num: u32, text: &str) -> Row {
    Row::Del(DelLine::new(LineNum::new(num).unwrap(), text))
}

fn add(num: u32, text: &str) -> Row {
    Row::Add(AddLine::new(LineNum::new(num).unwrap(), text))
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

fn file(hunks: Vec<Hunk>) -> DiffFile {
    DiffFile {
        path: "f.rs".into(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: FileStatus::Modified,
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        new_line_count: 20,
        old_line_count: 20,
        new_content: Vec::new(),
        old_content: Vec::new(),
        hunks,
        raw_hunks: Vec::new(),
        no_content: false,
    }
}

// The modified-pair block: after the collapse, the mod absorbs two tail
// lines (the section add + the closing blank) which count as adds; the
// absorbed reflow leaves no moved rows.
#[test]
fn counts_after_modified_pair_collapse() {
    let mut f = file(vec![hunk(vec![
        del(1, "let old_result = calculate();"),
        add(1, "let new_result = calculate();"),
        add(2, "extra line"),
        add(3, ""), // section break
        del(4, "moved line"),
        add(4, "moved line"),
        add(5, "after the blank"),
    ])]);
    mark_moves(&mut f);
    f.hunks[0].rows = collapse_hunk_rows(&f.hunks[0], None, None, None);
    let counts = file_counts(&f);
    assert_eq!(
        counts,
        FileCounts {
            adds: 3,
            dels: 0,
            mods: 1,
            movs: 1,
        },
        "counts +3 ~1 ~1 — the mod pair, the moved pair, and the tail lines count as adds"
    );
}

// A plain del→add pair that does NOT collapse (no similarity) counts one
// del and one add.
#[test]
fn counts_plain_del_and_add() {
    let mut f = file(vec![hunk(vec![
        del(1, "old one"),
        add(1, "completely different"),
    ])]);
    mark_moves(&mut f);
    f.hunks[0].rows = collapse_hunk_rows(&f.hunks[0], None, None, None);
    let counts = file_counts(&f);
    assert_eq!(counts.adds, 1, "one add");
    assert_eq!(counts.dels, 1, "one del");
    assert_eq!(counts.mods, 0);
    assert_eq!(counts.movs, 0);
}

// A moved pair counts as one relocation — only the target side counts.
#[test]
fn counts_moved_pair_is_neither_add_nor_del() {
    let mut f = file(vec![hunk(vec![del(1, "moved line"), add(1, "moved line")])]);
    mark_moves(&mut f);
    f.hunks[0].rows = collapse_hunk_rows(&f.hunks[0], None, None, None);
    let counts = file_counts(&f);
    // The moved pair: Row::Mov counts as +1 mov.
    assert_eq!(
        counts,
        FileCounts {
            adds: 0,
            dels: 0,
            mods: 0,
            movs: 1,
        },
        "a moved pair counts as one mov"
    );
}
