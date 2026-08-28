use crate::{
    app::{AppState, temp_notes_path},
    core::{PipelineOptions, pipeline::run_pipeline_full, wire::PipelineOutput},
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

#[test]
fn test_selection_anchor_span_snaps_to_coord_rows() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    // flat[0] is the file header (no coordinate); AppState starts on the
    // first coordinate row (1020). The engine pads the hunk with empty
    // above-context rows (1022-1024), then the real rows: ctx
    // " let x = 1;" (1025), the mod pair (1026), the add (1027). Walk to
    // the first real row, anchor, extend over the mod.
    for _ in 0..3 {
        state.move_down();
    }
    state.start_or_confirm_selection();
    state.move_down();
    state.move_down();

    let sel = state
        .compute_selection()
        .expect("anchor over coordinate rows must yield a selection");
    assert_eq!(sel.file_idx, 0);
    assert!(sel.start < sel.end, "range must span two rows");
    assert_eq!(sel.start, 1025);
    assert_eq!(sel.end, 1027);
    assert!(
        sel.side_is_new,
        "a range led by a new-side row is a new-side selection"
    );
}

#[test]
fn test_selection_v_confirm_clears_anchor_and_band_renders() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    for _ in 0..3 {
        state.move_down();
    }
    state.start_or_confirm_selection();
    state.move_down();
    state.move_down();

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("\x1b[48;5;240m"),
        "an active anchor must band the rows between anchor and cursor"
    );

    state.start_or_confirm_selection();
    assert!(state.anchor.is_none(), "confirming clears the anchor");
    let sel = state
        .confirmed_sel
        .expect("confirm must record a selection");
    assert_eq!(sel.start, 1025);
    assert_eq!(sel.end, 1027);

    let frame2 = state.render_frame(100, 30);
    assert!(
        frame2.contains("\x1b[48;5;240m"),
        "a confirmed selection must keep banding its coordinate range"
    );
}

#[test]
fn test_selection_requires_coord_rows() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    // Anchor on the file header (no coordinate) and confirm in place: the
    // range collapses onto the header which carries no line number, so
    // the reference computes no selection.
    state.move_up();
    state.start_or_confirm_selection();
    state.start_or_confirm_selection();
    assert!(state.anchor.is_none());
    assert!(state.confirmed_sel.is_none(), "header has no coordinate");

    // Anchoring on the header then walking onto a coordinate row snaps
    // the low end down to the first coordinate row of the file.
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    state.move_up();
    state.start_or_confirm_selection();
    state.move_down();
    let sel = state
        .compute_selection()
        .expect("anchor over the header must snap to a coordinate row");
    assert_eq!(sel.start, 1022, "snap to the first padded ctx row");
    assert_eq!(sel.end, 1022);
}
