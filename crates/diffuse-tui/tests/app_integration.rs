use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use diffuse_tui::{
    AppState, TuiCommand,
    core::{
        PipelineOptions,
        notes::notes_for_file,
        pipeline::{PipelineInput, run_pipeline_full},
        wire::PipelineOutput,
    },
    render::Badge,
    view::{diff_grid::FILE_HEADER_INDICATOR_COL, hunk_divider_text},
};

fn temp_notes_path() -> PathBuf {
    PathBuf::from("/nonexistent-diffuse-test-notes.json")
}

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

fn two_hunk_gap_output() -> PipelineOutput {
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
@@ -20,8 +21,9 @@
 // line 20
 // line 21
 // line 22
 // line 23
+// inserted two
 // line 24
 // line 25
 // line 26
 // line 27
"#;
    let new_lines: Vec<String> = (1..=30)
        .map(|i| {
            if i == 5 {
                "// inserted one".to_string()
            } else if i == 25 {
                "// inserted two".to_string()
            } else {
                format!("// line {i}")
            }
        })
        .collect();
    let old_lines: Vec<String> = (1..=28).map(|i| format!("// line {i}")).collect();
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run()
}

fn multi_file_output() -> PipelineOutput {
    let diff = r#"diff --git a/file1.rs b/file1.rs
index 1111111..2222222 100644
--- a/file1.rs
+++ b/file1.rs
@@ -1,3 +1,3 @@
 fn a() {}
-fn b() {}
+fn b_mod() {}
 fn c() {}
diff --git a/file2.rs b/file2.rs
index 3333333..4444444 100644
--- a/file2.rs
+++ b/file2.rs
@@ -1,3 +1,3 @@
 fn x() {}
-fn y() {}
+fn y_mod() {}
 fn z() {}
"#;
    let new_content = [
        (
            "file1.rs".to_string(),
            "fn a() {}\nfn b_mod() {}\nfn c() {}\n".to_string(),
        ),
        (
            "file2.rs".to_string(),
            "fn x() {}\nfn y_mod() {}\nfn z() {}\n".to_string(),
        ),
    ]
    .into_iter()
    .collect();
    let old_content = [
        (
            "file1.rs".to_string(),
            "fn a() {}\nfn b() {}\nfn c() {}\n".to_string(),
        ),
        (
            "file2.rs".to_string(),
            "fn x() {}\nfn y() {}\nfn z() {}\n".to_string(),
        ),
    ]
    .into_iter()
    .collect();
    run_pipeline_full(
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
        col: FILE_HEADER_INDICATOR_COL,
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
    assert!(
        sel.start.new_line_num.map(|n| n.get()) < sel.end.new_line_num.map(|n| n.get())
            || sel.start.old_line_num.map(|n| n.get()) < sel.end.old_line_num.map(|n| n.get()),
        "range must span two rows"
    );

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("\x1b[48;5;235m"),
        "active anchor must band rows"
    );

    state.start_or_confirm_selection();
    assert!(state.anchor.is_none());
    assert!(state.confirmed_sel.is_some());

    let frame2 = state.render_frame(100, 30);
    assert!(
        frame2.contains("\x1b[48;5;235m"),
        "confirmed selection bands"
    );

    state.execute(TuiCommand::ClearSelection);
    assert!(state.confirmed_sel.is_none());
}

// 5. Notes — draft, commit, callout, delete, persistence
#[test]
fn test_notes_commit_and_delete_via_public_api() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        path.clone(),
        diffuse_tui::render::DiffuseTheme::dark(),
    );

    let file = state.current_file().unwrap().path.clone();
    state.note_draft = Some("review this".into());
    state.commit_note_draft().expect("note commit must succeed");
    assert!(state.note_draft.is_none());
    assert_eq!(
        notes_for_file(&state.notes, "test_repo", "", &file).count(),
        1
    );
    let frame = state.render_frame(100, 30);
    assert!(frame.contains("💬"), "committed note must show callout");

    // Targeted deletion requires focusing the note line
    state.move_down();
    state.delete_note();
    assert_eq!(
        notes_for_file(&state.notes, "test_repo", "", &file).count(),
        0
    );
}

#[test]
fn test_rev_key_scopes_state_to_commit() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        path.clone(),
        diffuse_tui::render::DiffuseTheme::dark(),
    )
    .with_rev_key(Some("abc1234".into()));

    let file = state.current_file().unwrap().path.clone();
    state.note_draft = Some("commit-scoped note".into());
    state.commit_note_draft().expect("note commit must succeed");

    // Visible under the scoped rev_key, invisible under bare/empty or
    // any other commit; the display name stays human-readable.
    assert_eq!(
        notes_for_file(&state.notes, "test_repo", "abc1234", &file).count(),
        1
    );
    assert_eq!(
        notes_for_file(&state.notes, "test_repo", "", &file).count(),
        0
    );
    assert_eq!(
        notes_for_file(&state.notes, "test_repo", "def5678", &file).count(),
        0
    );
    assert_eq!(state.repo, "test_repo");
}

// 6. Finder — open, query, confirm, cancel
#[test]
fn test_finder_open_query_and_confirm() {
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
    );
    for _ in 0..4 {
        state.move_down();
    }
    let coord_before = state.flat_row_coord(&state.flat[state.row_idx]);
    assert!(coord_before.is_some());
    state.execute(TuiCommand::ToggleRawDiff);
    assert!(state.raw_diff);
    assert!(state.flat.iter().any(|fr| fr.is_hunk_divider()));
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
    let txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: Some(state.output.files[fi].hunks.len()),
        },
        h,
        gap_before,
    );
    let col = txt
        .find("▲")
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
        .position(|fr| fr.file_idx == fi && !fr.is_file_header())
        .unwrap();
    state.jump_to_row(row_in_file, false);
    state.reset_current_hunk();
    assert_eq!(
        state.output.files[fi].hunks.len(),
        2,
        "reset must split back"
    );
    let frame = state.render_frame(140, 80);
    assert!(frame.contains("──── Hunk 1/2 ·") || frame.contains("──── Hunk 2/2 ·"));
}

// 11. Streaming append — live file ingestion
#[test]
fn test_streaming_append_new_file() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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
    let out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
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
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
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

// 13. Visual initial screen layout and rendering
#[test]
fn test_initial_screen_rendered_visual_frame() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut state = AppState::new(
        sample_output(),
        "diffuse".into(),
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
    );
    let width = 80;
    let height = 18;
    let backend = TestBackend::new(width as u16, height as u16);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut screen_lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x as u16, y as u16)].symbol());
        }
        screen_lines.push(line);
    }

    println!("\n┌{}┐", "─".repeat(width));
    for (y, l) in screen_lines.iter().enumerate() {
        println!("│{:<width$}│ (row {y:02})", l, width = width);
    }
    println!("└{}┘\n", "─".repeat(width));

    // Row 0: Top Header Bar (repo + reviewed file count + global mod counts)
    assert!(screen_lines[0].starts_with(" diffuse 1 file +1 ~1"));
    assert!(screen_lines[0].contains("+1"));
    assert!(screen_lines[0].contains("~1"));
    assert!(!screen_lines[0].contains("filesystem.rs"));

    // Row 1: reserved secondary slot (blank when hidden).
    assert!(screen_lines[1].trim().is_empty());
    assert!(!screen_lines[1].contains("hunk"));

    // Row 2: File Header Banner in Diff Viewport
    assert!(screen_lines[2].contains("▼ File 1/1 · filesystem.rs"));
    assert!(screen_lines[2].contains("+1 ~1"));

    // Row 3: Hunk divider with context
    assert!(screen_lines[3].contains("Hunk 1/1 · 2 lines"));
    assert!(screen_lines[3].contains("more lines: ▼"));

    // Row 7..: Diff lines with line numbers and change markers
    assert!(screen_lines[7].contains("1025"));
    assert!(screen_lines[7].contains("let x = 1;"));
    assert!(screen_lines[8].contains("1026"));
    assert!(screen_lines[8].contains("~"));
    assert!(screen_lines[8].contains("comment — em dash updated"));
    assert!(screen_lines[9].contains("1027"));
    assert!(screen_lines[9].contains("+"));
    assert!(screen_lines[9].contains("let y = 2;"));

    // Bottom Rows: Divider + Keybindings Ribbon
    let divider_row = &screen_lines[height - 2];
    assert!(divider_row.contains("───"));
    let ribbon_row = &screen_lines[height - 1];
    assert!(ribbon_row.contains("[j/k]move"));
    assert!(ribbon_row.contains("[Enter]toggle"));
    assert!(ribbon_row.contains("[m]note"));
}

#[test]
fn test_initial_screen_multi_file_head_diff() {
    use ratatui::{Terminal, backend::TestBackend};

    let diff = r#"diff --git a/crates/diffuse-core/src/moves/mod.rs b/crates/diffuse-core/src/moves/mod.rs
--- a/crates/diffuse-core/src/moves/mod.rs
+++ b/crates/diffuse-core/src/moves/mod.rs
@@ -140,6 +140,8 @@ pub fn mark_moves(f: &mut DiffFile) {
             if matches!(r, Row::Del(_)) {
                 run.push((hi, ri));
             } else {
-                // flush the run
+                if !run.is_empty() {
+                    let moved_in_run: Vec<RowPos> = run;
+                }
diff --git a/crates/diffuse-core/src/moves/tests.rs b/crates/diffuse-core/src/moves/tests.rs
--- a/crates/diffuse-core/src/moves/tests.rs
+++ b/crates/diffuse-core/src/moves/tests.rs
@@ -1,3 +1,5 @@
+#[test]
+fn test_new() {}
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
    );

    let width = 100;
    let height = 20;
    let backend = TestBackend::new(width as u16, height as u16);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut screen_lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buffer[(x as u16, y as u16)].symbol());
        }
        screen_lines.push(line);
    }

    println!("\n┌{}┐", "─".repeat(width));
    for (y, l) in screen_lines.iter().enumerate() {
        println!("│{:<width$}│ (row {y:02})", l, width = width);
    }
    println!("└{}┘\n", "─".repeat(width));

    // Row 0: Header bar (repo + global counts, no file, no prefix)
    assert!(screen_lines[0].contains("diffuse"));
    assert!(!screen_lines[0].contains("crates/diffuse-core/src/moves/"));
    assert!(!screen_lines[0].contains("mod.rs"));

    // Row 1: secondary carries the common directory prefix.
    assert!(screen_lines[1].contains("crates/diffuse-core/src/moves/"));

    // Row 2: File header banner
    assert!(screen_lines[2].contains("▼ File 1/2 · mod.rs"));

    // Bottom Navigation Ribbon
    assert!(screen_lines[height - 1].contains("[j/k]move"));
    assert!(screen_lines[height - 1].contains("[n/p]file"));
}

// Long note callouts (including wide emoji) must be clamped to the viewport
// width like every other grid line.
#[test]
fn test_long_note_with_emoji_stays_within_viewport_width() {
    use diffuse_tui::view::visible_width;
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        path,
        diffuse_tui::render::DiffuseTheme::dark(),
    );
    state.note_draft = Some(
        "renders the identical golden (the extension's golden walk's rust leg \u{2014} \u{2705} passed); A/B on the real repo with a fairly long trailing tail to push past the edge".into(),
    );
    state.commit_note_draft().expect("note commit must succeed");
    let frame = state.render_frame(100, 30);
    assert!(frame.contains("\u{2705}"));
    for (i, line) in frame.lines().enumerate() {
        let vw = visible_width(line);
        assert!(vw <= 100, "frame line {i} width {vw} exceeds 100");
    }
}

// Deep scroll must seek the viewport by binary search and render the
// correct window: rows far past the top appear with intact content.
#[test]
fn test_deep_scroll_renders_correct_window() {
    use diffuse_tui::view::visible_width;
    let mut body = String::new();
    for i in 1..=400 {
        body.push_str(&format!("+line {i:04}\n"));
    }
    let diff = format!(
        "diff --git a/big.rs b/big.rs\n--- a/big.rs\n+++ b/big.rs\n@@ -0,0 +1,400 @@\n{body}"
    );
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
    );

    // Scroll deep and render: the window must show deep content, every
    // line within the frame width. The cursor travels with the scroll, as
    // in real scrolling, so scroll-follow keeps the window in place.
    state.row_idx = 300;
    state.scroll = 300;
    let frame = state.render_frame(100, 30);
    let plain = diffuse_tui::render::ansi::strip_ansi(&frame);
    assert!(
        plain.contains("line 0300"),
        "deep window must show deep content"
    );
    assert!(
        !plain.contains("line 0001"),
        "deep window must not show top content"
    );
    for (i, line) in frame.lines().enumerate() {
        let vw = visible_width(line);
        assert!(vw <= 100, "frame line {i} width {vw} exceeds 100");
    }

    // Scroll back to top: top content returns.
    state.row_idx = 0;
    state.scroll = 0;
    let frame = state.render_frame(100, 30);
    let plain = diffuse_tui::render::ansi::strip_ansi(&frame);
    assert!(plain.contains("line 0001"));
}

// Large-hunk scroll test: verifies repeated scrolled frames stay interactive
// and performant across 7,000 modified rows.
#[test]
fn test_perf_big_hunk_scrolled_frames() {
    let mut body = String::new();
    for i in 1..=7000 {
        body.push_str(&format!(
            "+content line {i:05} with some trailing text to widen rows\n"
        ));
    }
    body.push_str("+✅ passed — emoji line\n");
    let diff = format!(
        "diff --git a/big.rs b/big.rs\n--- a/big.rs\n+++ b/big.rs\n@@ -0,0 +1,7001 @@\n{body}"
    );
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
    );

    for scroll in [0, 1000, 3000, 6000] {
        state.scroll = scroll;
        let start = Instant::now();
        let frame = state.render_frame(200, 50);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(2),
            "scrolled frame at {scroll} took {elapsed:?}"
        );
        assert!(!frame.is_empty());
    }
}

fn load_scrollbar_grab_drag() -> Option<PipelineOutput> {
    let base =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/scrollbar-grab-drag");
    let patch_path = base.join("diff.patch");
    if !patch_path.is_file() {
        return None;
    }
    let patch = std::fs::read_to_string(&patch_path).ok()?;
    let meta_raw = std::fs::read_to_string(base.join("meta.json")).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&meta_raw).ok()?;
    let mut new_content = std::collections::HashMap::new();
    let mut old_content = std::collections::HashMap::new();
    for rec in meta["files"].as_array()? {
        let p = rec["path"].as_str()?.to_string();
        if let Some(n) = rec["new"].as_str() {
            let s = std::fs::read_to_string(base.join("files").join(n)).ok()?;
            new_content.insert(p.clone(), s);
        }
        if let Some(o) = rec["old"].as_str() {
            let s = std::fs::read_to_string(base.join("files").join(o)).ok()?;
            old_content.insert(p, s);
        }
    }
    Some(run_pipeline_full(
        &patch,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    ))
}

#[test]
fn test_reproduce_expand_file5_scrollbar_grab_drag() {
    let Some(out) = load_scrollbar_grab_drag() else {
        return;
    };
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        diffuse_tui::render::DiffuseTheme::dark(),
    );

    // Initial state: all files expanded, max_line_num is in 2000s, width 4
    assert_eq!(state.output.files.len(), 7);
    assert_eq!(state.line_num_width, 4);

    // Collapse File 4 (File 5/7 · app/tests.rs)
    state.toggle_file_collapse(4);
    assert_eq!(
        state.line_num_width, 3,
        "with file 4 collapsed, max line is < 1000"
    );

    let width: u16 = 120;
    let height: u16 = 40;

    // Position cursor on File 4's header
    let file4_flat = state.first_row_of_file(4);
    state.row_idx = file4_flat;
    state.vis_line = 0;

    // Render buffer with file 4 collapsed
    let buf_collapsed = state.render_to_buffer(width, height);

    let find_header_y = |buf: &ratatui::buffer::Buffer| -> Option<u16> {
        for y in 0..buf.area().height {
            let mut line = String::new();
            for x in 0..buf.area().width {
                line.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
            }
            if line.contains("File 5/7") || line.contains("app/tests.rs") {
                return Some(y);
            }
        }
        None
    };

    let y_before = find_header_y(&buf_collapsed).expect("file 4 header should be visible");

    // Now expand File 4
    state.toggle_file_collapse(4);
    assert_eq!(state.line_num_width, 4, "expanding file 4 restores width 4");

    let buf_expanded = state.render_to_buffer(width, height);
    let y_after = find_header_y(&buf_expanded).expect("file 4 header should still be visible");

    assert_eq!(
        y_after, y_before,
        "File header visual Y must be preserved across expand (expected {y_before}, got {y_after})"
    );

    // Collapsing File 4 again adapts width back to 3 and must also preserve visual
    // Y
    state.toggle_file_collapse(4);
    assert_eq!(
        state.line_num_width, 3,
        "re-collapsing file 4 restores width 3"
    );
    let buf_recollapsed = state.render_to_buffer(width, height);
    let y_recollapsed =
        find_header_y(&buf_recollapsed).expect("file 4 header should still be visible");
    assert_eq!(
        y_recollapsed, y_before,
        "File header visual Y must be preserved across re-collapse (expected {y_before}, got {y_recollapsed})"
    );
}
