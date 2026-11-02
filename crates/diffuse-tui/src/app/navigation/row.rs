use std::time::Instant;

use crate::app::AppState;

impl AppState {
    /// Viewport jump margin: 1/10 of the viewport height (rounded half-up,
    /// minimum 1 line).
    pub fn viewport_margin(&self) -> usize {
        ((self.viewport_height + 5) / 10).max(1)
    }

    /// Position cursor at `target_row` and adjust scroll to preserve a 1/10
    /// viewport margin above the jump point (or below if jumping to bottom).
    pub fn jump_to_row(&mut self, target_row: usize, is_bottom: bool) {
        self.scroll_pin = None;
        self.row_idx = target_row.min(self.flat.len().saturating_sub(1));
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        if is_bottom {
            let nav_count = self.row_nav_vis_count(self.row_idx, cw);
            self.vis_line = nav_count.saturating_sub(1);
        } else {
            self.vis_line = 0;
        }

        let margin = self.viewport_margin();
        let max_rows = self.viewport_height.max(4);
        let (starts, _, _, total_vis_lines) = self.cached_geometry_tuple(cw);

        let target_vis = starts.get(self.row_idx).copied().unwrap_or(0) + self.vis_line;

        if is_bottom {
            self.scroll = target_vis.saturating_sub(max_rows.saturating_sub(1 + margin));
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
            if item.file_idx != cur_fi || !item.is_diff_row() {
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
            self.jump_to_row_centered(idx);
        }
    }

    /// Position cursor at `target_row`. If the target row is within 20% to 80%
    /// of the viewport height, jump without scrolling. Otherwise, center the
    /// row at the middle of the screen.
    pub fn jump_to_row_centered(&mut self, target_row: usize) {
        self.scroll_pin = None;
        self.row_idx = target_row.min(self.flat.len().saturating_sub(1));
        self.vis_line = 0;

        let max_rows = self.viewport_height.max(4);
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let (starts, _, _, total_vis_lines) = self.cached_geometry_tuple(cw);
        let target_vis = starts.get(self.row_idx).copied().unwrap_or(0);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);

        let min_band = (max_rows * 20) / 100;
        let max_band = (max_rows * 80) / 100;

        let in_band = if let Some(screen_y) = target_vis.checked_sub(self.scroll) {
            screen_y >= min_band && screen_y <= max_band
        } else {
            false
        };

        if !in_band {
            self.scroll = target_vis.saturating_sub(max_rows / 2).min(max_scroll);
        } else {
            self.scroll = self.scroll.min(max_scroll);
        }
    }

    pub fn goto_file_idx(&mut self, fi: usize) {
        let first = self.first_row_of_file(fi);
        self.jump_to_row_top(first);
    }

    /// Position cursor at `target_row` with its first visual line at the top
    /// of the viewport (no jump margin), clamped to the scrollable range.
    pub(crate) fn jump_to_row_top(&mut self, target_row: usize) {
        self.row_idx = target_row.min(self.flat.len().saturating_sub(1));
        self.vis_line = 0;

        let max_rows = self.viewport_height.max(4);
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let (starts, _, _, total_vis_lines) = self.cached_geometry_tuple(cw);
        let target_vis = starts.get(self.row_idx).copied().unwrap_or(0);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = target_vis.min(max_scroll);
    }

    pub fn move_down(&mut self) {
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let nav_count = self.row_nav_vis_count(self.row_idx, cw);
        if self.vis_line + 1 < nav_count {
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
            let cw = if self.last_content_width > 0 {
                self.last_content_width
            } else {
                80
            };
            let prev_nav_count = self.row_nav_vis_count(self.row_idx, cw);
            self.vis_line = prev_nav_count.saturating_sub(1);
        }
    }

    /// Keyboard `j`: like [`AppState::move_down`], but wraps past the last
    /// segment of the last row to the top. Rendering re-anchors the viewport
    /// onto the cursor.
    pub fn move_down_wrap(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let last = self.flat.len() - 1;
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let nav_count = self.row_nav_vis_count(self.row_idx, cw);
        if self.row_idx == last && self.vis_line + 1 >= nav_count {
            self.row_idx = 0;
            self.vis_line = 0;
        } else {
            self.move_down();
        }
    }

    /// Keyboard `k`: like [`AppState::move_up`], but wraps above the first
    /// row to the last segment of the last row. Rendering re-anchors the
    /// viewport onto the cursor.
    pub fn move_up_wrap(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        if self.row_idx == 0 && self.vis_line == 0 {
            self.row_idx = self.flat.len() - 1;
            let cw = if self.last_content_width > 0 {
                self.last_content_width
            } else {
                80
            };
            let last_nav_count = self.row_nav_vis_count(self.row_idx, cw);
            self.vis_line = last_nav_count.saturating_sub(1);
        } else {
            self.move_up();
        }
    }

    /// Scroll to a specific visual line offset (used by scrollbar dragging and
    /// paging).
    pub fn scroll_to_visual(&mut self, target_vis: usize) {
        self.scroll_pin = None;
        let geo = self.cached_geometry(self.last_content_width);
        let starts = geo.starts;
        let counts = geo.counts;
        let nav_counts = geo.nav_counts;
        let total_vis_lines = geo.total_vis_lines;
        let max_rows = self.viewport_height.max(4);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = target_vis.min(max_scroll);

        // Keep cursor in view
        let cursor_flat = self.row_idx;
        let cursor_vis = starts.get(cursor_flat).copied().unwrap_or(0) + self.vis_line;

        let sticky =
            self.sticky_header_info_from_layout(&starts, &geo.seg_counts, geo.content_width);
        let sticky_count = sticky.map_or(0, |s| s.total_sticky_lines());
        let content_start = self.scroll + sticky_count;
        if cursor_vis < content_start {
            for (i, &s) in starts.iter().enumerate() {
                if s + counts[i] > content_start {
                    self.row_idx = i;
                    let offset = content_start.saturating_sub(s);
                    self.vis_line = offset.min(nav_counts[i].saturating_sub(1));
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
                    let offset = bottom_vis.saturating_sub(s);
                    target_vis_line = offset.min(nav_counts[i].saturating_sub(1));
                    break;
                }
            }
            self.row_idx = target_row;
            self.vis_line = target_vis_line;
        }
    }

    pub fn page_down(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let started = Instant::now();
        let scroll_before = self.scroll;
        let geo = self.cached_geometry(self.last_content_width);
        let starts = geo.starts;
        let nav_counts = geo.nav_counts;
        let total_vis_lines = geo.total_vis_lines;
        let max_rows = self.viewport_height.max(4);
        let bottom_vis = (self.scroll + max_rows)
            .saturating_sub(1)
            .min(total_vis_lines.saturating_sub(1));

        let mut target_row = self.row_idx;
        let mut target_vis_line = 0;
        for (i, &s) in starts.iter().enumerate().rev() {
            if s <= bottom_vis {
                target_row = i;
                let offset = bottom_vis.saturating_sub(s);
                target_vis_line = offset.min(nav_counts[i].saturating_sub(1));
                break;
            }
        }

        if self.row_idx == target_row && self.vis_line == target_vis_line {
            // Already at bottom -> scroll 1/3 of viewport down and place cursor
            // at new bottom
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
                    let offset = new_bottom_vis.saturating_sub(s);
                    new_target_vis_line = offset.min(nav_counts[i].saturating_sub(1));
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
        tracing::debug!(
            from_scroll = scroll_before,
            to_scroll = self.scroll,
            row_idx = self.row_idx,
            elapsed_ms = started.elapsed().as_millis(),
            "page_down"
        );
    }

    pub fn page_up(&mut self) {
        if self.flat.is_empty() {
            return;
        }
        let started = Instant::now();
        let scroll_before = self.scroll;
        let geo = self.cached_geometry(self.last_content_width);
        let starts = geo.starts;
        let counts = geo.counts;
        let nav_counts = geo.nav_counts;
        let sticky =
            self.sticky_header_info_from_layout(&starts, &geo.seg_counts, geo.content_width);
        let sticky_count = sticky.map_or(0, |s| s.total_sticky_lines());
        let top_vis = self.scroll + sticky_count;

        let mut target_row = self.row_idx;
        let mut target_vis_line = 0;
        for (i, &s) in starts.iter().enumerate() {
            if s + counts[i] > top_vis {
                target_row = i;
                let offset = top_vis.saturating_sub(s);
                target_vis_line = offset.min(nav_counts[i].saturating_sub(1));
                break;
            }
        }

        if self.row_idx == target_row && self.vis_line == target_vis_line {
            // Already at top -> scroll 1/3 of viewport up and place cursor at
            // new top
            let max_rows = self.viewport_height.max(4);
            let step = ((max_rows * 2 + 3) / 6).max(1);
            self.scroll_to_visual(self.scroll.saturating_sub(step));
            let new_sticky =
                self.sticky_header_info_from_layout(&starts, &geo.seg_counts, geo.content_width);
            let new_sticky_count = new_sticky.map_or(0, |s| s.total_sticky_lines());
            let new_top_vis = self.scroll + new_sticky_count;
            let mut new_target_row = self.row_idx;
            let mut new_target_vis_line = 0;
            for (i, &s) in starts.iter().enumerate() {
                if s + counts[i] > new_top_vis {
                    new_target_row = i;
                    let offset = new_top_vis.saturating_sub(s);
                    new_target_vis_line = offset.min(nav_counts[i].saturating_sub(1));
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
        tracing::debug!(
            from_scroll = scroll_before,
            to_scroll = self.scroll,
            row_idx = self.row_idx,
            elapsed_ms = started.elapsed().as_millis(),
            "page_up"
        );
    }

    pub fn move_top(&mut self) {
        self.jump_to_row(0, false);
    }

    pub fn move_bottom(&mut self) {
        if !self.flat.is_empty() {
            self.jump_to_row(self.flat.len() - 1, true);
        }
    }
}
