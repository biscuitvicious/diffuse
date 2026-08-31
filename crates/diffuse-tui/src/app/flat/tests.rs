use std::{fs, path::Path};

use crate::{
    app::{AppState, temp_notes_path},
    core::{
        PipelineOptions,
        counts::FileCounts,
        parse::RowKind,
        pipeline::run_pipeline_full,
        wire::{OutputFile, OutputHunk, OutputRow, PipelineOutput},
    },
    view::hunk_divider_text,
};

fn sample_output_two_hunks() -> PipelineOutput {
    let diff = "diff --git a/foo.rs b/foo.rs\n--- a/foo.rs\n+++ b/foo.rs\n@@ -1,3 +1,4 @@\n fn a() {\n+    let x = 1;\n     return 1;\n }\n@@ -30,3 +31,4 @@ fn b() {\n fn b() {\n+    let y = 2;\n     return 2;\n }\n";
    let mut old_lines = vec![
        "fn a() {".to_string(),
        "    return 1;".to_string(),
        "}".to_string(),
    ];
    for i in 4..=29 {
        old_lines.push(format!("    // filler {i}"));
    }
    old_lines.push("fn b() {".to_string());
    old_lines.push("    return 2;".to_string());
    old_lines.push("}".to_string());

    let mut new_lines = vec![
        "fn a() {".to_string(),
        "    let x = 1;".to_string(),
        "    return 1;".to_string(),
        "}".to_string(),
    ];
    for i in 5..=30 {
        new_lines.push(format!("    // filler {i}"));
    }
    new_lines.push("fn b() {".to_string());
    new_lines.push("    let y = 2;".to_string());
    new_lines.push("    return 2;".to_string());
    new_lines.push("}".to_string());

    let new_content = [("foo.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_content = [("foo.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    )
}
#[test]
fn test_hunk_starts_and_navigation() {
    let mut state = AppState::new(
        sample_output_two_hunks(),
        "test_repo".into(),
        temp_notes_path(),
    );
    assert_eq!(state.hunk_starts.len(), 2, "both hunks must be located");
    // Start from file header (row 0), next jumps to hunk 1's start
    state.row_idx = 0;
    state.next_hunk();
    assert_eq!(state.current_hunk(), Some(0));

    state.next_hunk();
    let at = state.row_idx;
    assert_eq!(
        state.current_hunk(),
        Some(1),
        "next_hunk must land in hunk 2"
    );
    assert!(
        at > state.hunk_starts[0].flat,
        "next_hunk must jump past hunk 1"
    );

    state.prev_hunk();
    assert_eq!(state.current_hunk(), Some(0), "prev_hunk returns to hunk 1");
}

#[test]
fn test_untracked_files_collapsed_by_default() {
    let diff = r#"diff --git a/tracked.rs b/tracked.rs
index 1111111..2222222 100644
--- a/tracked.rs
+++ b/tracked.rs
@@ -1 +1,2 @@
 fn main() {}
+fn extra() {}
"#;
    let new_content = [
        (
            "tracked.rs".to_string(),
            "fn main() {}\nfn extra() {}\n".to_string(),
        ),
        (
            "untracked.rs".to_string(),
            "fn new_untracked() {}\n".to_string(),
        ),
    ]
    .into_iter()
    .collect();

    let mut out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    // Add an untracked file to output
    out.files.push(OutputFile {
        path: "untracked.rs".to_string(),
        status: "untracked".to_string(),
        new_line_count: 1,
        old_line_count: 0,
        new_content: vec!["fn new_untracked() {}".to_string()],
        old_content: Vec::new(),
        hunks: vec![OutputHunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 1,
            rows: vec![OutputRow {
                kind: RowKind::Add,
                new_line: Some(1),
                old_line: None,
                text: "fn new_untracked() {}".to_string(),
                del_text: None,
                spans: Default::default(),
                runs: Vec::new(),
                wholesale: false,
                moved: false,
            }],
        }],
        raw_hunks: Vec::new(),
        counts: FileCounts {
            adds: 1,
            dels: 0,
            mods: 0,
            movs: 0,
        },
        fences: Vec::new(),
        notes: Vec::new(),
        content_runs: Vec::new(),
        rows: Vec::new(),
    });

    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());

    assert!(
        state.collapsed_files.contains(&1),
        "untracked file (index 1) must be collapsed by default"
    );
    assert!(
        !state.collapsed_files.contains(&0),
        "tracked file (index 0) must be expanded by default"
    );

    // The flat sequence must contain rows for file 0, but only the header row
    // for file 1
    let file_1_rows: Vec<_> = state.flat.iter().filter(|r| r.file_idx == 1).collect();
    assert_eq!(
        file_1_rows.len(),
        1,
        "collapsed file only has 1 flat row (header)"
    );
    assert!(file_1_rows[0].is_file_header);

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("▶ untracked.rs (untracked)"),
        "frame shows collapsed indicator"
    );

    // Toggle collapse on file 1 to expand it
    state.toggle_file_collapse(1);
    assert!(
        !state.collapsed_files.contains(&1),
        "untracked file is now expanded"
    );
    let file_1_rows_expanded: Vec<_> = state.flat.iter().filter(|r| r.file_idx == 1).collect();
    assert!(
        file_1_rows_expanded.len() > 1,
        "expanded file has header and content rows"
    );

    let frame_expanded = state.render_frame(100, 30);
    assert!(
        frame_expanded.contains("▼ untracked.rs (untracked)"),
        "frame shows expanded indicator"
    );
}

#[test]
fn test_hunks_with_hidden_gap_has_hunk_header_after_separator() {
    let diff = r#"diff --git a/cache.rs b/cache.rs
index 1111111..2222222 100644
--- a/cache.rs
+++ b/cache.rs
@@ -1,3 +1,4 @@
 fn a() {
 +    let x = 1;
     return 1;
 }
@@ -85,3 +86,4 @@
 fn b() {
 +    let y = 2;
     return 2;
 }
"#;
    let mut old_lines = vec![
        "fn a() {".to_string(),
        "    return 1;".to_string(),
        "}".to_string(),
    ];
    for i in 4..=84 {
        old_lines.push(format!("    // filler {i}"));
    }
    old_lines.push("fn b() {".to_string());
    old_lines.push("    return 2;".to_string());
    old_lines.push("}".to_string());

    let mut new_lines = vec![
        "fn a() {".to_string(),
        "    let x = 1;".to_string(),
        "    return 1;".to_string(),
        "}".to_string(),
    ];
    for i in 5..=85 {
        new_lines.push(format!("    // filler {i}"));
    }
    new_lines.push("fn b() {".to_string());
    new_lines.push("    let y = 2;".to_string());
    new_lines.push("    return 2;".to_string());
    new_lines.push("}".to_string());

    let new_content = [("cache.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_content = [("cache.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let mut state = AppState::new(output, "test_repo".into(), temp_notes_path());

    assert_eq!(state.hunk_starts.len(), 2, "must find 2 hunk starts");

    // Jump to hunk 2
    let h2_start = state.hunk_starts[1].flat;
    state.next_hunk();
    assert_eq!(
        state.row_idx, h2_start,
        "next_hunk must jump to the first row of Hunk 2"
    );
    assert_eq!(
        state.current_hunk(),
        Some(1),
        "current_hunk must be 1 when on the first row of Hunk 2"
    );

    // Render frame with viewport covering hunk 2 start: must show H2 divider
    // with hidden lines gap and buttons
    let gap = state.hunk_leading_gap(0, 1);
    let frame = state.render_frame(120, 40);
    let h2_txt = hunk_divider_text(1, &state.output.files[0].hunks[1], gap);
    assert!(
        frame.contains(&h2_txt),
        "rendered frame must show H2 divider with merged gap info"
    );
    assert!(
        !frame.contains("⋯ 72 lines hidden"),
        "standalone separator row must NOT be rendered"
    );
}

#[test]
fn test_diffuse_golden_plan_md_hunk_divider() {
    let wire_path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/diffuse-2026-08/wire.json"
    ));
    let json_str = fs::read_to_string(wire_path).expect("must read diffuse-2026-08/wire.json");
    let output: PipelineOutput = serde_json::from_str(&json_str).unwrap();
    let mut state = AppState::new(output, "diffuse".into(), temp_notes_path());

    // Find PLAN.md file index
    let fi = state
        .output
        .files
        .iter()
        .position(|f| f.path.contains("PLAN.md"))
        .expect("PLAN.md must exist");

    // Jump to PLAN.md
    let plan_file_row = state
        .flat
        .iter()
        .position(|fr| fr.file_idx == fi && fr.is_file_header)
        .expect("file header row for PLAN.md");
    state.jump_to_row(plan_file_row, false);

    let frame = state.render_frame(140, 40);
    assert!(frame.contains("H1"), "must contain H1 divider");
    assert!(frame.contains("H2"), "must contain H2 divider");
}
