use std::path::PathBuf;

use super::*;
use crate::core::{PipelineOptions, pipeline::run_pipeline_full, wire::PipelineOutput};

fn unique_notes_path() -> PathBuf {
    PathBuf::from("/nonexistent-diffuse-test-notes.json")
}

fn state_with_draft() -> AppState {
    let mut s = AppState::new(
        PipelineOutput {
            files: Vec::new(),
            branch: String::new(),
            untracked_skipped: Vec::new(),
        },
        "test".into(),
        unique_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    s.note_draft = Some("hello".into());
    s
}

fn state_with_rows() -> AppState {
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
    AppState::new(
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        ),
        "test_repo".into(),
        unique_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    )
}

#[test]
fn test_note_draft_enter_inserts_newline() {
    let mut s = state_with_draft();
    s.note_draft = Some("line 1".into());
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert_eq!(
        s.note_draft.as_deref(),
        Some("line 1\n"),
        "Enter must insert a newline in multiline draft"
    );
}

#[test]
fn test_note_draft_shift_and_alt_enter_insert_newline() {
    let mut s = state_with_draft();
    s.note_draft = Some("line 1".into());
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
    );
    assert_eq!(
        s.note_draft.as_deref(),
        Some("line 1\n"),
        "Shift+Enter must insert a newline in multiline draft"
    );

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
    );
    assert_eq!(
        s.note_draft.as_deref(),
        Some("line 1\n\n"),
        "Alt+Enter must insert a newline in multiline draft"
    );
}

#[test]
fn test_note_draft_ctrl_j_and_o_insert_newline_and_ctrl_n_navigates() {
    let mut s = state_with_draft();
    s.note_draft = Some("hello".into());

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
    );
    assert_eq!(s.note_draft.as_deref(), Some("hello\n"));

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)),
    );
    assert_eq!(s.note_draft.as_deref(), Some("hello\n\n"));

    // Ctrl-n is standard Readline move_down, not newline insertion
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)),
    );
    assert_eq!(s.note_draft.as_deref(), Some("hello\n\n"));
}

#[test]
fn test_note_draft_carriage_return_key_inserts_newline() {
    let mut s = state_with_draft();
    s.note_draft = Some("line 1".into());
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('\r'), KeyModifiers::NONE)),
    );
    assert_eq!(
        s.note_draft.as_deref(),
        Some("line 1\n"),
        "Char('\\r') must insert a newline in multiline draft"
    );
}

#[test]
fn test_note_draft_paste_normalizes_crlf_and_cr() {
    let mut s = state_with_draft();
    s.note_draft = Some(String::new());
    handle_event(
        &mut s,
        Event::Paste("line 1\r\nline 2\rline 3\nline 4".into()),
    );
    assert_eq!(
        s.note_draft.as_deref(),
        Some("line 1\nline 2\nline 3\nline 4"),
        "Pasted CRLF and CR must be normalized to LF"
    );
}

#[test]
fn test_help_modal_key_handling_dismiss_and_swallow() {
    let mut s = state_with_draft();
    s.show_help = true;

    // Pressing '@' while help is open must NOT open file finder
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('@'), KeyModifiers::NONE)),
    );
    assert!(s.show_help, "Help modal should stay open");
    assert!(s.file_finder.is_none(), "File finder must not open");

    // Pressing 'q' while help is open must close help, not quit app
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
    );
    assert!(!s.show_help, "Help modal should close on 'q'");
    assert!(!s.should_quit, "App must not quit on 'q' inside help modal");

    // Re-open help and test '?' dismissal
    s.show_help = true;
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)),
    );
    assert!(!s.show_help, "Help modal should close on '?'");

    // Re-open help and test 'Esc' dismissal
    s.show_help = true;
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    );
    assert!(!s.show_help, "Help modal should close on 'Esc'");
}

#[test]
fn test_note_draft_ctrl_enter_commits() {
    let mut s = state_with_draft();
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
    );
    assert!(s.note_draft.is_none());
}

#[test]
fn test_note_draft_ctrl_s_commits_and_ctrl_d_deletes_forward() {
    let mut s = state_with_draft();
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
    );
    assert!(s.note_draft.is_none());

    let mut s2 = state_with_draft();
    handle_event(
        &mut s2,
        Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
    );
    handle_event(
        &mut s2,
        Event::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
    );
    assert!(
        s2.note_draft.is_some(),
        "Ctrl-d must delete forward, not commit"
    );
    assert_eq!(s2.note_draft.as_deref(), Some("ello"));
}

#[test]
fn test_note_draft_esc_cancels() {
    let mut s = state_with_draft();
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    );
    assert!(s.note_draft.is_none());
}

#[test]
fn test_note_draft_readline_kill_yank_undo() {
    let mut s = state_with_draft();
    s.note_draft = Some("hello world".into());

    // Move to start: Ctrl-a
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
    );
    // Kill to end of line: Ctrl-k
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)),
    );
    assert_eq!(s.note_draft.as_deref(), Some(""));

    // Yank: Ctrl-y
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)),
    );
    assert_eq!(s.note_draft.as_deref(), Some("hello world"));

    // Undo: Ctrl-_
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('_'), KeyModifiers::CONTROL)),
    );
    assert_eq!(s.note_draft.as_deref(), Some(""));
}

#[test]
fn test_note_draft_readline_word_deletion() {
    let mut s = state_with_draft();
    s.note_draft = Some("foo bar baz".into());

    // Cursor is at end of text ("baz")
    // Kill word backward: Ctrl-w
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)),
    );
    assert_eq!(s.note_draft.as_deref(), Some("foo bar "));

    // Kill word backward again: Alt-Backspace
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT)),
    );
    assert_eq!(s.note_draft.as_deref(), Some("foo "));

    // Move home: Ctrl-a
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
    );
    // Kill word forward: Alt-d
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT)),
    );
    assert_eq!(s.note_draft.as_deref(), Some(" "));
}

#[test]
fn test_esc_cancels_anchor_before_quit() {
    let mut s = state_with_rows();
    s.move_down();
    s.start_or_confirm_selection();
    assert!(s.anchor.is_some());

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    );
    assert!(s.anchor.is_none(), "Esc must cancel the active anchor");
    assert!(!s.should_quit, "Esc must not quit while anchoring");

    // Hitting Esc with no active overlays/selection must NOT quit the app
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    );
    assert!(!s.should_quit, "Esc must never quit the application");
}

#[test]
fn test_v_starts_and_confirms_selection() {
    let mut s = state_with_rows();
    // Walk past the file header + the engine's padded above-context rows
    // (1022-1024) onto the real content (1025+).
    for _ in 0..3 {
        s.move_down();
    }

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)),
    );
    assert!(s.anchor.is_some(), "first v starts the anchor");

    s.move_down();
    s.move_down();
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::NONE)),
    );
    assert!(s.anchor.is_none(), "second v confirms and clears");
    let sel = s.confirmed_sel.expect("confirmed selection recorded");
    assert_eq!(sel.start, 1025);
    assert_eq!(sel.end, 1027);
}

#[test]
fn test_r_toggles_raw_diff() {
    let mut s = state_with_rows();
    assert!(!s.raw_diff);
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)),
    );
    assert!(s.raw_diff, "Shift+R enters raw diff view");
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE)),
    );
    assert!(!s.raw_diff, "R returns to the visual view");
}

#[test]
fn test_note_commit_on_range_selection() {
    let mut s = state_with_rows();
    for _ in 0..3 {
        s.move_down();
    }
    s.start_or_confirm_selection(); // anchor at 1025
    s.move_down();
    s.move_down();
    s.start_or_confirm_selection(); // confirm 1025..1027

    s.note_draft = Some("Range note test".into());
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
    );

    let empty = Vec::new();
    let notes = s
        .notes
        .get(&s.repo)
        .and_then(|m| m.get("filesystem.rs"))
        .unwrap_or(&empty);
    assert_eq!(notes.len(), 1, "note must be saved");
    assert_eq!(notes[0].start, 1025);
    assert_eq!(notes[0].end, 1027);
    assert_eq!(notes[0].text, "Range note test");
}

#[test]
fn test_note_delete_on_range_selection() {
    let mut s = state_with_rows();
    for _ in 0..3 {
        s.move_down();
    }
    s.start_or_confirm_selection(); // anchor at 1025
    s.move_down();
    s.move_down();
    s.start_or_confirm_selection(); // confirm 1025..1027

    s.note_draft = Some("To be deleted".into());
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
    );

    // Delete with range selection
    s.start_or_confirm_selection(); // anchor at 1027
    s.move_up();
    s.move_up();
    s.start_or_confirm_selection(); // confirm 1025..1027

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE)),
    );

    let empty = Vec::new();
    let notes = s
        .notes
        .get(&s.repo)
        .and_then(|m| m.get("filesystem.rs"))
        .unwrap_or(&empty);
    assert_eq!(notes.len(), 0, "note must be deleted by range selection");
}

#[test]
fn test_numeric_goto_buffer() {
    let mut s = state_with_rows();
    // Type '1', '0', '2', '6', 'g'
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)),
    );
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE)),
    );
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
    );
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE)),
    );
    assert_eq!(s.num_buffer, "1026");

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE)),
    );
    assert_eq!(s.num_buffer, "");
    let cur_row = s.current_flat_row().expect("cur row");
    assert_eq!(s.flat_row_coord(cur_row), Some(1026));

    // '0' with empty buffer resets col_offset to 0 without buffering
    s.col_offset = 12;
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('0'), KeyModifiers::NONE)),
    );
    assert_eq!(s.col_offset, 0);
    assert_eq!(s.num_buffer, "");

    // Backspace removes trailing digits from num_buffer
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::NONE)),
    );
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
    );
    assert_eq!(s.num_buffer, "42");
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
    );
    assert_eq!(s.num_buffer, "4");

    // Backspace on last digit empties buffer and triggers Dismiss
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
    );
    assert_eq!(s.num_buffer, "");

    // Esc clears the buffer
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('9'), KeyModifiers::NONE)),
    );
    assert_eq!(s.num_buffer, "9");
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    );
    assert_eq!(s.num_buffer, "");

    // Backspace with empty buffer acts as Dismiss (e.g. cancels active
    // selection)
    s.start_or_confirm_selection();
    assert!(s.anchor.is_some());
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
    );
    assert!(
        s.anchor.is_none(),
        "Backspace on empty buffer must dismiss selection"
    );
}

#[test]
fn test_tab_and_backtab_file_navigation() {
    let mut s = state_with_rows();
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
    );
    // Only one file in test repo, so it stays on file 0
    assert_eq!(s.current_file_idx(), 0);

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)),
    );
    assert_eq!(s.current_file_idx(), 0);
}

#[test]
fn test_caret_and_dollar_col_scroll() {
    let mut s = state_with_rows();
    s.col_offset = 20;
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('^'), KeyModifiers::NONE)),
    );
    assert_eq!(s.col_offset, 0, "^ must reset col_offset to 0");

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('$'), KeyModifiers::NONE)),
    );
    assert!(s.col_offset > 0, "$ must advance col_offset");
}

#[test]
fn test_file_finder_live_jump_and_cancel() {
    let diff = r#"diff --git a/apple.rs b/apple.rs
index 1111111..2222222 100644
--- a/apple.rs
+++ b/apple.rs
@@ -1,2 +1,3 @@
 fn apple() {}
+fn apple_extra() {}
diff --git a/banana.rs b/banana.rs
index 3333333..4444444 100644
--- a/banana.rs
+++ b/banana.rs
@@ -1,2 +1,3 @@
 fn banana() {}
+fn banana_extra() {}
"#;
    let new_content = [
        (
            "apple.rs".to_string(),
            "fn apple() {}\nfn apple_extra() {}\n".to_string(),
        ),
        (
            "banana.rs".to_string(),
            "fn banana() {}\nfn banana_extra() {}\n".to_string(),
        ),
    ]
    .into_iter()
    .collect();

    let mut s = AppState::new(
        run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        unique_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );

    assert_eq!(s.current_file_idx(), 0, "starts on file 0 (apple.rs)");
    let initial_row = s.row_idx;

    // Open file finder with '@'
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('@'), KeyModifiers::NONE)),
    );
    assert!(s.file_finder.is_some(), "file finder must be open");

    // Type 'b', 'a', 'n' -> matches banana.rs
    for c in ['b', 'a', 'n'] {
        handle_event(
            &mut s,
            Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
        );
    }

    let finder = s.file_finder.as_ref().unwrap();
    assert_eq!(finder.query, "ban");
    assert_eq!(finder.matches.len(), 1);
    assert_eq!(finder.matches[0].path, "banana.rs");
    assert_eq!(
        s.current_file_idx(),
        0,
        "does not auto-jump to banana.rs before confirmation"
    );
    assert_eq!(
        s.row_idx, initial_row,
        "stays on initial row while searching"
    );

    // Esc cancels and closes finder
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    );
    assert!(s.file_finder.is_none(), "file finder is closed");
    assert_eq!(s.current_file_idx(), 0, "restored to file 0 (apple.rs)");
    assert_eq!(s.row_idx, initial_row, "restored to initial row");

    // Open again, search, and press Enter to confirm
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('@'), KeyModifiers::NONE)),
    );
    for c in ['b', 'a', 'n'] {
        handle_event(
            &mut s,
            Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
        );
    }
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert!(s.file_finder.is_none(), "file finder confirmed and closed");
    assert_eq!(
        s.current_file_idx(),
        1,
        "stays on banana.rs after confirmation"
    );
    assert!(
        s.flat[s.row_idx].is_file_header(),
        "stays on the file banner row after confirmation"
    );
}

#[test]
fn test_enter_key_toggles_file_collapse() {
    let mut s = state_with_rows();
    s.jump_to_row(0, false);
    assert!(!s.collapsed_files.contains(&0), "starts expanded");
    let initial_len = s.flat.len();
    assert!(initial_len > 1);

    // Press Enter to collapse current file
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert!(s.collapsed_files.contains(&0), "now collapsed");
    assert_eq!(s.flat.len(), 1, "collapsed file has 1 row (header)");
    assert_eq!(s.row_idx, 0, "cursor on header row");

    // Press Enter again to expand
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    );
    assert!(!s.collapsed_files.contains(&0), "expanded again");
    assert_eq!(
        s.flat.len(),
        initial_len,
        "restored original flat row count"
    );
}

#[test]
fn test_mouse_scrollbar_drag() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let mut s = state_with_rows();
    // Render once to populate viewport geometry
    s.render_frame(80, 10);
    assert!(s.viewport_height > 0);

    // Click on scrollbar column (col 79, row 5)
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 79,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(s.is_dragging_scrollbar, "dragging flag should be active");

    // Drag down to row 8
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 79,
            row: 8,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(s.is_dragging_scrollbar);

    // Release mouse
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 79,
            row: 8,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(!s.is_dragging_scrollbar, "dragging flag should be cleared");
}

#[test]
fn test_mouse_drag_selection_and_click_reset_and_esc_dismiss() {
    let mut s = state_with_rows();
    s.render_frame(80, 24);
    let header_offset = s.last_viewport_start;

    // 1. Mouse Down on row 3 (relative to header)
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: (header_offset + 3) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(s.anchor.is_none(), "single click must not anchor");
    assert!(s.confirmed_sel.is_none());

    // 2. Drag to row 5
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 10,
            row: (header_offset + 5) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(
        s.anchor.is_some(),
        "dragging starts anchor at origin row and moves cursor"
    );

    // 3. Mouse Up (release button): should exit active dragging and confirm
    //    selection band
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 10,
            row: (header_offset + 5) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(
        s.anchor.is_none(),
        "mouse up must exit active selection anchor mode"
    );
    assert!(
        s.confirmed_sel.is_some(),
        "mouse up must preserve the selection as a confirmed selection band"
    );

    // 4. Pressing Escape must dismiss confirmed selection and NOT quit the app
    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
    );
    assert!(
        s.confirmed_sel.is_none(),
        "Esc must clear confirmed selection"
    );
    assert!(
        !s.should_quit,
        "Esc must not quit when dismissing selection"
    );

    // 5. Drag again to create a selection
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: (header_offset + 3) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 10,
            row: (header_offset + 5) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 10,
            row: (header_offset + 5) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(s.confirmed_sel.is_some());

    // 6. Clicking anywhere on screen must clear the selection
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: (header_offset + 2) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    assert!(
        s.confirmed_sel.is_none(),
        "clicking anywhere must reset and remove selection"
    );
    assert!(s.anchor.is_none());
}

#[test]
fn test_shift_click_and_shift_drag_selection_modification() {
    let mut s = state_with_rows();
    s.render_frame(80, 24);
    let header_offset = s.last_viewport_start;

    // 1. Initial drag select rows 4 to 5 (lines 1024 to 1025)
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: (header_offset + 4) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 10,
            row: (header_offset + 5) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 10,
            row: (header_offset + 5) as u16,
            modifiers: KeyModifiers::NONE,
        }),
    );
    let initial_sel = s.confirmed_sel.expect("confirmed selection exists");
    assert_eq!(initial_sel.start, 1024);
    assert_eq!(initial_sel.end, 1025);
    assert!(s.anchor.is_none(), "not in active drag mode");

    // 2. Shift-Click on row 7 (line 1027): modifies confirmed_sel in place without
    //    entering active drag
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: (header_offset + 7) as u16,
            modifiers: KeyModifiers::SHIFT,
        }),
    );
    assert!(
        s.anchor.is_none(),
        "Shift-click must not enter active selection mode"
    );
    let updated_sel = s.confirmed_sel.expect("confirmed selection modified");
    assert_eq!(updated_sel.start, 1024);
    assert_eq!(
        updated_sel.end, 1027,
        "Shift-click extended selection end to row 7 (1027)"
    );

    // 3. Shift-Drag: re-enters active selection mode while dragging
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: (header_offset + 7) as u16,
            modifiers: KeyModifiers::SHIFT,
        }),
    );
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 10,
            row: (header_offset + 7) as u16,
            modifiers: KeyModifiers::SHIFT,
        }),
    );
    assert!(
        s.anchor.is_some(),
        "Shift-drag re-enters active selection mode"
    );

    // 4. Release mouse: confirms extended selection and exits active mode
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 10,
            row: (header_offset + 7) as u16,
            modifiers: KeyModifiers::SHIFT,
        }),
    );
    assert!(s.anchor.is_none(), "mouse up clears active anchor");
    let drag_sel = s
        .confirmed_sel
        .expect("confirmed selection updated by drag");
    assert_eq!(drag_sel.start, 1024);
    assert_eq!(drag_sel.end, 1027);
}

#[test]
fn test_t_toggles_staged() {
    let mut s = state_with_rows();
    // 1. When commit range is set, 't' fails and reports status message
    s.commit_range = Some("HEAD~1..HEAD".into());
    assert!(s.status_message.is_none());

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE)),
    );
    assert!(
        s.status_message.is_some(),
        "Pressing 't' on commit range must set error status message"
    );
}

#[test]
fn test_shift_t_toggles_theme() {
    let mut s = state_with_rows();
    assert_eq!(s.theme, crate::render::DiffuseTheme::DEFAULT);

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT)),
    );
    assert_eq!(s.theme, crate::render::DiffuseTheme::LIGHT);

    handle_event(
        &mut s,
        Event::Key(KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT)),
    );
    assert_eq!(s.theme, crate::render::DiffuseTheme::DEFAULT);
}

#[test]
fn test_mouse_wheel_scroll_moves_viewport() {
    let mut lines = String::new();
    let mut diff_lines = String::new();
    for i in 1..=50 {
        lines.push_str(&format!("line {i}\n"));
        diff_lines.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,50 @@\n{diff_lines}"
    );
    let new_content = [("a.rs".to_string(), lines)].into_iter().collect();
    let mut s = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test".into(),
        unique_notes_path(),
        crate::render::DiffuseTheme::DEFAULT,
    );
    s.viewport_height = 10;
    s.last_content_width = 80;
    s.render_diff_viewport_lines(80, 10, 10);
    assert_eq!(s.scroll, 0);

    // ScrollDown should scroll viewport down by 3 lines
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
    );
    s.render_diff_viewport_lines(80, 10, 10);
    assert_eq!(s.scroll, 3);

    // ScrollUp should scroll viewport back up by 3 lines
    handle_event(
        &mut s,
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }),
    );
    s.render_diff_viewport_lines(80, 10, 10);
    assert_eq!(s.scroll, 0);
}
