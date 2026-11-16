use std::{collections::HashMap, fs, path::Path};

use crate::{
    app::{AppState, temp_notes_path},
    core::{
        PipelineOptions,
        counts::FileCounts,
        merge::RunKind,
        parse::RowKind,
        pipeline::run_pipeline_full,
        wire::{OutputFile, OutputHunk, OutputRow, OutputRun, PipelineOutput},
    },
    render::Badge,
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
        crate::render::DiffuseTheme::dark(),
    );
    assert_eq!(state.hunk_starts.len(), 2, "both hunks must be located");
    // Start from file header (row 0): the banner counts as inside hunk 1,
    // so next jumps to hunk 2's start
    state.row_idx = 0;
    state.next_hunk();
    assert_eq!(state.current_hunk(), Some(1));

    state.next_hunk();
    assert_eq!(
        state.current_hunk(),
        Some(0),
        "next_hunk must wrap past hunk 2"
    );

    state.prev_hunk();
    assert_eq!(state.current_hunk(), Some(1), "prev_hunk returns to hunk 2");
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
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Untracked,
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
                mov_side: None,
                new_line: Some(1),
                old_line: None,
                runs: vec![OutputRun::new(RunKind::Add, "fn new_untracked() {}")],
                wholesale: false,
                group: None,
            }],
        }],
        raw_hunks: Vec::new(),
        counts: FileCounts {
            adds: 1,
            dels: 0,
            mods: 0,
            movs: 0,
            notes: 0,
            ..Default::default()
        },
        fences: Vec::new(),
        notes: Vec::new(),
        content_runs: Vec::new(),
        rows: Vec::new(),
        is_binary: false,
        old_bytes: None,
        new_bytes: Some(21),
        raw_source: None,
    });

    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    assert!(file_1_rows[0].is_file_header());

    let frame = state.render_frame(100, 30);
    let plain_frame = crate::render::strip_ansi(&frame);
    assert!(
        plain_frame.contains("▶ untracked.rs 2/2 +1 (untracked, +21B) [collapsed]"),
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
    let plain_expanded = crate::render::strip_ansi(&frame_expanded);
    assert!(
        plain_expanded.contains("▼ untracked.rs 2/2 +1 (untracked, +21B)"),
        "frame shows expanded indicator"
    );
}

#[test]
fn test_renamed_files_collapsed_by_default_and_show_old_to_new_path() {
    let diff = r#"diff --git a/src/old.rs b/src/new.rs
similarity index 100%
rename from src/old.rs
rename to src/new.rs
"#;
    let mut new_content = HashMap::new();
    new_content.insert(
        "src/new.rs".to_string(),
        "fn alpha() {\n    println!(\"hello\");\n}\n".to_string(),
    );
    let out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    assert_eq!(out.files.len(), 1);
    assert_eq!(out.files[0].status, "renamed");
    assert_eq!(out.files[0].old_path.as_deref(), Some("src/old.rs"));
    assert_eq!(out.files[0].path, "src/new.rs");
    assert_eq!(out.files[0].rows.len(), 3);

    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    assert!(
        state.collapsed_files.contains(&0),
        "renamed file (index 0) must be collapsed by default"
    );
    assert_eq!(
        state.flat.len(),
        1,
        "collapsed file only has 1 flat row (header)"
    );

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("▶ ") && frame.contains("old.rs") && frame.contains("new.rs"),
        "frame shows collapsed indicator and old -> new path"
    );

    state.toggle_file_collapse(0);
    assert!(
        !state.collapsed_files.contains(&0),
        "renamed file is now expanded"
    );
    assert_eq!(
        state.flat.len(),
        4,
        "expanded file has header + 3 context rows"
    );

    let frame_expanded = state.render_frame(100, 30);
    assert!(
        frame_expanded.contains("▼ ")
            && frame_expanded.contains("old.rs")
            && frame_expanded.contains("new.rs"),
        "frame shows expanded indicator and old -> new path"
    );
    let raw_frame_expanded = crate::render::strip_ansi(&frame_expanded);
    assert!(
        raw_frame_expanded.contains("println!(\"hello\");"),
        "frame shows entire file content as context rows when expanded"
    );
}

#[test]
fn test_binary_files_collapsed_by_default_and_cannot_be_expanded() {
    let diff = r#"diff --git a/logo.png b/logo.png
index 1111111..2222222 100644
Binary files a/logo.png and b/logo.png differ
diff --git a/old.png b/new.png
similarity index 80%
rename from old.png
rename to new.png
Binary files a/old.png and b/new.png differ
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    assert_eq!(out.files.len(), 2);
    assert!(out.files[0].is_binary);
    assert!(out.files[1].is_binary);
    assert_eq!(out.files[0].display_status(), "binary");
    assert_eq!(out.files[1].display_status(), "binary");

    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    assert!(
        state.collapsed_files.contains(&0) && state.collapsed_files.contains(&1),
        "binary files must be collapsed by default"
    );
    assert_eq!(state.flat.len(), 2, "binary files only have header rows");

    let frame = state.render_frame(100, 30);
    let plain_frame = crate::render::strip_ansi(&frame);
    assert!(
        plain_frame.contains("════ logo.png 1/2"),
        "frame shows flat header rule without expand caret"
    );
    assert!(
        !frame.contains('▶') && !frame.contains('▼'),
        "frame must not show expand or collapse carets for binary files"
    );
    assert!(
        frame.contains("old.png") && frame.contains("new.png") && frame.contains("(binary)"),
        "frame shows rename with binary status"
    );

    // Attempting to expand binary file is a no-op
    state.toggle_file_collapse(0);
    assert!(
        state.collapsed_files.contains(&0),
        "binary file cannot be expanded"
    );
    assert_eq!(
        state.flat.len(),
        2,
        "binary file remains collapsed with 1 row"
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
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    let h2_txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: Some(state.output.files[0].hunks.len()),
        },
        &state.output.files[0].hunks[1],
        gap,
    );
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
        "/../../tests/golden/diffuse-2026-08/outputs/wire.jsonl"
    ));
    let jsonl_str = fs::read_to_string(wire_path).expect("must read diffuse-2026-08 wire.jsonl");
    let files = crate::core::wire::jsonl::decode_jsonl_str(&jsonl_str).unwrap();
    let output = PipelineOutput {
        files,
        branch: String::new(),
        untracked_skipped: vec![],
    };
    let mut state = AppState::new(
        output,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
        .position(|fr| fr.file_idx == fi && fr.is_file_header())
        .expect("file header row for PLAN.md");
    state.jump_to_row(plan_file_row, false);

    let frame = state.render_frame(140, 40);
    assert!(frame.contains("──── 1/"), "must contain H1 divider");
    assert!(frame.contains("──── 2/"), "must contain H2 divider");
}

#[test]
fn test_hunk_divider_not_placed_in_previous_hunk_deleted_lines() {
    let mut h0_rows = Vec::new();
    // Hunk 0: 5 context lines, then 5 deleted lines with old_line 107..=111
    for i in 1..=5 {
        h0_rows.push(OutputRow {
            kind: RowKind::Ctx,
            mov_side: None,
            new_line: Some(i),
            old_line: Some(i),
            runs: vec![OutputRun::new(RunKind::Ctx, format!("ctx {i}"))],
            wholesale: false,
            group: None,
        });
    }
    for old in 107..=111 {
        h0_rows.push(OutputRow {
            kind: RowKind::Del,
            mov_side: None,
            new_line: None,
            old_line: Some(old),
            runs: vec![OutputRun::new(RunKind::Del, format!("del {old}"))],
            wholesale: false,
            group: None,
        });
    }
    let h0 = OutputHunk {
        old_start: 1,
        old_count: 10,
        new_start: 1,
        new_count: 5,
        rows: h0_rows.clone(),
    };

    // Hunk 1: starts at new_line 111, old_line 283
    let mut h1_rows = Vec::new();
    for i in 0..5 {
        h1_rows.push(OutputRow {
            kind: RowKind::Add,
            mov_side: None,
            new_line: Some(111 + i),
            old_line: None,
            runs: vec![OutputRun::new(RunKind::Add, format!("add {}", 111 + i))],
            wholesale: false,
            group: None,
        });
    }
    let h1 = OutputHunk {
        old_start: 283,
        old_count: 0,
        new_start: 111,
        new_count: 5,
        rows: h1_rows.clone(),
    };

    let all_rows = [h0_rows, h1_rows].concat();
    let file = OutputFile {
        path: "test.rs".into(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".into(),
        counts: FileCounts {
            adds: 5,
            dels: 5,
            mods: 0,
            movs: 0,
            notes: 0,
            ..Default::default()
        },
        hunks: vec![h0, h1],
        raw_hunks: vec![],
        rows: all_rows,
        fences: vec![],
        notes: vec![],
        is_binary: false,
        new_line_count: 116,
        old_line_count: 300,
        new_bytes: Some(1000),
        old_bytes: Some(1000),
        new_content: vec![],
        old_content: vec![],
        raw_source: None,
        content_runs: vec![],
    };

    let output = PipelineOutput {
        files: vec![file],
        branch: "work".into(),
        untracked_skipped: vec![],
    };

    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    assert_eq!(state.hunk_starts.len(), 2);
    assert_eq!(
        state.hunk_starts[0].flat, 1,
        "Hunk 0 starts at flat row 1 (right after file header)"
    );
    // Hunk 0 has 10 rows (rows 1..=10), so Hunk 1 MUST start at flat row 11!
    assert_eq!(
        state.hunk_starts[1].flat, 11,
        "Hunk 1 must start after all 10 rows of Hunk 0, not inside deleted lines"
    );

    // Flat row 10 is the last row of Hunk 0 (deleted line 111).
    // It must belong to Hunk 0, NOT Hunk 1!
    state.row_idx = 10;
    assert_eq!(
        state.current_hunk(),
        Some(0),
        "Row 10 is the last deleted row of Hunk 0 and must belong to Hunk 0"
    );
}

#[test]
fn test_palette_rs_commit_hunk_starts_not_interleaved() {
    let diff_str = r#"diff --git a/palette.rs b/palette.rs
--- a/palette.rs
+++ b/palette.rs
@@ -1,3 +1,3 @@
-a
+b
 c
@@ -10,3 +10,3 @@
-d
+e
 f
"#;
    let pipeline_out = run_pipeline_full(diff_str, None, None, PipelineOptions::default());
    assert!(!pipeline_out.files.is_empty());
    let state = AppState::new(
        pipeline_out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let file = &state.output.files[0];
    assert_eq!(state.hunk_starts.len(), file.hunks.len());
    let mut expected_flat = 1;
    for (hi, hunk) in file.hunks.iter().enumerate() {
        assert_eq!(
            state.hunk_starts[hi].flat, expected_flat,
            "Hunk {hi} must start strictly after preceding hunks"
        );
        expected_flat += hunk.rows.len();
    }
}

#[test]
fn test_flat_and_hunk_starts_with_expanded_mods() {
    let diff_str = r#"diff --git a/f1.rs b/f1.rs
index 1111111..2222222 100644
--- a/f1.rs
+++ b/f1.rs
@@ -1,3 +1,3 @@
 fn f1() {
-    let val_a = 1;
+    let val_a = 2;
 }
diff --git a/f2.rs b/f2.rs
index 3333333..4444444 100644
--- a/f2.rs
+++ b/f2.rs
@@ -1,3 +1,3 @@
 fn f2() {
-    let val_b = 3;
+    let val_b = 4;
 }
"#;
    let pipeline_out = run_pipeline_full(diff_str, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        pipeline_out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    assert_eq!(state.hunk_starts.len(), 2);
    let f1_hunk_start_initial = state.hunk_starts[1].flat;

    // Expand all mod rows
    state.execute(crate::command::TuiCommand::ToggleExpandAllMods);
    assert_eq!(state.expanded_mods.len(), 2);

    // File 0 has one expanded mod (which added 1 extra flat row)
    // Therefore File 1's hunk start must have shifted by +1
    assert_eq!(state.hunk_starts[1].flat, f1_hunk_start_initial + 1);

    // Check flat rows in file 0
    let file0_rows: Vec<_> = state
        .flat
        .iter()
        .filter(|r| r.file_idx == 0 && r.is_diff_row())
        .collect();
    assert_eq!(file0_rows.len(), 4); // Ctx, Del, Add, Ctx
    assert_eq!(file0_rows[1].mod_side(), Some(diffuse_core::ModSide::Del));
    assert_eq!(file0_rows[2].mod_side(), Some(diffuse_core::ModSide::Add));
}

#[test]
fn test_adjacent_expanded_mods_merged_cluster_flat_order() {
    let diff_str = r#"diff --git a/f.rs b/f.rs
index 1111111..2222222 100644
--- a/f.rs
+++ b/f.rs
@@ -1,5 +1,5 @@
 fn f() {
-    let a = 1;
-    let b = 2;
-    let c = 3;
+    let a = 10;
+    let b = 20;
+    let c = 30;
 }
"#;
    let pipeline_out = run_pipeline_full(diff_str, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        pipeline_out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // Initial state: 5 rows (Ctx, Mod 0, Mod 1, Mod 2, Ctx)
    let diff_rows: Vec<_> = state.flat.iter().filter(|r| r.is_diff_row()).collect();
    assert_eq!(diff_rows.len(), 5);

    // Expand all mod rows
    state.execute(crate::command::TuiCommand::ToggleExpandAllMods);
    assert_eq!(state.expanded_mods.len(), 3);

    // Flat rows should now be: Ctx, Del 0, Del 1, Del 2, Add 0, Add 1, Add 2, Ctx
    let diff_rows_expanded: Vec<_> = state.flat.iter().filter(|r| r.is_diff_row()).collect();
    assert_eq!(diff_rows_expanded.len(), 8);

    assert!(!diff_rows_expanded[0].is_expanded_mod()); // Ctx

    // 3 Dels
    assert_eq!(
        diff_rows_expanded[1].mod_side(),
        Some(diffuse_core::ModSide::Del)
    );
    assert_eq!(diff_rows_expanded[1].diff_row_idx(), Some(1));
    assert_eq!(
        diff_rows_expanded[1].mod_glyph(),
        Some(diffuse_core::ModGlyph::Top)
    );

    assert_eq!(
        diff_rows_expanded[2].mod_side(),
        Some(diffuse_core::ModSide::Del)
    );
    assert_eq!(diff_rows_expanded[2].diff_row_idx(), Some(2));
    assert_eq!(
        diff_rows_expanded[2].mod_glyph(),
        Some(diffuse_core::ModGlyph::Mid)
    );

    assert_eq!(
        diff_rows_expanded[3].mod_side(),
        Some(diffuse_core::ModSide::Del)
    );
    assert_eq!(diff_rows_expanded[3].diff_row_idx(), Some(3));
    assert_eq!(
        diff_rows_expanded[3].mod_glyph(),
        Some(diffuse_core::ModGlyph::Mid)
    );

    // 3 Adds
    assert_eq!(
        diff_rows_expanded[4].mod_side(),
        Some(diffuse_core::ModSide::Add)
    );
    assert_eq!(diff_rows_expanded[4].diff_row_idx(), Some(1));
    assert_eq!(
        diff_rows_expanded[4].mod_glyph(),
        Some(diffuse_core::ModGlyph::Mid)
    );

    assert_eq!(
        diff_rows_expanded[5].mod_side(),
        Some(diffuse_core::ModSide::Add)
    );
    assert_eq!(diff_rows_expanded[5].diff_row_idx(), Some(2));
    assert_eq!(
        diff_rows_expanded[5].mod_glyph(),
        Some(diffuse_core::ModGlyph::Mid)
    );

    assert_eq!(
        diff_rows_expanded[6].mod_side(),
        Some(diffuse_core::ModSide::Add)
    );
    assert_eq!(diff_rows_expanded[6].diff_row_idx(), Some(3));
    assert_eq!(
        diff_rows_expanded[6].mod_glyph(),
        Some(diffuse_core::ModGlyph::Bottom)
    );

    assert!(!diff_rows_expanded[7].is_expanded_mod()); // Ctx

    // Render viewport lines and verify rendered line prefixes
    let (rendered_lines, ..) = state.render_diff_viewport_lines(80, 20, 20);
    let line_texts: Vec<String> = rendered_lines
        .into_iter()
        .map(|l| l.spans.into_iter().map(|s| s.content.to_string()).collect())
        .collect();
    let mod_lines: Vec<&String> = line_texts
        .iter()
        .filter(|t| t.contains("let a") || t.contains("let b") || t.contains("let c"))
        .collect();
    assert_eq!(mod_lines.len(), 6);
    assert!(mod_lines[0].contains("┌-") && mod_lines[0].contains("let a = 1"));
    assert!(mod_lines[1].contains("│-") && mod_lines[1].contains("let b = 2"));
    assert!(mod_lines[2].contains("│-") && mod_lines[2].contains("let c = 3"));
    assert!(mod_lines[3].contains("│+") && mod_lines[3].contains("let a = 10"));
    assert!(mod_lines[4].contains("│+") && mod_lines[4].contains("let b = 20"));
    assert!(mod_lines[5].contains("└+") && mod_lines[5].contains("let c = 30"));

    // Toggle collapse on row index 2 (b)
    state.row_idx = state
        .flat
        .iter()
        .position(|r| {
            r.diff_row_idx() == Some(2) && r.mod_side() == Some(diffuse_core::ModSide::Del)
        })
        .unwrap();
    state.toggle_current_mod_row();
    assert_eq!(state.expanded_mods.len(), 2);

    // After collapsing 'b', cluster splits:
    // Cluster 1 ('a'): Del 0 (Top), Add 0 (Bottom)
    // Collapsed 'b': Mod 1
    // Cluster 2 ('c'): Del 2 (Top), Add 2 (Bottom)
    let diff_rows_partial: Vec<_> = state.flat.iter().filter(|r| r.is_diff_row()).collect();
    assert_eq!(diff_rows_partial.len(), 7); // Ctx, Del 0, Add 0, Mod 1, Del 2, Add 2, Ctx

    assert_eq!(diff_rows_partial[1].diff_row_idx(), Some(1));
    assert_eq!(
        diff_rows_partial[1].mod_side(),
        Some(diffuse_core::ModSide::Del)
    );
    assert_eq!(
        diff_rows_partial[1].mod_glyph(),
        Some(diffuse_core::ModGlyph::Top)
    );

    assert_eq!(diff_rows_partial[2].diff_row_idx(), Some(1));
    assert_eq!(
        diff_rows_partial[2].mod_side(),
        Some(diffuse_core::ModSide::Add)
    );
    assert_eq!(
        diff_rows_partial[2].mod_glyph(),
        Some(diffuse_core::ModGlyph::Bottom)
    );

    assert_eq!(diff_rows_partial[3].diff_row_idx(), Some(2));
    assert!(!diff_rows_partial[3].is_expanded_mod()); // Collapsed

    assert_eq!(diff_rows_partial[4].diff_row_idx(), Some(3));
    assert_eq!(
        diff_rows_partial[4].mod_side(),
        Some(diffuse_core::ModSide::Del)
    );
    assert_eq!(
        diff_rows_partial[4].mod_glyph(),
        Some(diffuse_core::ModGlyph::Top)
    );

    assert_eq!(diff_rows_partial[5].diff_row_idx(), Some(3));
    assert_eq!(
        diff_rows_partial[5].mod_side(),
        Some(diffuse_core::ModSide::Add)
    );
    assert_eq!(
        diff_rows_partial[5].mod_glyph(),
        Some(diffuse_core::ModGlyph::Bottom)
    );
}
