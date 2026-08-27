//! Navigation — viewport and cursor movement.

use super::AppState;

impl AppState {
    /// Viewport jump margin: 1/10 of the viewport height (rounded half-up,
    /// minimum 1 line).
    pub fn viewport_margin(&self) -> usize {
        ((self.viewport_height + 5) / 10).max(1)
    }

    /// Position cursor at `target_row` and adjust scroll to preserve a 1/10
    /// viewport margin above the jump point (or below if jumping to bottom).
    pub fn jump_to_row(&mut self, target_row: usize, is_bottom: bool) {
        self.row_idx = target_row.min(self.flat.len().saturating_sub(1));
        self.vis_line = 0;

        let margin = self.viewport_margin();
        let max_rows = self.viewport_height.max(4);
        let (starts, _, _, total_vis_lines) = self.visual_geometry(self.last_content_width);

        let target_vis = starts.get(self.row_idx).copied().unwrap_or(0);

        if is_bottom {
            let hunk_div_offset = if self.hunk_starts.iter().any(|h| h.flat == self.row_idx) {
                1
            } else {
                0
            };
            self.scroll =
                (target_vis + hunk_div_offset).saturating_sub(max_rows.saturating_sub(1 + margin));
        } else {
            self.scroll = target_vis.saturating_sub(margin);
        }

        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = self.scroll.min(max_scroll);
    }

    /// Jump to the line in the current file closest to `line_num`.
    pub fn goto_line(&mut self, line_num: u32) {
        let cur_fi = self.current_file_idx();
        let mut best: Option<(usize, u32)> = None;
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != cur_fi || item.is_file_header {
                continue;
            }
            if let Some(c) = self.flat_row_coord(item) {
                let dist = c.abs_diff(line_num);
                match best {
                    None => best = Some((i, dist)),
                    Some((_, best_dist)) if dist < best_dist => best = Some((i, dist)),
                    _ => {}
                }
            }
        }
        if let Some((idx, _)) = best {
            self.jump_to_row(idx, false);
        }
    }

    pub fn goto_file_idx(&mut self, fi: usize) {
        let first = self.first_row_of_file(fi);
        self.jump_to_row(first, false);
    }

    pub fn move_down(&mut self) {
        let seg_count = self.row_seg_count(self.row_idx, self.last_content_width);
        if self.vis_line + 1 < seg_count {
            self.vis_line += 1;
        } else if self.row_idx + 1 < self.flat.len() {
            self.row_idx += 1;
            self.vis_line = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.vis_line > 0 {
            self.vis_line -= 1;
        } else if self.row_idx > 0 {
            self.row_idx -= 1;
            let prev_seg_count = self.row_seg_count(self.row_idx, self.last_content_width);
            self.vis_line = prev_seg_count.saturating_sub(1);
        }
    }

    pub fn next_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let target = self
            .hunk_starts
            .iter()
            .find(|h| h.flat > self.row_idx)
            .map(|h| h.flat)
            .unwrap_or(self.hunk_starts[0].flat);
        self.jump_to_row(target, false);
    }

    /// Scroll to a specific visual line offset (used by scrollbar dragging and
    /// paging).
    pub fn scroll_to_visual(&mut self, target_vis: usize) {
        let (starts, counts, seg_counts, total_vis_lines) =
            self.visual_geometry(self.last_content_width);
        let max_rows = self.viewport_height.max(4);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = target_vis.min(max_scroll);

        // Keep cursor in view
        let cursor_flat = self.row_idx;
        let hunk_div_offset = if self.hunk_starts.iter().any(|h| h.flat == cursor_flat) {
            1
        } else {
            0
        };
        let cursor_vis =
            starts.get(cursor_flat).copied().unwrap_or(0) + hunk_div_offset + self.vis_line;

        if cursor_vis < self.scroll {
            for (i, &s) in starts.iter().enumerate() {
                if s + counts[i] > self.scroll {
                    self.row_idx = i;
                    let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                        1
                    } else {
                        0
                    };
                    let offset = self.scroll.saturating_sub(s + hunk_div);
                    self.vis_line = offset.min(seg_counts[i].saturating_sub(1));
                    break;
                }
            }
        } else if cursor_vis >= self.scroll + max_rows {
            let bottom_vis = (self.scroll + max_rows).saturating_sub(1);
            let mut target_row = self.row_idx;
            let mut target_vis_line = 0;
            for (i, &s) in starts.iter().enumerate().rev() {
                if s <= bottom_vis {
                    target_row = i;
                    let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                        1
                    } else {
                        0
                    };
                    let offset = bottom_vis.saturating_sub(s + hunk_div);
                    target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                    break;
                }
            }
            self.row_idx = target_row;
            self.vis_line = target_vis_line;
        }
    }

    pub fn prev_hunk(&mut self) {
        if self.hunk_starts.is_empty() {
            return;
        }
        let target = self
            .hunk_starts
            .iter()
            .rev()
            .find(|h| h.flat < self.row_idx)
            .map(|h| h.flat)
            .unwrap_or(self.hunk_starts[self.hunk_starts.len() - 1].flat);
        self.jump_to_row(target, false);
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

    pub fn page_down(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let (starts, _counts, seg_counts, total_vis_lines) =
            self.visual_geometry(self.last_content_width);
        let max_rows = self.viewport_height.max(4);
        let bottom_vis = (self.scroll + max_rows)
            .saturating_sub(1)
            .min(total_vis_lines.saturating_sub(1));

        let mut target_row = self.row_idx;
        let mut target_vis_line = 0;
        for (i, &s) in starts.iter().enumerate().rev() {
            if s <= bottom_vis {
                target_row = i;
                let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                    1
                } else {
                    0
                };
                let offset = bottom_vis.saturating_sub(s + hunk_div);
                target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                break;
            }
        }

        if self.row_idx == target_row && self.vis_line == target_vis_line {
            // Already at bottom -> scroll 1/3 of viewport down and place cursor at new
            // bottom
            let step = ((max_rows * 2 + 3) / 6).max(1);
            self.scroll_to_visual(self.scroll + step);
            let new_bottom_vis = (self.scroll + max_rows)
                .saturating_sub(1)
                .min(total_vis_lines.saturating_sub(1));
            let mut new_target_row = self.row_idx;
            let mut new_target_vis_line = 0;
            for (i, &s) in starts.iter().enumerate().rev() {
                if s <= new_bottom_vis {
                    new_target_row = i;
                    let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                        1
                    } else {
                        0
                    };
                    let offset = new_bottom_vis.saturating_sub(s + hunk_div);
                    new_target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                    break;
                }
            }
            self.row_idx = new_target_row;
            self.vis_line = new_target_vis_line;
        } else {
            // Jump cursor to bottom of visible page
            self.row_idx = target_row;
            self.vis_line = target_vis_line;
        }
    }

    pub fn page_up(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let (starts, counts, seg_counts, _total_vis_lines) =
            self.visual_geometry(self.last_content_width);
        let top_vis = self.scroll;

        let mut target_row = self.row_idx;
        let mut target_vis_line = 0;
        for (i, &s) in starts.iter().enumerate() {
            if s + counts[i] > top_vis {
                target_row = i;
                let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                    1
                } else {
                    0
                };
                let offset = top_vis.saturating_sub(s + hunk_div);
                target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                break;
            }
        }

        if self.row_idx == target_row && self.vis_line == target_vis_line {
            // Already at top -> scroll 1/3 of viewport up and place cursor at new top
            let max_rows = self.viewport_height.max(4);
            let step = ((max_rows * 2 + 3) / 6).max(1);
            self.scroll_to_visual(self.scroll.saturating_sub(step));
            let new_top_vis = self.scroll;
            let mut new_target_row = self.row_idx;
            let mut new_target_vis_line = 0;
            for (i, &s) in starts.iter().enumerate() {
                if s + counts[i] > new_top_vis {
                    new_target_row = i;
                    let hunk_div = if self.hunk_starts.iter().any(|h| h.flat == i) {
                        1
                    } else {
                        0
                    };
                    let offset = new_top_vis.saturating_sub(s + hunk_div);
                    new_target_vis_line = offset.min(seg_counts[i].saturating_sub(1));
                    break;
                }
            }
            self.row_idx = new_target_row;
            self.vis_line = new_target_vis_line;
        } else {
            // Jump cursor to top of visible page
            self.row_idx = target_row;
            self.vis_line = target_vis_line;
        }
    }

    pub fn move_top(&mut self) {
        self.jump_to_row(0, false);
    }

    pub fn move_bottom(&mut self) {
        if !self.flat.is_empty() {
            self.jump_to_row(self.flat.len() - 1, true);
        }
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
    }
}

#[cfg(test)]
mod tests;
