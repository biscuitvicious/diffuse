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
    assert_eq!(moved_add.old_line(), Some(4), "paired with old_line 4");
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
