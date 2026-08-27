use diffuse_core::PipelineOptions;
use diffuse_tui::{AppState, TuiCommand};

fn sample_output() -> diffuse_core::wire::PipelineOutput {
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
    diffuse_core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    )
}

fn two_hunk_gap_output() -> diffuse_core::wire::PipelineOutput {
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -23,9 +24,9 @@
 // line 23
 // line 24
 // line 25
 // line 26
-// line 27 old
+// line 27 new
 // line 28
 // line 29
 // line 30
 // line 31
"#;
    let mut old_lines = Vec::new();
    for i in 1..=40 {
        if i == 27 {
            old_lines.push("// line 27 old".to_string());
        } else {
            old_lines.push(format!("// line {i}"));
        }
    }
    let mut new_lines = Vec::new();
    for i in 1..=40 {
        if i == 5 {
            new_lines.push("// inserted one".to_string());
        }
        if i == 28 {
            new_lines.push("// line 27 new".to_string());
        } else {
            new_lines.push(format!("// line {i}"));
        }
    }
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    diffuse_core::pipeline::PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run()
}

fn multi_file_output() -> diffuse_core::wire::PipelineOutput {
    let diff = r#"diff --git a/a.rs b/a.rs
index 1111111..2222222 100644
--- a/a.rs
+++ b/a.rs
@@ -1,3 +1,3 @@
- old a
+ new a
diff --git a/b.rs b/b.rs
index 1111111..2222222 100644
--- a/b.rs
+++ b/b.rs
@@ -1,3 +1,3 @@
- old b
+ new b
"#;
    let new_content = [
        ("a.rs".to_string(), "new a\n".to_string()),
        ("b.rs".to_string(), "new b\n".to_string()),
    ]
    .into_iter()
    .collect();
    let old_content = [
        ("a.rs".to_string(), "old a\n".to_string()),
        ("b.rs".to_string(), "old b\n".to_string()),
    ]
    .into_iter()
    .collect();
    diffuse_core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    )
}

// 1. Rendering — header, status, viewport, scrollbar agnostic to syntax
#[test]
fn test_render_frame_header_and_viewport() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-render-1.json"),
    );
    let frame = state.render_frame(120, 30);
    assert!(
        frame.contains("filesystem.rs"),
        "header must show file path"
    );
    assert!(frame.contains("test_repo") || frame.contains("filesystem.rs"));
    assert!(!frame.is_empty());
    // viewport respects width — lines are bounded (ANSI codes inflate length)
    for line in frame.lines().take(10) {
        assert!(line.len() < 800, "frame line must be bounded");
    }
}

// 2. File collapse — toggle via API and via header triangle click
#[test]
fn test_file_collapse_toggle_and_click() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-collapse.json"),
    );
    state.render_frame(120, 30);
    assert!(!state.collapsed_files.contains(&0));
    let full_len = state.flat.len();

    state.toggle_file_collapse(0);
    assert!(state.collapsed_files.contains(&0));
    assert_eq!(state.flat.len(), 1);

    state.toggle_file_collapse(0);
    assert!(!state.collapsed_files.contains(&0));
    assert_eq!(state.flat.len(), full_len);

    // via click on triangle
    state.render_frame(120, 30);
    let header_vis = state.visual_geometry(state.last_content_width).0[0];
    let click_row = state.last_viewport_start + header_vis.saturating_sub(state.scroll);
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: diffuse_tui::view::diff_grid::FILE_HEADER_INDICATOR_COL,
        shift: false,
    });
    assert!(state.collapsed_files.contains(&0));
}

// 3. Navigation — move, page, jump between hunks and files
#[test]
fn test_navigation_moves_and_hunk_jumps() {
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-nav.json"),
    );
    state.render_frame(80, 20);
    let start = state.row_idx;
    state.move_down();
    assert_ne!(state.row_idx, start);
    state.move_up();
    assert_eq!(state.row_idx, start);

    state.next_hunk();
    let after_next = state.row_idx;
    state.prev_hunk();
    assert!(state.row_idx <= after_next);

    let file_before = state.current_file_idx();
    state.next_file();
    assert_ne!(state.current_file_idx(), file_before);
    state.prev_file();
    assert_eq!(state.current_file_idx(), file_before);

    state.goto_file_idx(1);
    assert_eq!(state.current_file_idx(), 1);
    state.page_down();
    state.page_up();
    state.move_top();
    assert_eq!(state.row_idx, 0);
    state.move_bottom();
    assert!(state.row_idx > 0);
}

// 4. Selection — anchor, band, confirm, clear (black-box via render)
#[test]
fn test_selection_anchor_band_and_confirm() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-sel.json"),
    );
    // walk to first coordinate row then anchor
    for _ in 0..3 {
        state.move_down();
    }
    state.start_or_confirm_selection();
    state.move_down();
    state.move_down();
    let sel = state
        .compute_selection()
        .expect("anchor over coordinate rows must yield selection");
    assert!(sel.start < sel.end);

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("\x1b[48;5;240m"),
        "active anchor must band rows"
    );

    state.start_or_confirm_selection();
    assert!(state.anchor.is_none());
    assert!(state.confirmed_sel.is_some());

    let frame2 = state.render_frame(100, 30);
    assert!(
        frame2.contains("\x1b[48;5;240m"),
        "confirmed selection bands"
    );

    state.execute(TuiCommand::ClearSelection);
    assert!(state.confirmed_sel.is_none());
}

// 5. Notes — draft, commit, callout, delete, persistence
#[test]
fn test_notes_commit_and_delete_via_public_api() {
    let path = std::env::temp_dir().join(format!("diffuse-it-notes-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let mut state = AppState::new(sample_output(), "test_repo".into(), path.clone());

    let file = state.current_file().unwrap().path.clone();
    state.note_draft = Some("review this".into());
    state.commit_note_draft();
    assert!(state.note_draft.is_none());
    assert_eq!(
        diffuse_core::notes::notes_for_file(&state.notes, "test_repo", &file).count(),
        1
    );
    let frame = state.render_frame(100, 30);
    assert!(frame.contains("💬"), "committed note must show callout");

    state.delete_notes();
    assert_eq!(
        diffuse_core::notes::notes_for_file(&state.notes, "test_repo", &file).count(),
        0
    );
    let _ = std::fs::remove_file(path);
}

// 6. Finder — open, query, confirm, cancel
#[test]
fn test_finder_open_query_and_confirm() {
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-finder.json"),
    );
    assert!(state.file_finder.is_none());
    state.open_file_finder();
    assert!(state.file_finder.is_some());

    state.execute(TuiCommand::UpdateFileFinderQuery("a.rs".into()));
    assert!(state.file_finder.as_ref().unwrap().query.contains("a.rs"));

    // confirm should jump to matched file
    state.confirm_file_finder();
    assert!(state.file_finder.is_none());

    state.open_file_finder();
    state.cancel_file_finder();
    assert!(state.file_finder.is_none());
}

// 7. Raw diff — toggle via execute, cursor anchoring
#[test]
fn test_raw_diff_toggle_and_cursor_preserved() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-raw2.json"),
    );
    for _ in 0..4 {
        state.move_down();
    }
    let coord_before = state.flat_row_coord(&state.flat[state.row_idx]);
    assert!(coord_before.is_some());
    state.execute(TuiCommand::ToggleRawDiff);
    assert!(state.raw_diff);
    assert!(state.flat.iter().any(|fr| fr.is_hunk_divider));
    let frame = state.render_frame(120, 40);
    assert!(frame.contains("@@ -"));
    state.execute(TuiCommand::ToggleRawDiff);
    assert!(!state.raw_diff);
    let coord_after = state.flat_row_coord(&state.flat[state.row_idx]);
    assert_eq!(coord_before, coord_after);
}

// 8. Context expansion via public API — below/above, gap shrinks
#[test]
fn test_context_expand_below_and_above_shrinks_gap() {
    let mut state = AppState::new(
        two_hunk_gap_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-ctx.json"),
    )
    .with_expand_lines(6);
    let fi = 0;
    assert_eq!(state.output.files[fi].hunks.len(), 2);
    let gap_before = state.hunk_leading_gap(fi, 1);
    assert!(gap_before > 0);

    assert!(state.expand_hunk_below(fi, 0));
    let gap_after_below = state.hunk_leading_gap(fi, 1);
    assert!(gap_after_below < gap_before);

    assert!(state.expand_hunk_above(fi, 1));
    let gap_after_above = state.hunk_leading_gap(fi, 1);
    assert!(gap_after_above <= gap_after_below);
}

// 9. Hunk divider click — expand via hit-testing, divider stays pinned
#[test]
fn test_hunk_divider_click_expands_and_stays_pinned() {
    let mut state = AppState::new(
        two_hunk_gap_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-click.json"),
    )
    .with_expand_lines(6);
    state.render_frame(140, 80);
    let fi = 0;
    let gap_before = state.hunk_leading_gap(fi, 1);
    assert!(gap_before > 0);

    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;
    let div_vis = starts[div_flat];
    let click_row = state.last_viewport_start + div_vis.saturating_sub(state.scroll);

    let h = &state.output.files[fi].hunks[1];
    let txt = diffuse_tui::view::hunk_divider_text(1, h, gap_before);
    let col = txt
        .find("▲▲")
        .map(|i| txt[..i].chars().count())
        .unwrap_or(10);

    let gap_before_click = gap_before;
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col,
        shift: false,
    });
    let gap_after = state.hunk_leading_gap(fi, 1);
    assert!(gap_after < gap_before_click, "click must expand gap");
    // divider should still exist unless gap fully consumed and hunks merged
    let still_divider = state
        .hunk_starts
        .iter()
        .any(|hs| hs.file == fi && hs.hunk == 1);
    // if gap was exactly consumed, merge is expected; otherwise divider remains
    if gap_after > 0 {
        assert!(
            still_divider,
            "divider should remain when gap not fully consumed"
        );
    }
}

// 10. Expand all merges and reset restores
#[test]
fn test_expand_all_merges_and_reset_restores() {
    let mut state = AppState::new(
        two_hunk_gap_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-all.json"),
    )
    .with_expand_lines(12);
    let fi = 0;
    assert_eq!(state.output.files[fi].hunks.len(), 2);
    state.expand_all_hidden();
    assert_eq!(
        state.output.files[fi].hunks.len(),
        1,
        "expand_all must merge gap"
    );
    // find a row in merged hunk and reset
    let row_in_file = state
        .flat
        .iter()
        .position(|fr| fr.file_idx == fi && !fr.is_file_header)
        .unwrap();
    state.jump_to_row(row_in_file, false);
    state.reset_current_hunk();
    assert_eq!(
        state.output.files[fi].hunks.len(),
        2,
        "reset must split back"
    );
    let frame = state.render_frame(140, 80);
    assert!(frame.contains("───── H1 ·") || frame.contains("───── H2 ·"));
}

// 11. Streaming append — live file ingestion
#[test]
fn test_streaming_append_new_file() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-stream.json"),
    );
    let before_files = state.output.files.len();
    let diff = r#"diff --git a/new_stream.rs b/new_stream.rs
new file mode 100644
--- /dev/null
+++ b/new_stream.rs
@@ -0,0 +1 @@
+new content
"#;
    let new_content = [("new_stream.rs".to_string(), "new content\n".to_string())]
        .into_iter()
        .collect();
    let out = diffuse_core::pipeline::run_pipeline_full(
        diff,
        Some(new_content),
        None,
        PipelineOptions::default(),
    );
    let new_file = out
        .files
        .into_iter()
        .find(|f| f.path == "new_stream.rs")
        .unwrap();
    state.append_file(new_file);
    assert_eq!(state.output.files.len(), before_files + 1);
    assert!(
        state
            .flat
            .iter()
            .any(|fr| { state.output.files[fr.file_idx].path == "new_stream.rs" })
    );
}

// 12. Help modal — toggles and overlays
#[test]
fn test_help_modal_toggle_and_frame_contains_bindings() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        std::env::temp_dir().join("diffuse-it-help.json"),
    );
    let plain = state.render_frame(100, 30);
    assert!(!plain.contains("KEY BINDINGS"));
    state.execute(TuiCommand::ToggleHelp);
    let with_help = state.render_frame(100, 30);
    assert!(with_help.contains("KEY BINDINGS"));
    state.execute(TuiCommand::ToggleHelp);
    let plain2 = state.render_frame(100, 30);
    assert!(!plain2.contains("KEY BINDINGS"));
}
