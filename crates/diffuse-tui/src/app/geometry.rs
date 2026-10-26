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
                let Some(file_obj) = self.output.files.get(item.file_idx) else {
                    return 1;
                };
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
                let Some(file) = self.output.files.get(item.file_idx) else {
                    return 1;
                };
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

    /// Minimum scrollbar thumb height as a fraction of track height.
    pub const SCROLLBAR_MIN_PERCENT: f64 = 0.12;
    /// Absolute floor for scrollbar thumb height in visual lines.
    pub const SCROLLBAR_MIN_THUMB_LINES: usize = 2;

    /// Scrollbar thumb height for a track of height `track_h` over
    /// `total_vis_lines` visual lines with a `vh`-high viewport, or `None`
    /// when no scrollbar applies. Clamped to at least
    /// [`Self::SCROLLBAR_MIN_PERCENT`] of the track height and at least
    /// [`Self::SCROLLBAR_MIN_THUMB_LINES`].
    pub(crate) fn scrollbar_thumb_size(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
    ) -> Option<usize> {
        if total_vis_lines <= vh {
            return None;
        }
        let max_thumb = track_h.saturating_sub(1);
        if max_thumb < Self::SCROLLBAR_MIN_THUMB_LINES {
            return None;
        }
        let min_thumb = ((track_h as f64 * Self::SCROLLBAR_MIN_PERCENT).round() as usize)
            .clamp(Self::SCROLLBAR_MIN_THUMB_LINES, max_thumb);
        let prop_thumb = ((vh as f64 / total_vis_lines as f64) * track_h as f64).round() as usize;
        Some(prop_thumb.clamp(min_thumb, max_thumb))
    }

    /// Scroll delta per touch / wheel step on the scrollbar track.
    ///
    /// Computes the kinematic slope of the thumb travel range:
    /// each event moves by `max_scroll / span`, so traversing the track
    /// height in finger gestures navigates through the entire file at 1:1
    /// physical speed. Clamped to at least `min_step`.
    pub(crate) fn scrollbar_step(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        min_step: usize,
    ) -> usize {
        let max_scroll = total_vis_lines.saturating_sub(vh);
        if max_scroll == 0 || track_h <= 1 {
            return min_step;
        }
        let span =
            if let Some(thumb_size) = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines) {
                track_h.saturating_sub(thumb_size).max(1)
            } else {
                track_h.max(1)
            };
        ((max_scroll + span / 2) / span).max(min_step)
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
            let interior_min = header_vis;
            let interior_max = max_scroll.saturating_sub(1);
            if interior_max > interior_min {
                let t = (scroll.saturating_sub(interior_min) as f64)
                    / ((interior_max - interior_min) as f64);
                let num_buckets = span - 1;
                let bucket = ((t * num_buckets as f64).floor() as usize).min(span - 2);
                1 + bucket
            } else {
                1
            }
        } else {
            0
        }
        .min(span);
        Some((thumb_offset, thumb_size))
    }

    /// Continuous eighth-block subcell thumb geometry over a scrollbar track
    /// of height `track_h` for `scroll` within `total_vis_lines`.
    pub(crate) fn scrollbar_subcell_for(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        scroll: usize,
        _header_vis: usize,
    ) -> Option<ScrollbarSubcellThumb> {
        let thumb_size = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines)?;
        let max_scroll = total_vis_lines.saturating_sub(vh);
        let span = track_h.saturating_sub(thumb_size);
        let span_eighths = span * 8;
        let size_eighths = thumb_size * 8;

        let start_eighths = if max_scroll == 0 || span_eighths == 0 {
            0
        } else if scroll >= max_scroll {
            span_eighths
        } else if scroll == 0 {
            0
        } else if span_eighths >= 2 && max_scroll > 1 {
            let t = (scroll - 1) as f64 / (max_scroll - 1) as f64;
            let num_interior = span_eighths - 2;
            let step = (t * num_interior as f64).round() as usize;
            1 + step
        } else {
            1
        }
        .min(span_eighths);

        Some(ScrollbarSubcellThumb {
            start_eighths,
            size_eighths,
            track_h,
        })
    }

    /// Scroll position for a target thumb offset in `0..=span`.
    pub(crate) fn scrollbar_scroll_for_offset(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        header_vis: usize,
        target_offset: usize,
    ) -> usize {
        let max_scroll = total_vis_lines.saturating_sub(vh);
        if max_scroll == 0 || track_h <= 1 {
            return 0;
        }
        let Some(thumb_size) = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines) else {
            return 0;
        };
        let span = track_h.saturating_sub(thumb_size);
        if span == 0 || target_offset == 0 {
            return 0;
        }
        if target_offset >= span {
            return max_scroll;
        }
        let header_vis = header_vis.max(1);
        let interior_min = header_vis;
        let interior_max = max_scroll.saturating_sub(1);
        if interior_max > interior_min {
            let bucket = target_offset - 1;
            let num_buckets = span - 1;
            let t = (bucket as f64 + 0.5) / num_buckets as f64;
            interior_min + (t * (interior_max - interior_min) as f64).round() as usize
        } else {
            interior_min
        }
    }

    /// Scroll position for a scrollbar pointer at track row `row`: exact
    /// invertible mapping used identically by click-seek and drag.
    ///
    /// Respects the physical travel range `span = track_h - thumb_size`,
    /// ensuring that pointer row 0 yields scroll 0 (top of thumb at row 0),
    /// pointer row 1 yields thumb offset 1, the penultimate row yields
    /// `span - 1` (bottom of thumb at `track_h - 2`), and the last row
    /// yields `max_scroll` (bottom of thumb at `track_h - 1`). The thumb
    /// follows smoothly through [`Self::scrollbar_thumb_for`] with zero
    /// edge snapping.
    pub(crate) fn scrollbar_scroll_for_row(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        header_vis: usize,
        row: usize,
    ) -> usize {
        let max_scroll = total_vis_lines.saturating_sub(vh);
        if max_scroll == 0 || track_h <= 1 {
            return 0;
        }
        if max_scroll == 1 {
            return if row >= track_h / 2 { 1 } else { 0 };
        }
        let Some(thumb_size) = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines) else {
            return 0;
        };
        let span = track_h.saturating_sub(thumb_size);
        if span == 0 {
            return 0;
        }
        let r = row.min(track_h - 1);
        let target_offset = ((r as f64 / (track_h - 1) as f64) * span as f64).round() as usize;
        Self::scrollbar_scroll_for_offset(track_h, vh, total_vis_lines, header_vis, target_offset)
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
        crate::render::hunk_leading_gap(f, hi)
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

/// Standard Unicode lower block character for fraction `eighths` in 1..=8.
#[inline]
pub(crate) fn lower_block_glyph(eighths: u8) -> char {
    match eighths {
        1 => ' ', // U+2581 LOWER ONE EIGHTH BLOCK
        2 => '▂', // U+2582 LOWER ONE QUARTER BLOCK
        3 => '▃', // U+2583 LOWER THREE EIGHTHS BLOCK
        4 => '▄', // U+2584 LOWER HALF BLOCK
        5 => '▅', // U+2585 LOWER FIVE EIGHTHS BLOCK
        6 => '▆', // U+2586 LOWER THREE QUARTERS BLOCK
        7 => '▇', // U+2587 LOWER SEVEN EIGHTHS BLOCK
        8 => '█', // U+2588 FULL BLOCK
        _ => ' ',
    }
}

/// Visual classification and fractional decomposition of a single scrollbar
/// track cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollbarCell {
    /// Empty track cell (no thumb present).
    Track,
    /// Solid full thumb cell (all 8 eighths occupied).
    Full,
    /// Top edge of thumb entering from the bottom of this cell.
    /// `eighths` (1..=7) is the fraction filled by thumb at the cell bottom.
    TopEdge { eighths: u8 },
    /// Bottom edge of thumb exiting at the top of this cell.
    /// `eighths` (1..=7) is the fraction filled by thumb at the cell top.
    BottomEdge { eighths: u8 },
}

impl ScrollbarCell {
    /// Return the primary Unicode glyph representing this cell.
    #[must_use]
    pub(crate) fn glyph(self) -> char {
        match self {
            Self::Track => '│',
            Self::Full => '█',
            Self::TopEdge { eighths } => lower_block_glyph(eighths),
            Self::BottomEdge { eighths } => {
                if eighths == 4 {
                    '▀' // U+2580 UPPER HALF BLOCK
                } else {
                    lower_block_glyph(8 - eighths)
                }
            }
        }
    }

    /// Render this scrollbar cell into an ANSI SGR escape sequence string.
    pub(crate) fn render_ansi(self, palette: &crate::render::ThemePalette) -> String {
        use crate::render::ColorExt;
        let thumb_fg = palette.scrollbar_thumb.fg_code();
        let thumb_bg = palette.scrollbar_thumb.bg_code();
        let track_fg = palette.scrollbar_track.fg_code();
        let track_bg = palette.scrollbar_track.bg_code();
        let ch = self.glyph();

        match self {
            Self::Track => format!("{track_fg}{track_bg}{ch}\x1b[0m"),
            Self::Full => format!("{thumb_fg}{thumb_bg}{ch}\x1b[0m"),
            Self::TopEdge { .. } => format!("{thumb_fg}{track_bg}{ch}\x1b[0m"),
            Self::BottomEdge { eighths } => {
                if eighths == 4 {
                    format!("{thumb_fg}{track_bg}{ch}\x1b[0m")
                } else {
                    format!("{track_fg}{thumb_bg}{ch}\x1b[0m")
                }
            }
        }
    }

    /// Convert this cell into a styled Ratatui `Line`.
    pub(crate) fn to_ratatui_line(
        self,
        thumb_color: ratatui::style::Color,
        track_color: ratatui::style::Color,
    ) -> ratatui::text::Line<'static> {
        use ratatui::{
            style::Style,
            text::{Line, Span},
        };
        let ch = self.glyph().to_string();
        match self {
            Self::Track => Line::from(Span::styled(
                ch,
                Style::default().fg(track_color).bg(track_color),
            )),
            Self::Full => Line::from(Span::styled(
                ch,
                Style::default().fg(thumb_color).bg(thumb_color),
            )),
            Self::TopEdge { .. } => Line::from(Span::styled(
                ch,
                Style::default().fg(thumb_color).bg(track_color),
            )),
            Self::BottomEdge { eighths } => {
                if eighths == 4 {
                    Line::from(Span::styled(
                        ch,
                        Style::default().fg(thumb_color).bg(track_color),
                    ))
                } else {
                    Line::from(Span::styled(
                        ch,
                        Style::default().fg(track_color).bg(thumb_color),
                    ))
                }
            }
        }
    }
}

/// Continuous fractional geometry of the scrollbar thumb in eighths of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarSubcellThumb {
    /// Start position in eighths of a row (0 ..= span * 8).
    pub start_eighths: usize,
    /// Size of the thumb in eighths of a row (thumb_size * 8).
    pub size_eighths: usize,
    /// Total track height in terminal rows.
    pub track_h: usize,
}

impl ScrollbarSubcellThumb {
    /// Classify the scrollbar state for track row `r` (0 .. track_h).
    #[must_use]
    pub(crate) fn cell_at(&self, r: usize) -> ScrollbarCell {
        let c_start = r * 8;
        let c_end = (r + 1) * 8;
        let thumb_end = self.start_eighths + self.size_eighths;

        let ov_start = c_start.max(self.start_eighths);
        let ov_end = c_end.min(thumb_end);

        if ov_start >= ov_end {
            ScrollbarCell::Track
        } else if ov_start == c_start && ov_end == c_end {
            ScrollbarCell::Full
        } else if self.start_eighths > c_start {
            // Thumb begins inside this cell: thumb is in bottom fraction
            let frac = (c_end - self.start_eighths) as u8;
            ScrollbarCell::TopEdge { eighths: frac }
        } else {
            // Thumb ends inside this cell: thumb is in top fraction
            let frac = (thumb_end - c_start) as u8;
            ScrollbarCell::BottomEdge { eighths: frac }
        }
    }
}

#[cfg(test)]
mod tests;
