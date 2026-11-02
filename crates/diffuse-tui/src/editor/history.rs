//! History snapshots and undo stack management for Editor.

use super::buffer::Editor;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditorSnapshot {
    pub(crate) lines: Vec<String>,
    pub(crate) cursor_row: usize,
    pub(crate) cursor_col: usize,
}

impl Editor {
    /// Records an undo snapshot before mutating buffer contents.
    pub(crate) fn record_undo(&mut self) {
        self.cached_highlight = None;
        self.undo_stack.push(EditorSnapshot {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
        });
    }

    /// Reverts the last buffer mutation from the undo stack (`Ctrl-_`).
    pub fn undo(&mut self) -> bool {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.lines = snapshot.lines;
            self.cached_highlight = None;
            self.cursor_row = snapshot.cursor_row.min(self.lines.len().saturating_sub(1));
            let max_col = self.lines[self.cursor_row].chars().count();
            self.cursor_col = snapshot.cursor_col.min(max_col);
            self.goal_col = None;
            self.last_was_kill = false;
            true
        } else {
            false
        }
    }
}
