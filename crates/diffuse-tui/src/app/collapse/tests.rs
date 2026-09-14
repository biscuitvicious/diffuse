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

    let state = AppState::new(output, "test_repo".into(), temp_notes_path());

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

    let mut state = AppState::new(output, "test_repo".into(), temp_notes_path());

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

    let mut state = AppState::new(output, "test_repo".into(), temp_notes_path());

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

    let mut state = AppState::new(output, "test_repo".into(), temp_notes_path());
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
    let start = std::time::Instant::now();
    while !state.poll_on_demand() {
        if start.elapsed() > std::time::Duration::from_millis(500) {
            panic!("timed out waiting for async on-demand load");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    assert!(!state.loading_files.contains(&0));
    assert!(
        !state.is_streaming,
        "streaming indicator clears when loading finishes"
    );
    assert!(state.output.files[0].is_loaded());
    assert!(!state.output.files[0].hunks.is_empty());
}
