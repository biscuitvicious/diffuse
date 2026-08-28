use crate::{
    app::AppState,
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
    let mut state = AppState::new(
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        ),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-test-notes-header-triangle.json"),
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
