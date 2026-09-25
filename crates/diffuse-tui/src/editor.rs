//! Text editor component and external editor launcher for diffuse TUI.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::sync::Arc;

pub mod external;
pub use external::*;

pub type HighlightingCallback = Arc<dyn Fn(&[String]) -> Vec<Vec<Span<'static>>> + Send + Sync>;

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditorSnapshot {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

/// Standalone, navigable multi-line text editor with soft-wrap navigation,
/// Unix Readline editing shortcuts, and pluggable syntax highlighting.
#[derive(Clone)]
pub struct Editor {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    goal_col: Option<usize>,
    scroll_row: usize,
    width: usize,
    highlighter: Option<HighlightingCallback>,
    cached_highlight: Option<Vec<Vec<Span<'static>>>>,
    kill_ring: Vec<String>,
    last_was_kill: bool,
    undo_stack: Vec<EditorSnapshot>,
}

impl std::fmt::Debug for Editor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Editor")
            .field("lines", &self.lines)
            .field("cursor_row", &self.cursor_row)
            .field("cursor_col", &self.cursor_col)
            .field("goal_col", &self.goal_col)
            .field("scroll_row", &self.scroll_row)
            .field("width", &self.width)
            .finish()
    }
}

impl Editor {
    /// Creates a new editor instance pre-populated with `initial_text`.
    pub fn new(initial_text: &str) -> Self {
        let lines: Vec<String> = if initial_text.is_empty() {
            vec![String::new()]
        } else {
            initial_text.split('\n').map(|s| s.to_string()).collect()
        };
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            goal_col: None,
            scroll_row: 0,
            width: 80,
            highlighter: None,
            cached_highlight: None,
            kill_ring: Vec::new(),
            last_was_kill: false,
            undo_stack: Vec::new(),
        }
    }

    /// Sets the pluggable syntax highlighting callback.
    pub fn with_highlighter(mut self, h: HighlightingCallback) -> Self {
        self.highlighter = Some(h);
        self.cached_highlight = None;
        self
    }

    /// Attaches or clears the pluggable highlighting callback.
    pub fn set_highlighter(&mut self, h: Option<HighlightingCallback>) {
        self.highlighter = h;
        self.cached_highlight = None;
    }

    /// Returns the full text joined with newlines.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Returns the slice of logical lines.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Returns current logical cursor coordinate `(row, col)`.
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    /// Sets the logical cursor coordinate, clamping to document boundaries.
    pub fn set_cursor(&mut self, row: usize, col: usize) {
        self.cursor_row = row.min(self.lines.len().saturating_sub(1));
        let max_col = self.lines[self.cursor_row].chars().count();
        self.cursor_col = col.min(max_col);
        self.goal_col = None;
    }

    /// Sets the editor width used for soft-wrapping visual calculations.
    pub fn set_width(&mut self, width: usize) {
        self.width = width.max(1);
    }

    /// Records an undo snapshot before mutating buffer contents.
    fn record_undo(&mut self) {
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

    /// Number of visual segments a logical line wraps into at `width`.
    pub fn line_seg_count(line: &str, width: usize) -> usize {
        let w = crate::render::width::display_width(line);
        if w == 0 { 1 } else { w.div_ceil(width.max(1)) }
    }

    /// Display column offset for character index `char_idx` in `line`.
    fn cursor_display_col(line: &str, char_idx: usize) -> usize {
        let byte_idx = line
            .char_indices()
            .nth(char_idx)
            .map_or(line.len(), |(b, _)| b);
        crate::render::width::display_width(&line[..byte_idx])
    }

    /// Character index in `line` whose prefix display width matches or is
    /// closest to `target_disp_col`.
    fn char_idx_for_display_col(line: &str, target_disp_col: usize) -> usize {
        let mut char_count = 0usize;
        for (idx, (b, _)) in line.char_indices().enumerate() {
            let w = crate::render::width::display_width(&line[..b]);
            if w >= target_disp_col {
                return idx;
            }
            char_count = idx + 1;
        }
        if crate::render::width::display_width(line) <= target_disp_col {
            return char_count;
        }
        char_count
    }

    /// Visual position `(vis_seg, vis_col)` within the current logical line.
    fn cursor_vis_pos(&self) -> (usize, usize) {
        let width = self.width.max(1);
        let seg_count = Self::line_seg_count(&self.lines[self.cursor_row], width);
        let disp_col = Self::cursor_display_col(&self.lines[self.cursor_row], self.cursor_col);
        let vis_seg = (disp_col / width).min(seg_count.saturating_sub(1));
        let vis_col = disp_col.saturating_sub(vis_seg * width);
        (vis_seg, vis_col)
    }

    /// Sets the cursor from a visual segment index and column on logical line
    /// `row`.
    fn set_cursor_from_vis_pos(&mut self, row: usize, vis_seg: usize, vis_col: usize) {
        self.cursor_row = row.min(self.lines.len().saturating_sub(1));
        let width = self.width.max(1);
        let target_disp_col = vis_seg * width + vis_col;
        self.cursor_col =
            Self::char_idx_for_display_col(&self.lines[self.cursor_row], target_disp_col);
    }

    /// Move cursor one character to the left, wrapping to previous line if at
    /// start.
    pub fn move_left(&mut self) {
        self.goal_col = None;
        self.last_was_kill = false;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].chars().count();
        }
    }

    /// Move cursor one character to the right, wrapping to next line if at end.
    pub fn move_right(&mut self) {
        self.goal_col = None;
        self.last_was_kill = false;
        let char_count = self.lines[self.cursor_row].chars().count();
        if self.cursor_col < char_count {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    /// Move cursor to the beginning of the logical line (`Home`, `Ctrl-a`).
    pub fn move_home(&mut self) {
        self.cursor_col = 0;
        self.goal_col = None;
        self.last_was_kill = false;
    }

    /// Move cursor to the end of the logical line (`End`, `Ctrl-e`).
    pub fn move_end(&mut self) {
        self.cursor_col = self.lines[self.cursor_row].chars().count();
        self.goal_col = None;
        self.last_was_kill = false;
    }

    /// Move cursor to the start of the document (`Ctrl-Home`).
    pub fn move_top(&mut self) {
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.goal_col = None;
        self.last_was_kill = false;
    }

    /// Move cursor to the end of the document (`Ctrl-End`).
    pub fn move_bottom(&mut self) {
        self.cursor_row = self.lines.len().saturating_sub(1);
        self.cursor_col = self.lines[self.cursor_row].chars().count();
        self.goal_col = None;
        self.last_was_kill = false;
    }

    /// Move cursor up one visual row, preserving the sticky goal column.
    pub fn move_up(&mut self) {
        self.last_was_kill = false;
        let (vis_seg, vis_col) = self.cursor_vis_pos();
        let goal = self.goal_col.unwrap_or(vis_col);
        self.goal_col = Some(goal);
        if vis_seg > 0 {
            self.set_cursor_from_vis_pos(self.cursor_row, vis_seg - 1, goal);
        } else if self.cursor_row > 0 {
            let prev_row = self.cursor_row - 1;
            let prev_segs = Self::line_seg_count(&self.lines[prev_row], self.width);
            self.set_cursor_from_vis_pos(prev_row, prev_segs.saturating_sub(1), goal);
        }
    }

    /// Move cursor down one visual row, preserving the sticky goal column.
    pub fn move_down(&mut self) {
        self.last_was_kill = false;
        let (vis_seg, vis_col) = self.cursor_vis_pos();
        let goal = self.goal_col.unwrap_or(vis_col);
        self.goal_col = Some(goal);
        let cur_segs = Self::line_seg_count(&self.lines[self.cursor_row], self.width);
        if vis_seg + 1 < cur_segs {
            self.set_cursor_from_vis_pos(self.cursor_row, vis_seg + 1, goal);
        } else if self.cursor_row + 1 < self.lines.len() {
            self.set_cursor_from_vis_pos(self.cursor_row + 1, 0, goal);
        }
    }

    fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// Move cursor forward to the end of the current or next word
    /// (`Ctrl-Right`, `Alt-f`).
    pub fn move_word_forward(&mut self) {
        self.goal_col = None;
        self.last_was_kill = false;
        let line = &self.lines[self.cursor_row];
        let chars: Vec<char> = line.chars().collect();
        let mut idx = self.cursor_col;

        if idx >= chars.len() {
            if self.cursor_row + 1 < self.lines.len() {
                self.cursor_row += 1;
                self.cursor_col = 0;
            }
            return;
        }

        // Skip non-word chars
        while idx < chars.len() && !Self::is_word_char(chars[idx]) {
            idx += 1;
        }
        // Skip word chars
        while idx < chars.len() && Self::is_word_char(chars[idx]) {
            idx += 1;
        }
        self.cursor_col = idx;
    }

    /// Move cursor backward to the start of the current or previous word
    /// (`Ctrl-Left`, `Alt-b`).
    pub fn move_word_backward(&mut self) {
        self.goal_col = None;
        self.last_was_kill = false;
        if self.cursor_col == 0 {
            if self.cursor_row > 0 {
                self.cursor_row -= 1;
                self.cursor_col = self.lines[self.cursor_row].chars().count();
            }
            return;
        }

        let line = &self.lines[self.cursor_row];
        let chars: Vec<char> = line.chars().collect();
        let mut idx = self.cursor_col;

        // Skip non-word chars backward
        while idx > 0 && !Self::is_word_char(chars[idx - 1]) {
            idx -= 1;
        }
        // Skip word chars backward
        while idx > 0 && Self::is_word_char(chars[idx - 1]) {
            idx -= 1;
        }
        self.cursor_col = idx;
    }

    fn push_kill(&mut self, text: &str, prepend: bool) {
        if text.is_empty() {
            return;
        }
        if self.last_was_kill
            && let Some(last) = self.kill_ring.last_mut()
        {
            if prepend {
                last.insert_str(0, text);
            } else {
                last.push_str(text);
            }
        } else {
            self.kill_ring.push(text.to_string());
        }
        self.last_was_kill = true;
    }

    /// Kill text from cursor to end of line (`Ctrl-k`).
    pub fn kill_to_end_of_line(&mut self) {
        self.record_undo();
        self.goal_col = None;
        let char_count = self.lines[self.cursor_row].chars().count();
        if self.cursor_col < char_count {
            let line = &mut self.lines[self.cursor_row];
            let byte_idx = line
                .char_indices()
                .nth(self.cursor_col)
                .map(|(b, _)| b)
                .unwrap();
            let killed = line[byte_idx..].to_string();
            line.truncate(byte_idx);
            self.push_kill(&killed, false);
        } else if self.cursor_row + 1 < self.lines.len() {
            let next_line = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next_line);
            self.push_kill("\n", false);
        }
    }

    /// Kill text from beginning of line to cursor (`Ctrl-u`).
    pub fn kill_to_start_of_line(&mut self) {
        self.record_undo();
        self.goal_col = None;
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            let byte_idx = line
                .char_indices()
                .nth(self.cursor_col)
                .map(|(b, _)| b)
                .unwrap();
            let killed = line[..byte_idx].to_string();
            line.drain(..byte_idx);
            self.cursor_col = 0;
            self.push_kill(&killed, true);
        }
    }

    /// Kill word backward before cursor (`Ctrl-w`, `Alt-Backspace`).
    pub fn kill_word_backward(&mut self) {
        if self.cursor_col == 0 {
            if self.cursor_row > 0 {
                self.record_undo();
                self.delete_backward();
            }
            return;
        }
        self.record_undo();
        self.goal_col = None;
        let line = &self.lines[self.cursor_row];
        let chars: Vec<char> = line.chars().collect();
        let mut idx = self.cursor_col;

        while idx > 0 && !Self::is_word_char(chars[idx - 1]) {
            idx -= 1;
        }
        while idx > 0 && Self::is_word_char(chars[idx - 1]) {
            idx -= 1;
        }

        let line = &mut self.lines[self.cursor_row];
        let start_b = line.char_indices().nth(idx).map(|(b, _)| b).unwrap();
        let end_b = line
            .char_indices()
            .nth(self.cursor_col)
            .map_or(line.len(), |(b, _)| b);
        let killed = line[start_b..end_b].to_string();
        line.drain(start_b..end_b);
        self.cursor_col = idx;
        self.push_kill(&killed, true);
    }

    /// Kill word forward after cursor (`Alt-d`, `Alt-Delete`).
    pub fn kill_word_forward(&mut self) {
        let char_count = self.lines[self.cursor_row].chars().count();
        if self.cursor_col >= char_count {
            if self.cursor_row + 1 < self.lines.len() {
                self.record_undo();
                self.delete_forward();
            }
            return;
        }
        self.record_undo();
        self.goal_col = None;
        let line = &self.lines[self.cursor_row];
        let chars: Vec<char> = line.chars().collect();
        let mut idx = self.cursor_col;

        while idx < chars.len() && !Self::is_word_char(chars[idx]) {
            idx += 1;
        }
        while idx < chars.len() && Self::is_word_char(chars[idx]) {
            idx += 1;
        }

        let line = &mut self.lines[self.cursor_row];
        let start_b = line
            .char_indices()
            .nth(self.cursor_col)
            .map(|(b, _)| b)
            .unwrap();
        let end_b = line.char_indices().nth(idx).map_or(line.len(), |(b, _)| b);
        let killed = line[start_b..end_b].to_string();
        line.drain(start_b..end_b);
        self.push_kill(&killed, false);
    }

    /// Insert the most recently killed text at the cursor position (`Ctrl-y`).
    pub fn yank(&mut self) {
        if let Some(text) = self.kill_ring.last().cloned() {
            self.record_undo();
            self.insert_str(&text);
            self.last_was_kill = false;
        }
    }

    /// Inserts a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        self.record_undo();
        self.goal_col = None;
        self.last_was_kill = false;
        let line = &mut self.lines[self.cursor_row];
        let byte_idx = line
            .char_indices()
            .nth(self.cursor_col)
            .map_or(line.len(), |(b, _)| b);
        line.insert(byte_idx, c);
        self.cursor_col += 1;
    }

    /// Inserts a multi-line string at the cursor position.
    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                self.insert_newline();
            } else {
                let line = &mut self.lines[self.cursor_row];
                let byte_idx = line
                    .char_indices()
                    .nth(self.cursor_col)
                    .map_or(line.len(), |(b, _)| b);
                line.insert(byte_idx, c);
                self.cursor_col += 1;
            }
        }
        self.goal_col = None;
        self.last_was_kill = false;
    }

    /// Splits the current line at cursor into two lines (`Enter`).
    pub fn insert_newline(&mut self) {
        self.record_undo();
        self.goal_col = None;
        self.last_was_kill = false;
        let line = &self.lines[self.cursor_row];
        let byte_idx = line
            .char_indices()
            .nth(self.cursor_col)
            .map_or(line.len(), |(b, _)| b);
        let tail = line[byte_idx..].to_string();
        self.lines[self.cursor_row].truncate(byte_idx);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.lines.insert(self.cursor_row, tail);
    }

    /// Deletes the character before the cursor (`Backspace`, `Ctrl-h`).
    pub fn delete_backward(&mut self) {
        self.record_undo();
        self.goal_col = None;
        self.last_was_kill = false;
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            let line = &mut self.lines[self.cursor_row];
            if let Some((byte_idx, _)) = line.char_indices().nth(self.cursor_col) {
                line.remove(byte_idx);
            }
        } else if self.cursor_row > 0 {
            let current_line = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].chars().count();
            self.lines[self.cursor_row].push_str(&current_line);
        }
    }

    /// Deletes the character under the cursor (`Delete`, `Ctrl-d`).
    pub fn delete_forward(&mut self) {
        self.record_undo();
        self.goal_col = None;
        self.last_was_kill = false;
        let char_count = self.lines[self.cursor_row].chars().count();
        if self.cursor_col < char_count {
            let line = &mut self.lines[self.cursor_row];
            if let Some((byte_idx, _)) = line.char_indices().nth(self.cursor_col) {
                line.remove(byte_idx);
            }
        } else if self.cursor_row + 1 < self.lines.len() {
            let next_line = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next_line);
        }
    }

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

    /// Renders the editor into `area`, handling viewport scrolling and
    /// positioning the terminal's hardware cursor.
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let width = area.width as usize;
        let height = area.height as usize;
        self.set_width(width);

        let mut all_vis_lines: Vec<Line<'static>> = Vec::new();
        let mut cursor_vis_y = 0usize;
        let mut cursor_vis_x = 0usize;
        let mut total_vis_rows = 0usize;

        let all_spans = if let Some(highlighter) = &self.highlighter {
            if let Some(ref cached) = self.cached_highlight {
                cached.clone()
            } else {
                let computed = highlighter(&self.lines);
                self.cached_highlight = Some(computed.clone());
                computed
            }
        } else {
            Vec::new()
        };

        for (l_idx, line) in self.lines.iter().enumerate() {
            let spans = all_spans.get(l_idx).cloned().unwrap_or_else(|| {
                vec![Span::styled(
                    line.to_string(),
                    ratatui::style::Style::default().fg(ratatui::style::Color::Indexed(231)),
                )]
            });

            let seg_count = Self::line_seg_count(line, width);
            if l_idx == self.cursor_row {
                let disp_col = Self::cursor_display_col(line, self.cursor_col);
                let vis_seg = (disp_col / width).min(seg_count.saturating_sub(1));
                let vis_col = disp_col.saturating_sub(vis_seg * width);
                cursor_vis_y = total_vis_rows + vis_seg;
                cursor_vis_x = vis_col;
            }

            for s_idx in 0..seg_count {
                let seg_spans = slice_spans(&spans, s_idx * width, width);
                all_vis_lines.push(Line::from(seg_spans));
            }
            total_vis_rows += seg_count;
        }

        // Auto-scroll viewport to keep cursor visible
        if cursor_vis_y < self.scroll_row {
            self.scroll_row = cursor_vis_y;
        } else if cursor_vis_y >= self.scroll_row + height {
            self.scroll_row = cursor_vis_y.saturating_sub(height - 1);
        }

        // Render visible visual lines
        for (v_offset, vis_idx) in (self.scroll_row..self.scroll_row + height).enumerate() {
            if let Some(line) = all_vis_lines.get(vis_idx) {
                let row_area = Rect::new(area.x, area.y + v_offset as u16, area.width, 1);
                f.render_widget(Paragraph::new(line.clone()), row_area);
            }
        }

        // Set native hardware cursor position
        if cursor_vis_y >= self.scroll_row && cursor_vis_y < self.scroll_row + height {
            let screen_x = area.x + (cursor_vis_x as u16).min(area.width.saturating_sub(1));
            let screen_y = area.y + (cursor_vis_y - self.scroll_row) as u16;
            f.set_cursor_position(Position {
                x: screen_x,
                y: screen_y,
            });
        }
    }
}

/// Creates a pluggable syntax highlighting callback that formats Markdown text
/// using diffuse's standard Markdown classifier and theme across the full
/// document.
pub fn markdown_highlighter(theme: crate::render::DiffuseTheme) -> HighlightingCallback {
    Arc::new(move |lines: &[String]| {
        let doc = lines.join("\n");
        let doc_tokens = diffuse_syntax::classifier::markdown_document_tokens(&doc);
        let mut out = Vec::with_capacity(lines.len());
        for line_tokens in doc_tokens {
            let mut ansi_buf = String::new();
            for tok in line_tokens {
                theme.write_token(tok.color, tok.text, "ctx", None, tok.sgr, &mut ansi_buf);
            }
            let ratatui_line = crate::view::ansi_to_ratatui_line(&ansi_buf);
            out.push(ratatui_line.spans);
        }
        while out.len() < lines.len() {
            out.push(vec![Span::styled(
                lines[out.len()].clone(),
                ratatui::style::Style::default().fg(ratatui::style::Color::Indexed(231)),
            )]);
        }
        out
    })
}

/// Slices styled `Span`s by visual display column boundaries
/// `start_col..start_col + max_cols`.
fn slice_spans(spans: &[Span<'static>], start_col: usize, max_cols: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut cur_col = 0usize;
    let seg_end = start_col + max_cols;

    for span in spans {
        let span_w = crate::render::width::display_width(&span.content);
        let span_start = cur_col;
        let span_end = cur_col + span_w;
        cur_col = span_end;

        if span_end <= start_col || span_start >= seg_end {
            continue;
        }

        let s = start_col.max(span_start);
        let e = seg_end.min(span_end);
        let piece = crate::render::slice_by_visual_width(
            &span.content,
            s.saturating_sub(span_start),
            e.saturating_sub(s),
        );
        if !piece.is_empty() {
            out.push(Span::styled(piece, span.style));
        }
    }
    out
}

#[cfg(test)]
mod tests;
