//! Terminal key event handling and Readline shortcut dispatch.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::buffer::Editor;

/// Action outcome from handling a key event in [`Editor`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EditorAction {
    /// Key event was ignored or had no effect.
    None,
    /// Text buffer contents were modified.
    Modified,
    /// Cursor position or viewport scroll moved without altering text.
    CursorMoved,
    /// User requested save / commit (`Ctrl-s`, `Ctrl-Enter`).
    Submit,
    /// User requested cancel / dismiss (`Esc`).
    Cancel,
}

impl Editor {
    /// Dispatch a terminal key event to standard Readline editing commands.
    pub fn handle_key(&mut self, key: KeyEvent) -> EditorAction {
        let is_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let is_alt = key.modifiers.contains(KeyModifiers::ALT);
        let is_shift = key.modifiers.contains(KeyModifiers::SHIFT);

        // Submit shortcuts: Ctrl-s, Ctrl-Enter
        if (is_ctrl && matches!(key.code, KeyCode::Char('s') | KeyCode::Enter))
            || (is_alt && matches!(key.code, KeyCode::Char('s')))
        {
            return EditorAction::Submit;
        }

        // Cancel shortcut: Esc
        if key.code == KeyCode::Esc {
            return EditorAction::Cancel;
        }

        // Undo: Ctrl-_, Ctrl-/, or Ctrl-z
        if is_ctrl
            && matches!(
                key.code,
                KeyCode::Char('_') | KeyCode::Char('/') | KeyCode::Char('z')
            )
        {
            if self.undo() {
                return EditorAction::Modified;
            }
            return EditorAction::None;
        }

        // Newline shortcuts: Ctrl-j (LF), Ctrl-m (CR), Ctrl-o, \n, \r
        if (is_ctrl
            && matches!(
                key.code,
                KeyCode::Char('j') | KeyCode::Char('m') | KeyCode::Char('o')
            ))
            || matches!(key.code, KeyCode::Char('\n') | KeyCode::Char('\r'))
        {
            self.insert_newline();
            return EditorAction::Modified;
        }

        // Unix Readline shortcuts
        if is_ctrl && !is_alt {
            match key.code {
                KeyCode::Char('a') => {
                    self.move_home();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Char('e') => {
                    self.move_end();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Char('b') => {
                    self.move_left();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Char('f') => {
                    self.move_right();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Char('p') => {
                    self.move_up();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Char('n') => {
                    self.move_down();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Char('h') => {
                    self.delete_backward();
                    return EditorAction::Modified;
                }
                KeyCode::Char('d') => {
                    self.delete_forward();
                    return EditorAction::Modified;
                }
                KeyCode::Char('k') => {
                    self.kill_to_end_of_line();
                    return EditorAction::Modified;
                }
                KeyCode::Char('u') => {
                    self.kill_to_start_of_line();
                    return EditorAction::Modified;
                }
                KeyCode::Char('w') => {
                    self.kill_word_backward();
                    return EditorAction::Modified;
                }
                KeyCode::Char('y') => {
                    self.yank();
                    return EditorAction::Modified;
                }
                KeyCode::Left => {
                    self.move_word_backward();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Right => {
                    self.move_word_forward();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Home => {
                    self.move_top();
                    return EditorAction::CursorMoved;
                }
                KeyCode::End => {
                    self.move_bottom();
                    return EditorAction::CursorMoved;
                }
                _ => {}
            }
        }

        // Alt shortcuts
        if is_alt && !is_ctrl {
            match key.code {
                KeyCode::Char('b') | KeyCode::Left => {
                    self.move_word_backward();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Char('f') | KeyCode::Right => {
                    self.move_word_forward();
                    return EditorAction::CursorMoved;
                }
                KeyCode::Char('d') | KeyCode::Delete => {
                    self.kill_word_forward();
                    return EditorAction::Modified;
                }
                KeyCode::Backspace => {
                    self.kill_word_backward();
                    return EditorAction::Modified;
                }
                _ => {}
            }
        }

        // Standard navigation and editing keys
        match key.code {
            KeyCode::Left => {
                self.move_left();
                EditorAction::CursorMoved
            }
            KeyCode::Right => {
                self.move_right();
                EditorAction::CursorMoved
            }
            KeyCode::Up => {
                self.move_up();
                EditorAction::CursorMoved
            }
            KeyCode::Down => {
                self.move_down();
                EditorAction::CursorMoved
            }
            KeyCode::Home => {
                self.move_home();
                EditorAction::CursorMoved
            }
            KeyCode::End => {
                self.move_end();
                EditorAction::CursorMoved
            }
            KeyCode::PageUp => {
                for _ in 0..10 {
                    self.move_up();
                }
                EditorAction::CursorMoved
            }
            KeyCode::PageDown => {
                for _ in 0..10 {
                    self.move_down();
                }
                EditorAction::CursorMoved
            }
            KeyCode::Backspace => {
                self.delete_backward();
                EditorAction::Modified
            }
            KeyCode::Delete => {
                self.delete_forward();
                EditorAction::Modified
            }
            KeyCode::Enter => {
                self.insert_newline();
                EditorAction::Modified
            }
            KeyCode::Char(c) if !is_ctrl && !is_alt => {
                self.insert_char(c);
                EditorAction::Modified
            }
            KeyCode::Tab => {
                self.insert_str("    ");
                EditorAction::Modified
            }
            KeyCode::BackTab if is_shift => {
                // Outdent if at start of line with spaces
                let line = &self.lines[self.cursor_row];
                let leading_spaces = line.chars().take_while(|&ch| ch == ' ').count();
                if leading_spaces > 0 && self.cursor_col <= leading_spaces {
                    let to_remove = 4.min(leading_spaces);
                    self.record_undo();
                    for _ in 0..to_remove {
                        self.lines[self.cursor_row].remove(0);
                    }
                    self.cursor_col = self.cursor_col.saturating_sub(to_remove);
                    EditorAction::Modified
                } else {
                    EditorAction::None
                }
            }
            _ => EditorAction::None,
        }
    }
}
