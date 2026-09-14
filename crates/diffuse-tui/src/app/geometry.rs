//! Geometry — pure width-dependent layout calculations.

use std::collections::HashMap;

use crate::render::display_text;

use super::AppState;

impl AppState {
    pub fn row_seg_count(&self, idx: usize, content_width: usize) -> usize {
        let Some(item) = self.flat.get(idx) else {
            return 1;
        };
        if item.is_file_header {
            let file_obj = &self.output.files[item.file_idx];
            let is_collapsed = self.collapsed_files.contains(&item.file_idx);
            let txt = crate::view::diff_grid::file_header_text_with_prefix(
                file_obj,
                is_collapsed,
                self.output.common_directory_prefix().as_deref(),
            );
            use unicode_width::UnicodeWidthStr;
            let len = txt.width();
            let text_cols = content_width.max(1);
            return if len == 0 { 1 } else { len.div_ceil(text_cols) };
        }
        if item.is_hunk_divider {
            use unicode_width::UnicodeWidthStr;
            let hunks = if !self.output.files[item.file_idx].raw_hunks.is_empty() {
                &self.output.files[item.file_idx].raw_hunks
            } else {
                &self.output.files[item.file_idx].hunks
            };
            let h = &hunks[item.hunk_idx];
            let sep_text = format!(
                "@@ -{},{} +{},{} @@",
                h.old_start, h.old_count, h.new_start, h.new_count
            );
            let len = sep_text.width();
            let text_cols = content_width.max(1);
            return if len == 0 { 1 } else { len.div_ceil(text_cols) };
        }
        let Some(r) = self.row_of(item) else {
            return 1;
        };
        use unicode_width::UnicodeWidthStr;
        if self.raw_diff {
            let text_cols = content_width.saturating_sub(6).max(1);
            let exp = crate::render::expand_tabs(&r.text, 4);
            let len = exp.width();
            if len == 0 { 1 } else { len.div_ceil(text_cols) }
        } else {
            let text_cols = content_width.saturating_sub(6).max(1);
            let disp = display_text(
                &r.text,
                r.del_text.as_deref(),
                r.kind,
                &r.runs,
                self.col_offset,
                self.word_diff,
            );
            let exp = crate::render::expand_tabs(&disp, 4);
            let len = exp.width();
            if len == 0 { 1 } else { len.div_ceil(text_cols) }
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
        let txt = crate::view::diff_grid::hunk_divider_text(hi, hunk, hidden_gap);
        use unicode_width::UnicodeWidthStr;
        let len = txt.width();
        let text_cols = content_width.max(1);
        if len == 0 { 1 } else { len.div_ceil(text_cols) }
    }

    /// Returns the number of visual lines taken by a trailing gap divider at
    /// `content_width`.
    pub fn bottom_gap_seg_count(&self, fi: usize, content_width: usize) -> usize {
        let gap = self.hunk_trailing_gap(fi);
        if gap == 0 {
            return 0;
        }
        let txt = crate::view::diff_grid::bottom_gap_divider_text(gap);
        use unicode_width::UnicodeWidthStr;
        let len = txt.width();
        let text_cols = content_width.max(1);
        if len == 0 { 1 } else { len.div_ceil(text_cols) }
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
    /// total visual lines.
    pub fn visual_geometry(
        &self,
        content_width: usize,
    ) -> (Vec<usize>, Vec<usize>, Vec<usize>, usize) {
        let callouts = self.note_callouts();
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
        let mut total_vis_lines = 0;

        for (idx, item) in self.flat.iter().enumerate() {
            starts.push(total_vis_lines);
            let div_lines = if let Some(&(div_fi, div_hi)) = div_at.get(&idx) {
                self.hunk_div_seg_count(div_fi, div_hi, content_width)
            } else {
                0
            };
            let row_segs = self.row_seg_count(idx, content_width);
            let note_lines = callouts.get(&idx).map_or(0, std::vec::Vec::len);
            let trailing_gap_lines = if !self.raw_diff
                && !item.is_file_header
                && idx == self.last_row_of_file(item.file_idx)
            {
                self.bottom_gap_seg_count(item.file_idx, content_width)
            } else {
                0
            };
            let item_total = div_lines + row_segs + note_lines + trailing_gap_lines;
            counts.push(item_total);
            seg_counts.push(row_segs.max(1));
            total_vis_lines += item_total;
        }

        (starts, counts, seg_counts, total_vis_lines)
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
            if item.file_idx != fi || item.is_file_header {
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
        let div_at: HashMap<usize, (usize, usize)> = if self.raw_diff {
            HashMap::new()
        } else {
            self.hunk_starts
                .iter()
                .map(|h| (h.flat, (h.file, h.hunk)))
                .collect()
        };
        let callouts = self.note_callouts();
        let width = if self.terminal_width > 0 {
            self.terminal_width
        } else if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let content_width = if self.raw_diff {
            width
        } else {
            width.saturating_sub(1)
        };

        let mut vis_i = 0;
        for (i, _item) in self.flat.iter().enumerate() {
            if let Some(&(div_fi, div_hi)) = div_at.get(&i) {
                vis_i += self.hunk_div_seg_count(div_fi, div_hi, content_width);
            }
            let seg_count = self.row_seg_count(i, content_width);
            if i == self.row_idx {
                return vis_i + self.vis_line.min(seg_count.saturating_sub(1));
            }
            vis_i += seg_count;
            vis_i += callouts.get(&i).map_or(0, Vec::len);
        }
        vis_i
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
        let Some(f) = self.output.files.get(fi) else {
            return 0;
        };
        let Some(last_hunk) = f.hunks.last() else {
            return 0;
        };
        let last_end = last_hunk
            .rows
            .iter()
            .rev()
            .find_map(|r| r.new_line.or(r.old_line))
            .unwrap_or_else(|| {
                if last_hunk.new_count > 0 {
                    last_hunk.new_start + last_hunk.new_count.saturating_sub(1)
                } else {
                    last_hunk.old_start + last_hunk.old_count.saturating_sub(1)
                }
            });
        let total_lines = f.new_line_count.max(f.old_line_count) as u32;
        total_lines.saturating_sub(last_end) as usize
    }
}

#[cfg(test)]
mod tests;
