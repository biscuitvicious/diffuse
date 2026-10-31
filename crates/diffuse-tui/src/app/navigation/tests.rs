use crate::{
    app::{AppState, temp_notes_path},
    core::{
        PipelineOptions,
        merge::RunKind,
        pipeline::run_pipeline_full,
        wire::{OutputRun, PipelineOutput},
    },
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
    let prefix: String = (1..1025).map(|i| format!("line {i}\n")).collect();
    let new_content = [(
        "filesystem.rs".to_string(),
        format!(
            "{prefix}    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}}\n"
        ),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "filesystem.rs".to_string(),
        format!("{prefix}    let x = 1;\n    // old comment — em dash\n}}\n"),
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

#[test]
fn test_app_state_scroll_through_all_rows() {
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

    let out = run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );

    // Step down through every single row and verify render_frame never panics
    for _ in 0..state.flat.len() + 5 {
        let frame = state.render_frame(100, 30);
        assert!(!frame.is_empty());
        state.move_down();
    }

    // Step up through every single row
    for _ in 0..state.flat.len() + 5 {
        let frame = state.render_frame(80, 24);
        assert!(!frame.is_empty());
        state.move_up();
    }
}

#[test]
fn test_page_down_up_scrolls_one_third_viewport() {
    let diff = r#"diff --git a/many.rs b/many.rs
index 1111111..2222222 100644
--- a/many.rs
+++ b/many.rs
@@ -1,1 +1,40 @@
-old
+line 1
+line 2
+line 3
+line 4
+line 5
+line 6
+line 7
+line 8
+line 9
+line 10
+line 11
+line 12
+line 13
+line 14
+line 15
+line 16
+line 17
+line 18
+line 19
+line 20
+line 21
+line 22
+line 23
+line 24
+line 25
+line 26
+line 27
+line 28
+line 29
+line 30
+line 31
+line 32
+line 33
+line 34
+line 35
+line 36
+line 37
+line 38
+line 39
+line 40
"#;
    let new_content = [(
        "many.rs".to_string(),
        (1..=40).map(|i| format!("line {i}\n")).collect::<String>(),
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
    state.render_frame(80, 20);
    assert_eq!(state.scroll, 0);

    // 1st pg down moves cursor to bottom of current visible page without
    // scrolling
    state.page_down();
    assert_eq!(state.scroll, 0);
    assert!(state.row_idx > 0);

    // 2nd pg down (already at bottom) scrolls 1/3 of vh (5 lines)
    state.page_down();
    assert_eq!(state.scroll, 5);

    // 3rd pg down scrolls another 5 lines
    state.page_down();
    assert_eq!(state.scroll, 10);

    // 1st pg up moves cursor to top of current visible page without scrolling
    state.page_up();
    assert_eq!(state.scroll, 10);

    // 2nd pg up (already at top) scrolls 1/3 of vh (5 lines)
    state.page_up();
    assert_eq!(state.scroll, 5);

    // In raw diff mode: first move cursor to bottom, then scroll
    state.toggle_raw_diff();
    state.render_frame(80, 20);
    state.move_top();
    state.page_down(); // to bottom
    let raw_scroll = state.scroll;
    state.page_down(); // scroll 1/3
    assert_eq!(state.scroll, raw_scroll + 5);
    state.page_up(); // to top
    state.page_up(); // scroll 1/3
    assert_eq!(state.scroll, raw_scroll);
}

#[test]
fn test_page_up_with_sticky_header_scrolls_one_third() {
    let mut lines = String::new();
    let mut diff_lines = String::new();
    for i in 1..=100 {
        lines.push_str(&format!("line {i}\n"));
        diff_lines.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,100 @@\n{diff_lines}"
    );
    let new_content = [("a.rs".to_string(), lines)].into_iter().collect();
    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;
    state.last_content_width = 80;

    // Scroll down into content so sticky header is active (scroll = 30)
    state.scroll_to_visual(30);
    state.render_diff_viewport_lines(80, 20, 20);
    assert_eq!(state.scroll, 30);

    // Place cursor at the bottom of the viewport
    state.page_down();
    state.render_diff_viewport_lines(80, 20, 20);
    assert_eq!(state.scroll, 30);

    // 1st page_up moves cursor to first visible content line (under sticky headers)
    state.page_up();
    state.render_diff_viewport_lines(80, 20, 20);
    assert_eq!(state.scroll, 30, "first page_up must not shift scroll");

    // 2nd page_up must scroll 1/3 viewport (~7 lines), NOT just 1-2 lines
    state.page_up();
    state.render_diff_viewport_lines(80, 20, 20);
    assert_eq!(state.scroll, 23, "second page_up must scroll 1/3 viewport");
}

#[test]
fn test_goto_file_lands_on_banner_with_viewport_margin() {
    let mut lines1 = String::new();
    let mut lines2 = String::new();
    for i in 1..=30 {
        lines1.push_str(&format!("file1 line {i}\n"));
        lines2.push_str(&format!("file2 line {i}\n"));
    }
    let diff = format!(
        "diff --git a/file1.rs b/file1.rs\nindex 000..111 100644\n--- a/file1.rs\n+++ b/file1.rs\n@@ -1,30 +1,30 @@\n{lines1}diff --git a/file2.rs b/file2.rs\nindex 222..333 100644\n--- a/file2.rs\n+++ b/file2.rs\n@@ -1,30 +1,30 @@\n{lines2}"
    );
    let new_content = [
        ("file1.rs".to_string(), lines1),
        ("file2.rs".to_string(), lines2),
    ]
    .into_iter()
    .collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;

    // Jump to file 1 (file2.rs)
    state.goto_file_idx(1);
    let header_row_file2 = state.first_row_of_file(1);
    assert_eq!(
        state.row_idx, header_row_file2,
        "goto_file_idx must land directly on the file banner row"
    );
    assert!(
        state.flat[state.row_idx].is_file_header(),
        "row must be the file header banner"
    );

    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let target_vis = starts[header_row_file2];
    let expected_margin = state.viewport_margin();
    assert_eq!(
        state.scroll,
        target_vis.saturating_sub(expected_margin),
        "scroll must precede file banner by 1/10 of viewport"
    );
}

#[test]
fn test_goto_line_centers_when_outside_20_80_percent() {
    let mut lines = String::new();
    let mut diff_lines = String::new();
    for i in 1..=50 {
        lines.push_str(&format!("line {i}\n"));
        diff_lines.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/big.rs b/big.rs\nindex 000..111 100644\n--- a/big.rs\n+++ b/big.rs\n@@ -0,0 +1,50 @@\n{diff_lines}"
    );
    let new_content = [("big.rs".to_string(), lines)].into_iter().collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;
    // Initial scroll is 0. Target line 30 has target_vis = 30.
    // Relative to scroll 0, screen row is 30, which is outside 20%..80% (4..16).
    // It must center line 30 at viewport_height / 2 = 10.
    state.goto_line(30);
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let target_vis = starts[state.row_idx];
    assert_eq!(
        state.scroll,
        target_vis.saturating_sub(state.viewport_height / 2),
        "goto_line outside 20-80% must center target line at middle of screen"
    );
    assert_eq!(
        target_vis - state.scroll,
        state.viewport_height / 2,
        "screen row must be exactly middle of screen"
    );
}

#[test]
fn test_goto_line_preserves_scroll_when_within_20_80_percent() {
    let mut lines = String::new();
    let mut diff_lines = String::new();
    for i in 1..=50 {
        lines.push_str(&format!("line {i}\n"));
        diff_lines.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/big.rs b/big.rs\nindex 000..111 100644\n--- a/big.rs\n+++ b/big.rs\n@@ -0,0 +1,50 @@\n{diff_lines}"
    );
    let new_content = [("big.rs".to_string(), lines)].into_iter().collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;

    // First jump to line 30 (target_vis = 31) -> centers at scroll = 31 - 10 = 21
    // (screen row 10).
    state.goto_line(30);
    assert_eq!(state.scroll, 21);

    // Now jump to line 25 (target_vis 26).
    // Screen row is 26 - 21 = 5.
    // 20% of 20 = 4, 80% of 20 = 16. 5 is within 4..=16.
    // Scroll must NOT change!
    state.goto_line(25);
    assert_eq!(
        state.scroll, 21,
        "goto_line within 20-80% must not change scroll"
    );
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    assert_eq!(starts[state.row_idx], 26);

    // Boundary: line 24 (target_vis 25, screen row 25 - 21 = 4, exactly 20%).
    state.goto_line(24);
    assert_eq!(
        state.scroll, 21,
        "goto_line at exactly 20% boundary must not change scroll"
    );

    // Boundary: line 36 (target_vis 37, screen row 37 - 21 = 16, exactly 80%).
    state.goto_line(36);
    assert_eq!(
        state.scroll, 21,
        "goto_line at exactly 80% boundary must not change scroll"
    );

    // Outside lower boundary: line 23 (target_vis 24, screen row would be 24 - 21 =
    // 3 < 4). Must center line 23 at scroll = 24 - 10 = 14.
    state.goto_line(23);
    assert_eq!(
        state.scroll, 14,
        "goto_line below 20% must center target line"
    );

    // Outside upper boundary: line 37 (target_vis 38, screen row 38 - 14 = 24 >
    // 16). Must center line 37 at scroll = 38 - 10 = 28.
    state.goto_line(37);
    assert_eq!(
        state.scroll, 28,
        "goto_line above 80% must center target line"
    );
}

#[test]
fn test_jump_to_row_centered_edge_cases() {
    let mut lines = String::new();
    let mut diff_lines = String::new();
    for i in 1..=50 {
        lines.push_str(&format!("line {i}\n"));
        diff_lines.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/big.rs b/big.rs\nindex 000..111 100644\n--- a/big.rs\n+++ b/big.rs\n@@ -0,0 +1,50 @@\n{diff_lines}"
    );
    let new_content = [("big.rs".to_string(), lines)].into_iter().collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;
    state.last_content_width = 0; // exercises fallback to 80

    // Jump to row 0 (top of document, target_vis = 0 < vh / 2)
    state.scroll = 20;
    state.jump_to_row_centered(0);
    assert_eq!(state.row_idx, 0);
    assert_eq!(state.scroll, 0, "target_vis < vh / 2 saturates to 0");

    // Jump to row beyond flat length (clamped to flat.len() - 1)
    let last_row = state.flat.len() - 1;
    state.jump_to_row_centered(9999);
    assert_eq!(state.row_idx, last_row);

    // Target above scroll window (target_vis < self.scroll)
    state.scroll = 30;
    state.jump_to_row_centered(5);
    let (starts, _, _, _) = state.visual_geometry(80);
    let vis_5 = starts[5];
    assert_eq!(
        state.scroll,
        vis_5.saturating_sub(state.viewport_height / 2),
        "target above scroll window centers"
    );
}

#[test]
fn test_move_bottom_preserves_viewport_margin_below() {
    let mut lines = String::new();
    for i in 1..=50 {
        lines.push_str(&format!("line {i}\n"));
    }
    let diff = format!(
        "diff --git a/big.rs b/big.rs\nindex 000..111 100644\n--- a/big.rs\n+++ b/big.rs\n@@ -1,50 +1,50 @@\n{lines}"
    );
    let new_content = [("big.rs".to_string(), lines)].into_iter().collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;

    state.move_bottom();
    assert_eq!(state.row_idx, state.flat.len() - 1);
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let cursor_vis = starts[state.row_idx];
    let margin = state.viewport_margin();
    let max_rows = state.viewport_height.max(4);
    assert_eq!(
        state.scroll,
        cursor_vis.saturating_sub(max_rows.saturating_sub(1 + margin)),
        "move_bottom must scroll down so bottom line is margin lines above viewport bottom"
    );
}

#[test]
fn test_viewport_margin_rounding_half_up() {
    let mut state = AppState::new(
        PipelineOutput {
            files: Vec::new(),
            branch: String::new(),
            untracked_skipped: Vec::new(),
        },
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );

    state.viewport_height = 0;
    assert_eq!(state.viewport_margin(), 1, "0 -> min 1");

    state.viewport_height = 4;
    assert_eq!(state.viewport_margin(), 1, "4 -> 1");

    state.viewport_height = 14;
    assert_eq!(state.viewport_margin(), 1, "14 (1.4) -> 1");

    state.viewport_height = 15;
    assert_eq!(state.viewport_margin(), 2, "15 (1.5) -> 2");

    state.viewport_height = 20;
    assert_eq!(state.viewport_margin(), 2, "20 (2.0) -> 2");

    state.viewport_height = 24;
    assert_eq!(state.viewport_margin(), 2, "24 (2.4) -> 2");

    state.viewport_height = 25;
    assert_eq!(state.viewport_margin(), 3, "25 (2.5) -> 3");

    state.viewport_height = 35;
    assert_eq!(state.viewport_margin(), 4, "35 (3.5) -> 4");
}

#[test]
fn test_next_and_prev_file_land_banner_at_row_zero() {
    let mut lines_a = String::new();
    let mut lines_b = String::new();
    let mut lines_c = String::new();
    let mut diff_a = String::new();
    let mut diff_b = String::new();
    let mut diff_c = String::new();
    for i in 1..=30 {
        lines_a.push_str(&format!("a line {i}\n"));
        lines_b.push_str(&format!("b line {i}\n"));
        lines_c.push_str(&format!("c line {i}\n"));
        diff_a.push_str(&format!("+a line {i}\n"));
        diff_b.push_str(&format!("+b line {i}\n"));
        diff_c.push_str(&format!("+c line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}diff --git a/b.rs b/b.rs\nindex 222..333 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -0,0 +1,30 @@\n{diff_b}diff --git a/c.rs b/c.rs\nindex 444..555 100644\n--- a/c.rs\n+++ b/c.rs\n@@ -0,0 +1,30 @@\n{diff_c}"
    );
    let new_content = [
        ("a.rs".to_string(), lines_a),
        ("b.rs".to_string(), lines_b),
        ("c.rs".to_string(), lines_c),
    ]
    .into_iter()
    .collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;

    assert_eq!(state.current_file_idx(), 0, "starts on file 0 (a.rs)");

    // Press 'n' -> next_file(): cursor lands on the file banner at row #0.
    // J/K treat the banner as inside the first hunk, so J still advances.
    state.next_file();
    assert_eq!(state.current_file_idx(), 1, "navigates to file 1 (b.rs)");
    let b_banner = state.first_row_of_file(1);
    assert_eq!(state.row_idx, b_banner, "lands on b.rs banner");
    assert!(state.flat[state.row_idx].is_file_header());
    let (starts, _, _, total_vis_lines) = state.visual_geometry(state.last_content_width);
    let max_rows = state.viewport_height.max(4);
    let max_scroll = total_vis_lines.saturating_sub(max_rows);
    assert_eq!(
        state.scroll,
        starts[b_banner].min(max_scroll),
        "next_file must position scroll so the file banner is at row #0"
    );
    assert_eq!(
        starts[b_banner] - state.scroll,
        0,
        "file banner must sit exactly at screen row #0"
    );

    // J from the banner counts as inside H1, so it advances past it.
    let before = state.row_idx;
    state.next_hunk();
    assert!(
        state.row_idx != before,
        "J after n must advance past the banner's hunk"
    );

    // That J crossed into file 2 (c.rs); 'p' returns to file 1 (b.rs).
    state.prev_file();
    assert_eq!(state.current_file_idx(), 1, "returns to file 1 (b.rs)");
    assert_eq!(state.row_idx, b_banner, "lands on b.rs banner");

    // Press 'n' again to go to file 2 (c.rs) which is near/at bottom
    state.next_file();
    assert_eq!(state.current_file_idx(), 2, "navigates to file 2 (c.rs)");
    let c_banner = state.first_row_of_file(2);
    assert_eq!(state.row_idx, c_banner);
    assert_eq!(
        state.scroll,
        starts[c_banner].min(max_scroll),
        "at the very bottom of the scrollable view, scroll is clamped to max_scroll"
    );

    // Press 'p' -> prev_file() back to file 1 (b.rs) banner
    state.prev_file();
    assert_eq!(
        state.current_file_idx(),
        1,
        "navigates back to file 1 (b.rs)"
    );
    assert_eq!(state.row_idx, b_banner, "lands on b.rs banner");
    assert_eq!(state.scroll, starts[b_banner].min(max_scroll));
    assert_eq!(
        starts[b_banner] - state.scroll,
        0,
        "b.rs banner sits at row #0"
    );

    // Press 'p' -> prev_file() back to file 0 (a.rs) banner
    state.prev_file();
    assert_eq!(
        state.current_file_idx(),
        0,
        "navigates back to file 0 (a.rs)"
    );
    let a_banner = state.first_row_of_file(0);
    assert_eq!(state.row_idx, a_banner);
    assert_eq!(state.scroll, starts[a_banner].min(max_scroll));
    assert_eq!(
        starts[a_banner] - state.scroll,
        0,
        "a.rs banner sits at row #0"
    );
}

#[test]
fn test_prev_file_in_first_file_reveals_banner() {
    let mut lines_a = String::new();
    let mut lines_b = String::new();
    let mut diff_a = String::new();
    let mut diff_b = String::new();
    for i in 1..=30 {
        lines_a.push_str(&format!("a line {i}\n"));
        lines_b.push_str(&format!("b line {i}\n"));
        diff_a.push_str(&format!("+a line {i}\n"));
        diff_b.push_str(&format!("+b line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}diff --git a/b.rs b/b.rs\nindex 222..333 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -0,0 +1,30 @@\n{diff_b}"
    );
    let new_content = [("a.rs".to_string(), lines_a), ("b.rs".to_string(), lines_b)]
        .into_iter()
        .collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;
    state.last_content_width = 80;

    // Move deep into the first file with its banner scrolled out of view.
    let a_banner = state.first_row_of_file(0);
    let last_a = state.first_row_of_file(1) - 1;
    state.row_idx = last_a;
    state.vis_line = 0;
    let (starts, _, _, _) = state.visual_geometry(80);
    state.scroll = starts[a_banner] + 5;
    assert!(
        starts[a_banner] < state.scroll,
        "precondition: a.rs banner is scrolled out of view"
    );

    // 'p' in the first file scrolls the banner back as the top row, landing
    // the cursor on it like a regular file jump.
    state.prev_file();
    assert_eq!(state.current_file_idx(), 0, "stays in file 0 (a.rs)");
    assert_eq!(state.row_idx, a_banner, "cursor lands on the banner");
    let (starts, _, _, total_vis_lines) = state.visual_geometry(80);
    let max_scroll = total_vis_lines.saturating_sub(state.viewport_height.max(4));
    assert_eq!(
        state.scroll,
        starts[a_banner].min(max_scroll),
        "banner must be the top row"
    );
    assert_eq!(starts[a_banner] - state.scroll, 0);

    // A further 'p' wraps around to the last file.
    state.prev_file();
    assert_eq!(state.current_file_idx(), 1, "wraps to file 1 (b.rs)");
    let b_banner = state.first_row_of_file(1);
    assert_eq!(state.row_idx, b_banner, "cursor lands on b.rs banner");
    let b_banner = state.first_row_of_file(1);
    let (starts, _, _, total_vis_lines) = state.visual_geometry(80);
    let max_scroll = total_vis_lines.saturating_sub(state.viewport_height.max(4));
    assert_eq!(state.scroll, starts[b_banner].min(max_scroll));
    assert_eq!(starts[b_banner] - state.scroll, 0);
}

#[test]
fn test_next_file_wraps_around_at_end() {
    let mut lines_a = String::new();
    let mut lines_b = String::new();
    let mut diff_a = String::new();
    let mut diff_b = String::new();
    for i in 1..=30 {
        lines_a.push_str(&format!("a line {i}\n"));
        lines_b.push_str(&format!("b line {i}\n"));
        diff_a.push_str(&format!("+a line {i}\n"));
        diff_b.push_str(&format!("+b line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}diff --git a/b.rs b/b.rs\nindex 222..333 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -0,0 +1,30 @@\n{diff_b}"
    );
    let new_content = [("a.rs".to_string(), lines_a), ("b.rs".to_string(), lines_b)]
        .into_iter()
        .collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;
    state.last_content_width = 80;

    // 'n' to the last file, then 'n' wraps to the first file.
    state.next_file();
    assert_eq!(state.current_file_idx(), 1, "navigates to file 1 (b.rs)");
    state.next_file();
    assert_eq!(state.current_file_idx(), 0, "wraps to file 0 (a.rs)");
    let a_banner = state.first_row_of_file(0);
    assert_eq!(state.row_idx, a_banner, "cursor lands on a.rs banner");
    let (starts, _, _, total_vis_lines) = state.visual_geometry(80);
    let max_scroll = total_vis_lines.saturating_sub(state.viewport_height.max(4));
    assert_eq!(state.scroll, starts[a_banner].min(max_scroll));
    assert_eq!(starts[a_banner] - state.scroll, 0);
}

#[test]
fn test_hunk_jump_wraps_around_at_edges() {
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
    // The insertion above shifts new-side line numbers by one: new line
    // (i) carries old line (i-1)'s text, keeping the tape's declared
    // matching consistent (padding positions agree on both sides).
    for i in 5..=30 {
        new_lines.push(format!("    // filler {}", i - 1));
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

    let mut state = AppState::new(
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        ),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 6;
    state.last_content_width = 80;
    let first = state.hunk_starts[0].flat;
    let last = state.hunk_starts[state.hunk_starts.len() - 1].flat;

    // 'J' on the last hunk wraps to the first hunk.
    state.row_idx = last;
    state.next_hunk();
    assert_eq!(state.row_idx, first, "J wraps to the first hunk");

    // 'K' on the first hunk wraps to the last hunk.
    state.row_idx = first;
    state.prev_hunk();
    assert_eq!(state.row_idx, last, "K wraps to the last hunk");
}

#[test]
fn test_hunk_jump_places_head_line_at_top() {
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
    // The insertion above shifts new-side line numbers by one: new line
    // (i) carries old line (i-1)'s text, keeping the tape's declared
    // matching consistent (padding positions agree on both sides).
    for i in 5..=30 {
        new_lines.push(format!("    // filler {}", i - 1));
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

    let mut state = AppState::new(
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        ),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 6;
    state.last_content_width = 80;
    state.row_idx = 0;
    state.scroll = 0;
    assert_eq!(state.hunk_starts.len(), 2, "both hunks must be located");

    // J from the banner counts as inside hunk 1, so a single press lands
    // hunk 2 with its head line at screen row #0, not 10% below top.
    state.next_hunk();
    assert_eq!(state.current_hunk(), Some(1), "lands in hunk 2");
    let (starts, _, _, total_vis_lines) = state.visual_geometry(80);
    let h1_vis = starts[state.hunk_starts[1].flat];
    let max_scroll = total_vis_lines.saturating_sub(state.viewport_height.max(4));
    assert_eq!(
        state.scroll,
        h1_vis.min(max_scroll),
        "J must position scroll so the hunk head line is at row #0"
    );
    assert_eq!(
        h1_vis - state.scroll,
        0,
        "hunk head line must sit exactly at screen row #0"
    );

    // K -> back to hunk 1, which is the file's first hunk: banner stays at
    // row #0 with the head below it.
    state.prev_hunk();
    assert_eq!(state.current_hunk(), Some(0), "returns to hunk 1");
    let (starts, _, _, _) = state.visual_geometry(80);
    let h0_banner = state.first_row_of_file(0);
    let h0_vis = starts[state.hunk_starts[0].flat];
    assert_eq!(state.scroll, starts[h0_banner].min(max_scroll));
    assert_eq!(starts[h0_banner] - state.scroll, 0);
    assert!(h0_vis - state.scroll >= 1);
}

#[test]
fn test_move_down_up_wrap_around_edges() {
    let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
     let x = 1;
-    // old comment
+    // new comment
+    let y = 2;
 }
 "#;
    let new_content = [(
        "filesystem.rs".to_string(),
        "fn test() {\n    let x = 1;\n    // new comment\n    let y = 2;\n}\n".to_string(),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "filesystem.rs".to_string(),
        "fn test() {\n    let x = 1;\n    // old comment\n}\n".to_string(),
    )]
    .into_iter()
    .collect();

    let mut state = AppState::new(
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        ),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.last_content_width = 80;
    let last = state.flat.len() - 1;
    assert!(last > 0, "fixture must have multiple rows");
    let last_segs = state.row_seg_count(last, 80);

    // 'j' on the last segment of the last row wraps to the top.
    state.row_idx = last;
    state.vis_line = last_segs.saturating_sub(1);
    state.move_down_wrap();
    assert_eq!(state.row_idx, 0, "j at bottom wraps to first row");
    assert_eq!(state.vis_line, 0);

    // 'k' on the first row wraps to the bottom.
    state.move_up_wrap();
    assert_eq!(state.row_idx, last, "k at top wraps to last row");
    assert_eq!(
        state.vis_line,
        last_segs.saturating_sub(1),
        "k wrap lands on the last visual segment"
    );

    // Plain moves (mouse wheel path) clamp instead of wrapping.
    state.row_idx = last;
    state.vis_line = last_segs.saturating_sub(1);
    state.move_down();
    assert_eq!(state.row_idx, last, "wheel down clamps at bottom");
    state.row_idx = 0;
    state.vis_line = 0;
    state.move_up();
    assert_eq!(state.row_idx, 0, "wheel up clamps at top");
}

#[test]
fn test_hunk_jump_onto_first_hunk_keeps_banner() {
    let mut lines_a = String::new();
    let mut lines_b = String::new();
    let mut diff_a = String::new();
    let mut diff_b = String::new();
    for i in 1..=30 {
        lines_a.push_str(&format!("a line {i}\n"));
        lines_b.push_str(&format!("b line {i}\n"));
        diff_a.push_str(&format!("+a line {i}\n"));
        diff_b.push_str(&format!("+b line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}diff --git a/b.rs b/b.rs\nindex 222..333 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -0,0 +1,30 @@\n{diff_b}"
    );
    let new_content = [("a.rs".to_string(), lines_a), ("b.rs".to_string(), lines_b)]
        .into_iter()
        .collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;
    state.last_content_width = 80;
    state.row_idx = 0;
    state.scroll = 0;
    let a_hunk = state
        .hunk_starts
        .iter()
        .find(|h| h.file == 0)
        .map(|h| h.flat)
        .expect("a.rs hunk");
    let b_hunk = state
        .hunk_starts
        .iter()
        .find(|h| h.file == 1)
        .map(|h| h.flat)
        .expect("b.rs hunk");
    let a_banner = state.first_row_of_file(0);
    let b_banner = state.first_row_of_file(1);

    // J from the banner counts as inside H1, so it lands the next file's
    // first hunk and keeps that file's banner at the top row.
    state.next_hunk();
    assert_eq!(state.row_idx, b_hunk, "cursor on b.rs hunk head");
    let (starts, _, _, total_vis_lines) = state.visual_geometry(80);
    let max_scroll = total_vis_lines.saturating_sub(state.viewport_height.max(4));
    assert_eq!(state.scroll, starts[b_banner].min(max_scroll));
    assert_eq!(starts[b_banner] - state.scroll, 0, "b.rs banner at row #0");

    // K back onto a first hunk likewise reveals its banner.
    state.prev_hunk();
    assert_eq!(state.row_idx, a_hunk, "cursor on a.rs hunk head");
    let (starts, _, _, total_vis_lines) = state.visual_geometry(80);
    let max_scroll = total_vis_lines.saturating_sub(state.viewport_height.max(4));
    assert_eq!(state.scroll, starts[a_banner].min(max_scroll));
    assert_eq!(starts[a_banner] - state.scroll, 0, "a.rs banner at row #0");
}

#[test]
fn test_hunk_jump_from_banner_counts_as_inside_first_hunk() {
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
    // The insertion above shifts new-side line numbers by one: new line
    // (i) carries old line (i-1)'s text, keeping the tape's declared
    // matching consistent (padding positions agree on both sides).
    for i in 5..=30 {
        new_lines.push(format!("    // filler {}", i - 1));
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

    let mut state = AppState::new(
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        ),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 6;
    state.last_content_width = 80;

    // Cursor on the file banner counts as inside hunk #1: J skips to H2.
    state.row_idx = 0;
    assert!(state.flat[state.row_idx].is_file_header());
    state.next_hunk();
    assert_eq!(
        state.row_idx, state.hunk_starts[1].flat,
        "J from banner must skip H1 and land in H2"
    );
    assert_eq!(state.current_hunk(), Some(1));

    // K from the banner goes to the hunk before H1 (wrapping here).
    state.row_idx = 0;
    state.prev_hunk();
    assert_eq!(
        state.row_idx,
        state.hunk_starts[state.hunk_starts.len() - 1].flat,
        "K from banner must leave H1 backwards"
    );
}

#[test]
fn test_hunk_jump_from_hunkless_banner_falls_back_to_row() {
    use crate::core::{
        counts::FileCounts,
        merge::RunKind,
        parse::RowKind,
        wire::{OutputFile, OutputHunk, OutputRow, OutputRun},
    };

    let diff = r#"diff --git a/tracked.rs b/tracked.rs
index 1111111..2222222 100644
--- a/tracked.rs
+++ b/tracked.rs
@@ -1 +1,2 @@
 fn main() {}
+fn extra() {}
"#;
    let new_content = [(
        "tracked.rs".to_string(),
        "fn main() {}\nfn extra() {}\n".to_string(),
    )]
    .into_iter()
    .collect();
    let mut out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    out.files.insert(
        0,
        OutputFile {
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
            new_bytes: None,
            raw_source: None,
        },
    );

    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.viewport_height = 20;
    state.last_content_width = 80;
    // Untracked files collapse by default: banner only, no hunk starts.
    assert!(state.collapsed_files.contains(&0));
    assert!(state.hunk_starts.iter().all(|h| h.file != 0));

    // J from the hunkless banner falls back to plain row order.
    state.row_idx = 0;
    assert!(state.flat[state.row_idx].is_file_header());
    state.next_hunk();
    let tracked_hunk = state
        .hunk_starts
        .iter()
        .find(|h| h.file == 1)
        .map(|h| h.flat)
        .expect("tracked.rs must have a hunk");
    assert_eq!(state.row_idx, tracked_hunk);
}

#[test]
fn test_sub_row_navigation_through_notes_and_wrapped_lines() {
    let out = sample_output();
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.last_content_width = 80;
    state.viewport_height = 20;

    // Position cursor on row 2 (code row without hunk divider)
    state.row_idx = 2;
    state.vis_line = 0;

    // Add a 2-paragraph note draft (2 rendered lines) and commit it to row 2
    state.note_draft = Some("First note line\n\nSecond note line".into());
    state.commit_note_draft().expect("note commit must succeed");

    let cur_row = state.row_idx;
    let seg_count = state.row_seg_count(cur_row, 80);
    assert_eq!(seg_count, 1);

    // Initial vis_line is 0 (code row)
    assert_eq!(state.vis_line, 0);

    // move_down should step into note line 0 (vis_line = 1)
    state.move_down();
    assert_eq!(state.row_idx, cur_row);
    assert_eq!(state.vis_line, 1);

    // move_down should step into note line 1 (vis_line = 2, blank paragraph
    // separator)
    state.move_down();
    assert_eq!(state.row_idx, cur_row);
    assert_eq!(state.vis_line, 2);

    // move_down should step into note line 2 (vis_line = 3, Second note line)
    state.move_down();
    assert_eq!(state.row_idx, cur_row);
    assert_eq!(state.vis_line, 3);

    // Next move_down should advance to next row with vis_line = 0
    state.move_down();
    assert_eq!(state.row_idx, cur_row + 1);
    assert_eq!(state.vis_line, 0);

    // move_up should step back to the last note line of cur_row (vis_line = 3)
    state.move_up();
    assert_eq!(state.row_idx, cur_row);
    assert_eq!(state.vis_line, 3);

    state.move_up();
    assert_eq!(state.row_idx, cur_row);
    assert_eq!(state.vis_line, 2);

    state.move_up();
    assert_eq!(state.row_idx, cur_row);
    assert_eq!(state.vis_line, 1);

    state.move_up();
    assert_eq!(state.row_idx, cur_row);
    assert_eq!(state.vis_line, 0);
}

#[test]
fn test_viewport_scrolling_through_tall_content_without_min_vis_snap() {
    let out = sample_output();
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.last_content_width = 80;
    state.viewport_height = 10;

    state.row_idx = 1;
    state.vis_line = 0;
    let tall_note = (0..15)
        .map(|i| format!("- Note line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    state.note_draft = Some(tall_note);
    state.commit_note_draft().expect("note commit must succeed");

    // Move down 12 times into the note
    for _ in 0..12 {
        state.move_down();
    }
    assert_eq!(state.row_idx, 1);
    assert_eq!(state.vis_line, 12);

    // Render frame to update scroll clamping
    state.render_frame(80, 10);
    assert!(
        state.scroll > 1,
        "scroll ({}) should advance past min_vis (1) when cursor is at vis_line 12",
        state.scroll
    );
}

#[test]
fn test_viewport_scrolling_through_tall_wrapped_diff_line_without_min_vis_snap() {
    let mut out = sample_output();
    out.files[0].rows[0].runs = vec![OutputRun::new(RunKind::Ctx, "word ".repeat(300))];
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.last_content_width = 80;
    state.viewport_height = 10;

    let target_row = state.first_row_of_file(0) + 1;
    state.row_idx = target_row;
    state.vis_line = 0;

    let seg_count = state.row_seg_count(target_row, 80);
    assert!(seg_count >= 15, "should produce at least 15 segments");

    for _ in 0..12 {
        state.move_down();
    }
    assert_eq!(state.row_idx, target_row);
    assert_eq!(state.vis_line, 12);

    state.render_frame(80, 10);
    assert!(
        state.scroll > target_row,
        "scroll ({}) should advance past row top ({}) when cursor is at segment 12",
        state.scroll,
        target_row
    );
}

#[test]
fn test_hunk_divider_selectable_navigation() {
    let out = sample_output();
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.last_content_width = 80;
    assert!(state.flat[0].is_file_header());
    state.row_idx = 0;
    state.vis_line = 0;

    // Moving down from file header lands on hunk divider of row 1 (vis_line ==
    // 0)
    state.move_down();
    assert_eq!(state.row_idx, 1);
    assert_eq!(state.vis_line, 0);
    assert!(
        state.is_cursor_on_hunk_divider().is_some(),
        "vis_line 0 of hunk start row must be on the hunk divider"
    );

    // Moving down again lands on the first code row of row 1 (vis_line == 1)
    state.move_down();
    assert_eq!(state.row_idx, 1);
    assert_eq!(state.vis_line, 1);
    assert!(
        state.is_cursor_on_hunk_divider().is_none(),
        "vis_line 1 of hunk start row is the code segment, not the hunk divider"
    );

    // Moving up steps back to the hunk divider (vis_line == 0)
    state.move_up();
    assert_eq!(state.row_idx, 1);
    assert_eq!(state.vis_line, 0);
    assert!(state.is_cursor_on_hunk_divider().is_some());

    // Moving up steps back to the file header
    state.move_up();
    assert_eq!(state.row_idx, 0);
    assert_eq!(state.vis_line, 0);
}

#[test]
fn test_move_bottom_lands_on_bottom_gap_divider() {
    let diff = r#"diff --git a/trailing.rs b/trailing.rs
index 000..111 100644
--- a/trailing.rs
+++ b/trailing.rs
@@ -1,3 +1,3 @@
 line 1
-line 2
+line 2 mod
 line 3
"#;
    let old_content = (1..=20).map(|i| format!("line {i}\n")).collect::<String>();
    let new_content = (1..=20)
        .map(|i| {
            if i == 2 {
                "line 2 mod\n".to_string()
            } else {
                format!("line {i}\n")
            }
        })
        .collect::<String>();

    let mut state = AppState::new(
        diffuse_core::pipeline::PipelineInput::new(diff)
            .with_new_content([("trailing.rs".to_string(), new_content)].into())
            .with_old_content([("trailing.rs".to_string(), old_content)].into())
            .run(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    state.last_content_width = 80;
    state.viewport_height = 20;

    assert!(
        state.hunk_trailing_gap(0) > 0,
        "file must have a trailing gap"
    );

    // Press G (MoveToBottom)
    state.move_bottom();

    assert_eq!(state.row_idx, state.flat.len() - 1);
    assert!(
        state.is_cursor_on_bottom_divider().is_some(),
        "move_bottom (G) must position cursor on bottom gap divider"
    );

    let last = state.flat.last().unwrap();
    assert!(
        last.is_bottom_gap_divider(),
        "last flat row must be bottom gap divider"
    );
    assert!(
        !last.is_diff_row(),
        "bottom gap divider must not be a diff row"
    );
    assert_eq!(
        state.vis_line, 0,
        "vis_line must be 0 on first-class divider row"
    );

    // Move up with k -> moves to last diff row (row_idx = last - 1)
    state.move_up();
    assert_eq!(state.row_idx, state.flat.len() - 2);
    assert_eq!(state.is_cursor_on_bottom_divider(), None);

    // Move down with j -> moves back to bottom gap divider (row_idx = last)
    state.move_down();
    assert_eq!(state.row_idx, state.flat.len() - 1);
    assert_eq!(state.is_cursor_on_bottom_divider(), Some(0));

    // Pressing Enter expands context below
    let gap_before = state.hunk_trailing_gap(0);
    state.execute(crate::TuiCommand::ToggleCollapseFile(None));
    let gap_after = state.hunk_trailing_gap(0);
    assert!(
        gap_after < gap_before,
        "Enter on bottom divider expands context below"
    );
}
