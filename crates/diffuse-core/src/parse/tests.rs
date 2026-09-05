//! Unit tests for diff parsing and line number reconstruction.

use super::*;
const DIFF: &str = "diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -10,1 +10,2 @@ fn old() {
-old line one
+new line one
+new line two
diff --git a/README.md b/README.md
new file mode 100644
index 000..333
--- /dev/null
+++ b/README.md
@@ -0,0 +1,2 @@
+# Title
+Body
diff --git a/old.txt b/old.txt
deleted file mode 100644
index 444..000
--- a/old.txt
+++ /dev/null
@@ -1,3 +0,0 @@
-gone
-bye
diff --git a/old_renamed.rs b/new_renamed.rs
similarity index 90%
rename from old_renamed.rs
rename to new_renamed.rs
index 555..666 100644
--- a/old_renamed.rs
+++ b/new_renamed.rs
@@ -1,1 +1,1 @@
-same
+changed
";

#[test]
fn parse_diff_multi_file_new_deleted_rename() {
    let files = parse_diff(DIFF);
    assert_eq!(files.len(), 4, "expected 4 files");
    let a = &files[0];
    assert_eq!(a.path, "src/a.rs");
    assert_eq!(a.status, FileStatus::Modified, "file 0 modified");
    assert_eq!(a.hunks.len(), 1, "file 0 one hunk");
    assert_eq!(
        a.hunks[0]
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Add(_)))
            .count(),
        2,
        "2 adds"
    );
    assert_eq!(
        a.hunks[0]
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Del(_)))
            .count(),
        1,
        "1 del"
    );
    let add_row = a.hunks[0]
        .rows
        .iter()
        .find(|r| matches!(r, Row::Add(add_line) if add_line.text == "new line one"))
        .expect("add row");
    assert_eq!(add_row.new_line(), Some(10), "add row newLine 10");
    let del_row = a.hunks[0]
        .rows
        .iter()
        .find(|r| matches!(r, Row::Del(_)))
        .expect("del row");
    assert_eq!(del_row.old_line(), Some(10), "del row oldLine 10");
    assert_eq!(files[1].status, FileStatus::Added, "README added");
    assert_eq!(files[2].status, FileStatus::Deleted, "old.txt deleted");
    assert_eq!(files[3].status, FileStatus::Renamed);
    assert_eq!(
        files[3].old_path.as_deref(),
        Some("old_renamed.rs"),
        "rename from"
    );
    assert_eq!(files[3].path, "new_renamed.rs", "rename to");
}

#[test]
fn parse_diff_pure_rename_zero_hunks() {
    let diff = "diff --git a/src/old.rs b/src/new.rs
similarity index 100%
rename from src/old.rs
rename to src/new.rs
";
    let files = parse_diff(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].status, FileStatus::Renamed);
    assert_eq!(files[0].old_path.as_deref(), Some("src/old.rs"));
    assert_eq!(files[0].path, "src/new.rs");
    assert!(files[0].hunks.is_empty());
}

#[test]
fn test_diff_components_reverse_involution() {
    let files = parse_diff(DIFF);
    for f in &files {
        let rev = f.reverse();
        assert_eq!(
            rev.reverse(),
            *f,
            "DiffFile reverse is an involution for {}",
            f.path
        );
        for h in &f.hunks {
            let h_rev = h.reverse();
            assert_eq!(h_rev.reverse(), *h, "Hunk reverse is an involution");
            for r in &h.rows {
                let r_rev = r.reverse();
                assert_eq!(r_rev.reverse(), *r, "Row reverse is an involution");
            }
        }
    }

    // Spans reverse
    let spans = Spans {
        del: vec![(0, 5), (10, 15)],
        add: vec![(2, 8)],
    };
    let rev_spans = spans.reverse();
    assert_eq!(rev_spans.del, spans.add);
    assert_eq!(rev_spans.add, spans.del);
    assert_eq!(rev_spans.reverse(), spans);

    // Mod row reverse
    let mod_row = Row::Mod {
        old_line: DelLine::new(LineNum::new(38).unwrap(), "old line text"),
        new_line: AddLine::with_spans(LineNum::new(42).unwrap(), "new line text", spans.clone()),
    };
    let rev_mod = mod_row.reverse();
    assert_eq!(rev_mod.kind(), RowKind::Mod);
    assert_eq!(rev_mod.text(), "old line text");
    assert_eq!(rev_mod.del_text(), Some("new line text"));
    assert_eq!(rev_mod.new_line(), Some(38));
    assert_eq!(rev_mod.old_line(), Some(42));
    assert_eq!(rev_mod.reverse(), mod_row);
}

#[test]
fn test_parse_diff_tab_terminated_paths() {
    let diff = "diff --git a/plan/Ratatui Migration.md b/plan/Ratatui Migration.md\t
--- a/plan/Ratatui Migration.md\t
+++ b/plan/Ratatui Migration.md\t
@@ -1,1 +1,2 @@
 # Heading
+New Line
";
    let files = parse_diff(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "plan/Ratatui Migration.md");
    assert!(!files[0].path.ends_with('\t'));
}

#[test]
fn test_parse_diff_quoted_paths_with_trailing_spaces_and_tabs() {
    let diff = "diff --git \"a/file with space \" \"b/file with space \"
--- \"a/file with space \"
+++ \"b/file with space \"
@@ -1,1 +1,1 @@
-old
+new
diff --git \"a/file\\twith\\ttab\" \"b/file\\twith\\ttab\"
--- \"a/file\\twith\\ttab\"
+++ \"b/file\\twith\\ttab\"
@@ -1,1 +1,1 @@
-old
+new
";
    let files = parse_diff(diff);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, "file with space ");
    assert_eq!(files[1].path, "file\twith\ttab");
}

#[test]
fn test_parse_diff_mode_change_only() {
    let diff = "diff --git a/script.sh b/script.sh
old mode 100644
new mode 100755
";
    let files = parse_diff(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "script.sh");
    assert_eq!(files[0].old_mode.as_deref(), Some("100644"));
    assert_eq!(files[0].new_mode.as_deref(), Some("100755"));
    assert_eq!(files[0].status, FileStatus::ModeChanged);
    assert!(files[0].hunks.is_empty());
}

#[test]
fn test_parse_diff_rename_and_mode_change() {
    let diff = "diff --git a/old.sh b/new.sh
old mode 100644
new mode 100755
similarity index 100%
rename from old.sh
rename to new.sh
";
    let files = parse_diff(diff);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "new.sh");
    assert_eq!(files[0].old_path.as_deref(), Some("old.sh"));
    assert_eq!(files[0].old_mode.as_deref(), Some("100644"));
    assert_eq!(files[0].new_mode.as_deref(), Some("100755"));
    assert_eq!(files[0].status, FileStatus::Renamed);
    assert!(files[0].hunks.is_empty());
}

#[test]
fn test_parse_diff_binary_files() {
    let diff = "diff --git a/logo.png b/logo.png
index 1234567..89abcdef 100644
Binary files a/logo.png and b/logo.png differ
diff --git a/old.bin b/new.bin
similarity index 80%
rename from old.bin
rename to new.bin
Binary files a/old.bin and b/new.bin differ
";
    let files = parse_diff(diff);
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].path, "logo.png");
    assert!(files[0].is_binary);
    assert_eq!(files[0].status, FileStatus::Modified);
    assert!(files[0].hunks.is_empty());

    assert_eq!(files[1].path, "new.bin");
    assert_eq!(files[1].old_path.as_deref(), Some("old.bin"));
    assert!(files[1].is_binary);
    assert_eq!(files[1].status, FileStatus::Renamed);
    assert!(files[1].hunks.is_empty());
}
