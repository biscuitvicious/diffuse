use crate::app::AppState;

impl AppState {
    pub(crate) fn on_mouse_scroll(&mut self, up: bool, n: usize, row: usize, col: usize) {
        let cw = self.last_content_width;
        let track_h = self.screen_height;
        let vh = self.viewport_height;
        tracing::debug!(
            up,
            row,
            col,
            n,
            is_dragging_scrollbar = self.is_dragging_scrollbar,
            "on_mouse_scroll"
        );
        let (_, _, _, total_vis) = self.cached_geometry_tuple(cw);
        let has_scrollbar = !self.raw_diff && total_vis > vh && vh > 2;
        if has_scrollbar && col >= cw.saturating_sub(1) && row < track_h {
            let step = Self::scrollbar_step(track_h, vh, total_vis, n);
            if up {
                self.scroll_to_visual(self.scroll + step);
            } else {
                self.scroll_to_visual(self.scroll.saturating_sub(step));
            }
        } else if up {
            self.scroll_to_visual(self.scroll.saturating_sub(n));
        } else {
            self.scroll_to_visual(self.scroll + n);
        }
    }

    pub(crate) fn on_scrollbar_click(&mut self, row: usize, col: usize) {
        let cw = self.last_content_width;
        let track_h = self.screen_height;
        let vh = self.viewport_height;
        self.is_dragging_scrollbar = true;
        let (_starts, counts, _seg, total_vis) = self.cached_geometry_tuple(cw);
        let header_vis = counts.first().copied().unwrap_or(1);
        if let Some((thumb_off, thumb_size)) =
            Self::scrollbar_thumb_for(track_h, vh, total_vis, self.scroll, header_vis)
        {
            if row >= thumb_off && row < thumb_off + thumb_size {
                // Clicked directly on the thumb: preserve current scroll
                // position and record the relative grab offset within the
                // thumb.
                self.scrollbar_drag_grab = Some(row - thumb_off);
                tracing::debug!(
                    row,
                    col,
                    thumb_off,
                    thumb_size,
                    grab = row - thumb_off,
                    "scrollbar thumb grab (no snap)"
                );
            } else {
                // Clicked on the track outside the thumb: snap immediately.
                let target_vis =
                    Self::scrollbar_scroll_for_row(track_h, vh, total_vis, header_vis, row);
                tracing::debug!(row, col, target_vis, "scrollbar track seek");
                self.scroll_to_visual(target_vis);
                if let Some((new_off, new_size)) =
                    Self::scrollbar_thumb_for(track_h, vh, total_vis, target_vis, header_vis)
                {
                    let grab = row.saturating_sub(new_off).min(new_size.saturating_sub(1));
                    self.scrollbar_drag_grab = Some(grab);
                } else {
                    self.scrollbar_drag_grab = None;
                }
            }
        } else {
            let target_vis =
                Self::scrollbar_scroll_for_row(track_h, vh, total_vis, header_vis, row);
            tracing::debug!(row, col, target_vis, "scrollbar mousedown");
            self.scroll_to_visual(target_vis);
            self.scrollbar_drag_grab = None;
        }
    }

    pub(crate) fn on_scrollbar_drag(&mut self, row: usize) {
        let cw = self.last_content_width;
        let track_h = self.screen_height;
        let vh = self.viewport_height;
        let (_, counts, _, total_vis) = self.cached_geometry_tuple(cw);
        let header_vis = counts.first().copied().unwrap_or(1);
        let target_vis = if let Some(grab) = self.scrollbar_drag_grab {
            let target_offset = row.saturating_sub(grab);
            Self::scrollbar_scroll_for_offset(track_h, vh, total_vis, header_vis, target_offset)
        } else {
            Self::scrollbar_scroll_for_row(track_h, vh, total_vis, header_vis, row)
        };
        tracing::debug!(
            row,
            target_vis,
            grab = self.scrollbar_drag_grab,
            prev_scroll = self.scroll,
            "scrollbar drag"
        );
        self.scroll_to_visual(target_vis);
    }

    pub(crate) fn on_mouse_drag_end(&mut self) {
        tracing::debug!(
            is_dragging_scrollbar = self.is_dragging_scrollbar,
            "EndMouseDrag"
        );
        self.is_dragging_scrollbar = false;
        self.scrollbar_drag_grab = None;
        if let Some(anchor) = self.anchor {
            if anchor != self.row_idx {
                self.confirmed_sel = self.compute_selection();
            }
            self.anchor = None;
        }
    }
}
