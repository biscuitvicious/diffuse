use super::old_content_from_diff;

// A del whose CONTENT starts with a dash (`-- item` — a markdown list
// bullet `- item`) is a real del; only the three-dash `--- a/file`
// header is skipped. The diffuse-repo PLAN.md had a `- \`--golden\`…`
// bullet that the port's `rest.starts_with('-')` check swallowed,
// shifting every later old line by one.
#[test]
fn del_content_starting_with_dash_is_kept() {
    let diff = "diff --git a/PLAN.md b/PLAN.md\n--- a/PLAN.md\n+++ b/PLAN.md\n@@ -113,2 +114,7 @@\n-- `--golden` switches the output\n-  test, §5).\n+new\n";
    let oc = old_content_from_diff(diff, "PLAN.md");
    // 115 = old lines 1..=114 + the diff's trailing newline (an empty
    // context line — the TS behaves identically, verified against it).
    assert_eq!(oc.len(), 115, "lines 1..=114 present");
    assert_eq!(
        oc[112], "- `--golden` switches the output",
        "the dash-content del survives"
    );
    assert_eq!(oc[113], "  test, §5).");
}

// The context-line write fires when the slot is past the array's end
// (the TS's `out[oi - 1] === undefined`).
#[test]
fn context_line_after_del_run() {
    let diff = "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -100,2 +100,2 @@\n-        None\n     }\n";
    let oc = old_content_from_diff(diff, "x.rs");
    assert_eq!(oc[99], "        None");
    assert_eq!(oc[100], "    }", "the context line after the del");
}

// A NEW file's hunk is `@@ -0,0 +1,N @@` — old start 0. The JS `oi - 1`
// is a number; usize must saturate: the old side stays empty (the
// diffuse corpus's extension/pi/ files hit this).
#[test]
fn new_file_hunk_old_start_zero() {
    let diff = "diff --git a/extension/pi/x.ts b/extension/pi/x.ts\nnew file mode 100644\n--- /dev/null\n+++ b/extension/pi/x.ts\n@@ -0,0 +1,2 @@\n+line one\n+line two\n";
    let oc = old_content_from_diff(diff, "extension/pi/x.ts");
    assert_eq!(oc.len(), 0, "a new file has no old side");
}
