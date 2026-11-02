//! Standalone navigable multi-line text editor buffer and Readline operations.

use std::fmt::{self, Debug};

use ratatui::text::Span;

use super::{highlight::HighlightingCallback, history::EditorSnapshot};

/// Standalone, navigable multi-line text editor with soft-wrap navigation,
/// Unix Readline editing shortcuts, and pluggable syntax highlighting.
#[derive(Clone)]
pub struct Editor {
    pub(crate) lines: Vec<String>,
    pub(crate) cursor_row: usize,
    pub(crate) cursor_col: usize,
    pub(crate) goal_col: Option<usize>,
    pub(crate) scroll_row: usize,
    pub(crate) width: usize,
    pub(crate) highlighter: Option<HighlightingCallback>,
    pub(crate) cached_highlight: Option<Vec<Vec<Span<'static>>>>,
    pub(crate) kill_ring: Vec<String>,
    pub(crate) last_was_kill: bool,
    pub(crate) undo_stack: Vec<EditorSnapshot>,
}

impl Debug for Editor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

    /// Number of visual segments a logical line wraps into at `width`.
    pub fn line_seg_count(line: &str, width: usize) -> usize {
        let w = crate::render::width::display_width(line);
        if w == 0 { 1 } else { w.div_ceil(width.max(1)) }
    }

    /// Display column offset for character index `char_idx` in `line`.
    pub(crate) fn cursor_display_col(line: &str, char_idx: usize) -> usize {
        let byte_idx = line
            .char_indices()
            .nth(char_idx)
            .map_or(line.len(), |(b, _)| b);
        crate::render::width::display_width(&line[..byte_idx])
    }

    /// Character index in `line` whose prefix display width matches or is
    /// closest to `target_disp_col`.
    pub(crate) fn char_idx_for_display_col(line: &str, target_disp_col: usize) -> usize {
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
    pub(crate) fn cursor_vis_pos(&self) -> (usize, usize) {
        let width = self.width.max(1);
        let seg_count = Self::line_seg_count(&self.lines[self.cursor_row], width);
        let disp_col = Self::cursor_display_col(&self.lines[self.cursor_row], self.cursor_col);
        let vis_seg = (disp_col / width).min(seg_count.saturating_sub(1));
        let vis_col = disp_col.saturating_sub(vis_seg * width);
        (vis_seg, vis_col)
    }

    /// Sets the cursor from a visual segment index and column on logical line
    /// `row`.
    pub(crate) fn set_cursor_from_vis_pos(&mut self, row: usize, vis_seg: usize, vis_col: usize) {
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

    pub(crate) fn is_word_char(c: char) -> bool {
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

    pub(crate) fn push_kill(&mut self, text: &str, prepend: bool) {
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
}
