//! TUI Event Handling & Input Processing.

use super::app::AppState;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

/// Handle incoming crossterm terminal events.
pub fn handle_event(state: &mut AppState, event: Event) {
    match event {
        Event::Key(key) => handle_key_event(state, key),
        Event::Mouse(mouse) => handle_mouse_event(state, mouse),
        _ => {}
    }
}

fn handle_key_event(state: &mut AppState, key: KeyEvent) {
    // Handling note draft typing
    if let Some(ref mut draft) = state.note_draft {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Enter {
            let text = draft.trim().to_string();
            state.note_draft = None;
            if !text.is_empty() {
                // Note saving action placeholder
            }
            return;
        }

        match key.code {
            KeyCode::Esc => {
                state.note_draft = None;
            }
            KeyCode::Enter => {
                // One-line draft: Enter commits. A literal '\n' would break the
                // single-line status rendering.
                let text = draft.trim().to_string();
                state.note_draft = None;
                if !text.is_empty() {
                    // Note saving action placeholder
                }
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
            state.show_help = false;
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
        (KeyModifiers::NONE, KeyCode::Home) | (KeyModifiers::NONE, KeyCode::Char('g')) => {
            state.move_top();
        }
        (KeyModifiers::NONE, KeyCode::End) | (KeyModifiers::SHIFT, KeyCode::Char('G')) => {
            state.move_bottom();
        }
        (KeyModifiers::NONE, KeyCode::Char('n')) | (KeyModifiers::NONE, KeyCode::Char(']')) => {
            state.next_file();
        }
        (KeyModifiers::NONE, KeyCode::Char('p')) | (KeyModifiers::NONE, KeyCode::Char('[')) => {
            state.prev_file();
        }
        (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => {
            state.col_offset = state.col_offset.saturating_sub(4);
        }
        (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => {
            state.col_offset += 4;
        }
        (KeyModifiers::NONE, KeyCode::Char('w')) => {
            state.word_diff = !state.word_diff;
        }
        (KeyModifiers::NONE, KeyCode::Char('y')) => {
            state.syntax_on = !state.syntax_on;
        }
        (KeyModifiers::NONE, KeyCode::Char('?')) => {
            state.show_help = !state.show_help;
        }
        (KeyModifiers::NONE, KeyCode::Char('m')) => {
            state.note_draft = Some(String::new());
        }
        _ => {}
    }
}

fn handle_mouse_event(state: &mut AppState, mouse: MouseEvent) {
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
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bin_pipeline::PipelineOutput;

    fn state_with_draft() -> AppState {
        let mut s = AppState::new(
            PipelineOutput {
                files: Vec::new(),
                branch: String::new(),
                untracked_skipped: Vec::new(),
            },
            "test".into(),
        );
        s.note_draft = Some("hello".into());
        s
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
}
