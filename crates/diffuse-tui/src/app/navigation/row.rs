use std::time::Instant;

use crate::{
    app::AppState,
    core::{parse::RowKind, wire::MovSide},
};

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

    /// Jump to the opposing side of a moved line (`MovSrc` <-> `MovTgt`).
    ///
    /// Reuses [`Self::jump_to_row_centered`] so that if the target row is
    /// visible anywhere in the view, only the cursor moves; otherwise the view
    /// is scrolled to 20% or 80% depending on the jump direction. Returns
    /// `true` if a jump occurred.
    pub fn goto_opposing_move(&mut self) -> bool {
        let Some(target_idx) = self.opposing_move_row_idx(self.row_idx) else {
            return false;
        };
        self.jump_to_row_centered(target_idx);
        true
    }

    /// Finds the flat index of the opposing moved row for the row at
    /// `from_idx`.
    pub fn opposing_move_row_idx(&self, from_idx: usize) -> Option<usize> {
        let fr = self.flat.get(from_idx)?;
        let row = self.row_of(fr)?;
        if row.kind != RowKind::Mov {
            return None;
        }
        let cur_side = row.mov_side?;
        let target_side = match cur_side {
            MovSide::Src => MovSide::Tgt,
            MovSide::Tgt => MovSide::Src,
        };
        let cur_fi = fr.file_idx;
        let cur_text = row.text();
        let cur_key = cur_text.trim_start();

        let mut cur_occ = 0;
        let mut found_from = false;
        let mut matching_opposing = Vec::new();
        let mut matching_by_runs = Vec::new();

        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != cur_fi || !item.is_diff_row() {
                continue;
            }
            let Some(r) = self.row_of(item) else { continue };
            if r.kind != RowKind::Mov {
                continue;
            }
            if r.mov_side == Some(cur_side) {
                if !found_from {
                    if i == from_idx {
                        found_from = true;
                    } else if r.text().trim_start() == cur_key {
                        cur_occ += 1;
                    }
                }
            } else if r.mov_side == Some(target_side) {
                if r.text().trim_start() == cur_key {
                    matching_opposing.push(i);
                } else if r.runs == row.runs {
                    matching_by_runs.push(i);
                }
            }
        }

        let pool = if !matching_opposing.is_empty() {
            &matching_opposing
        } else if !matching_by_runs.is_empty() {
            &matching_by_runs
        } else {
            return None;
        };

        pool.get(cur_occ).copied().or_else(|| pool.last().copied())
    }

    /// Position cursor at `target_row`. If the target row is visible anywhere
    /// in the viewport (outside sticky headers), jump without scrolling.
    /// Otherwise, position the row at either 20% or 80% of the viewport height
    /// depending on the direction of the jump ("closest" edge).
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
        let (starts, _, seg_counts, total_vis_lines) = self.cached_geometry_tuple(cw);
        let target_vis = starts.get(self.row_idx).copied().unwrap_or(0);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);

        let sticky = self.sticky_header_info_from_layout(&starts, &seg_counts, cw);
        let sticky_count = sticky.map_or(0, |s| s.effective_sticky_lines_for_row(self.row_idx));
        let visible_top = self.scroll + sticky_count;
        let visible_bottom = self.scroll + max_rows;

        let band_20 = (max_rows * 20) / 100;
        let band_80 = (max_rows * 80) / 100;

        if target_vis < visible_top {
            self.scroll = target_vis.saturating_sub(band_20).min(max_scroll);
        } else if target_vis >= visible_bottom {
            self.scroll = target_vis.saturating_sub(band_80).min(max_scroll);
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
