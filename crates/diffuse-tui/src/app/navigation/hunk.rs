use crate::app::AppState;

impl AppState {
    pub fn next_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let anchor = self.hunk_nav_anchor();
        let pos = self
            .hunk_starts
            .iter()
            .position(|h| h.flat > anchor)
            .unwrap_or(0);
        self.jump_to_hunk_pos(pos);
    }

    pub fn prev_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let anchor = self.hunk_nav_anchor();
        let pos = self
            .hunk_starts
            .iter()
            .rposition(|h| h.flat < anchor)
            .unwrap_or(self.hunk_starts.len() - 1);
        self.jump_to_hunk_pos(pos);
    }

    /// Row that hunk jumps navigate from: a file banner counts as inside its
    /// first hunk, so J advances past H1 instead of landing on its first
    /// line. Banners of files without loaded hunks fall back to the cursor.
    fn hunk_nav_anchor(&self) -> usize {
        let cur_fi = self.current_file_idx();
        if self
            .flat
            .get(self.row_idx)
            .is_some_and(|r| r.is_file_header())
            && let Some(h) = self.hunk_starts.iter().find(|h| h.file == cur_fi)
        {
            return h.flat;
        }
        self.row_idx
    }

    /// Jump to the hunk at `pos` in [`AppState::hunk_starts`]. Later hunks
    /// place their head line at the top of the viewport; the first hunk of
    /// a file keeps its file banner at the top instead, like file jumps.
    fn jump_to_hunk_pos(&mut self, pos: usize) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let pos = pos.min(self.hunk_starts.len() - 1);
        let fi = self.hunk_starts[pos].file;
        let flat = self.hunk_starts[pos].flat;
        let is_first = !self.hunk_starts[..pos].iter().any(|h| h.file == fi);
        if is_first {
            self.land_on_first_hunk(fi);
        } else {
            self.jump_to_row_top(flat);
        }
    }

    pub fn current_hunk(&self) -> Option<usize> {
        let fi = self.current_file_idx();
        let f = self.output.files.get(fi)?;
        if f.hunks.is_empty() {
            return None;
        }
        let mut cur = 0;
        for hs in &self.hunk_starts {
            if hs.file == fi && self.row_idx >= hs.flat {
                cur = hs.hunk;
            }
        }
        Some(cur)
    }

    pub fn next_file(&mut self) {
        let cur_fi = self.current_file_idx();
        for i in self.row_idx + 1..self.flat.len() {
            let next_fi = self.flat[i].file_idx;
            if next_fi > cur_fi {
                self.goto_file_idx(next_fi);
                return;
            }
        }
        // Past the last file: wrap around to the first file. Single-file
        // diffs stay put.
        if let Some(first) = self.flat.first() {
            let first_fi = first.file_idx;
            if first_fi != cur_fi {
                self.goto_file_idx(first_fi);
            }
        }
    }

    pub fn prev_file(&mut self) {
        let cur_fi = self.current_file_idx();
        for i in (0..self.row_idx).rev() {
            let prev_fi = self.flat[i].file_idx;
            if prev_fi < cur_fi {
                self.goto_file_idx(prev_fi);
                return;
            }
        }
        // Already in the first file: if its banner is scrolled out of view,
        // scroll it back as the top row. A further 'p' wraps to the last file.
        let banner = self.first_row_of_file(cur_fi);
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let (starts, _, _, _) = self.cached_geometry_tuple(cw);
        if starts.get(banner).copied().unwrap_or(0) < self.scroll {
            self.goto_file_idx(cur_fi);
            return;
        }
        if let Some(last) = self.flat.last() {
            let last_fi = last.file_idx;
            if last_fi != cur_fi {
                self.goto_file_idx(last_fi);
            }
        }
    }

    /// Move the cursor onto the file's first hunk head line while scrolling
    /// so the file banner stays the top row of the viewport — hunk
    /// navigation continues from a hunk without losing the file context.
    /// Files without loaded hunks keep the banner position from
    /// [`AppState::goto_file_idx`].
    fn land_on_first_hunk(&mut self, fi: usize) {
        let Some(h) = self.hunk_starts.iter().find(|h| h.file == fi) else {
            return;
        };
        self.row_idx = h.flat.min(self.flat.len().saturating_sub(1));
        self.vis_line = 0;

        let banner = self.first_row_of_file(fi);
        let max_rows = self.viewport_height.max(4);
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let (starts, _, _, total_vis_lines) = self.cached_geometry_tuple(cw);
        let banner_vis = starts.get(banner).copied().unwrap_or(0);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = banner_vis.min(max_scroll);
    }
}
