//! Geometry — pure width-dependent layout calculations.

use std::{collections::HashMap, panic::Location, time::Instant, vec::Vec};

use crate::{
    render::{display_text, width::display_width},
    types::FlatRowKind,
};

use super::AppState;

/// Cached visual layout: prefix sums of per-row visual heights plus the
/// note callouts folded into them. Any mutation of diff content, flat
/// layout, or notes must clear it via [`AppState::bump_content`] or
/// [`AppState::bump_notes`]; frame rendering then seeks the viewport by
/// binary search instead of remeasuring the whole content.
#[derive(Clone, Default)]
pub(crate) struct CachedGeometry {
    pub(crate) content_width: usize,
    pub(crate) raw_diff: bool,
    pub(crate) col_offset: usize,
    pub(crate) word_diff: bool,
    pub(crate) starts: Vec<usize>,
    pub(crate) counts: Vec<usize>,
    pub(crate) seg_counts: Vec<usize>,
    pub(crate) nav_counts: Vec<usize>,
    pub(crate) total_vis_lines: usize,
    pub(crate) callouts: HashMap<usize, Vec<String>>,
}

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

    pub fn row_seg_count(&self, idx: usize, content_width: usize) -> usize {
        let Some(item) = self.flat.get(idx) else {
            return 1;
        };
        match item.kind {
            FlatRowKind::FileHeader => {
                let file_obj = &self.output.files[item.file_idx];
                let is_collapsed = self.collapsed_files.contains(&item.file_idx);
                let txt = crate::view::diff_grid::file_header_text_with_prefix(
                    file_obj,
                    crate::render::Badge {
                        idx: item.file_idx,
                        total: self.output.files.len(),
                    },
                    is_collapsed,
                    self.output.common_directory_prefix().as_deref(),
                );
                let len = display_width(txt);
                let text_cols = content_width.max(1);
                if len == 0 { 1 } else { len.div_ceil(text_cols) }
            }
            FlatRowKind::HunkDivider { hunk_idx } => {
                let hunks = if !self.output.files[item.file_idx].raw_hunks.is_empty() {
                    &self.output.files[item.file_idx].raw_hunks
                } else {
                    &self.output.files[item.file_idx].hunks
                };
                let h = &hunks[hunk_idx];
                let sep_text = format!(
                    "@@ -{},{} +{},{} @@",
                    h.old_start, h.old_count, h.new_start, h.new_count
                );
                let len = display_width(sep_text);
                let text_cols = content_width.max(1);
                if len == 0 { 1 } else { len.div_ceil(text_cols) }
            }
            FlatRowKind::BottomGapDivider { .. } => {
                self.bottom_gap_seg_count(item.file_idx, content_width)
            }
            FlatRowKind::DiffRow { .. } => {
                let Some(r) = self.row_of(item) else {
                    return 1;
                };
                let text = r.text();
                if self.raw_diff {
                    let text_cols = content_width.saturating_sub(6).max(1);
                    let exp = crate::render::expand_tabs(&text, 4);
                    let len = display_width(exp);
                    if len == 0 { 1 } else { len.div_ceil(text_cols) }
                } else {
                    let text_cols = content_width.saturating_sub(6).max(1);
                    let del_text = r.del_text();
                    let disp = display_text(
                        &text,
                        del_text.as_deref(),
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
        }
    }

    /// Returns the number of visual lines taken by a hunk divider at
    /// `content_width`.
    pub fn hunk_div_seg_count(&self, fi: usize, hi: usize, content_width: usize) -> usize {
        let Some(f) = self.output.files.get(fi) else {
            return 1;
        };
        let Some(hunk) = f.hunks.get(hi) else {
            return 1;
        };
        let hidden_gap = self.hunk_leading_gap(fi, hi);
        let txt = crate::view::diff_grid::hunk_divider_text(
            crate::render::Badge {
                idx: hi,
                total: f.hunks.len(),
            },
            hunk,
            hidden_gap,
        );
        crate::view::diff_grid::divider_seg_count(&txt, content_width)
    }

    /// Returns the number of visual lines taken by a trailing gap divider at
    /// `content_width`.
    pub fn bottom_gap_seg_count(&self, fi: usize, content_width: usize) -> usize {
        let gap = self.hunk_trailing_gap(fi);
        if gap == 0 {
            return 0;
        }
        let txt = crate::view::diff_grid::bottom_gap_divider_text(gap);
        crate::view::diff_grid::divider_seg_count(&txt, content_width)
    }

    /// Computes the visual line offset of the bottom gap divider of `file[fi]`
    /// from pre-calculated layout arrays.
    pub(crate) fn bottom_divider_vis_line_from_layout(
        &self,
        fi: usize,
        starts: &[usize],
    ) -> Option<usize> {
        if self.raw_diff || self.hunk_trailing_gap(fi) == 0 {
            return None;
        }
        let bot_row = self
            .flat
            .iter()
            .position(|r| r.file_idx == fi && r.is_bottom_gap_divider())?;
        starts.get(bot_row).copied()
    }

    /// Visual line offset where the bottom (trailing gap) divider of `file[fi]`
    /// starts, or `None` if the file has no trailing gap or raw diff mode is
    /// active.
    pub fn bottom_divider_vis_line(&self, fi: usize, content_width: usize) -> Option<usize> {
        let callouts = self.note_callouts(content_width);
        let (starts, _, _, _, _) = self.compute_geometry(content_width, &callouts);
        self.bottom_divider_vis_line_from_layout(fi, &starts)
    }

    /// Visual line offset where hunk divider `hi` of `file[fi]` starts.
    /// If `hi >= hunks.len()`, targets the bottom gap divider of `file[fi]`.
    pub fn hunk_divider_vis_line(
        &self,
        fi: usize,
        hi: usize,
        content_width: usize,
    ) -> Option<usize> {
        let is_bottom = self
            .output
            .files
            .get(fi)
            .is_some_and(|f| hi >= f.hunks.len());
        if is_bottom {
            return self.bottom_divider_vis_line(fi, content_width);
        }
        let hs = self
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == hi)?;
        let (starts, _, _, _) = self.visual_geometry(content_width);
        starts.get(hs.flat).copied()
    }

    /// Scrollbar thumb height for a track of height `track_h` over
    /// `total_vis_lines` visual lines with a `vh`-high viewport, or `None`
    /// when no scrollbar applies. Minimum 2 rows.
    pub(crate) fn scrollbar_thumb_size(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
    ) -> Option<usize> {
        if track_h <= 2 || total_vis_lines <= vh {
            return None;
        }
        let max_thumb = track_h.saturating_sub(1).max(2);
        Some(
            ((vh as f64 / total_vis_lines as f64) * track_h as f64)
                .round()
                .clamp(2.0, max_thumb as f64) as usize,
        )
    }

    /// Thumb `(offset, size)` for a scrollbar track of height `track_h`
    /// over `total_vis_lines` visual lines with a `vh`-high viewport at
    /// the given scroll position, or `None` when no scrollbar applies.
    /// `header_vis` is the first file header's visual height: the thumb
    /// leaves the top row once the header scrolls out. Single source of
    /// truth shared by the renderer and mouse hit-testing so the drawn
    /// thumb and the grab area can never disagree.
    pub(crate) fn scrollbar_thumb_for(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        scroll: usize,
        header_vis: usize,
    ) -> Option<(usize, usize)> {
        let thumb_size = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines)?;
        let max_scroll = total_vis_lines.saturating_sub(vh);
        // The thumb touches the first track row only while the first
        // file's header is in view, and the last track row only while the
        // final line is in view; interior scroll positions stay strictly
        // inside the span.
        let span = track_h.saturating_sub(thumb_size);
        let header_vis = header_vis.max(1);
        let thumb_offset = if max_scroll == 0 || scroll >= max_scroll {
            span
        } else if scroll < header_vis {
            0
        } else if span >= 2 {
            (1.0 + ((scroll - 1) as f64 / (max_scroll - 1) as f64) * (span - 2) as f64).floor()
                as usize
        } else {
            ((scroll as f64 / max_scroll as f64) * span as f64).floor() as usize
        }
        .min(span);
        Some((thumb_offset, thumb_size))
    }

    /// Scroll position for a scrollbar pointer at track row `row`: one
    /// exact mapping used identically by click-seek and drag —
    /// proportional throughout:
    ///
    /// ```text
    /// scroll = round(clamp(row, 0, H−1) · max_scroll / (H−1))
    /// ```
    ///
    /// Pointer row 0 always yields scroll 0 and the last track row always
    /// yields `max_scroll`; everything between is proportional with no
    /// flats, so the thumb moves on every pointer row and never sticks or
    /// snaps. The thumb follows through [`Self::scrollbar_thumb_for`],
    /// whose endpoint pins make pointer extremes, scroll extremes, and
    /// thumb extremes coincide exactly.
    pub(crate) fn scrollbar_scroll_for_row(track_h: usize, max_scroll: usize, row: usize) -> usize {
        if max_scroll == 0 || track_h <= 1 {
            return 0;
        }
        if max_scroll == 1 {
            return 0;
        }
        let r = row.min(track_h - 1);
        ((r as f64 / (track_h - 1) as f64) * max_scroll as f64).round() as usize
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
                    && g.raw_diff == self.raw_diff
                    && g.col_offset == self.col_offset
                    && g.word_diff == self.word_diff
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
            raw_diff: self.raw_diff,
            col_offset: self.col_offset,
            word_diff: self.word_diff,
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
        callouts: &HashMap<usize, Vec<String>>,
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>, usize) {
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

        for (idx, _item) in self.flat.iter().enumerate() {
            starts.push(total_vis_lines);
            let div_lines = if let Some(&(div_fi, div_hi)) = div_at.get(&idx) {
                self.hunk_div_seg_count(div_fi, div_hi, content_width)
            } else {
                0
            };
            let row_segs = self.row_seg_count(idx, content_width);
            let note_lines = callouts.get(&idx).map_or(0, Vec::len);
            let item_total = div_lines + row_segs + note_lines;
            counts.push(item_total);
            seg_counts.push(row_segs.max(1));
            nav_counts.push(item_total.max(1));
            total_vis_lines += item_total;
        }

        (starts, counts, seg_counts, nav_counts, total_vis_lines)
    }

    /// Marks all rows belonging to the current hunk envelope for gutter
    /// highlight.
    pub(crate) fn hunk_ctx_extent_rows(&self) -> Vec<bool> {
        let mut set = vec![false; self.flat.len()];
        let Some(hi) = self.current_hunk() else {
            return set;
        };
        let fi = self.current_file_idx();
        let f = self
            .output
            .files
            .get(fi)
            .expect("current file index out of bounds in output.files");
        let h = f
            .hunks
            .get(hi)
            .expect("current hunk index out of bounds in f.hunks");
        let old_lo = h.old_start;
        let old_hi = h.old_start + h.old_count;
        let new_lo = h.new_start;
        let new_hi = h.new_start + h.new_count;

        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != fi || !item.is_diff_row() {
                continue;
            }
            let Some(r) = self.row_of(item) else {
                continue;
            };
            if let Some(nl) = r.new_line
                && nl >= new_lo
                && nl < new_hi
            {
                set[i] = true;
                continue;
            }
            if let Some(ol) = r.old_line
                && ol >= old_lo
                && ol < old_hi
            {
                set[i] = true;
            }
        }
        set
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

    /// Return the number of hidden lines preceding hunk `hi` in file `fi`.
    pub fn hunk_leading_gap(&self, fi: usize, hi: usize) -> usize {
        let Some(f) = self.output.files.get(fi) else {
            return 0;
        };
        let Some(hunk) = f.hunks.get(hi) else {
            return 0;
        };
        let hunk_new_start = hunk.rows.iter().find_map(|r| r.new_line);
        let hunk_old_start = hunk.rows.iter().find_map(|r| r.old_line);

        if hi == 0 {
            if let Some(ns) = hunk_new_start {
                ns.saturating_sub(1) as usize
            } else if let Some(os) = hunk_old_start {
                os.saturating_sub(1) as usize
            } else {
                hunk.new_start.max(hunk.old_start).saturating_sub(1) as usize
            }
        } else {
            let prev = &f.hunks[hi - 1];
            let prev_new_end = prev.rows.iter().rev().find_map(|r| r.new_line);
            let prev_old_end = prev.rows.iter().rev().find_map(|r| r.old_line);

            if let (Some(cur_new), Some(prev_new)) = (hunk_new_start, prev_new_end) {
                cur_new.saturating_sub(prev_new + 1) as usize
            } else if let (Some(cur_old), Some(prev_old)) = (hunk_old_start, prev_old_end) {
                cur_old.saturating_sub(prev_old + 1) as usize
            } else {
                let cur_s = hunk.new_start.max(hunk.old_start);
                let prev_e = if prev.new_count > 0 {
                    prev.new_start + prev.new_count.saturating_sub(1)
                } else {
                    prev.old_start + prev.old_count.saturating_sub(1)
                };
                cur_s.saturating_sub(prev_e + 1) as usize
            }
        }
    }

    /// Return the number of hidden trailing lines after the last hunk in file
    /// `fi`.
    pub fn hunk_trailing_gap(&self, fi: usize) -> usize {
        self.output
            .files
            .get(fi)
            .map_or(0, crate::app::flat::file_trailing_gap)
    }
}

#[cfg(test)]
mod tests;
