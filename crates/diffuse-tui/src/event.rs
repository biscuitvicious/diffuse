//! TUI Event Handling & Input Processing.

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use super::{app::AppState, command::TuiCommand};

/// Handle incoming crossterm terminal events.
pub fn handle_event(state: &mut AppState, event: Event) {
    match event {
        Event::Key(key) => handle_key_event(state, key),
        Event::Mouse(mouse) => handle_mouse_event(state, mouse),
        Event::Paste(text) => handle_paste_event(state, &text),
        _ => {}
    }
}

fn handle_paste_event(state: &mut AppState, text: &str) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if state.note_editor.is_none()
        && let Some(ref draft) = state.note_draft
    {
        let mut ed = crate::editor::Editor::new(draft)
            .with_highlighter(crate::editor::markdown_highlighter(state.theme));
        let r = ed.lines().len().saturating_sub(1);
        let c = ed.lines()[r].chars().count();
        ed.set_cursor(r, c);
        state.note_editor = Some(ed);
    }
    if let Some(ref mut editor) = state.note_editor {
        editor.insert_str(&normalized);
        state.note_draft = Some(editor.text());
    } else if let Some(ref mut finder) = state.file_finder {
        finder.query.push_str(&normalized);
        let files = state.output.files.clone();
        finder.update_matches(&files);
    }
}

#[tracing::instrument(level = "trace", skip(state), fields(code = ?key.code, modifiers = ?key.modifiers))]
fn handle_key_event(state: &mut AppState, key: KeyEvent) {
    // Help Modal Mode (swallows all background keys, closes on
    // Esc/?/q/Enter/Space)
    if state.show_help {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc)
            | (_, KeyCode::Char('?'))
            | (KeyModifiers::NONE, KeyCode::Char('q'))
            | (KeyModifiers::NONE, KeyCode::Enter)
            | (KeyModifiers::NONE, KeyCode::Char(' ')) => {
                state.show_help = false;
            }
            _ => {}
        }
        return;
    }

    // File Finder Dialog Mode
    if state.file_finder.is_some() {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                state.execute(TuiCommand::CloseFileFinder);
            }
            (_, KeyCode::Enter) => {
                state.execute(TuiCommand::ConfirmFileFinder);
            }
            (_, KeyCode::Up)
            | (KeyModifiers::CONTROL, KeyCode::Char('p'))
            | (KeyModifiers::SHIFT, KeyCode::BackTab)
            | (KeyModifiers::NONE, KeyCode::BackTab) => {
                if let Some(finder) = &mut state.file_finder {
                    finder.move_up();
                }
            }
            (_, KeyCode::Down)
            | (KeyModifiers::CONTROL, KeyCode::Char('n'))
            | (KeyModifiers::NONE, KeyCode::Tab) => {
                if let Some(finder) = &mut state.file_finder {
                    finder.move_down();
                }
            }
            (_, KeyCode::Backspace) => {
                let files = state.output.files.clone();
                if let Some(finder) = &mut state.file_finder {
                    finder.query.pop();
                    finder.update_matches(&files);
                }
            }
            (KeyModifiers::NONE, KeyCode::Char(c)) | (KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let files = state.output.files.clone();
                if let Some(finder) = &mut state.file_finder {
                    finder.query.push(c);
                    finder.update_matches(&files);
                }
            }
            _ => {}
        }
        return;
    }

    // Note Draft Editing Mode
    if state.note_editor.is_none()
        && let Some(ref draft) = state.note_draft
    {
        let mut ed = crate::editor::Editor::new(draft)
            .with_highlighter(crate::editor::markdown_highlighter(state.theme));
        let r = ed.lines().len().saturating_sub(1);
        let c = ed.lines()[r].chars().count();
        ed.set_cursor(r, c);
        state.note_editor = Some(ed);
    }

    if let Some(ref mut editor) = state.note_editor {
        match editor.handle_key(key) {
            crate::editor::EditorAction::Submit => {
                state.execute(TuiCommand::CommitNoteDraft);
            }
            crate::editor::EditorAction::Cancel => {
                state.execute(TuiCommand::CancelNoteDraft);
            }
            crate::editor::EditorAction::Modified
            | crate::editor::EditorAction::CursorMoved
            | crate::editor::EditorAction::None => {
                state.note_draft = Some(editor.text());
            }
        }
        return;
    }

    // Normal Reviewer Navigation & Commands
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            state.execute(TuiCommand::Quit);
        }
        (KeyModifiers::NONE, KeyCode::Esc) => {
            state.execute(TuiCommand::Dismiss);
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            state.execute(TuiCommand::ToggleCollapseFile(None));
        }
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            state.execute(TuiCommand::MoveDown(1));
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            state.execute(TuiCommand::MoveUp(1));
        }
        (KeyModifiers::SHIFT, KeyCode::Char('J')) | (KeyModifiers::NONE, KeyCode::Char('}')) => {
            state.execute(TuiCommand::JumpHunkNext);
        }
        (KeyModifiers::SHIFT, KeyCode::Char('K')) | (KeyModifiers::NONE, KeyCode::Char('{')) => {
            state.execute(TuiCommand::JumpHunkPrev);
        }
        (KeyModifiers::NONE, KeyCode::PageDown)
        | (KeyModifiers::CONTROL, KeyCode::Char('d'))
        | (KeyModifiers::CONTROL, KeyCode::Char('f'))
        | (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            state.execute(TuiCommand::PageDown);
        }
        (KeyModifiers::NONE, KeyCode::PageUp)
        | (KeyModifiers::CONTROL, KeyCode::Char('u'))
        | (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
            state.execute(TuiCommand::PageUp);
        }
        (KeyModifiers::NONE, KeyCode::Char(c @ '0'..='9')) => {
            state.num_buffer.push(c);
        }
        (KeyModifiers::NONE, KeyCode::Home) | (KeyModifiers::NONE, KeyCode::Char('g')) => {
            if !state.num_buffer.is_empty() {
                let line = state.num_buffer.parse::<u32>().ok();
                state.num_buffer.clear();
                if let Some(l) = line {
                    state.execute(TuiCommand::JumpToLine(l));
                }
            } else {
                state.execute(TuiCommand::MoveToTop);
            }
        }
        (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
            if !state.num_buffer.is_empty() {
                let line = state.num_buffer.parse::<u32>().ok();
                state.num_buffer.clear();
                if let Some(l) = line {
                    state.execute(TuiCommand::JumpToLine(l));
                }
            } else {
                state.execute(TuiCommand::MoveToBottom);
            }
        }
        (KeyModifiers::NONE, KeyCode::Char('n'))
        | (KeyModifiers::NONE, KeyCode::Char(']'))
        | (KeyModifiers::NONE, KeyCode::Tab) => {
            state.execute(TuiCommand::JumpFileNext);
        }
        (KeyModifiers::NONE, KeyCode::Char('p'))
        | (KeyModifiers::NONE, KeyCode::Char('['))
        | (KeyModifiers::SHIFT, KeyCode::BackTab)
        | (KeyModifiers::NONE, KeyCode::BackTab) => {
            state.execute(TuiCommand::JumpFilePrev);
        }
        (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => {
            state.execute(TuiCommand::ScrollLeft(4));
        }
        (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => {
            state.execute(TuiCommand::ScrollRight(4));
        }
        (KeyModifiers::NONE, KeyCode::Char('^')) => {
            state.col_offset = 0;
        }
        (KeyModifiers::NONE, KeyCode::Char('$')) => {
            state.execute(TuiCommand::ScrollRight(16));
        }
        (KeyModifiers::NONE, KeyCode::Char('w')) => {
            state.execute(TuiCommand::ToggleWordDiff);
        }
        (KeyModifiers::NONE, KeyCode::Char('a'))
        | (KeyModifiers::SHIFT, KeyCode::Char('A'))
        | (KeyModifiers::NONE, KeyCode::Char('+'))
        | (KeyModifiers::SHIFT, KeyCode::Char('E')) => {
            state.execute(TuiCommand::ExpandAllHidden);
        }
        (KeyModifiers::NONE, KeyCode::Char('c')) | (KeyModifiers::NONE, KeyCode::Char('z')) => {
            state.execute(TuiCommand::ResetCurrentHunkContext);
        }
        (KeyModifiers::SHIFT, KeyCode::Char('C')) | (KeyModifiers::SHIFT, KeyCode::Char('Z')) => {
            state.execute(TuiCommand::ResetAllContext);
        }
        (KeyModifiers::NONE, KeyCode::Char('y')) => {
            state.execute(TuiCommand::ToggleSyntax);
        }
        (KeyModifiers::NONE, KeyCode::Char('R')) | (KeyModifiers::SHIFT, KeyCode::Char('R')) => {
            state.execute(TuiCommand::ToggleRawDiff);
        }
        (KeyModifiers::NONE, KeyCode::Char('?')) => {
            state.execute(TuiCommand::ToggleHelp);
        }
        (KeyModifiers::NONE, KeyCode::Char('@')) | (KeyModifiers::SHIFT, KeyCode::Char('@')) => {
            state.execute(TuiCommand::OpenFileFinder);
        }
        (KeyModifiers::NONE, KeyCode::Char('m')) => {
            state.execute(TuiCommand::StartNoteDraft);
        }
        (KeyModifiers::NONE, KeyCode::Char('v')) | (KeyModifiers::NONE, KeyCode::Char('V')) => {
            state.execute(TuiCommand::StartSelection);
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::Char('D')) => {
            state.execute(TuiCommand::DeleteNote);
        }
        (KeyModifiers::NONE, KeyCode::Char('o')) | (KeyModifiers::SHIFT, KeyCode::Char('O')) => {
            state.execute(TuiCommand::OpenEditor);
        }
        (KeyModifiers::NONE, KeyCode::Char('s')) => {
            state.execute(TuiCommand::Stage);
        }
        (KeyModifiers::NONE, KeyCode::Char('t')) => {
            state.execute(TuiCommand::ToggleStaged);
        }
        (KeyModifiers::SHIFT, KeyCode::Char('T'))
        | (KeyModifiers::CONTROL, KeyCode::Char('t'))
        | (KeyModifiers::NONE, KeyCode::F(2)) => {
            state.execute(TuiCommand::ToggleTheme);
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
            state.execute(TuiCommand::Unstage);
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
            state.execute(TuiCommand::Discard);
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
    let col = mouse.column as usize;
    let row = mouse.row as usize;
    let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);

    match mouse.kind {
        MouseEventKind::ScrollDown => {
            state.execute(TuiCommand::MouseScrollDown(3));
        }
        MouseEventKind::ScrollUp => {
            state.execute(TuiCommand::MouseScrollUp(3));
        }
        MouseEventKind::Down(MouseButton::Left) => {
            state.execute(TuiCommand::MouseClick { row, col, shift });
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            state.execute(TuiCommand::MouseDrag { row, col, shift });
        }
        MouseEventKind::Up(MouseButton::Left) => {
            state.execute(TuiCommand::EndMouseDrag);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
