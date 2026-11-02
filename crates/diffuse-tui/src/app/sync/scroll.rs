use crate::app::AppState;

impl AppState {
    pub(crate) fn adjust_scroll(&mut self) {
        if self.flat.is_empty() {
            self.scroll = 0;
            return;
        }
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let (starts, counts, _, total_vis_lines) = self.cached_geometry_tuple(cw);
        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);

        let cur_vis = starts.get(self.row_idx).copied().unwrap_or(0);
        let cur_count = counts.get(self.row_idx).copied().unwrap_or(1);

        if cur_vis < self.scroll {
            self.scroll = cur_vis;
        } else if cur_vis + cur_count > self.scroll + max_rows {
            self.scroll = (cur_vis + cur_count).saturating_sub(max_rows);
        }
        self.scroll = self.scroll.min(max_scroll);
    }

    pub fn adjust_scroll_for_expanded_file(&mut self, fi: usize) {
        if self.collapsed_files.contains(&fi) || !self.is_file_loaded(fi) {
            return;
        }
        let vh = self.viewport_height;
        if vh == 0 {
            return;
        }

        let header_flat = self.first_row_of_file(fi);
        let last_flat = self.last_row_of_file(fi);

        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let (starts, counts, _, total_vis_lines) = self.cached_geometry_tuple(cw);
        let Some(&header_vis) = starts.get(header_flat) else {
            return;
        };
        let Some(&last_vis) = starts.get(last_flat) else {
            return;
        };
        let last_count = counts.get(last_flat).copied().unwrap_or(1);
        let file_end_vis = last_vis + last_count;
        let file_span = file_end_vis.saturating_sub(header_vis);

        // Header must be within the visible viewport to be considered
        // "far down to the bottom of the view".
        if header_vis < self.scroll || header_vis >= self.scroll + vh {
            return;
        }

        let screen_row = header_vis - self.scroll;
        let lines_below = vh - screen_row;

        let target_fill = (vh / 5).max(1);
        let desired_visible = target_fill.min(file_span);

        if lines_below < desired_visible {
            let needed_scroll_down = desired_visible - lines_below;
            let max_rows = vh.max(1);
            let max_scroll = total_vis_lines.saturating_sub(max_rows);
            self.scroll = (self.scroll + needed_scroll_down).min(max_scroll);

            // Keep cursor in view if it was above the new scroll position
            if self.row_idx < header_flat
                && starts.get(self.row_idx).copied().unwrap_or(0) < self.scroll
            {
                self.row_idx = header_flat;
                self.vis_line = 0;
            }
        }
    }
}
