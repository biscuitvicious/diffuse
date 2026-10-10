use std::collections::HashMap;

use crate::{
    app::{AppState, temp_notes_path},
    core::{
        PipelineOptions,
        merge::RunKind,
        parse::RowKind,
        pipeline::run_pipeline_full,
        wire::{OutputRun, PipelineOutput},
    },
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
    // Add a very long line to file 0 (row 1 in diff, not the hunk divider row)
    out.files[0].rows[1].runs = vec![OutputRun::new(RunKind::Ctx, "A".repeat(150))];
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.row_idx = 2;
    // content_width = 80 - 1 (sb) = 79. text_cols = 79 - 6 = 73.
    // 150 chars -> 3 segments (73 + 73 + 4).
    let seg_count = state.row_seg_count(state.row_idx, 79);
    assert_eq!(seg_count, 3, "150 chars should produce 3 visual segments");

    state.last_content_width = 79;
    state.vis_line = 0;

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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
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
        crate::render::DiffuseTheme::DEFAULT,
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
        frame.contains("\x1b[48;5;235m") || frame.contains("\x1b[48;5;236m"),
        "the current hunk's divider is the bright bar"
    );
    assert!(
        frame.contains("\x1b[38;5;245m"),
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
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
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
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    // Context row selected (row 1, vis_line 1 following the hunk divider at
    // vis_line 0)
    state.row_idx = 1;
    state.vis_line = 1;
    let frame_ctx = state.render_frame(80, 24);
    assert!(
        frame_ctx.contains("\x1b[48;5;237m\x1b[38;5;"),
        "selected context row must start with cursor background before the gutter numbers"
    );

    // Unselected addition row starts with addition background before the gutter
    // numbers
    assert!(
        frame_ctx.contains("\x1b[48;2;28;50;30m\x1b[38;5;120m"),
        "addition row must start with green tint before gutter numbers"
    );
}

#[test]
fn test_file_header_wrapping_seg_count() {
    let mut out = sample_output();
    out.files[0].path = "crates/diffuse-core/src/very/long/nested/path/to/my_file.rs".to_string();
    let state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    let header_idx = 0;
    assert!(state.flat[header_idx].is_file_header());

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

#[test]
fn test_scrollbar_thumb_snaps_only_at_edges() {
    // Pure thumb geometry over a full-height track (taller than the
    // viewport: chrome rows included).
    let (track_h, vh, total) = (14, 10, 60);
    let max_scroll = total - vh;
    let header_vis = 1;
    let geom = |scroll: usize| {
        AppState::scrollbar_thumb_for(track_h, vh, total, scroll, header_vis)
            .expect("scrollbar must apply")
    };

    let (_, size) = geom(0);
    assert!(size >= 2, "thumb must be at least 2 lines high, got {size}");

    let (off_top, _) = geom(0);
    assert_eq!(off_top, 0, "header in view must put thumb at top");

    let (off_high, size_high) = geom(1);
    assert!(
        off_high >= 1,
        "header out of view must keep thumb strictly below the top, got {off_high}"
    );
    assert!(
        off_high + size_high < track_h,
        "thumb near top must not touch the last row"
    );

    let (off_low, size_low) = geom(max_scroll - 1);
    assert!(
        off_low + size_low < track_h,
        "one row still hidden must keep thumb off the last row, got {}",
        off_low + size_low
    );

    let (off_bottom, size_bottom) = geom(max_scroll);
    assert_eq!(
        off_bottom + size_bottom,
        track_h,
        "final line in view must put thumb at the bottom"
    );
}

#[test]
fn test_scrollbar_scroll_for_row_formula() {
    // Proportional core: row 0 yields 0, the last row yields max_scroll,
    // everything between is exact. Live-log scale: 2672-line content on
    // a 74-row track.
    assert_eq!(AppState::scrollbar_scroll_for_row(74, 2672, 0), 0);
    assert_eq!(AppState::scrollbar_scroll_for_row(74, 2672, 73), 2672);
    assert_eq!(AppState::scrollbar_scroll_for_row(74, 2672, 1), 37);
    assert_eq!(AppState::scrollbar_scroll_for_row(74, 2672, 72), 2672 - 37);
    assert_eq!(AppState::scrollbar_scroll_for_row(74, 2672, 2), 73);
    // Pointer past the end clamps to the last row.
    assert_eq!(AppState::scrollbar_scroll_for_row(74, 2672, 1000), 2672);
    // Degenerate inputs yield 0, never divide by zero.
    assert_eq!(AppState::scrollbar_scroll_for_row(74, 0, 5), 0);
    assert_eq!(AppState::scrollbar_scroll_for_row(1, 100, 0), 0);
    // Monotone across the track.
    let mut prev = 0;
    for r in 1..74 {
        let s = AppState::scrollbar_scroll_for_row(74, 2672, r);
        assert!(s >= prev, "mapping must not go backwards at row {r}");
        prev = s;
    }
}

#[test]
fn test_wrapped_line_never_overflows_scrollbar_gutter() {
    let diff = r#"diff --git a/render.txt b/render.txt
--- a/render.txt
+++ b/render.txt
@@ -4316,1 +3320,1 @@
-{48;5;22}{38;5;151}4316 {38;5;120}+{39}{23}{48;5;22}		{48;5;22}{48;5;22}{38;5;150}// the tokens stay in one coordinate space. Wholesale mods yield the{39}{48;5;22}{48;5;22}{0}
+{48;5;22}{38;5;151}3320 {38;5;120}+{39}{23}{48;5;22}		{48;5;22}{48;5;22}{38;5;150}// the tokens stay in one coordinate space. Wholesale mods yield the{39}{48;5;22}{48;5;22}{0}
"#;
    let old_line = "{48;5;22}{38;5;151}4316 {38;5;120}+{39}{23}{48;5;22}\t\t{48;5;22}{48;5;22}{38;5;150}// the tokens stay in one coordinate space. Wholesale mods yield the{39}{48;5;22}{48;5;22}{0}\n";
    let new_line = "{48;5;22}{38;5;151}3320 {38;5;120}+{39}{23}{48;5;22}\t\t{48;5;22}{48;5;22}{38;5;150}// the tokens stay in one coordinate space. Wholesale mods yield the{39}{48;5;22}{48;5;22}{0}\n";

    let old_content = [("render.txt".to_string(), old_line.to_string())]
        .into_iter()
        .collect();
    let new_content = [("render.txt".to_string(), new_line.to_string())]
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
        "repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );

    let (grid_lines, has_sb, _thumb_off, _thumb_size, cw) =
        state.render_diff_viewport_lines(50, 10, 10);
    assert!(has_sb);
    assert_eq!(cw, 49);

    for (i, line) in grid_lines.iter().enumerate() {
        assert!(
            !line.contains('\t'),
            "grid line {i} must never contain raw tab characters"
        );
    }

    let frame = state.render_frame(50, 15);
    for (i, row) in frame.lines().enumerate() {
        println!("row {i}: vw={}, content={:?}", visible_width(row), row);
        assert!(
            !row.contains('\t'),
            "frame row {i} must never contain raw tab characters"
        );
        assert!(
            visible_width(row) <= 50,
            "frame row {i} must never exceed terminal width 50"
        );
    }
}
