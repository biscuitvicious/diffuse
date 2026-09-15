use crate::{
    app::{AppState, temp_notes_path},
    core::{PipelineOptions, pipeline::run_pipeline_full, wire::PipelineOutput},
};

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
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());

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
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
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
        state.flat[state.row_idx].is_file_header,
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
fn test_goto_line_preserves_viewport_margin_above() {
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
    );
    state.viewport_height = 20;

    state.goto_line(30);
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let target_vis = starts[state.row_idx];
    let margin = state.viewport_margin();
    assert_eq!(
        state.scroll,
        target_vis.saturating_sub(margin),
        "goto_line must precede target line by 1/10 viewport margin"
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
fn test_next_and_prev_file_navigation_centers_banner_at_row_zero() {
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
    );
    state.viewport_height = 20;

    assert_eq!(state.current_file_idx(), 0, "starts on file 0 (a.rs)");

    // Press 'n' -> next_file()
    state.next_file();
    assert_eq!(state.current_file_idx(), 1, "navigates to file 1 (b.rs)");
    let b_banner = state.first_row_of_file(1);
    assert_eq!(state.row_idx, b_banner, "lands on b.rs banner");
    assert!(state.flat[state.row_idx].is_file_header);

    let (starts, _, _, total_vis_lines) = state.visual_geometry(state.last_content_width);
    let b_vis = starts[b_banner];
    let max_rows = state.viewport_height.max(4);
    let max_scroll = total_vis_lines.saturating_sub(max_rows);
    assert_eq!(
        state.scroll,
        b_vis.min(max_scroll),
        "next_file must position scroll so banner is at row #0 of the main view"
    );
    assert_eq!(
        b_vis - state.scroll,
        0,
        "banner must sit exactly at screen row #0"
    );

    // Press 'n' again to go to file 2 (c.rs) which is near/at bottom
    state.next_file();
    assert_eq!(state.current_file_idx(), 2, "navigates to file 2 (c.rs)");
    let c_banner = state.first_row_of_file(2);
    assert_eq!(state.row_idx, c_banner);
    let c_vis = starts[c_banner];
    assert_eq!(
        state.scroll,
        c_vis.min(max_scroll),
        "at the very bottom of the scrollable view, scroll is clamped to max_scroll"
    );

    // Press 'p' -> prev_file() back to file 1 (b.rs)
    state.prev_file();
    assert_eq!(
        state.current_file_idx(),
        1,
        "navigates back to file 1 (b.rs)"
    );
    assert_eq!(state.scroll, b_vis.min(max_scroll));
    assert_eq!(b_vis - state.scroll, 0, "b.rs banner sits at row #0");

    // Press 'p' -> prev_file() back to file 0 (a.rs)
    state.prev_file();
    assert_eq!(
        state.current_file_idx(),
        0,
        "navigates back to file 0 (a.rs)"
    );
    assert_eq!(state.scroll, 0, "a.rs sits at row #0");
}
