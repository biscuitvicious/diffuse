use super::*;
use crate::core::PipelineOptions;
use diffuse_core::{notes::Side as NoteSide, wire::OutputNote};

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
    let rendered = render_pipeline_output(&out, &DiffuseTheme::DEFAULT, true);
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
    assert!(rendered.contains("\x1b[38;5;120m") || rendered.contains("\x1b[38;5;203m"));
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
    let single_raw = render_raw_file_diff(&file_fallback, None);
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
        old_bytes: Some(0),
        new_bytes: Some(0),
        raw_source: None,
    };
    let visual = render_file_diff(&file, None, &DiffuseTheme::DEFAULT, true);
    assert!(
        crate::ansi::strip_ansi(&visual).contains("src/old.rs -> new.rs (renamed, 0B)"),
        "zero counts omitted, status parens follow the path: {visual:?}"
    );

    let raw = render_raw_file_diff(&file, None);
    assert!(
        crate::ansi::strip_ansi(&raw).contains("src/old.rs -> new.rs (renamed, 0B)"),
        "zero counts omitted, status parens follow the path: {raw:?}"
    );
}

#[test]
fn test_zero_notes_not_rendered() {
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
    // Styled render: zero notes must be invisible.
    let rendered = render_file_diff(&out.files[0], None, &DiffuseTheme::DEFAULT, true);
    assert!(
        !rendered.contains("0 note"),
        "styled render must not show '0 notes' when there are no notes"
    );
    // Raw render: same requirement.
    let raw = render_raw_file_diff(&out.files[0], None);
    assert!(
        !raw.contains("0 note"),
        "raw render must not show '0 notes' when there are no notes"
    );
}

#[test]
fn test_nonzero_notes_still_rendered() {
    let file = OutputFile {
        path: "src/lib.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 1,
        old_line_count: 1,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts::default(),
        fences: vec![],
        notes: vec![OutputNote {
            id: "n1".to_string(),
            file: "src/lib.rs".to_string(),
            side: NoteSide::New,
            start: 1,
            end: 1,
            text: "a note".to_string(),
            created_at: 0,
            runs: vec![],
            line_hash: None,
        }],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    let rendered = render_file_diff(&file, None, &DiffuseTheme::DEFAULT, true);
    assert!(
        rendered.contains("1 note"),
        "styled render must show '1 note' when there is 1 note"
    );
    let raw = render_raw_file_diff(&file, None);
    assert!(
        raw.contains("1 note"),
        "raw render must show '1 note' when there is 1 note"
    );
}

#[test]
fn test_write_file_header_bar_direct_buffer() {
    let mut file = OutputFile {
        path: "src/lib.rs".to_string(),
        old_path: None,
        old_mode: None,
        new_mode: None,
        status: "modified".to_string(),
        new_line_count: 1,
        old_line_count: 1,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: diffuse_core::counts::FileCounts {
            adds: 5,
            dels: 2,
            mods: 3,
            movs: 1,
            notes: 0,
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
    };
    let mut buf = String::new();
    write_file_header_bar(&mut buf, &file, None, &DiffuseTheme::DEFAULT).unwrap();
    assert!(buf.starts_with("\x1b[48;5;234m\x1b[1m\x1b[38;5;231m"));
    assert!(buf.contains("+5"));
    assert!(buf.contains("−2"));
    assert!(buf.contains("~3"));
    assert!(buf.contains("≈1"));
    assert!(buf.ends_with(" \x1b[0m\n"));
    assert!(!buf.contains("(modified)"));

    // And with added status:
    file.status = "added".to_string();
    let mut buf_added = String::new();
    write_file_header_bar(&mut buf_added, &file, None, &DiffuseTheme::DEFAULT).unwrap();
    assert!(buf_added.contains("(added)"));
    // Badged variant carries the file position badge.
    let mut buf_badged = String::new();
    write_file_header_bar(
        &mut buf_badged,
        &file,
        Some(Badge { idx: 0, total: 2 }),
        &DiffuseTheme::DEFAULT,
    )
    .unwrap();
    assert!(
        buf_badged.contains("F1/2"),
        "badged banner must show file position: {buf_badged:?}"
    );
}

#[test]
fn test_render_file_diff_resets_background_at_end_of_lines() {
    let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-old line\n+new line\n";
    let new_content = [("a.txt".to_string(), "new line\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("a.txt".to_string(), "old line\n".to_string())]
        .into_iter()
        .collect();
    let out = crate::core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let rendered = render_file_diff(&out.files[0], None, &DiffuseTheme::DEFAULT, true);
    let rows: Vec<_> = rendered.lines().skip(1).collect();
    assert!(!rows.is_empty(), "must have rendered diff rows");
    for line in rows {
        assert!(
            line.ends_with("\x1b[0m") || line.ends_with("\x1b[49m"),
            "each diff line must reset background/SGR at end of line to prevent margin color bleeding, but got: {line:?}"
        );
    }
}
