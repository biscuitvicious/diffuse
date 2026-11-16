//! Visual line height caching, geometry projection, and cursor coordinate
//! calculation.

use std::{collections::HashMap, panic::Location, time::Instant, vec::Vec};

use crate::{
    app::AppState,
    core::wire::OutputFile,
    render::{display_text, width::display_width},
    types::{FlatRow, FlatRowKind, LineNumTracking},
};
use diffuse_core::ModSide;

/// Cached visual layout: prefix sums of per-row visual heights plus the
/// note callouts folded into them. Any mutation of diff content, flat
/// layout, or notes must clear it via [`AppState::bump_content`] or
/// [`AppState::bump_notes`]; frame rendering then seeks the viewport by
/// binary search instead of remeasuring the whole content.
#[derive(Clone, Default)]
pub(crate) struct CachedGeometry {
    pub(crate) content_width: usize,
    pub(crate) old_line_num_width: usize,
    pub(crate) new_line_num_width: usize,
    pub(crate) show_old_line_gutter: Option<bool>,
    pub(crate) raw_diff: bool,
    pub(crate) col_offset: usize,
    pub(crate) word_diff: bool,
    pub(crate) side_by_side: bool,
    pub(crate) starts: Vec<usize>,
    pub(crate) counts: Vec<usize>,
    pub(crate) seg_counts: Vec<usize>,
    pub(crate) nav_counts: Vec<usize>,
    pub(crate) total_vis_lines: usize,
    pub(crate) callouts: HashMap<usize, Vec<ratatui::text::Line<'static>>>,
}

/// Visual width of the context expansion arrow gutter column in the TUI.
pub const EXPAND_GUTTER_WIDTH: usize = 1;

/// Minimum code text columns required for each pane individually to enable
/// side-by-side mode automatically in the interactive TUI.
pub const MIN_SIDE_BY_SIDE_CONTENT_COLS: usize = 60;

/// Minimum remaining content columns required for the old-line number gutter
/// to automatically activate.
pub const MIN_CONTENT_COLS_FOR_OLD_GUTTER: usize = 100;

impl AppState {
    /// Bump after any mutation of diff content (files, hunks, rows, flat
    /// layout): invalidates cached visual geometry.
    #[track_caller]
    pub(crate) fn bump_content(&mut self) {
        self.geo_cache.iter_mut().for_each(|slot| *slot = None);
        tracing::debug!(
            caller = %Location::caller(),
            "geometry cache invalidated (content)"
        );
    }

    /// Bump after any mutation of the review-notes database: note callouts
    /// feed row heights, invalidating cached visual geometry.
    #[track_caller]
    pub(crate) fn bump_notes(&mut self) {
        self.geo_cache.iter_mut().for_each(|slot| *slot = None);
        tracing::debug!(
            caller = %Location::caller(),
            "geometry cache invalidated (notes)"
        );
    }

    /// Returns the effective content width considering terminal width,
    /// scrollbar reservations, and last rendered content width.
    pub(crate) fn effective_content_width(&self) -> usize {
        if self.last_content_width > 0 {
            self.last_content_width
        } else if self.terminal_width > 0 {
            if self.raw_diff {
                self.terminal_width
            } else {
                self.terminal_width.saturating_sub(1)
            }
        } else {
            80
        }
    }

    /// Returns true if the old line number gutter is active for the given
    /// `content_width`.
    ///
    /// When `show_old_line_gutter` is `None` (auto), activates only when old
    /// lines exist (`old_line_num.width > 0`) and diff content still has at
    /// least 100 columns remaining after expanding the old-line gutter.
    /// When `Some(true)`, forces the old-line gutter visible as long as
    /// content width fits the expanded gutter.
    /// When `Some(false)`, forces the old-line gutter hidden.
    #[inline]
    pub fn is_old_line_gutter_active(&self, content_width: usize) -> bool {
        if self.old_line_num.width == 0 {
            return false;
        }
        let expanded_gutter_w = EXPAND_GUTTER_WIDTH
            + diffuse_render::dual_gutter_width(self.old_line_num.width, self.new_line_num.width);
        match self.show_old_line_gutter {
            Some(true) => content_width > expanded_gutter_w,
            Some(false) => false,
            None => {
                content_width.saturating_sub(expanded_gutter_w) >= MIN_CONTENT_COLS_FOR_OLD_GUTTER
            }
        }
    }

    /// Visual width of the line number prefix gutter for a specific content
    /// width.
    #[inline]
    pub fn gutter_width_for(&self, content_width: usize) -> usize {
        let old_w = if self.is_old_line_gutter_active(content_width) {
            self.old_line_num.width
        } else {
            0
        };
        diffuse_render::dual_gutter_width(old_w, self.new_line_num.width) + EXPAND_GUTTER_WIDTH
    }

    /// Visual width of the line number prefix gutter for current effective
    /// content width.
    #[inline]
    pub fn gutter_width(&self) -> usize {
        self.gutter_width_for(self.effective_content_width())
    }

    /// Returns true if `width` (the full terminal width in columns) provides at
    /// least [`MIN_SIDE_BY_SIDE_CONTENT_COLS`] text columns for both the left
    /// and right panes after accounting for the scrollbar, center divider, and
    /// both line number gutters.
    pub fn is_wide_enough_for_side_by_side(&self, width: usize) -> bool {
        let content_width = width.saturating_sub(1);
        let (left_w, right_w) = diffuse_render::split_pane_widths(content_width);
        let left_gutter_w =
            EXPAND_GUTTER_WIDTH + diffuse_render::gutter_width(self.old_line_num.width);
        let right_gutter_w = diffuse_render::gutter_width(self.new_line_num.width);
        let left_cols = left_w.saturating_sub(left_gutter_w);
        let right_cols = right_w.saturating_sub(right_gutter_w);
        left_cols >= MIN_SIDE_BY_SIDE_CONTENT_COLS && right_cols >= MIN_SIDE_BY_SIDE_CONTENT_COLS
    }

    /// Returns true if side-by-side (split) layout is currently active.
    ///
    /// Raw diff mode always displays in unified mode (not split), so this
    /// returns false when `raw_diff` is active even if `side_by_side` is
    /// set to true.
    #[inline]
    pub fn is_side_by_side(&self) -> bool {
        self.side_by_side && !self.raw_diff
    }

    /// Assert that `content_width` can accommodate the line number prefix
    /// gutter plus at least one column of code text; fails fast with an
    /// informative error otherwise.
    #[inline]
    pub fn assert_content_width_fits_gutter(&self, content_width: usize) {
        if self.is_side_by_side() {
            let (left_w, right_w) = diffuse_render::split_pane_widths(content_width);
            let left_gutter_w =
                EXPAND_GUTTER_WIDTH + diffuse_render::gutter_width(self.old_line_num.width);
            let right_gutter_w = diffuse_render::gutter_width(self.new_line_num.width);
            if left_w <= left_gutter_w || right_w <= right_gutter_w {
                panic!(
                    "terminal content width ({content_width}) is too narrow for side-by-side layout: panes require {left_gutter_w} and {right_gutter_w} columns for gutters"
                );
            }
            return;
        }
        let gutter_w = self.gutter_width_for(content_width);
        if content_width <= gutter_w {
            panic!(
                "terminal content width ({content_width}) is too narrow: line number gutter requires {gutter_w} columns (max line number is {}, width is {})",
                self.new_line_num.max_num, self.new_line_num.width
            );
        }
    }

    /// Recalculates the maximum old and new line numbers across all
    /// loaded/uncollapsed files in the grid by inspecting the tail of each
    /// file (O(files)).
    pub fn recalculate_line_num_width(&mut self) {
        let mut old_max: u32 = 0;
        let mut new_max: u32 = 0;
        for (fi, file) in self.output.files.iter().enumerate() {
            if self.collapsed_files.contains(&fi) {
                continue;
            }
            let (file_old_max, file_new_max) = Self::file_max_line_nums(file, self.raw_diff);
            if file_old_max > old_max {
                old_max = file_old_max;
            }
            if file_new_max > new_max {
                new_max = file_new_max;
            }
        }
        self.old_line_num.max_num = old_max;
        self.new_line_num.max_num = new_max;

        let new_old_width = LineNumTracking::compute_width(old_max, self.explicit_line_num_width);
        let new_new_width = LineNumTracking::compute_width(new_max, self.explicit_line_num_width);

        if new_old_width != self.old_line_num.width || new_new_width != self.new_line_num.width {
            let cw = self.effective_content_width();
            let cursor_screen_y = if !self.flat.is_empty() {
                let old_geo = self.cached_geometry(cw);
                let old_min_vis = old_geo.starts.get(self.row_idx).copied().unwrap_or(0);
                let old_nav = old_geo.nav_counts.get(self.row_idx).copied().unwrap_or(1);
                let old_cursor_vis = old_min_vis + self.vis_line.min(old_nav.saturating_sub(1));
                Some((old_cursor_vis as isize) - (self.scroll as isize))
            } else {
                None
            };
            self.old_line_num.width = new_old_width;
            self.new_line_num.width = new_new_width;
            self.bump_content();
            if let Some(screen_y) = cursor_screen_y {
                let new_geo = self.cached_geometry(cw);
                let new_min_vis = new_geo.starts.get(self.row_idx).copied().unwrap_or(0);
                let new_nav = new_geo.nav_counts.get(self.row_idx).copied().unwrap_or(1);
                let new_cursor_vis = new_min_vis + self.vis_line.min(new_nav.saturating_sub(1));
                let new_scroll = ((new_cursor_vis as isize) - screen_y).max(0) as usize;
                let max_rows = self.viewport_height.max(1);
                let max_scroll = new_geo.total_vis_lines.saturating_sub(max_rows);
                self.scroll = new_scroll.min(max_scroll);
            }
        }
    }

    /// Inspect the tail hunk header of a single file to determine its highest
    /// old and new line numbers directly without scanning any rows.
    pub fn file_max_line_nums(file: &OutputFile, raw_diff: bool) -> (u32, u32) {
        let hunks = if raw_diff && !file.raw_hunks.is_empty() {
            &file.raw_hunks
        } else {
            &file.hunks
        };
        if let Some(last_hunk) = hunks.last() {
            let old_max = if last_hunk.old_count > 0 {
                last_hunk.old_start + last_hunk.old_count - 1
            } else {
                0
            };
            let new_max = if last_hunk.new_count > 0 {
                last_hunk.new_start + last_hunk.new_count - 1
            } else {
                0
            };
            (old_max, new_max)
        } else {
            let old_count = file.old_line_count as u32;
            let new_count = file.new_line_count as u32;
            if old_count > 0 || new_count > 0 {
                (old_count, new_count)
            } else if let Some(last_row) = file.rows.last() {
                (
                    last_row.old_line.unwrap_or(0),
                    last_row.new_line.unwrap_or(0),
                )
            } else {
                (0, 0)
            }
        }
    }

    /// Inspect the tail hunk header of a single file to determine its highest
    /// line number directly without scanning any rows.
    pub fn file_max_line_num(file: &OutputFile, raw_diff: bool) -> u32 {
        let (old_max, new_max) = Self::file_max_line_nums(file, raw_diff);
        old_max.max(new_max)
    }

    /// Visual wrapped line segment count for row `idx` at `content_width`.
    #[inline]
    pub fn row_seg_count(&self, idx: usize, content_width: usize) -> usize {
        self.row_seg_count_with_gutter(idx, content_width, self.gutter_width_for(content_width))
    }

    /// Visual segment count for non-diff rows (file headers, hunk dividers,
    /// bottom gap dividers) that span the full viewport width regardless of
    /// layout mode.
    fn non_diff_seg_count(&self, item: &FlatRow, content_width: usize) -> Option<usize> {
        match item.kind {
            FlatRowKind::FileHeader => {
                let file_obj = self.output.files.get(item.file_idx)?;
                if self.raw_diff {
                    let (l1, l2) = crate::render::raw_file_header_lines(file_obj);
                    let text_cols = content_width.max(1);
                    let s1 = display_width(&l1).div_ceil(text_cols).max(1);
                    let s2 = display_width(&l2).div_ceil(text_cols).max(1);
                    Some(s1 + s2)
                } else {
                    let is_collapsed = self.collapsed_files.contains(&item.file_idx);
                    let txt = crate::view::diff_grid::file_header_text_with_prefix(
                        file_obj,
                        self.file_badge(item.file_idx),
                        is_collapsed,
                        self.output.common_directory_prefix().as_deref(),
                    );
                    let len = display_width(txt);
                    let text_cols = content_width.max(1);
                    Some(if len == 0 { 1 } else { len.div_ceil(text_cols) })
                }
            }
            FlatRowKind::HunkDivider { hunk_idx } => {
                let file = self.output.files.get(item.file_idx)?;
                let hunks = if !file.raw_hunks.is_empty() {
                    &file.raw_hunks
                } else {
                    &file.hunks
                };
                let h = &hunks[hunk_idx];
                let sep_text = format!(
                    "@@ -{},{} +{},{} @@",
                    h.old_start, h.old_count, h.new_start, h.new_count
                );
                let len = display_width(sep_text);
                let text_cols = content_width.max(1);
                Some(if len == 0 { 1 } else { len.div_ceil(text_cols) })
            }
            FlatRowKind::BottomGapDivider { .. } => {
                Some(self.bottom_gap_seg_count(item.file_idx, content_width))
            }
            _ => None,
        }
    }

    /// Visual wrapped line segment count for row `idx` given a pre-computed
    /// line number gutter width.
    pub(crate) fn row_seg_count_with_gutter(
        &self,
        idx: usize,
        content_width: usize,
        gutter_w: usize,
    ) -> usize {
        let Some(item) = self.flat.get(idx) else {
            return 1;
        };
        if let Some(segs) = self.non_diff_seg_count(item, content_width) {
            return segs;
        }
        let Some(r) = self.row_of(item) else {
            return 1;
        };
        let text = match item.kind {
            FlatRowKind::ExpandedMod {
                side: ModSide::Del, ..
            } => r.old_text(),
            FlatRowKind::ExpandedMod {
                side: ModSide::Add, ..
            } => r.new_text(),
            _ => r.text(),
        };
        if self.raw_diff {
            let text_cols = content_width.saturating_sub(gutter_w).max(1);
            let exp = crate::render::expand_tabs(&text, 4);
            let len = display_width(exp);
            if len == 0 { 1 } else { len.div_ceil(text_cols) }
        } else {
            let text_cols = content_width.saturating_sub(gutter_w).max(1);
            let disp = display_text(
                &text,
                r.is_aux(),
                r.kind,
                &r.runs,
                self.col_offset,
                self.word_diff,
            );
            let exp = crate::render::expand_tabs(&disp, 4);
            let len = display_width(exp);
            if len == 0 { 1 } else { len.div_ceil(text_cols) }
        }
    }

    /// Visual wrapped line segment count for row `idx` in side-by-side
    /// (dual-pane) layout given the respective content column budgets.
    pub(crate) fn row_seg_count_side_by_side(
        &self,
        idx: usize,
        content_width: usize,
        left_cols: usize,
        right_cols: usize,
    ) -> usize {
        let Some(item) = self.flat.get(idx) else {
            return 1;
        };
        if let Some(segs) = self.non_diff_seg_count(item, content_width) {
            return segs;
        }
        let Some(r) = self.row_of(item) else {
            return 1;
        };
        diffuse_render::side_by_side_row_seg_count(r, left_cols, right_cols, self.col_offset)
    }

    /// Computes full visual layout mapping: starts, counts, `seg_counts`, and
    /// total visual lines. Always freshly computed; frame rendering should
    /// prefer [`AppState::cached_geometry`].
    pub fn visual_geometry(
        &self,
        content_width: usize,
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>, usize) {
        let callouts = self.note_callouts(content_width);
        let (starts, counts, seg_counts, _nav_counts, total_vis_lines) =
            self.compute_geometry(content_width, &callouts);
        (starts, counts, seg_counts, total_vis_lines)
    }

    /// Number of navigable visual sub-lines for row `row_idx`: includes all
    /// wrapped code segments plus all note callout lines attached to this row.
    pub fn row_nav_vis_count(&mut self, row_idx: usize, content_width: usize) -> usize {
        let geo = self.cached_geometry(content_width);
        geo.nav_counts.get(row_idx).copied().unwrap_or(1)
    }

    /// Cached visual layout for frame rendering: recomputed only when the
    /// viewport width, diff-affecting view flags, content, or notes changed
    /// since the last call; otherwise a clone of the stored layout.
    pub(crate) fn cached_geometry(&mut self, content_width: usize) -> CachedGeometry {
        if let Some(hit) = self.geo_cache.iter().find_map(|slot| {
            slot.as_ref().filter(|g| {
                g.content_width == content_width
                    && g.old_line_num_width == self.old_line_num.width
                    && g.new_line_num_width == self.new_line_num.width
                    && g.show_old_line_gutter == self.show_old_line_gutter
                    && g.raw_diff == self.raw_diff
                    && g.col_offset == self.col_offset
                    && g.word_diff == self.word_diff
                    && g.side_by_side == self.is_side_by_side()
            })
        }) {
            tracing::trace!(content_width, rows = self.flat.len(), "geometry cache hit");
            return hit.clone();
        }
        let started = Instant::now();
        let callouts = self.note_callouts(content_width);
        let (starts, counts, seg_counts, nav_counts, total_vis_lines) =
            self.compute_geometry(content_width, &callouts);
        tracing::debug!(
            content_width,
            rows = self.flat.len(),
            note_rows = callouts.len(),
            total_vis_lines,
            elapsed_ms = started.elapsed().as_millis(),
            "geometry cache miss: recomputed layout"
        );
        let geo = CachedGeometry {
            content_width,
            old_line_num_width: self.old_line_num.width,
            new_line_num_width: self.new_line_num.width,
            show_old_line_gutter: self.show_old_line_gutter,
            raw_diff: self.raw_diff,
            col_offset: self.col_offset,
            word_diff: self.word_diff,
            side_by_side: self.is_side_by_side(),
            starts,
            counts,
            seg_counts,
            nav_counts,
            total_vis_lines,
            callouts,
        };
        // Two slots: keep the scrollbar pre-pass and viewport widths
        // resident so they never evict each other.
        self.geo_cache = [Some(geo.clone()), self.geo_cache[0].clone()];
        geo
    }

    /// Cached (starts, counts, seg_counts, total) tuple for callers that
    /// only need the layout numbers. Same cache as
    /// [`AppState::cached_geometry`].
    pub(crate) fn cached_geometry_tuple(
        &mut self,
        content_width: usize,
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>, usize) {
        let geo = self.cached_geometry(content_width);
        (geo.starts, geo.counts, geo.seg_counts, geo.total_vis_lines)
    }

    /// Shared layout computation over pre-resolved note callouts.
    pub(crate) fn compute_geometry(
        &self,
        content_width: usize,
        callouts: &HashMap<usize, Vec<ratatui::text::Line<'static>>>,
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>, usize) {
        self.assert_content_width_fits_gutter(content_width);

        let div_at: HashMap<usize, (usize, usize)> = if self.raw_diff {
            HashMap::new()
        } else {
            self.hunk_starts
                .iter()
                .map(|h| (h.flat, (h.file, h.hunk)))
                .collect()
        };

        let mut starts = Vec::with_capacity(self.flat.len());
        let mut counts = Vec::with_capacity(self.flat.len());
        let mut seg_counts = Vec::with_capacity(self.flat.len());
        let mut nav_counts = Vec::with_capacity(self.flat.len());
        let mut total_vis_lines = 0;
        let gutter_w = self.gutter_width_for(content_width);

        let (left_w, right_w) = diffuse_render::split_pane_widths(content_width);
        let left_gutter_w =
            EXPAND_GUTTER_WIDTH + diffuse_render::gutter_width(self.old_line_num.width);
        let right_gutter_w = diffuse_render::gutter_width(self.new_line_num.width);
        let left_cols = left_w.saturating_sub(left_gutter_w).max(1);
        let right_cols = right_w.saturating_sub(right_gutter_w).max(1);

        for (idx, _item) in self.flat.iter().enumerate() {
            starts.push(total_vis_lines);
            let div_lines = if let Some(&(div_fi, div_hi)) = div_at.get(&idx) {
                self.hunk_div_seg_count(div_fi, div_hi, content_width)
            } else {
                0
            };
            let row_segs = if self.is_side_by_side() {
                self.row_seg_count_side_by_side(idx, content_width, left_cols, right_cols)
            } else {
                self.row_seg_count_with_gutter(idx, content_width, gutter_w)
            };
            let note_lines = callouts.get(&idx).map_or(0, Vec::len);
            let item_total = div_lines + row_segs + note_lines;
            counts.push(item_total);
            seg_counts.push(row_segs.max(1));
            nav_counts.push(item_total.max(1));
            total_vis_lines += item_total;
        }

        (starts, counts, seg_counts, nav_counts, total_vis_lines)
    }

    /// Compute the exact visual line position of the cursor in the current
    /// layout.
    pub fn cursor_visual_line(&self) -> usize {
        let cw = self.effective_content_width();
        let callouts = self.note_callouts(cw);
        let (starts, _, _, nav_counts, _) = self.compute_geometry(cw, &callouts);
        let min_vis = starts.get(self.row_idx).copied().unwrap_or(0);
        let cur_row_nav = nav_counts.get(self.row_idx).copied().unwrap_or(1);
        min_vis + self.vis_line.min(cur_row_nav.saturating_sub(1))
    }
}
