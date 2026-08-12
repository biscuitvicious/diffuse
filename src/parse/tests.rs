use super::*;

// Port of the parseDiff test block from test/review.test.ts.
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
diff --git a/renamed.rs b/renamed.rs
similarity index 90%
rename from a/renamed.rs
rename to b/renamed.rs
index 555..666 100644
--- a/renamed.rs
+++ b/renamed.rs
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
            .filter(|r| r.kind == RowKind::Add)
            .count(),
        2,
        "2 adds"
    );
    assert_eq!(
        a.hunks[0]
            .rows
            .iter()
            .filter(|r| r.kind == RowKind::Del)
            .count(),
        1,
        "1 del"
    );
    let add_row = a.hunks[0]
        .rows
        .iter()
        .find(|r| r.kind == RowKind::Add && r.text == "new line one")
        .expect("add row");
    assert_eq!(add_row.new_line, Some(10), "add row newLine 10");
    let del_row = a.hunks[0]
        .rows
        .iter()
        .find(|r| r.kind == RowKind::Del)
        .expect("del row");
    assert_eq!(del_row.old_line, Some(10), "del row oldLine 10");
    assert_eq!(files[1].status, FileStatus::Added, "README added");
    assert_eq!(files[2].status, FileStatus::Deleted, "old.txt deleted");
    assert_eq!(files[3].status, FileStatus::Renamed);
    assert_eq!(files[3].old_path.as_deref(), Some("renamed.rs"), "rename");
}
