use std::{
    collections::HashMap,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::{
    app::{AppState, temp_notes_path},
    command::TuiCommand,
    core::{PipelineOptions, pipeline::run_pipeline_full},
};

#[test]
fn test_clicking_file_header_triangle_toggles_collapse() {
    let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
     let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
     let y = 2;
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
    let mut state = AppState::new(
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        ),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.render_frame(120, 30);

    assert!(!state.collapsed_files.contains(&0), "starts expanded");
    let initial_len = state.flat.len();

    // Click exactly on the ▼ triangle (char col 12) on the header row
    let header_vis = state
        .visual_geometry(state.last_content_width)
        .0
        .first()
        .copied()
        .unwrap_or(0);
    let click_row = state.last_viewport_start + header_vis.saturating_sub(state.scroll);

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL,
        shift: false,
    });
    assert!(
        state.collapsed_files.contains(&0),
        "clicking the triangle must collapse the file"
    );
    assert_eq!(state.flat.len(), 1, "collapsed to header only");

    // Expand again via the (now ▸) triangle
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL,
        shift: false,
    });
    assert!(
        !state.collapsed_files.contains(&0),
        "clicking again must expand the file"
    );
    assert_eq!(state.flat.len(), initial_len);

    // A click elsewhere on the header must NOT toggle — it selects.
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL + 10,
        shift: false,
    });
    assert!(
        !state.collapsed_files.contains(&0),
        "non-triangle header clicks must not collapse"
    );
}

#[test]
fn test_default_collapse_pure_renames_and_mode_changes() {
    let diff = r#"diff --git a/pure_renamed_old.rs b/pure_renamed_new.rs
similarity index 100%
rename from pure_renamed_old.rs
rename to pure_renamed_new.rs
diff --git a/renamed_with_changes_old.rs b/renamed_with_changes_new.rs
similarity index 90%
rename from renamed_with_changes_old.rs
rename to renamed_with_changes_new.rs
--- a/renamed_with_changes_old.rs
+++ b/renamed_with_changes_new.rs
@@ -1,1 +1,1 @@
-old
+new
diff --git a/script.sh b/script.sh
old mode 100644
new mode 100755
"#;
    let new_content = [
        (
            "pure_renamed_new.rs".to_string(),
            "fn same() {}\n".to_string(),
        ),
        (
            "renamed_with_changes_new.rs".to_string(),
            "new\n".to_string(),
        ),
        ("script.sh".to_string(), "#!/bin/sh\n".to_string()),
    ]
    .into_iter()
    .collect();
    let old_content = [
        (
            "pure_renamed_old.rs".to_string(),
            "fn same() {}\n".to_string(),
        ),
        (
            "renamed_with_changes_old.rs".to_string(),
            "old\n".to_string(),
        ),
        ("script.sh".to_string(), "#!/bin/sh\n".to_string()),
    ]
    .into_iter()
    .collect();

    let output = run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );

    let state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // File 0: Pure rename -> collapsed by default
    assert!(
        state.collapsed_files.contains(&0),
        "pure rename must be collapsed by default"
    );

    // File 1: Rename with changes -> NOT collapsed by default
    assert!(
        !state.collapsed_files.contains(&1),
        "rename with content changes must NOT be collapsed by default"
    );

    // File 2: Pure mode change -> collapsed by default
    assert!(
        state.collapsed_files.contains(&2),
        "pure mode change must be collapsed by default"
    );
}

#[test]
fn test_deleted_files_collapsed_by_default_and_renders_deletes_on_expansion() {
    let diff = r#"diff --git a/deleted.rs b/deleted.rs
deleted file mode 100644
--- a/deleted.rs
+++ /dev/null
@@ -1,3 +0,0 @@
-line 1
-line 2
-line 3
"#;
    let old_content = [(
        "deleted.rs".to_string(),
        "line 1\nline 2\nline 3\n".to_string(),
    )]
    .into_iter()
    .collect();

    let output = run_pipeline_full(diff, None, Some(old_content), PipelineOptions::default());

    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // Deleted file starts collapsed by default
    assert!(
        state.collapsed_files.contains(&0),
        "deleted file must be collapsed by default"
    );
    assert_eq!(state.flat.len(), 1, "only header row in flat view");

    // Expand deleted file
    state.toggle_file_collapse(0);
    assert!(
        !state.collapsed_files.contains(&0),
        "deleted file must be expanded"
    );
    assert!(state.flat.len() > 1, "expanded deleted file must have rows");

    let f = &state.output.files[0];
    assert_eq!(f.counts.dels, 3);
    assert_eq!(f.counts.adds, 0);
    assert_eq!(f.hunks.len(), 1);
    for row in &f.hunks[0].rows {
        assert_eq!(row.kind, crate::core::parse::RowKind::Del);
    }
}

#[test]
fn test_collapsing_file_releases_memory_and_expanding_reloads() {
    let diff = r#"diff --git a/file.rs b/file.rs
--- a/file.rs
+++ b/file.rs
@@ -1,2 +1,2 @@
-old line
+new line
"#;
    let new_content = [("file.rs".to_string(), "new line\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("file.rs".to_string(), "old line\n".to_string())]
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

    assert!(!state.collapsed_files.contains(&0));
    assert!(state.output.files[0].is_loaded());
    let initial_flat_len = state.flat.len();
    assert!(initial_flat_len > 1);

    // Collapse the file: must release heavy memory
    state.toggle_file_collapse(0);
    assert!(state.collapsed_files.contains(&0));
    assert_eq!(
        state.flat.len(),
        1,
        "collapsed file has only header row in flat"
    );
    assert!(
        !state.output.files[0].is_loaded(),
        "collapsed file must have its rendered memory released"
    );
    assert!(state.output.files[0].hunks.is_empty());
    assert!(state.output.files[0].raw_hunks.is_empty());

    // Expanding it again: renders on demand and reloads
    state.toggle_file_collapse(0);
    assert!(!state.collapsed_files.contains(&0));
    assert!(
        state.output.files[0].is_loaded(),
        "expanding must reload the file"
    );
    assert_eq!(state.flat.len(), initial_flat_len);
    assert!(!state.output.files[0].hunks.is_empty());
}

#[test]
fn test_async_on_demand_expansion_sets_streaming_indicator() {
    let diff = r#"diff --git a/lazy.rs b/lazy.rs
--- a/lazy.rs
+++ b/lazy.rs
@@ -1,1 +1,1 @@
-old
+new
"#;
    let new_content = [("lazy.rs".to_string(), "new\n".to_string())]
        .into_iter()
        .collect();
    let old_content = [("lazy.rs".to_string(), "old\n".to_string())]
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
    state.enable_async_on_demand();

    // Collapse to release memory
    state.toggle_file_collapse(0);
    assert!(!state.output.files[0].is_loaded());

    // Expand with async on-demand enabled: starts loading in background
    state.toggle_file_collapse(0);
    assert!(state.loading_files.contains(&0));
    assert!(
        state.is_streaming,
        "streaming indicator must be active while loading"
    );

    // Wait up to 500ms for the background worker to finish
    let start = Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > Duration::from_millis(500) {
            panic!("timed out waiting for async on-demand load");
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(!state.loading_files.contains(&0));
    assert!(
        !state.is_streaming,
        "streaming indicator clears when loading finishes"
    );
    assert!(state.output.files[0].is_loaded());
    assert!(!state.output.files[0].hunks.is_empty());
}

fn create_multi_file_state(
    file_count: usize,
    rows_per_file: usize,
) -> (
    AppState,
    mpsc::Sender<(usize, crate::core::wire::OutputFile)>,
) {
    let mut diff = String::new();
    let mut new_map = HashMap::new();
    let mut old_map = HashMap::new();

    for i in 0..file_count {
        let name = format!("file_{i:02}.rs");
        diff.push_str(&format!(
            "diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1,{rows_per_file} +1,{rows_per_file} @@\n"
        ));
        let mut old_text = String::new();
        let mut new_text = String::new();
        for r in 0..rows_per_file {
            diff.push_str(&format!(
                "-old line {r} in {name}\n+new line {r} in {name}\n"
            ));
            old_text.push_str(&format!("old line {r} in {name}\n"));
            new_text.push_str(&format!("new line {r} in {name}\n"));
        }
        old_map.insert(name.clone(), old_text);
        new_map.insert(name, new_text);
    }

    let output = run_pipeline_full(
        &diff,
        Some(new_map),
        Some(old_map),
        PipelineOptions::default(),
    );

    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    for i in 0..file_count {
        state.collapsed_files.insert(i);
        state.output.files[i].release_rendered_memory();
    }
    state.rebuild_flat_preserving_cursor(0);

    let (tx, rx) = mpsc::channel();
    state.on_demand_tx = Some(tx.clone());
    state.on_demand_rx = Some(rx);

    (state, tx)
}

#[test]
fn test_expand_last_file_at_bottom_scrolls_down_after_load() {
    // 25 files, each with 10 diff lines.
    // All 25 files start collapsed and unloaded.
    let (mut state, _) = create_multi_file_state(25, 10);
    state.viewport_height = 20;
    state.last_content_width = 80;

    // Viewport height = 20.
    // Flat has 25 rows (one header per collapsed file).
    // Scroll to 5 so that the last file (file 24) is at the very bottom row of
    // the viewport (row 19).
    state.scroll = 5;
    state.row_idx = 24;

    let header_vis = 24;
    let screen_row_before = header_vis - state.scroll;
    assert_eq!(screen_row_before, 19, "file 24 is at the very bottom line");
    assert!(!state.is_file_loaded(24));

    // Expand the last file. In async mode, it begins loading in the background.
    state.toggle_file_collapse(24);
    assert!(!state.collapsed_files.contains(&24));
    assert!(state.loading_files.contains(&24));

    // BEFORE load completes:
    // Total visual lines is still 25, max_scroll is 5, scroll cannot move.
    assert_eq!(
        state.scroll, 5,
        "scroll must NOT have moved before the file loaded"
    );

    // Wait for the background worker to finish loading.
    let start = Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > Duration::from_millis(1000) {
            panic!("timed out waiting for async on-demand load");
        }
        thread::sleep(Duration::from_millis(5));
    }

    // AFTER load completes:
    // The file is loaded with 10 diff lines.
    // 20% of viewport height (20) is 4 lines.
    // Previously screen_row was 19 (1 line from bottom).
    // Now we must scroll down by 3 lines so file 24 occupies rows 16..=19 (4
    // lines = 20% from below).
    assert!(state.is_file_loaded(24));
    assert_eq!(
        state.scroll, 8,
        "must scroll down by 3 lines after loading completes to fill 20% from below"
    );
    let screen_row_after = header_vis - state.scroll;
    assert_eq!(screen_row_after, 16);
}

#[test]
fn test_expand_middle_file_at_bottom_scrolls_down_after_load() {
    let (mut state, _) = create_multi_file_state(25, 10);
    state.viewport_height = 20;
    state.last_content_width = 80;

    // Place middle file (file 12) at row 18 (bottom 2 lines of the 20-row
    // viewport) header_vis = 12, so scroll = 0 would put it at 12.
    // Let's set scroll = 0 and use file 18 at row 18:
    // header_vis of file 18 is 18.
    // screen_row = 18 - 0 = 18.
    state.scroll = 0;
    state.row_idx = 18;

    let header_vis = 18;
    let screen_row_before = header_vis - state.scroll;
    assert_eq!(
        screen_row_before, 18,
        "file 18 is at row 18 (2 lines from bottom)"
    );

    state.toggle_file_collapse(18);

    let start = Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > Duration::from_millis(1000) {
            panic!("timed out waiting for async on-demand load");
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(state.is_file_loaded(18));
    // 20% of 20 = 4 lines. Lines below was 2 (rows 18 and 19).
    // It must scroll down by 2 lines to make file 18 header sit at row 16.
    assert_eq!(
        state.scroll, 2,
        "must scroll down by 2 lines to fill 20% from below"
    );
    let screen_row_after = header_vis - state.scroll;
    assert_eq!(screen_row_after, 16);
}

#[test]
fn test_expand_first_file_not_at_bottom_does_not_scroll() {
    let (mut state, _) = create_multi_file_state(5, 10);
    state.viewport_height = 20;
    state.last_content_width = 80;
    state.scroll = 0;
    state.row_idx = 0;

    // File 0 is at row 0 (top of viewport).
    state.toggle_file_collapse(0);

    let start = Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > Duration::from_millis(1000) {
            panic!("timed out waiting for async on-demand load");
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(state.is_file_loaded(0));
    assert_eq!(
        state.scroll, 0,
        "first file at top of viewport must not scroll down"
    );
}

#[test]
fn test_expand_file_with_fewer_lines_fills_up_to_file_content() {
    // File with only 1 diff line (+1 header + 1 hunk divider = 3 visual lines
    // total)
    let (mut state, _) = create_multi_file_state(25, 1);
    state.viewport_height = 20;
    state.last_content_width = 80;

    // Put file 24 at row 19 (the last row)
    state.scroll = 5;
    state.row_idx = 24;

    state.toggle_file_collapse(24);

    let start = Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > Duration::from_millis(1000) {
            panic!("timed out waiting for async on-demand load");
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(state.is_file_loaded(24));
    // Target fill is 4 (20% of 20), but file 24 only has 3 visual lines total!
    // So it fills UP TO 3 lines (desired_visible = 3).
    // lines_below was 1, so it scrolls down by 2 (scroll 5 -> 7).
    assert_eq!(
        state.scroll, 7,
        "must scroll down by 2 lines since file only has 3 visual lines total"
    );
    let screen_row_after = 24 - state.scroll;
    assert_eq!(screen_row_after, 17);
}

#[test]
fn test_expand_file_cursor_clamped_if_scrolled_off_top() {
    let (mut state, _) = create_multi_file_state(25, 10);
    state.viewport_height = 20;
    state.last_content_width = 80;

    // Cursor is on file 5 (row 5). Scroll is 5.
    // File 24 is at row 24 (screen row 19).
    state.scroll = 5;
    state.row_idx = 5; // Header of file 5 is at visual line 5, currently at the top of the viewport (screen row 0).

    state.toggle_file_collapse(24);

    let start = Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > Duration::from_millis(1000) {
            panic!("timed out waiting for async on-demand load");
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(state.is_file_loaded(24));
    // Scroll moved from 5 to 8.
    // File 5 (visual line 5) would now be off-screen above the viewport (5 <
    // 8). The cursor must have been updated to file 24's header so it
    // remains visible!
    assert_eq!(state.scroll, 8);
    assert_eq!(state.row_idx, 24);
}

#[test]
fn test_adjust_scroll_for_expanded_file_noop_cases() {
    let (mut state, _) = create_multi_file_state(5, 10);
    state.viewport_height = 20;
    state.last_content_width = 80;

    // 1. vh == 0
    state.viewport_height = 0;
    state.adjust_scroll_for_expanded_file(0);
    assert_eq!(state.scroll, 0);

    // Reset vh
    state.viewport_height = 20;

    // 2. collapsed file
    state.adjust_scroll_for_expanded_file(0);
    assert_eq!(state.scroll, 0);

    // 3. Header above viewport
    state.collapsed_files.remove(&0);
    state.ensure_file_loaded_sync(0);
    state.scroll = 10;
    state.adjust_scroll_for_expanded_file(0);
    assert_eq!(state.scroll, 10);

    // 4. Header below viewport
    state.scroll = 0;
    // File 4 is at visual line 20+ if file 0 is expanded.
    state.collapsed_files.remove(&4);
    state.ensure_file_loaded_sync(4);
    let (starts, _, _, _) = state.visual_geometry(80);
    let header_vis_4 = starts[state.first_row_of_file(4)];
    if header_vis_4 >= 20 {
        state.adjust_scroll_for_expanded_file(4);
        assert_eq!(state.scroll, 0);
    }
}

#[test]
fn test_expand_file_cursor_move_while_streaming_cancels_auto_scroll() {
    let (mut state, _) = create_multi_file_state(25, 10);
    state.viewport_height = 20;
    state.last_content_width = 80;

    // Put file 24 at row 19 (the very bottom line of the 20-row viewport)
    state.scroll = 5;
    state.row_idx = 24;

    state.toggle_file_collapse(24);
    assert!(state.loading_files.contains(&24));

    // While still streaming, user moves cursor up
    state.execute(TuiCommand::MoveUp(1));
    assert_eq!(state.row_idx, 23);

    // Wait for the background worker to finish loading
    let start = Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > Duration::from_millis(1000) {
            panic!("timed out waiting for async on-demand load");
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(state.is_file_loaded(24));
    // Since the user moved cursor while streaming, we do NOT jump downwards!
    assert_eq!(
        state.scroll, 5,
        "moving cursor while streaming must cancel auto-scroll"
    );
}

#[test]
fn test_expand_file_ui_change_while_streaming_cancels_auto_scroll() {
    let (mut state, _) = create_multi_file_state(25, 10);
    state.viewport_height = 20;
    state.last_content_width = 80;

    // Put file 24 at row 19
    state.scroll = 5;
    state.row_idx = 24;

    state.toggle_file_collapse(24);
    assert!(state.loading_files.contains(&24));

    // While still streaming, user performs a UI change (e.g. toggles raw diff)
    state.execute(TuiCommand::ToggleRawDiff);

    // Wait for background worker to finish
    let start = Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > Duration::from_millis(1000) {
            panic!("timed out waiting for async on-demand load");
        }
        thread::sleep(Duration::from_millis(5));
    }

    assert!(state.is_file_loaded(24));
    // Must not jump downwards to scroll file 24 into view
    assert_eq!(
        state.scroll, 5,
        "UI state change while streaming must cancel auto-scroll"
    );
}

#[test]
fn test_enter_on_wrapped_file_header_preserves_visual_line() {
    let diff = r#"diff --git a/a_very_long_path_to_some_deeply_nested_file_in_the_repository.rs b/a_very_long_path_to_some_deeply_nested_file_in_the_repository.rs
index 1111111..2222222 100644
--- a/a_very_long_path_to_some_deeply_nested_file_in_the_repository.rs
+++ b/a_very_long_path_to_some_deeply_nested_file_in_the_repository.rs
@@ -1,3 +1,3 @@
 fn main() {
-    let x = 1;
+    let x = 2;
 }
"#;
    let new_content = [(
        "a_very_long_path_to_some_deeply_nested_file_in_the_repository.rs".to_string(),
        "fn main() {\n    let x = 2;\n}\n".to_string(),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "a_very_long_path_to_some_deeply_nested_file_in_the_repository.rs".to_string(),
        "fn main() {\n    let x = 1;\n}\n".to_string(),
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
        crate::render::DiffuseTheme::dark(),
    );
    // Render at width 40 so the long header wraps across at least 2 lines.
    state.render_frame(40, 20);

    let cw = state.effective_content_width();
    assert!(
        state.row_seg_count(0, cw) >= 2,
        "file header must wrap across at least 2 lines"
    );

    // Position cursor on the file header row, second visual line
    state.row_idx = 0;
    state.vis_line = 1;

    // Press Enter to collapse the file
    state.execute(TuiCommand::ToggleCollapseFile(None));
    assert!(state.collapsed_files.contains(&0), "file must be collapsed");
    assert_eq!(
        state.row_idx, 0,
        "cursor must remain on the file header row"
    );
    assert_eq!(
        state.vis_line, 1,
        "cursor must remain on the second row of the file header after collapsing"
    );

    // Press Enter to expand the file
    state.execute(TuiCommand::ToggleCollapseFile(None));
    assert!(!state.collapsed_files.contains(&0), "file must be expanded");
    assert_eq!(
        state.row_idx, 0,
        "cursor must remain on the file header row"
    );
    assert_eq!(
        state.vis_line, 1,
        "cursor must remain on the second row of the file header after expanding"
    );
}
