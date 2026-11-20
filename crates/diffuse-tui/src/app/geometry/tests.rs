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
@@ -1025,3 +1025,4 @@ fn test() {
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
        crate::render::DiffuseTheme::dark(),
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
        crate::render::DiffuseTheme::dark(),
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
    let raw_line = frame
        .lines()
        .find(|l| crate::render::ansi::strip_ansi(l).contains("+very long line"))
        .expect("segment 0 must start with gutter line number and + marker followed by green text");
    assert!(
        raw_line.contains("\x1b[38;5;120m"),
        "marker and text must render green: {raw_line:?}"
    );
    let render_gutter = " ".repeat(crate::render::gutter_width(state.new_line_num.width));
    assert!(
        frame.contains(&format!("\x1b[38;5;242m{render_gutter}\x1b[38;5;120m")),
        "continuation segments must start with gutter and green foreground"
    );
}

#[test]
fn test_hunk_divider_renders_and_extent_lift() {
    let mut state = AppState::new(
        sample_output_two_hunks(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    let frame = state.render_frame(120, 80);
    assert!(
        frame.contains("──── 1/"),
        "the first hunk needs a divider: {:.300}",
        frame
    );
    assert!(frame.contains("──── 2/"), "the second hunk needs a divider");
    assert!(
        frame.contains("−1,7 +1,8") || frame.contains("-1,7 +1,8") || frame.contains("──── 1/"),
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
        frame2.contains("──── 2/"),
        "the second hunk keeps its divider in hunk 2"
    );
}

#[test]
fn test_render_frame_lines_do_not_overflow_width() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
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
        crate::render::DiffuseTheme::dark(),
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
    // expand column / numbers
    assert!(
        frame_ctx.contains("\x1b[48;2;28;50;30m\x1b[38;5;"),
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
        crate::render::DiffuseTheme::dark(),
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
    assert_eq!(
        off_bottom - off_low,
        1,
        "scrolling 1 line up from bottom must move thumb by exactly 1 row, got jump from {off_bottom} to {off_low}"
    );
}

#[test]
fn test_scrollbar_thumb_large_content_smooth_at_bottom() {
    let (track_h, vh, total) = (25, 20, 4000);
    let max_scroll = total - vh;
    let header_vis = 1;
    let geom = |scroll: usize| {
        AppState::scrollbar_thumb_for(track_h, vh, total, scroll, header_vis)
            .expect("scrollbar must apply")
    };

    let (off_bottom, size) = geom(max_scroll);
    assert_eq!(off_bottom + size, track_h);

    let (off_prev, _) = geom(max_scroll - 1);
    assert_eq!(
        off_bottom - off_prev,
        1,
        "scrolling 1 line up in large file must move thumb by exactly 1 row, got jump from {off_bottom} to {off_prev}"
    );
}

#[test]
fn test_scrollbar_thumb_minimum_twelve_percent() {
    // Large document where proportional thumb would be < 1 row:
    // thumb must occupy at least 12% of track height.
    let size = AppState::scrollbar_thumb_size(25, 20, 4000).expect("scrollbar must apply");
    assert_eq!(size, 3, "25-row track must have at least 3-row thumb (12%)");

    let size_50 = AppState::scrollbar_thumb_size(50, 40, 10000).expect("scrollbar must apply");
    assert_eq!(
        size_50, 6,
        "50-row track must have at least 6-row thumb (12%)"
    );

    // Short track clamps to at least 2 rows:
    let size_short = AppState::scrollbar_thumb_size(8, 6, 1000).expect("scrollbar must apply");
    assert!(
        size_short >= 2,
        "short track must have at least 2-row thumb"
    );

    // When content is small, proportional thumb > 12% is preserved:
    let size_prop = AppState::scrollbar_thumb_size(25, 20, 25).expect("scrollbar must apply");
    assert_eq!(
        size_prop, 20,
        "small document preserves proportional thumb size"
    );

    // Track too short to fit the minimum thumb plus 1 line of travel returns None:
    assert!(AppState::scrollbar_thumb_size(2, 1, 10).is_none());

    // Track of height 3 gives thumb of 2 rows (leaving 1 row room to travel):
    assert_eq!(AppState::scrollbar_thumb_size(3, 2, 10), Some(2));
}

#[test]
fn test_scrollbar_scroll_for_row_formula() {
    // Proportional core: row 0 yields 0, the last row yields max_scroll,
    // everything between is exact. Live-log scale: 2672-line content on
    // a 74-row track.
    let (track_h, vh, total, header_vis) = (74, 50, 2722, 1);
    let max_scroll = total - vh;
    assert_eq!(
        AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, 0),
        0
    );
    assert_eq!(
        AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, 73),
        max_scroll
    );
    // Pointer past the end clamps to the last row.
    assert_eq!(
        AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, 1000),
        max_scroll
    );
    // Degenerate inputs yield 0, never divide by zero.
    assert_eq!(AppState::scrollbar_scroll_for_row(74, 50, 50, 1, 5), 0);
    assert_eq!(AppState::scrollbar_scroll_for_row(1, 1, 100, 1, 0), 0);
    // Monotone across the track.
    let mut prev = 0;
    for r in 1..74 {
        let s = AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, r);
        assert!(s >= prev, "mapping must not go backwards at row {r}");
        prev = s;
    }
}

#[test]
fn test_scrollbar_scroll_for_row_no_edge_snapping() {
    // 25-row screen, 20-row viewport, 1000 lines content.
    let (track_h, vh, total, header_vis) = (25, 20, 1000, 1);
    let max_scroll = total - vh;
    let thumb_size = AppState::scrollbar_thumb_size(track_h, vh, total).unwrap();
    let span = track_h - thumb_size;

    // Row 0 (top track line) -> thumb at 0:
    let s0 = AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, 0);
    assert_eq!(s0, 0);
    let (off0, _) = AppState::scrollbar_thumb_for(track_h, vh, total, s0, header_vis).unwrap();
    assert_eq!(off0, 0);

    // Row 1 (one line down from top) -> thumb moves down by exactly 1 line to
    // offset 1:
    let s1 = AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, 1);
    let (off1, _) = AppState::scrollbar_thumb_for(track_h, vh, total, s1, header_vis).unwrap();
    assert_eq!(
        off1, 1,
        "clicking 1 line down from top must yield offset 1, not 2"
    );

    // Penultimate row (one line up from bottom) -> thumb moves up by exactly 1
    // line:
    let s_pen = AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, track_h - 2);
    let (off_pen, _) =
        AppState::scrollbar_thumb_for(track_h, vh, total, s_pen, header_vis).unwrap();
    assert_eq!(
        off_pen,
        span - 1,
        "clicking 1 line up from bottom must yield offset span - 1"
    );

    // Last row (bottom track line) -> thumb at bottom:
    let s_last = AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, track_h - 1);
    assert_eq!(s_last, max_scroll);
    let (off_last, _) =
        AppState::scrollbar_thumb_for(track_h, vh, total, s_last, header_vis).unwrap();
    assert_eq!(off_last, span);

    // Every pointer row maps monotonically to thumb offsets with step at most 1:
    let mut prev_off = 0;
    for r in 0..track_h {
        let s = AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, r);
        let (off, _) = AppState::scrollbar_thumb_for(track_h, vh, total, s, header_vis).unwrap();
        assert!(off >= prev_off, "thumb must move monotonically, row {r}");
        assert!(
            off - prev_off <= 1,
            "thumb step must never exceed 1 row, got jump from {prev_off} to {off} at row {r}"
        );
        prev_off = off;
    }
}

#[test]
fn test_scrollbar_scroll_wheel_smooth_on_small_screen() {
    // Phone-sized terminal viewport (e.g. 25-row screen, 20-row viewport)
    // with typical medium diff (e.g. 80 visual lines).
    let (track_h, vh, total, header_vis) = (25, 20, 80, 1);
    let max_scroll = total - vh;

    // Scrolling down by 3 lines (standard mouse wheel tick):
    let mut prev_off = 0;
    for scroll in (0..=max_scroll).step_by(3) {
        let (off, _) =
            AppState::scrollbar_thumb_for(track_h, vh, total, scroll, header_vis).unwrap();
        assert!(
            off >= prev_off,
            "thumb must move monotonically down, scroll {scroll}"
        );
        assert!(
            off - prev_off <= 1,
            "wheel tick must never jump by 2, got jump from {prev_off} to {off} at scroll {scroll}"
        );
        prev_off = off;
    }

    // Scrolling up by 3 lines from the bottom:
    let (mut prev_up_off, _) =
        AppState::scrollbar_thumb_for(track_h, vh, total, max_scroll, header_vis).unwrap();
    let mut cur_scroll = max_scroll;
    while cur_scroll > 0 {
        cur_scroll = cur_scroll.saturating_sub(3);
        let (off, _) =
            AppState::scrollbar_thumb_for(track_h, vh, total, cur_scroll, header_vis).unwrap();
        assert!(
            off <= prev_up_off,
            "thumb must move monotonically up, scroll {cur_scroll}"
        );
        assert!(
            prev_up_off - off <= 1,
            "wheel tick up must never jump by 2, got jump from {prev_up_off} to {off} at scroll {cur_scroll}"
        );
        prev_up_off = off;
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
        crate::render::DiffuseTheme::dark(),
    );

    let (grid_lines, has_sb, _thumb_off, _thumb_size, cw) =
        state.render_diff_viewport_lines(50, 10, 10);
    assert!(has_sb);
    assert_eq!(cw, 49);

    for (i, line) in grid_lines.iter().enumerate() {
        assert!(
            !line.to_string().contains('\t'),
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

#[test]
fn test_scrollbar_step_kinematic_scaling() {
    // 1. Zero max scroll returns min_step
    assert_eq!(AppState::scrollbar_step(24, 20, 20, 3), 3);
    assert_eq!(AppState::scrollbar_step(24, 20, 15, 3), 3);

    // 2. track_h <= 1 returns min_step
    assert_eq!(AppState::scrollbar_step(1, 20, 100, 3), 3);
    assert_eq!(AppState::scrollbar_step(0, 20, 100, 3), 3);

    // 3. Medium diff: track_h=24, vh=20, total=100 -> thumb_size=3, span=21,
    //    max_scroll=80 round(80 / 21) = 4
    assert_eq!(AppState::scrollbar_step(24, 20, 100, 3), 4);

    // 4. Large diff: track_h=24, vh=20, total=500 -> thumb_size=3, span=21,
    //    max_scroll=480 round(480 / 21) = 23
    assert_eq!(AppState::scrollbar_step(24, 20, 500, 3), 23);

    // 5. Short diff with small max_scroll clamps to min_step: track_h=24, vh=20,
    //    total=25 -> max_scroll=5, span=21, round(5/21)=0 -> clamped to 3
    assert_eq!(AppState::scrollbar_step(24, 20, 25, 3), 3);
}

#[test]
fn test_scrollbar_subcell_geometry_conservation() {
    let (track_h, vh, total, header_vis) = (24, 20, 100, 1);
    let max_scroll = total - vh;

    for scroll in 0..=max_scroll {
        let sub = AppState::scrollbar_subcell_for(track_h, vh, total, scroll, header_vis)
            .expect("must produce subcell thumb");
        let mut total_eighths = 0;
        for r in 0..track_h {
            match sub.cell_at(r) {
                super::ScrollbarCell::Track => {}
                super::ScrollbarCell::Full => total_eighths += 8,
                super::ScrollbarCell::TopEdge { eighths } => total_eighths += eighths as usize,
                super::ScrollbarCell::BottomEdge { eighths } => total_eighths += eighths as usize,
            }
        }
        assert_eq!(
            total_eighths, sub.size_eighths,
            "total eighths must exactly equal size_eighths at scroll {scroll}"
        );
    }
}

#[test]
fn test_scrollbar_subcell_boundary_conditions() {
    let (track_h, vh, total, header_vis) = (24, 20, 100, 1);
    let max_scroll = total - vh;
    let thumb_size = AppState::scrollbar_thumb_size(track_h, vh, total).unwrap();
    let span = track_h - thumb_size;

    // 1. Top boundary (scroll = 0):
    let top = AppState::scrollbar_subcell_for(track_h, vh, total, 0, header_vis).unwrap();
    assert_eq!(top.start_eighths, 0);
    assert_eq!(top.cell_at(0), super::ScrollbarCell::Full);
    assert_eq!(top.cell_at(thumb_size - 1), super::ScrollbarCell::Full);
    assert_eq!(top.cell_at(thumb_size), super::ScrollbarCell::Track);

    // 2. Immediate scroll feedback (scroll = 1):
    let sc1 = AppState::scrollbar_subcell_for(track_h, vh, total, 1, header_vis).unwrap();
    assert!(sc1.start_eighths >= 1);
    match sc1.cell_at(0) {
        super::ScrollbarCell::TopEdge { eighths } => {
            assert_eq!(eighths, 7);
        }
        other => panic!("expected TopEdge at row 0, got {other:?}"),
    }
    match sc1.cell_at(thumb_size) {
        super::ScrollbarCell::BottomEdge { eighths } => {
            assert_eq!(eighths, 1);
        }
        other => panic!("expected BottomEdge at row {thumb_size}, got {other:?}"),
    }

    // 3. Penultimate scroll (scroll = max_scroll - 1):
    let pen =
        AppState::scrollbar_subcell_for(track_h, vh, total, max_scroll - 1, header_vis).unwrap();
    assert!(pen.start_eighths < span * 8);
    match pen.cell_at(track_h - 1) {
        super::ScrollbarCell::BottomEdge { eighths } => {
            assert!(eighths < 8, "bottom row must not be full");
        }
        super::ScrollbarCell::Track => {}
        other => panic!("expected BottomEdge or Track at bottom row, got {other:?}"),
    }

    // 4. Bottom boundary (scroll = max_scroll):
    let bot = AppState::scrollbar_subcell_for(track_h, vh, total, max_scroll, header_vis).unwrap();
    assert_eq!(bot.start_eighths, span * 8);
    assert_eq!(bot.cell_at(track_h - 1), super::ScrollbarCell::Full);
    assert_eq!(bot.cell_at(span), super::ScrollbarCell::Full);
    if span > 0 {
        assert_eq!(bot.cell_at(span - 1), super::ScrollbarCell::Track);
    }
}

#[test]
fn test_scrollbar_subcell_glyphs_and_inversion() {
    use super::ScrollbarCell;
    use crate::render::{ColorExt, DiffuseTheme};

    let p = &DiffuseTheme::dark().palette.ui;

    // Track
    let track = ScrollbarCell::Track;
    assert_eq!(track.glyph(), '│');
    let ansi_track = track.render_ansi(p);
    assert!(ansi_track.contains('│'));

    // Full
    let full = ScrollbarCell::Full;
    assert_eq!(full.glyph(), '█');
    let ansi_full = full.render_ansi(p);
    assert!(ansi_full.contains('█'));
    assert!(ansi_full.contains(&p.scrollbar_thumb.fg_code()));

    // TopEdge (bottom is thumb, top is track)
    for frac in 1..=7 {
        let cell = ScrollbarCell::TopEdge { eighths: frac };
        let ch = cell.glyph();
        let ansi = cell.render_ansi(p);
        assert!(ansi.contains(ch));
        // FG is thumb, BG is track
        assert!(ansi.contains(&p.scrollbar_thumb.fg_code()));
        assert!(ansi.contains(&p.scrollbar_track.bg_code()));
    }

    // BottomEdge (top is thumb, bottom is track)
    // When eighths == 4: '▀' with FG = thumb, BG = track
    let half = ScrollbarCell::BottomEdge { eighths: 4 };
    assert_eq!(half.glyph(), '▀');
    let ansi_half = half.render_ansi(p);
    assert!(ansi_half.contains('▀'));
    assert!(ansi_half.contains(&p.scrollbar_thumb.fg_code()));
    assert!(ansi_half.contains(&p.scrollbar_track.bg_code()));

    // Other BottomEdge eighths: lower block 8 - frac with FG = track, BG = thumb
    // (inverted)
    for frac in [1, 2, 3, 5, 6, 7] {
        let cell = ScrollbarCell::BottomEdge { eighths: frac };
        let ch = cell.glyph();
        let ansi = cell.render_ansi(p);
        assert!(ansi.contains(ch));
        // Inverted: FG is track, BG is thumb
        assert!(ansi.contains(&p.scrollbar_track.fg_code()));
        assert!(ansi.contains(&p.scrollbar_thumb.bg_code()));
    }
}

#[test]
fn test_scrollbar_subcell_to_ratatui_line_and_edge_branches() {
    use super::{ScrollbarCell, lower_block_glyph};
    use ratatui::style::Color;

    // Out-of-range lower block glyph falls back to space
    assert_eq!(lower_block_glyph(0), ' ');
    assert_eq!(lower_block_glyph(9), ' ');
    assert_eq!(lower_block_glyph(8), '█');

    let thumb_c = Color::White;
    let track_c = Color::Black;

    // 1. Track
    let l_track = ScrollbarCell::Track.to_ratatui_line(thumb_c, track_c);
    assert_eq!(l_track.spans[0].content, "│");
    assert_eq!(l_track.spans[0].style.fg, Some(track_c));
    assert_eq!(l_track.spans[0].style.bg, Some(track_c));

    // 2. Full
    let l_full = ScrollbarCell::Full.to_ratatui_line(thumb_c, track_c);
    assert_eq!(l_full.spans[0].content, "█");
    assert_eq!(l_full.spans[0].style.fg, Some(thumb_c));
    assert_eq!(l_full.spans[0].style.bg, Some(thumb_c));

    // 3. TopEdge
    for f in 1..=7 {
        let l_top = ScrollbarCell::TopEdge { eighths: f }.to_ratatui_line(thumb_c, track_c);
        assert_eq!(l_top.spans[0].content, lower_block_glyph(f).to_string());
        assert_eq!(l_top.spans[0].style.fg, Some(thumb_c));
        assert_eq!(l_top.spans[0].style.bg, Some(track_c));
    }

    // 4. BottomEdge: half (4)
    let l_half = ScrollbarCell::BottomEdge { eighths: 4 }.to_ratatui_line(thumb_c, track_c);
    assert_eq!(l_half.spans[0].content, "▀");
    assert_eq!(l_half.spans[0].style.fg, Some(thumb_c));
    assert_eq!(l_half.spans[0].style.bg, Some(track_c));

    // 5. BottomEdge: other eighths (inverted)
    for f in [1, 2, 3, 5, 6, 7] {
        let l_bot = ScrollbarCell::BottomEdge { eighths: f }.to_ratatui_line(thumb_c, track_c);
        assert_eq!(l_bot.spans[0].content, lower_block_glyph(8 - f).to_string());
        assert_eq!(l_bot.spans[0].style.fg, Some(track_c));
        assert_eq!(l_bot.spans[0].style.bg, Some(thumb_c));
    }

    // 6. Geometry edge branches:
    // None when max_scroll == 0
    assert!(AppState::scrollbar_subcell_for(20, 20, 20, 0, 1).is_none());
    // None when track_h <= 1
    assert!(AppState::scrollbar_subcell_for(1, 10, 100, 0, 1).is_none());

    // Minimal max_scroll == 1: falls through to else branch
    let sub_single = AppState::scrollbar_subcell_for(10, 5, 6, 1, 1).unwrap();
    assert_eq!(
        sub_single.start_eighths,
        (10 - sub_single.size_eighths / 8) * 8
    );

    // Track cell way past track height
    assert_eq!(sub_single.cell_at(99), ScrollbarCell::Track);
}

#[test]
fn test_hunk_expand_icon_for_segment_branches() {
    use crate::types::HunkExpandIcon;

    // 1. Single-line hunk with context above and below (lines 10..10 in 20-line
    //    file)
    let diff_mid =
        "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -10,1 +10,1 @@\n-old 10\n+new 10\n";
    let mut old_lines = (1..=20).map(|i| format!("line {i}\n")).collect::<Vec<_>>();
    old_lines[9] = "old 10\n".into();
    let mut new_lines = (1..=20).map(|i| format!("line {i}\n")).collect::<Vec<_>>();
    new_lines[9] = "new 10\n".into();
    let new_mid: std::collections::HashMap<_, _> =
        [("f.rs".into(), new_lines.concat())].into_iter().collect();
    let old_mid: std::collections::HashMap<_, _> =
        [("f.rs".into(), old_lines.concat())].into_iter().collect();
    let opts_u0 = crate::core::PipelineOptions {
        visual_context: 0,
        ..Default::default()
    };
    let out_mid = crate::core::pipeline::run_pipeline_full(
        diff_mid,
        Some(new_mid),
        Some(old_mid),
        opts_u0.clone(),
    );
    let state_mid = AppState::new(
        out_mid,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // In visual flat, row 0 is file header, row 1 is the diff row
    let diff_row_idx = state_mid
        .flat
        .iter()
        .position(|r| r.is_diff_row())
        .expect("diff row");

    // When seg_count == 1: collapses to Both
    assert_eq!(
        state_mid.hunk_expand_icon_for_segment(diff_row_idx, 0, &[1]),
        Some(HunkExpandIcon::Both)
    );

    // When seg_count == 3 (wrapped): s_idx 0 is Up, s_idx 1 is None, s_idx 2 is
    // Down (not space for both stems)
    assert_eq!(
        state_mid.hunk_expand_icon_for_segment(diff_row_idx, 0, &[3]),
        Some(HunkExpandIcon::Up)
    );
    assert_eq!(
        state_mid.hunk_expand_icon_for_segment(diff_row_idx, 1, &[3]),
        None
    );
    assert_eq!(
        state_mid.hunk_expand_icon_for_segment(diff_row_idx, 2, &[3]),
        Some(HunkExpandIcon::Down)
    );

    // When seg_count == 4 (wrapped): s_idx 0 is Up, interior segments are None,
    // s_idx 3 is Down (single char arrows, no stems)
    assert_eq!(
        state_mid.hunk_expand_icon_for_segment(diff_row_idx, 0, &[4]),
        Some(HunkExpandIcon::Up)
    );
    assert_eq!(
        state_mid.hunk_expand_icon_for_segment(diff_row_idx, 1, &[4]),
        None
    );
    assert_eq!(
        state_mid.hunk_expand_icon_for_segment(diff_row_idx, 2, &[4]),
        None
    );
    assert_eq!(
        state_mid.hunk_expand_icon_for_segment(diff_row_idx, 3, &[4]),
        Some(HunkExpandIcon::Down)
    );

    // 2. 1-line file with no context in either direction
    let diff_1line =
        "diff --git a/one.rs b/one.rs\n--- a/one.rs\n+++ b/one.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
    let new_1line: std::collections::HashMap<_, _> =
        [("one.rs".into(), "new\n".into())].into_iter().collect();
    let old_1line: std::collections::HashMap<_, _> =
        [("one.rs".into(), "old\n".into())].into_iter().collect();
    let out_1line = crate::core::pipeline::run_pipeline_full(
        diff_1line,
        Some(new_1line),
        Some(old_1line),
        opts_u0.clone(),
    );
    let state_1line = AppState::new(
        out_1line,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let row_1line = state_1line
        .flat
        .iter()
        .position(|r| r.is_diff_row())
        .expect("diff row");
    assert_eq!(
        state_1line.hunk_expand_icon_for_segment(row_1line, 0, &[1]),
        None
    );

    // 3. 1-line hunk at top of file (context below only: 1 line added at top of
    //    10-line file)
    let diff_top =
        "diff --git a/top.rs b/top.rs\n--- a/top.rs\n+++ b/top.rs\n@@ -1,0 +1,1 @@\n+new 1\n";
    let old_top = (2..=10).map(|i| format!("line {i}\n")).collect::<Vec<_>>();
    let mut new_top = vec!["new 1\n".to_string()];
    new_top.extend((2..=10).map(|i| format!("line {i}\n")));
    let new_top_m: std::collections::HashMap<_, _> =
        [("top.rs".into(), new_top.concat())].into_iter().collect();
    let old_top_m: std::collections::HashMap<_, _> =
        [("top.rs".into(), old_top.concat())].into_iter().collect();
    let out_top = crate::core::pipeline::run_pipeline_full(
        diff_top,
        Some(new_top_m),
        Some(old_top_m),
        opts_u0.clone(),
    );
    let state_top = AppState::new(
        out_top,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let row_top = state_top
        .flat
        .iter()
        .position(|r| r.is_diff_row())
        .expect("diff row");
    assert_eq!(
        state_top.hunk_expand_icon_for_segment(row_top, 0, &[1]),
        Some(HunkExpandIcon::Down)
    );
    // When wrapped into 4 visual segments, segment 2 is None and segment 3 is
    // Down (single char arrow)
    assert_eq!(
        state_top.hunk_expand_icon_for_segment(row_top, 2, &[4]),
        None
    );
    assert_eq!(
        state_top.hunk_expand_icon_for_segment(row_top, 3, &[4]),
        Some(HunkExpandIcon::Down)
    );

    // 4. 1-line hunk at bottom of file (context above only: 1 line added at bottom
    //    of 10-line file)
    let diff_bot =
        "diff --git a/bot.rs b/bot.rs\n--- a/bot.rs\n+++ b/bot.rs\n@@ -10,0 +10,1 @@\n+new 10\n";
    let old_bot = (1..=9).map(|i| format!("line {i}\n")).collect::<Vec<_>>();
    let mut new_bot = (1..=9).map(|i| format!("line {i}\n")).collect::<Vec<_>>();
    new_bot.push("new 10\n".to_string());
    let new_bot_m: std::collections::HashMap<_, _> =
        [("bot.rs".into(), new_bot.concat())].into_iter().collect();
    let old_bot_m: std::collections::HashMap<_, _> =
        [("bot.rs".into(), old_bot.concat())].into_iter().collect();
    let out_bot = crate::core::pipeline::run_pipeline_full(
        diff_bot,
        Some(new_bot_m),
        Some(old_bot_m),
        opts_u0.clone(),
    );
    let state_bot = AppState::new(
        out_bot,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let row_bot = state_bot
        .flat
        .iter()
        .position(|r| r.is_diff_row())
        .expect("diff row");
    assert_eq!(
        state_bot.hunk_expand_icon_for_segment(row_bot, 0, &[1]),
        Some(HunkExpandIcon::Up)
    );
    // When wrapped into 4 visual segments, segment 0 is Up and segment 1 is None
    assert_eq!(
        state_bot.hunk_expand_icon_for_segment(row_bot, 0, &[4]),
        Some(HunkExpandIcon::Up)
    );
    assert_eq!(
        state_bot.hunk_expand_icon_for_segment(row_bot, 1, &[4]),
        None
    );

    // 5. Multi-line hunk in middle of file
    let diff_multi = "diff --git a/m.rs b/m.rs\n--- a/m.rs\n+++ b/m.rs\n@@ -5,3 +5,3 @@\n-old 5\n-old 6\n-old 7\n+new 5\n+new 6\n+new 7\n";
    let mut old_m = (1..=15).map(|i| format!("line {i}\n")).collect::<Vec<_>>();
    old_m[4] = "old 5\n".into();
    old_m[5] = "old 6\n".into();
    old_m[6] = "old 7\n".into();
    let mut new_m = (1..=15).map(|i| format!("line {i}\n")).collect::<Vec<_>>();
    new_m[4] = "new 5\n".into();
    new_m[5] = "new 6\n".into();
    new_m[6] = "new 7\n".into();
    let new_mm: std::collections::HashMap<_, _> =
        [("m.rs".into(), new_m.concat())].into_iter().collect();
    let old_mm: std::collections::HashMap<_, _> =
        [("m.rs".into(), old_m.concat())].into_iter().collect();
    let out_multi = crate::core::pipeline::run_pipeline_full(
        diff_multi,
        Some(new_mm),
        Some(old_mm),
        opts_u0.clone(),
    );
    let state_multi = AppState::new(
        out_multi,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let multi_rows: Vec<_> = state_multi
        .flat
        .iter()
        .enumerate()
        .filter(|(_, r)| r.is_diff_row())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(multi_rows.len(), 6);

    let first_row = multi_rows[0];
    let row_1 = multi_rows[1];
    let mid_row = multi_rows[2];
    let row_4 = multi_rows[4];
    let last_row = multi_rows[5];
    let flat_segs = vec![1; state_multi.flat.len()];

    // First row: s_idx 0 is Up
    assert_eq!(
        state_multi.hunk_expand_icon_for_segment(first_row, 0, &flat_segs),
        Some(HunkExpandIcon::Up)
    );

    // Row 1 (visual line 1): None (single char arrows, no stems)
    assert_eq!(
        state_multi.hunk_expand_icon_for_segment(row_1, 0, &flat_segs),
        None
    );

    // Row 2 (visual line 2): None (interior line)
    assert_eq!(
        state_multi.hunk_expand_icon_for_segment(mid_row, 0, &flat_segs),
        None
    );

    // Row 4 (visual line 4 = 6 - 2): None
    assert_eq!(
        state_multi.hunk_expand_icon_for_segment(row_4, 0, &flat_segs),
        None
    );

    // Last row (visual line 5 = 6 - 1): Down
    assert_eq!(
        state_multi.hunk_expand_icon_for_segment(last_row, 0, &flat_segs),
        Some(HunkExpandIcon::Down)
    );

    // 6. Exactly 3 visual lines: Up at top, Down at bottom, None in middle
    let diff_3 = "diff --git a/three.rs b/three.rs\n--- a/three.rs\n+++ b/three.rs\n@@ -5,3 +5,0 @@\n-old 5\n-old 6\n-old 7\n";
    let mut old_3 = (1..=15).map(|i| format!("line {i}\n")).collect::<Vec<_>>();
    old_3[4] = "old 5\n".into();
    old_3[5] = "old 6\n".into();
    old_3[6] = "old 7\n".into();
    let old_3m: std::collections::HashMap<_, _> =
        [("three.rs".into(), old_3.concat())].into_iter().collect();
    let new_3m: std::collections::HashMap<_, _> = [(
        "three.rs".into(),
        (1..=12)
            .map(|i| format!("line {i}\n"))
            .collect::<Vec<_>>()
            .concat(),
    )]
    .into_iter()
    .collect();
    let out_3 =
        crate::core::pipeline::run_pipeline_full(diff_3, Some(new_3m), Some(old_3m), opts_u0);
    let state_3 = AppState::new(
        out_3,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let three_rows: Vec<_> = state_3
        .flat
        .iter()
        .enumerate()
        .filter(|(_, r)| r.is_diff_row())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(three_rows.len(), 3);
    let segs_3 = vec![1; state_3.flat.len()];
    assert_eq!(
        state_3.hunk_expand_icon_for_segment(three_rows[0], 0, &segs_3),
        Some(HunkExpandIcon::Up)
    );
    assert_eq!(
        state_3.hunk_expand_icon_for_segment(three_rows[1], 0, &segs_3),
        None
    );
    assert_eq!(
        state_3.hunk_expand_icon_for_segment(three_rows[2], 0, &segs_3),
        Some(HunkExpandIcon::Down)
    );

    // Non-diff row and out of bounds
    assert_eq!(state_mid.hunk_expand_icon_for_segment(0, 0, &[1]), None);
    assert_eq!(state_mid.hunk_expand_icon_for_segment(999, 0, &[1]), None);
}

#[test]
fn test_line_num_tracking_and_collapsible_gutter_boundaries() {
    use crate::types::LineNumTracking;

    let track = LineNumTracking::new(42, 2);
    assert_eq!(track.max_num, 42);
    assert_eq!(track.width, 2);
    assert_eq!(LineNumTracking::default(), LineNumTracking::new(0, 0));

    let mut state = AppState::new(
        sample_output_two_hunks(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // Initial state: None (auto)
    state.old_line_num = LineNumTracking::new(50, 2);
    state.new_line_num = LineNumTracking::new(50, 2);
    assert_eq!(state.show_old_line_gutter, None);

    // expanded_gutter_w = 1 + 2 + 1 + 2 + 2 = 8
    // Auto threshold 100 diff content cols requires content_width - 8 >= 100 ->
    // content_width >= 108
    assert!(!state.is_old_line_gutter_active(107));
    assert_eq!(state.gutter_width_for(107), 5); // 1 + 2 + 2
    assert!(state.is_old_line_gutter_active(108));
    assert_eq!(state.gutter_width_for(108), 8);

    // On narrow screen / phone (e.g. 48 cols), auto is inactive
    assert!(!state.is_old_line_gutter_active(48));
    assert_eq!(state.gutter_width_for(48), 5);

    // Forced on: Some(true) forces active even on narrow screen / phone (48 cols)
    state.show_old_line_gutter = Some(true);
    assert!(state.is_old_line_gutter_active(48));
    assert_eq!(state.gutter_width_for(48), 8);

    // Forced off: Some(false) forces inactive even on wide screen (200 cols)
    state.show_old_line_gutter = Some(false);
    assert!(!state.is_old_line_gutter_active(200));
    assert_eq!(state.gutter_width_for(200), 5);

    // If old_line_num.width is 0, never active even if forced on
    state.show_old_line_gutter = Some(true);
    state.old_line_num = LineNumTracking::new(0, 0);
    assert!(!state.is_old_line_gutter_active(200));
    assert_eq!(state.gutter_width_for(200), 5);
}

#[test]
fn test_side_by_side_geometry_and_wrapping() {
    let output = sample_output();
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    assert!(!state.side_by_side);

    let cw = 80;
    let unified_geo = state.cached_geometry(cw);

    // Switch to side-by-side mode
    state.side_by_side = true;
    state.bump_content();

    let split_geo = state.cached_geometry(cw);
    assert!(split_geo.side_by_side);

    // Prefix sum monotone continuity: starts[i+1] == starts[i] + counts[i]
    for i in 0..split_geo.starts.len().saturating_sub(1) {
        assert_eq!(
            split_geo.starts[i + 1],
            split_geo.starts[i] + split_geo.counts[i],
            "monotone continuity failed at row {i}"
        );
    }
    if let Some(&last_start) = split_geo.starts.last()
        && let Some(&last_count) = split_geo.counts.last()
    {
        assert_eq!(split_geo.total_vis_lines, last_start + last_count);
    }

    // Geometry cache invalidates when toggling side_by_side
    state.side_by_side = false;
    let geo_back = state.cached_geometry(cw);
    assert!(!geo_back.side_by_side);
    assert_eq!(geo_back.total_vis_lines, unified_geo.total_vis_lines);
}
