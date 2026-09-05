use super::*;
use crate::core::PipelineOptions;

#[test]
fn test_output_ends_with_terminal_reset() {
    let diff = "diff --git a/a.txt b/a.txt\nindex 1111111..2222222 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line one\n+new line one\n";
    let new_content = [("a.txt".to_string(), "new line one\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line one\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let rendered = render_pipeline_output(&out, &DiffuseTheme::MAGIC, true);
    assert!(
        rendered.ends_with("\x1b[0m"),
        "terminal must be left in a clean SGR state"
    );
}

#[test]
fn test_render_raw_pipeline_output() {
    let diff = "diff --git a/a.txt b/a.txt\nindex 1111111..2222222 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line one\n+new line one\n";
    let new_content = [("a.txt".to_string(), "new line one\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line one\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let rendered = render_raw_pipeline_output(&out);
    assert!(rendered.ends_with("\x1b[0m"));
    assert!(rendered.contains("a.txt"));
    // Must contain @@ hunk header
    assert!(rendered.contains("@@ -1,2 +1,2 @@"));
    // Must contain colorized number gutter
    assert!(rendered.contains("\x1b[38;5;151m") || rendered.contains("\x1b[3;38;5;174m"));
    // Must contain colorized change symbols + / -
    assert!(rendered.contains("\x1b[38;5;120m+") || rendered.contains("\x1b[38;5;203m-"));
    // No mod ~ symbols in diff rows
    let diff_body: Vec<_> = rendered.lines().skip(1).collect();
    for line in &diff_body {
        assert!(!line.contains("\x1b[38;5;214m~"));
    }

    // Also test with empty raw_hunks fallback
    let mut file_fallback = out.files[0].clone();
    file_fallback.raw_hunks.clear();
    file_fallback.counts.mods = 1;
    file_fallback.counts.movs = 1;
    let single_raw = render_raw_file_diff(&file_fallback);
    assert!(single_raw.contains("~1"));
    assert!(single_raw.contains("moved"));
    assert!(single_raw.contains("@@ -1,2 +1,2 @@"));
}

#[test]
fn test_render_renamed_file_diff() {
    let file = OutputFile {
        path: "src/new.rs".to_string(),
        old_path: Some("src/old.rs".to_string()),
        old_mode: None,
        new_mode: None,
        status: "renamed".to_string(),
        new_line_count: 0,
        old_line_count: 0,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        bytes: 0,
        old_bytes: 0,
        new_bytes: 0,
    };
    let visual = render_file_diff(&file, &DiffuseTheme::MAGIC, true);
    assert!(visual.contains("src/old.rs -> new.rs (0B)"));

    let raw = render_raw_file_diff(&file);
    assert!(raw.contains("src/old.rs -> new.rs (0B)"));
}
