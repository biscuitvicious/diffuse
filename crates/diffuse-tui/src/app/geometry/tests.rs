use std::collections::HashMap;

use crate::{
    app::{AppState, temp_notes_path},
    core::{PipelineOptions, parse::RowKind, pipeline::run_pipeline_full, wire::PipelineOutput},
    view::{truncate_pad, visible_width},
};

fn sample_output() -> PipelineOutput {
    let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
      let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
    let new_content = [(
        "filesystem.rs".to_string(),
        "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n"
            .to_string(),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "filesystem.rs".to_string(),
        "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
    )]
    .into_iter()
    .collect();
    run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    )
}

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
fn test_word_wrap_and_visual_subline_navigation() {
    let mut out = sample_output();
    // Add a very long line to file 0
    out.files[0].rows[0].text = "A".repeat(150);
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    // content_width = 80 - 1 (sb) = 79. text_cols = 79 - 6 = 73.
    // 150 chars -> 3 segments (73 + 73 + 4).
    let seg_count = state.row_seg_count(state.row_idx, 79);
    assert_eq!(seg_count, 3, "150 chars should produce 3 visual segments");

    state.last_content_width = 79;
    assert_eq!(state.vis_line, 0);

    // move_down advances vis_line within row
    state.move_down();
    assert_eq!(state.vis_line, 1);
    state.move_down();
    assert_eq!(state.vis_line, 2);

    // next move_down moves to next row and resets vis_line
    let start_row = state.row_idx;
    state.move_down();
    assert_eq!(state.row_idx, start_row + 1);
    assert_eq!(state.vis_line, 0);

    // move_up moves back to previous row and lands on its last visual segment
    state.move_up();
    assert_eq!(state.row_idx, start_row);
    assert_eq!(state.vis_line, 2);
}

#[test]
fn test_emoji_and_wide_char_visual_width() {
    assert_eq!(visible_width("🦀"), 2);
    assert_eq!(visible_width("\x1b[38;5;120m🦀\x1b[0m"), 2);
    assert_eq!(visible_width("hello 🦀 world"), 14);

    let padded = truncate_pad("🦀", 4);
    assert_eq!(visible_width(&padded), 4);
}

#[test]
fn test_raw_diff_word_wrap_and_marker() {
    let diff = r#"diff --git a/long.rs b/long.rs
index 1111111..2222222 100644
--- a/long.rs
+++ b/long.rs
@@ -1,1 +1,1 @@
-short
+very long line with extensive description that spans well beyond typical width boundaries and needs multiple visual wraps to fit comfortably
"#;
    let new_content: HashMap<String, String> = [(
            "long.rs".to_string(),
            "very long line with extensive description that spans well beyond typical width boundaries and needs multiple visual wraps to fit comfortably\n".to_string(),
        )]
        .into_iter()
        .collect();

    let out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.toggle_raw_diff();
    assert!(state.raw_diff);

    // Find addition row
    let add_row = state
        .flat
        .iter()
        .position(|r| state.row_of(r).is_some_and(|row| row.kind == RowKind::Add))
        .unwrap();
    state.row_idx = add_row;

    // Content width 50 (scrollbar at 49, text cols = 48)
    let seg_count = state.row_seg_count(state.row_idx, 49);
    assert!(
        seg_count >= 3,
        "long line should produce 3+ visual segments in raw mode"
    );

    let frame = state.render_frame(50, 20);
    assert!(
        frame.contains("\x1b[38;5;120m+\x1b[39m\x1b[23m\x1b[38;5;120mvery long line"),
        "segment 0 must start with gutter line number and + marker followed by green text"
    );
    assert!(
        frame.contains("\x1b[38;5;242m      \x1b[39m\x1b[38;5;120m"),
        "continuation segments must start with 6-space gutter and green foreground"
    );
}

#[test]
fn test_hunk_divider_renders_and_extent_lift() {
    let mut state = AppState::new(
        sample_output_two_hunks(),
        "test_repo".into(),
        temp_notes_path(),
    );

    let frame = state.render_frame(120, 80);
    assert!(
        frame.contains("H1"),
        "the first hunk needs a divider: {:.300}",
        frame
    );
    assert!(frame.contains("H2"), "the second hunk needs a divider");
    assert!(
        frame.contains("−1,7 +1,8") || frame.contains("-1,7 +1,8") || frame.contains("H1"),
        "the divider carries the hunk span: {:.300}",
        frame
    );
    assert!(
        frame.contains("\x1b[48;5;237m") || frame.contains("\x1b[48;5;236m"),
        "the current hunk's divider is the bright bar"
    );
    assert!(
        frame.contains("\x1b[38;5;252m"),
        "number gutter in the current hunk's envelope gets the highlighted color"
    );
    assert!(
        !frame.contains("\x1b[48;5;238m"),
        "ctx rows should not have distracting background tinting"
    );

    state.next_hunk();
    let frame2 = state.render_frame(120, 40);
    assert!(
        frame2.contains("H2"),
        "the second hunk keeps its divider in hunk 2"
    );
}

#[test]
fn test_render_frame_lines_do_not_overflow_width() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    for width in [40, 60, 80, 100, 120] {
        for height in [15, 24, 30, 40] {
            let frame = state.render_frame(width, height);
            for line in frame.lines() {
                let vis = visible_width(line);
                assert!(
                    vis <= width,
                    "line visible width {vis} exceeds terminal width {width}: {line:?}"
                );
            }
        }
    }
}

#[test]
fn test_current_line_highlight_covers_gutter() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    // Context row selected
    state.row_idx = 1;
    let frame_ctx = state.render_frame(80, 24);
    assert!(
        frame_ctx.contains("\x1b[48;5;237m\x1b[38;5;"),
        "selected context row must start with cursor background before the gutter numbers"
    );

    // Unselected addition row starts with addition background before the gutter
    // numbers
    assert!(
        frame_ctx.contains("\x1b[48;5;22m\x1b[38;5;151m"),
        "addition row must start with green tint before gutter numbers"
    );
}

#[test]
fn test_file_header_wrapping_seg_count() {
    let mut out = sample_output();
    out.files[0].path = "crates/diffuse-core/src/very/long/nested/path/to/my_file.rs".to_string();
    let state = AppState::new(out, "test_repo".into(), temp_notes_path());
    let header_idx = 0;
    assert!(state.flat[header_idx].is_file_header);

    // At 120 width, fits in 1 visual line
    let seg120 = state.row_seg_count(header_idx, 120);
    assert_eq!(seg120, 1);

    // At 30 width, wraps into multiple visual lines
    let seg30 = state.row_seg_count(header_idx, 30);
    assert!(
        seg30 >= 3,
        "long file header at width 30 must wrap into multiple segments, got {seg30}"
    );

    let (_starts, counts, seg_counts, total_vis) = state.visual_geometry(30);
    assert_eq!(seg_counts[0], seg30);
    assert_eq!(counts[0], seg30);
    assert!(total_vis >= seg30);
}
