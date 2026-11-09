//! Visual line height caching, geometry projection, and cursor coordinate
//! calculation.

use std::{collections::HashMap, panic::Location, time::Instant, vec::Vec};

use crate::{
    render::{display_text, width::display_width},
    types::FlatRowKind,
};
use diffuse_core::ModSide;

use crate::app::AppState;

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
    pub(crate) callouts: HashMap<usize, Vec<ratatui::text::Line<'static>>>,
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
                    self.file_badge(item.file_idx),
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
            FlatRowKind::DiffRow { .. } | FlatRowKind::ExpandedMod { .. } => {
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
        callouts: &HashMap<usize, Vec<ratatui::text::Line<'static>>>,
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
