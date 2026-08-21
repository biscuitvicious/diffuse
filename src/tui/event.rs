//! TUI Event Handling & Input Processing.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::app::AppState;

/// Handle incoming crossterm terminal events.
pub fn handle_event(state: &mut AppState, event: Event) {
    match event {
        Event::Key(key) => handle_key_event(state, key),
        Event::Mouse(mouse) => handle_mouse_event(state, mouse),
        _ => {}
    }
}

fn handle_key_event(state: &mut AppState, key: KeyEvent) {
    // File Finder Dialog Mode
    if state.file_finder.is_some() {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                state.cancel_file_finder();
            }
            (_, KeyCode::Enter) => {
                state.confirm_file_finder();
            }
            (_, KeyCode::Up)
            | (KeyModifiers::CONTROL, KeyCode::Char('p'))
            | (KeyModifiers::SHIFT, KeyCode::BackTab)
            | (KeyModifiers::NONE, KeyCode::BackTab) => {
                if let Some(finder) = &mut state.file_finder {
                    finder.move_up();
                }
                state.track_finder_selection();
            }
            (_, KeyCode::Down)
            | (KeyModifiers::CONTROL, KeyCode::Char('n'))
            | (KeyModifiers::NONE, KeyCode::Tab) => {
                if let Some(finder) = &mut state.file_finder {
                    finder.move_down();
                }
                state.track_finder_selection();
            }
            (_, KeyCode::Backspace) => {
                let files = state.output.files.clone();
                if let Some(finder) = &mut state.file_finder {
                    finder.query.pop();
                    finder.update_matches(&files);
                }
                state.track_finder_selection();
            }
            (KeyModifiers::NONE, KeyCode::Char(c)) | (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let files = state.output.files.clone();
                if let Some(finder) = &mut state.file_finder {
                    finder.query.push(c);
                    finder.update_matches(&files);
                }
                state.track_finder_selection();
            }
            _ => {}
        }
        return;
    }

    // Note Draft Editing Mode draft typing
    if let Some(ref mut draft) = state.note_draft {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter {
            state.commit_note_draft();
            return;
        }

        match key.code {
            KeyCode::Esc => {
                state.note_draft = None;
            }
            KeyCode::Enter => {
                // One-line draft: Enter commits. A literal '\n' would break the
                // single-line status rendering.
                state.commit_note_draft();
            }
            KeyCode::Backspace => {
                draft.pop();
            }
            KeyCode::Char(c) => {
                draft.push(c);
            }
            _ => {}
        }
        return;
    }

    // Normal Reviewer Navigation & Commands
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            state.should_quit = true;
        }
        (KeyModifiers::NONE, KeyCode::Esc) => {
            if state.show_help {
                state.show_help = false;
            } else if state.anchor.is_some() {
                state.anchor = None;
            } else {
                state.should_quit = true;
            }
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            state.toggle_current_file_collapse();
        }
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            state.move_down();
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            state.move_up();
        }
        (KeyModifiers::SHIFT, KeyCode::Char('J')) | (KeyModifiers::NONE, KeyCode::Char('}')) => {
            state.next_hunk();
        }
        (KeyModifiers::SHIFT, KeyCode::Char('K')) | (KeyModifiers::NONE, KeyCode::Char('{')) => {
            state.prev_hunk();
        }
        (KeyModifiers::NONE, KeyCode::PageDown)
        | (KeyModifiers::CONTROL, KeyCode::Char('d'))
        | (KeyModifiers::CONTROL, KeyCode::Char('f'))
        | (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            state.page_down();
        }
        (KeyModifiers::NONE, KeyCode::PageUp)
        | (KeyModifiers::CONTROL, KeyCode::Char('u'))
        | (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
            state.page_up();
        }
        (KeyModifiers::NONE, KeyCode::Char(c @ '0'..='9')) => {
            state.num_buffer.push(c);
        }
        (KeyModifiers::NONE, KeyCode::Home) | (KeyModifiers::NONE, KeyCode::Char('g')) => {
            if !state.num_buffer.is_empty() {
                let line = state.num_buffer.parse::<u32>().ok();
                state.num_buffer.clear();
                if let Some(l) = line {
                    state.goto_line(l);
                }
            } else {
                state.move_top();
            }
        }
        (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
            if !state.num_buffer.is_empty() {
                let line = state.num_buffer.parse::<u32>().ok();
                state.num_buffer.clear();
                if let Some(l) = line {
                    state.goto_line(l);
                }
            } else {
                state.move_bottom();
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('n'))
        | (KeyModifiers::NONE, KeyCode::Char(']'))
        | (KeyModifiers::NONE, KeyCode::Tab) => {
            state.next_file();
        }
        (KeyModifiers::NONE, KeyCode::Char('p'))
        | (KeyModifiers::NONE, KeyCode::Char('['))
        | (KeyModifiers::SHIFT, KeyCode::BackTab)
        | (KeyModifiers::NONE, KeyCode::BackTab) => {
            state.prev_file();
        }
        (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => {
            state.col_offset = state.col_offset.saturating_sub(4);
        }
        (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => {
            state.col_offset += 4;
        }
        (KeyModifiers::NONE, KeyCode::Char('^')) => {
            state.col_offset = 0;
        }
        (KeyModifiers::NONE, KeyCode::Char('$')) => {
            state.col_offset += 16;
        }
        (KeyModifiers::NONE, KeyCode::Char('w')) => {
            state.word_diff = !state.word_diff;
        }
        (KeyModifiers::NONE, KeyCode::Char('y')) => {
            state.syntax_on = !state.syntax_on;
        }
        (KeyModifiers::NONE, KeyCode::Char('R')) | (KeyModifiers::SHIFT, KeyCode::Char('R')) => {
            state.toggle_raw_diff();
        }
        (KeyModifiers::NONE, KeyCode::Char('?')) => {
            state.show_help = !state.show_help;
        }
        (KeyModifiers::NONE, KeyCode::Char('@')) | (KeyModifiers::SHIFT, KeyCode::Char('@')) => {
            state.open_file_finder();
        }
        (KeyModifiers::NONE, KeyCode::Char('m')) => {
            state.note_draft = Some(String::new());
        }
        (KeyModifiers::NONE, KeyCode::Char('v')) | (KeyModifiers::NONE, KeyCode::Char('V')) => {
            state.start_or_confirm_selection();
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::Char('D')) => {
            state.delete_notes();
        }
        (KeyModifiers::NONE, KeyCode::Char('o')) | (KeyModifiers::SHIFT, KeyCode::Char('O')) => {
            if let Some(file) = state.current_file()
                && let Some(row) = state.current_flat_row()
                && let Some(c) = AppState::row_coord(&row.row)
            {
                let range = state.compute_selection().and_then(|sel| {
                    if sel.side_is_new {
                        Some((sel.start, sel.end))
                    } else {
                        None
                    }
                });
                state.pending_editor = Some(super::editor::EditorRequest {
                    file: file.path.clone(),
                    line: c,
                    range,
                });
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('s')) => {
            let repo_root = state.repo_root.clone();
            let git_args = state.git_args.clone();
            match super::actions::stage_action(state, &repo_root, &git_args, false) {
                Ok(()) => {
                    state.status_message = Some(("staged hunk".into(), true));
                    let _ = super::actions::reload_diff(state, &repo_root, &git_args);
                }
                Err(e) => state.status_message = Some((e, false)),
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Char('S')) => {
            let repo_root = state.repo_root.clone();
            let git_args = state.git_args.clone();
            match super::actions::stage_action(state, &repo_root, &git_args, true) {
                Ok(()) => {
                    state.status_message = Some(("staged file".into(), true));
                    let _ = super::actions::reload_diff(state, &repo_root, &git_args);
                }
                Err(e) => state.status_message = Some((e, false)),
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('u')) => {
            let repo_root = state.repo_root.clone();
            let git_args = state.git_args.clone();
            match super::actions::unstage_action(state, &repo_root, &git_args, false) {
                Ok(()) => {
                    state.status_message = Some(("unstaged hunk".into(), true));
                    let _ = super::actions::reload_diff(state, &repo_root, &git_args);
                }
                Err(e) => state.status_message = Some((e, false)),
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Char('U')) => {
            let repo_root = state.repo_root.clone();
            let git_args = state.git_args.clone();
            match super::actions::unstage_action(state, &repo_root, &git_args, true) {
                Ok(()) => {
                    state.status_message = Some(("unstaged file".into(), true));
                    let _ = super::actions::reload_diff(state, &repo_root, &git_args);
                }
                Err(e) => state.status_message = Some((e, false)),
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('x')) => {
            let repo_root = state.repo_root.clone();
            let git_args = state.git_args.clone();
            match super::actions::discard_action(state, &repo_root, &git_args, false) {
                Ok(stash_ref) => {
                    let msg = if let Some(sr) = stash_ref {
                        format!("discarded hunk (stash: {sr})")
                    } else {
                        "discarded hunk".into()
                    };
                    state.status_message = Some((msg, true));
                    let _ = super::actions::reload_diff(state, &repo_root, &git_args);
                }
                Err(e) => state.status_message = Some((e, false)),
            }
        }
        (KeyModifiers::SHIFT, KeyCode::Char('X')) => {
            let repo_root = state.repo_root.clone();
            let git_args = state.git_args.clone();
            match super::actions::discard_action(state, &repo_root, &git_args, true) {
                Ok(stash_ref) => {
                    let msg = if let Some(sr) = stash_ref {
                        format!("discarded file (stash: {sr})")
                    } else {
                        "discarded file".into()
                    };
                    state.status_message = Some((msg, true));
                    let _ = super::actions::reload_diff(state, &repo_root, &git_args);
                }
                Err(e) => state.status_message = Some((e, false)),
            }
        }
        _ => {}
    }
}

fn handle_mouse_event(state: &mut AppState, mouse: MouseEvent) {
    use crossterm::event::MouseButton;
    let header_offset = state.last_viewport_start;
    let vh = state.viewport_height;
    let col = mouse.column as usize;
    let row = mouse.row as usize;
    let is_on_scrollbar = col >= state.last_content_width;

    match mouse.kind {
        MouseEventKind::ScrollDown => {
            state.move_down();
            state.move_down();
            state.move_down();
        }
        MouseEventKind::ScrollUp => {
            state.move_up();
            state.move_up();
            state.move_up();
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if is_on_scrollbar && row >= header_offset && row < header_offset + vh {
                state.is_dragging_scrollbar = true;
                let row_in_vh = row - header_offset;
                let (_starts, _counts, _seg, total_vis) =
                    state.visual_geometry(state.last_content_width);
                let max_scroll = total_vis.saturating_sub(vh);
                if max_scroll > 0 && vh > 1 {
                    let target_vis =
                        ((row_in_vh as f64 / (vh - 1) as f64) * max_scroll as f64).round() as usize;
                    state.scroll_to_visual(target_vis);
                }
            } else if row >= header_offset && row < header_offset + vh {
                state.is_dragging_scrollbar = false;
                let target_vis = state.scroll + (row - header_offset);
                let (starts, counts, _seg, _total_vis) =
                    state.visual_geometry(state.last_content_width);
                for (i, &s) in starts.iter().enumerate() {
                    if target_vis >= s && target_vis < s + counts[i] {
                        state.row_idx = i;
                        state.vis_line = 0;
                        break;
                    }
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if state.is_dragging_scrollbar {
                let clamped_row = row.clamp(header_offset, header_offset + vh.saturating_sub(1));
                let row_in_vh = clamped_row - header_offset;
                let (_starts, _counts, _seg, total_vis) =
                    state.visual_geometry(state.last_content_width);
                let max_scroll = total_vis.saturating_sub(vh);
                if max_scroll > 0 && vh > 1 {
                    let target_vis =
                        ((row_in_vh as f64 / (vh - 1) as f64) * max_scroll as f64).round() as usize;
                    state.scroll_to_visual(target_vis);
                }
            } else if row >= header_offset && row < header_offset + vh {
                if state.anchor.is_none() {
                    state.anchor = Some(state.row_idx);
                }
                let target_vis = state.scroll + (row - header_offset);
                let (starts, counts, _seg, _total_vis) =
                    state.visual_geometry(state.last_content_width);
                for (i, &s) in starts.iter().enumerate() {
                    if target_vis >= s && target_vis < s + counts[i] {
                        state.row_idx = i;
                        state.vis_line = 0;
                        break;
                    }
                }
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            state.is_dragging_scrollbar = false;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::PipelineOutput;

    fn unique_notes_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "diffuse-test-notes-{}-{}.json",
            std::process::id(),
            crate::notes::now_millis()
        ))
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
        let contents = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n"
                .to_string(),
        )]
        .into_iter()
        .collect();
        let old_contents = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        AppState::new(
            crate::pipeline::run_pipeline_full(diff, &contents, Some(&old_contents), None, None),
            "test_repo".into(),
            unique_notes_path(),
        )
    }

    #[test]
    fn test_note_draft_enter_commits_without_newline() {
        let mut s = state_with_draft();
        handle_event(
            &mut s,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert!(
            s.note_draft.is_none(),
            "Enter must commit the draft, got {:?}",
            s.note_draft
        );
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
    fn test_note_draft_esc_cancels() {
        let mut s = state_with_draft();
        handle_event(
            &mut s,
            Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        );
        assert!(s.note_draft.is_none());
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
    }

    #[test]
    fn test_v_starts_and_confirms_selection() {
        let mut s = state_with_rows();
        // Walk past the file header + the engine's padded above-context rows
        // (1020-1024) onto the real content (1025+).
        for _ in 0..5 {
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
        for _ in 0..5 {
            s.move_down();
        }
        s.start_or_confirm_selection(); // anchor at 1025
        s.move_down();
        s.move_down();
        s.start_or_confirm_selection(); // confirm 1025..1027

        s.note_draft = Some("Range note test".into());
        handle_event(
            &mut s,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
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
        for _ in 0..5 {
            s.move_down();
        }
        s.start_or_confirm_selection(); // anchor at 1025
        s.move_down();
        s.move_down();
        s.start_or_confirm_selection(); // confirm 1025..1027

        s.note_draft = Some("To be deleted".into());
        handle_event(
            &mut s,
            Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
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
        assert_eq!(AppState::row_coord(&cur_row.row), Some(1026));
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
        let contents = [
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
            crate::pipeline::run_pipeline_full(diff, &contents, None, None, None),
            "test_repo".into(),
            unique_notes_path(),
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
            1,
            "immediately jumped to banana.rs live"
        );

        // Esc cancels and restores initial position
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
    }

    #[test]
    fn test_enter_key_toggles_file_collapse() {
        let mut s = state_with_rows();
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
}
