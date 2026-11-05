use std::{fs, path::Path};

use crate::{
    app::{AppState, temp_notes_path},
    core::{
        PipelineOptions,
        streaming::stream_diff_pipeline,
        wire::{OutputFile, PipelineOutput},
    },
};
#[test]
fn test_incremental_file_append_and_streaming_navigation() {
    let diff = "diff --git a/file1.rs b/file1.rs\n--- a/file1.rs\n+++ b/file1.rs\n@@ -1,2 +1,2 @@\n-fn one() {}\n+fn one_mod() {}\ndiff --git a/file2.rs b/file2.rs\n--- a/file2.rs\n+++ b/file2.rs\n@@ -1,2 +1,2 @@\n-fn two() {}\n+fn two_mod() {}\n";
    let new_content = [
        ("file1.rs".to_string(), "fn one_mod() {}\n".to_string()),
        ("file2.rs".to_string(), "fn two_mod() {}\n".to_string()),
    ]
    .into_iter()
    .collect();

    let consumer = stream_diff_pipeline(
        diff,
        Some(new_content),
        None,
        PipelineOptions::default(),
        Some(2),
    );

    let initial_output = PipelineOutput {
        files: Vec::new(),
        branch: "main".into(),
        untracked_skipped: Vec::new(),
    };

    let mut state = AppState::new(
        initial_output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.is_streaming = true;
    assert_eq!(state.output.files.len(), 0);

    let files: Vec<OutputFile> = consumer.collect();
    assert_eq!(files.len(), 2);

    // Ingest file 0
    state.append_file(files[0].clone());
    assert_eq!(state.output.files.len(), 1);
    assert_eq!(state.current_file_idx(), 0);
    assert_eq!(state.output.files[0].path, "file1.rs");

    // Ingest file 1
    state.append_file(files[1].clone());
    assert_eq!(state.output.files.len(), 2);
    assert_eq!(state.output.files[1].path, "file2.rs");

    // Navigate to next file
    state.next_file();
    assert_eq!(state.current_file_idx(), 1);
    assert_eq!(state.current_file().unwrap().path, "file2.rs");

    // Navigate to prev file
    state.prev_file();
    assert_eq!(state.current_file_idx(), 0);
    assert_eq!(state.current_file().unwrap().path, "file1.rs");
}

#[test]
fn test_streaming_append_file_multi_hunk_with_gap() {
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

    // Simulate streaming: start with empty AppState and append files one by one
    let mut empty_output = output.clone();
    empty_output.files.clear();
    let mut state = AppState::new(
        empty_output,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    for file in output.files {
        state.append_file(file);
    }

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

    let plan_hunks: Vec<_> = state
        .hunk_starts
        .iter()
        .filter(|hs| hs.file == fi)
        .collect();
    assert_eq!(
        plan_hunks.len(),
        state.output.files[fi].hunks.len(),
        "streaming append_file must register hunk starts for all hunks in PLAN.md"
    );

    let frame = state.render_frame(140, 40);
    assert!(
        frame.contains("──── H1/"),
        "streaming render must contain H1 divider"
    );
    assert!(
        frame.contains("──── H2/"),
        "streaming render must contain H2 divider"
    );
}

#[test]
fn test_reset_for_streaming_reload() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n-fn one() {}\n+fn one_mod() {}\n";
    let new_content = [("a.rs".to_string(), "fn one_mod() {}\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        None,
        PipelineOptions::default(),
    );
    let mut state = AppState::new(
        out,
        "test".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    assert_eq!(state.output.files.len(), 1);
    assert!(!state.flat.is_empty());
    assert!(!state.hunk_starts.is_empty());

    state.reset_for_streaming_reload();

    assert!(state.output.files.is_empty());
    assert!(state.flat.is_empty());
    assert!(state.hunk_starts.is_empty());
    assert!(state.hunk_meta.is_empty());
    assert!(state.collapsed_files.is_empty());
    assert_eq!(state.row_idx, 0);
    assert_eq!(state.vis_line, 0);
    assert_eq!(state.scroll, 0);
    assert_eq!(state.col_offset, 0);
    assert!(state.is_streaming);
    assert!(state.last_stream_activity.is_some());
}

#[test]
fn test_reload_files_preserves_cursor_and_screen_position() {
    let diff = "diff --git a/file1.rs b/file1.rs\n--- a/file1.rs\n+++ b/file1.rs\n@@ -1,2 +1,2 @@\n-fn one() {}\n+fn one_mod() {}\ndiff --git a/file2.rs b/file2.rs\n--- a/file2.rs\n+++ b/file2.rs\n@@ -1,2 +1,2 @@\n-fn two() {}\n+fn two_mod() {}\n";
    let new_content = [
        ("file1.rs".to_string(), "fn one_mod() {}\n".to_string()),
        ("file2.rs".to_string(), "fn two_mod() {}\n".to_string()),
    ]
    .into_iter()
    .collect();

    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        None,
        PipelineOptions::default(),
    );
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 3;
    state.terminal_width = 80;
    state.last_content_width = 80;

    // Navigate to file2.rs and set cursor and scroll
    state.next_file();
    assert_eq!(state.current_file().unwrap().path, "file2.rs");
    state.scroll = 2; // Simulate being scrolled down 2 lines

    let snapshot = state.capture_cursor_snapshot();
    assert_eq!(snapshot.file_path, "file2.rs");

    // Simulate reloading file1.rs only
    let updated_file1_diff = "diff --git a/file1.rs b/file1.rs\n--- a/file1.rs\n+++ b/file1.rs\n@@ -1,2 +1,2 @@\n-fn one() {}\n+fn one_v2() {}\n";
    let updated_file1_content = [("file1.rs".to_string(), "fn one_v2() {}\n".to_string())]
        .into_iter()
        .collect();
    let updated_out = crate::core::pipeline::run_pipeline_full(
        updated_file1_diff,
        Some(updated_file1_content),
        None,
        PipelineOptions::default(),
    );

    state.reload_files(updated_out.files, &["file1.rs".to_string()]);

    // Cursor must still be on file2.rs at the same screen Y position!
    assert_eq!(state.current_file().unwrap().path, "file2.rs");
    let current_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);
    assert_eq!(
        current_screen_y, snapshot.screen_y,
        "screen Y position must be preserved after reload"
    );
}

#[test]
fn test_replace_or_insert_file_preserves_cursor() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,2 +1,2 @@\n-fn one() {}\n+fn one_mod() {}\n";
    let new_content = [("a.rs".to_string(), "fn one_mod() {}\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        None,
        PipelineOptions::default(),
    );
    let mut state = AppState::new(
        out,
        "test".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.row_idx = 1;
    let snapshot = state.capture_cursor_snapshot();

    // Replace file in-place
    let same_file = state.output.files[0].clone();
    state.replace_or_insert_file(same_file, &snapshot);
    assert_eq!(state.row_idx, 1);
    assert_eq!(state.output.files.len(), 1);
}
