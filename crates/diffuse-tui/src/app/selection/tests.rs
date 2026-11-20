use crate::{
    app::{AppState, temp_notes_path},
    core::{PipelineOptions, pipeline::run_pipeline_full, wire::PipelineOutput},
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

/// Primary display number of a selection bound (new side preferred).
fn coord_num(c: diffuse_core::notes::DiffCoord) -> u32 {
    c.new_line_num
        .or(c.old_line_num)
        .map(|n| n.get())
        .unwrap_or(0)
}

#[test]
fn test_selection_anchor_span_snaps_to_coord_rows() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    assert!(
        coord_num(sel.start) < coord_num(sel.end),
        "range must span two rows"
    );
    assert_eq!(coord_num(sel.start), 1025);
    assert_eq!(coord_num(sel.end), 1027);
    assert!(
        sel.start.new_line_num.is_some() && sel.end.new_line_num.is_some(),
        "a range led by a new-side row carries new-side numbers on both bounds"
    );
}

#[test]
fn test_selection_v_confirm_clears_anchor_and_band_renders() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    for _ in 0..3 {
        state.move_down();
    }
    state.start_or_confirm_selection();
    state.move_down();
    state.move_down();

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("\x1b[48;5;235m"),
        "an active anchor must band the rows between anchor and cursor"
    );

    state.start_or_confirm_selection();
    assert!(state.anchor.is_none(), "confirming clears the anchor");
    let sel = state
        .confirmed_sel
        .expect("confirm must record a selection");
    assert_eq!(coord_num(sel.start), 1025);
    assert_eq!(coord_num(sel.end), 1027);

    let frame2 = state.render_frame(100, 30);
    assert!(
        frame2.contains("\x1b[48;5;235m"),
        "a confirmed selection must keep banding its coordinate range"
    );
}

#[test]
fn test_selection_requires_coord_rows() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    // Anchor on the file header (no coordinate) and confirm in place: the
    // range collapses onto the header which carries no line number, so
    // the reference computes no selection.
    state.row_idx = 0;
    state.vis_line = 0;
    state.start_or_confirm_selection();
    state.start_or_confirm_selection();
    assert!(state.anchor.is_none());
    assert!(state.confirmed_sel.is_none(), "header has no coordinate");

    // Anchoring on the header then walking onto a coordinate row snaps
    // the low end down to the first coordinate row of the file.
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.row_idx = 0;
    state.vis_line = 0;
    state.start_or_confirm_selection();
    state.move_down();
    let sel = state
        .compute_selection()
        .expect("anchor over the header must snap to a coordinate row");
    assert_eq!(
        coord_num(sel.start),
        1022,
        "snap to the first padded ctx row"
    );
    assert_eq!(coord_num(sel.end), 1022);
}

#[test]
fn test_selection_non_coord_gap_returns_none_when_snapping_inverts() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    // Insert non-coordinate divider rows between two coordinate rows.
    state
        .flat
        .insert(2, crate::types::FlatRow::hunk_divider(0, 0));
    state
        .flat
        .insert(3, crate::types::FlatRow::hunk_divider(0, 0));

    state.anchor = Some(2);
    state.row_idx = 3;

    assert!(
        state.compute_selection().is_none(),
        "selection spanning exclusively non-coordinate rows must return None when snapping crosses"
    );
}

#[test]
fn test_selection_loc_label_formatting() {
    use crate::types::Selection;
    use diffuse_core::notes::DiffCoord;

    let sel_mod = Selection {
        file_idx: 0,
        start: DiffCoord::both(1615, 1617),
        end: DiffCoord::both(1616, 1618),
    };
    assert_eq!(sel_mod.loc_label("src/wire.rs"), "src/wire.rs:1617-1618");

    let sel_mixed = Selection {
        file_idx: 0,
        start: DiffCoord::both(8, 12),
        end: DiffCoord::old_side(10),
    };
    assert_eq!(
        sel_mixed.loc_label("src/types.rs"),
        "src/types.rs:12-old 10"
    );

    let sel_single = Selection {
        file_idx: 0,
        start: DiffCoord::both(8, 12),
        end: DiffCoord::both(8, 12),
    };
    assert_eq!(sel_single.loc_label("src/types.rs"), "src/types.rs:12");

    let sel_old_only = Selection {
        file_idx: 0,
        start: DiffCoord::old_side(5),
        end: DiffCoord::old_side(10),
    };
    assert_eq!(
        sel_old_only.loc_label("src/lib.rs"),
        "src/lib.rs:old 5-old 10"
    );
}
