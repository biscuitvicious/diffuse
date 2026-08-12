//! Port of the `fileCounts` assertions from test/review.test.ts: the
//! header counts. Moves are neither added nor removed; mods are one row;
//! a mod's absorbed tail lines count as adds.

use crate::collapse::collapse_hunk_rows;
use crate::counts::file_counts;
use crate::moves::mark_moves;
use crate::parse::{DiffFile, FileStatus, Hunk, Row, RowKind};

fn row(kind: RowKind, text: &str, new_line: Option<u32>, old_line: Option<u32>) -> Row {
    Row::new(kind, text, new_line, old_line)
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
        status: FileStatus::Modified,
        new_line_count: 20,
        old_line_count: 20,
        new_content: Vec::new(),
        old_content: Vec::new(),
        hunks,
        no_content: false,
    }
}

// The modified-pair block: after the collapse, the mod absorbs two tail
// lines (the section add + the closing blank) which count as adds; the
// absorbed reflow leaves no moved rows.
#[test]
fn counts_after_modified_pair_collapse() {
    let mut f = file(vec![hunk(vec![
        row(RowKind::Del, "old one", None, Some(1)),
        row(RowKind::Add, "new one", Some(1), None),
        row(RowKind::Add, "extra line", Some(2), None),
        row(RowKind::Add, "", Some(3), None), // section break
        row(RowKind::Del, "moved line", None, Some(4)),
        row(RowKind::Add, "moved line", Some(4), None),
        row(RowKind::Add, "after the blank", Some(5), None),
    ])]);
    mark_moves(&mut f);
    f.hunks[0].rows = collapse_hunk_rows(&f.hunks[0], false, None, None);
    let counts = file_counts(&f);
    assert_eq!(
        counts,
        crate::counts::FileCounts {
            adds: 3,
            dels: 0,
            mods: 1,
            moved: 0,
        },
        "counts +3 ~1 — the absorbed reflow leaves no moved rows, the tail lines count as adds"
    );
}

// A plain del→add pair that does NOT collapse (no similarity) counts one
// del and one add.
#[test]
fn counts_plain_del_and_add() {
    let mut f = file(vec![hunk(vec![
        row(RowKind::Del, "old one", None, Some(1)),
        row(RowKind::Add, "completely different", Some(1), None),
    ])]);
    mark_moves(&mut f);
    f.hunks[0].rows = collapse_hunk_rows(&f.hunks[0], false, None, None);
    let counts = file_counts(&f);
    assert_eq!(counts.adds, 1, "one add");
    assert_eq!(counts.dels, 1, "one del");
    assert_eq!(counts.mods, 0);
    assert_eq!(counts.moved, 0);
}

// A moved pair counts as moved, not as an add or a del.
#[test]
fn counts_moved_pair_is_neither_add_nor_del() {
    let mut f = file(vec![hunk(vec![
        row(RowKind::Del, "moved line", None, Some(1)),
        row(RowKind::Add, "moved line", Some(1), None),
    ])]);
    mark_moves(&mut f);
    f.hunks[0].rows = collapse_hunk_rows(&f.hunks[0], false, None, None);
    let counts = file_counts(&f);
    // The moved pair is a pure reflow — fully absorbed: the del drops with
    // its paired add, nothing survives the collapse (verified against the
    // TS engine: same all-zero counts).
    assert_eq!(
        counts,
        crate::counts::FileCounts {
            adds: 0,
            dels: 0,
            mods: 0,
            moved: 0,
        },
        "a pure reflow is invisible"
    );
}
