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

    let mut state = AppState::new(initial_output, "test_repo".into(), temp_notes_path());
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
        "/../../tests/golden/mirage-2025-08/wire.json"
    ));
    let json_str = fs::read_to_string(wire_path).expect("must read mirage-2025-08/wire.json");
    let output: PipelineOutput = serde_json::from_str(&json_str).unwrap();

    // Simulate streaming: start with empty AppState and append files one by one
    let mut empty_output = output.clone();
    empty_output.files.clear();
    let mut state = AppState::new(empty_output, "mirage".into(), temp_notes_path());

    for file in output.files {
        state.append_file(file);
    }

    // Find cache.rs file index
    let fi = state
        .output
        .files
        .iter()
        .position(|f| f.path.contains("cache.rs"))
        .expect("cache.rs must exist");

    // Jump to cache.rs
    let cache_file_row = state
        .flat
        .iter()
        .position(|fr| fr.file_idx == fi && fr.is_file_header)
        .expect("file header row for cache.rs");
    state.jump_to_row(cache_file_row, false);

    let cache_hunks: Vec<_> = state
        .hunk_starts
        .iter()
        .filter(|hs| hs.file == fi)
        .collect();
    assert_eq!(
        cache_hunks.len(),
        state.output.files[fi].hunks.len(),
        "streaming append_file must register hunk starts for all hunks in cache.rs"
    );

    let frame = state.render_frame(140, 40);
    assert!(
        frame.contains("───── H1 ·"),
        "streaming render must contain H1 divider"
    );
    assert!(
        frame.contains("───── H2 ·"),
        "streaming render must contain H2 divider"
    );
}
