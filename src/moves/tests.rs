
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
            row(
                RowKind::Add,
                "        self.mount_stack.get(guest_path)",
                Some(1060),
                None,
            ),
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
    assert!(
        del_iter.is_none() || !del_iter.unwrap().moved,
        "the interior chain del is NOT a move (the run's first del didn't move)"
    );
    let add_iter = f.hunks[1]
        .rows
        .iter()
        .find(|r| r.kind == RowKind::Add && r.text.trim() == ".iter()");
    assert!(
        add_iter.is_none() || !add_iter.unwrap().moved,
        "the get method's chain add is NOT marked moved"
    );
    if let Some(a) = add_iter {
        assert!(
            a.del_text.is_none(),
            "the paired delText is dropped with the move"
        );
    }
}
