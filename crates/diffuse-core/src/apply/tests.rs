use super::apply_file_diff;

fn s(lines: &[&str]) -> Vec<String> {
    lines.iter().map(std::string::ToString::to_string).collect()
}

#[test]
fn apply_a_simple_hunk() {
    let old = s(&["a", "b", "c", "d", "e"]);
    let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -2,2 +2,1 @@\n-b\n c\n";
    assert_eq!(apply_file_diff(diff, &old), s(&["a", "c", "d", "e"]));
}

#[test]
fn apply_insert_and_delete() {
    let old = s(&["a", "b", "c"]);
    let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,1 +1,2 @@\n a\n+B\n";
    assert_eq!(apply_file_diff(diff, &old), s(&["a", "B", "b", "c"]));
}

#[test]
fn apply_two_hunks_reverse_order() {
    // Two non-overlapping hunks; the second's net change must not shift
    // the first's offset (applied reverse old-start order).
    let old = s(&["a", "b", "c", "d", "e"]);
    let diff =
        "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,1 +1,2 @@\n a\n+A\n@@ -4,1 +5,2 @@\n d\n+D\n";
    assert_eq!(
        apply_file_diff(diff, &old),
        s(&["a", "A", "b", "c", "d", "D", "e"])
    );
}

#[test]
fn apply_new_file_hunk_old_start_zero() {
    let diff = "diff --git a/x b/x\nnew file mode 100644\n--- /dev/null\n+++ b/x\n@@ -0,0 +1,2 @@\n+one\n+two\n";
    assert_eq!(apply_file_diff(diff, &[]), s(&["one", "two"]));
}

#[test]
fn apply_drops_no_newline_marker() {
    let old = s(&["a", "b"]);
    let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -2,1 +2,1 @@\n-b\n+bee\n\\ No newline at end of file\n";
    assert_eq!(apply_file_diff(diff, &old), s(&["a", "bee"]));
}
