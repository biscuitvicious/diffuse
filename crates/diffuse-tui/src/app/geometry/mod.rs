//! Geometry — pure width-dependent layout calculations.

use std::collections::HashMap;

use crate::{
    render::display_text,
    view::{bottom_gap_divider_text, file_header_text, header::visible_width, hunk_divider_text},
};

use super::AppState;

impl AppState {
    pub fn row_seg_count(&self, idx: usize, content_width: usize) -> usize {
        let Some(item) = self.flat.get(idx) else {
            return 1;
        };
        if item.is_file_header {
            return 1;
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
            let len = r.text.width();
            if len == 0 { 1 } else { len.div_ceil(text_cols) }
        } else {
            let text_cols = content_width.saturating_sub(6).max(1);
            let disp = display_text(
                &r.text,
                r.del_text.as_deref(),
                r.kind,
                r.moved,
                &r.runs,
                self.col_offset,
                self.word_diff,
            );
            let len = disp.width();
            if len == 0 { 1 } else { len.div_ceil(text_cols) }
        }
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
                if let Some(f) = self.output.files.get(div_fi)
                    && let Some(h) = f.hunks.get(div_hi)
                {
                    let gap = self.hunk_leading_gap(div_fi, div_hi);
                    let txt = hunk_divider_text(div_hi, h, gap);
                    let w = visible_width(&txt);
                    ((w + content_width - 1) / content_width.max(1)).max(1)
                } else {
                    1
                }
            } else {
                0
            };

            let header_lines = if item.is_file_header {
                let file = &self.output.files[item.file_idx];
                let is_collapsed = self.collapsed_files.contains(&item.file_idx);
                let txt = file_header_text(file, is_collapsed);
                let w = visible_width(&txt);
                ((w + content_width - 1) / content_width.max(1)).max(1)
            } else {
                0
            };

            let row_segs = if item.is_file_header {
                0
            } else {
                self.row_seg_count(idx, content_width)
            };
            let note_lines = callouts.get(&idx).map_or(0, std::vec::Vec::len);
            let trailing_gap_lines = if !self.raw_diff
                && !item.is_file_header
                && idx == self.last_row_of_file(item.file_idx)
                && self.hunk_trailing_gap(item.file_idx) > 0
            {
                let gap = self.hunk_trailing_gap(item.file_idx);
                let txt = bottom_gap_divider_text(gap);
                let w = visible_width(&txt);
                ((w + content_width - 1) / content_width.max(1)).max(1)
            } else {
                0
            };
            let item_total = div_lines + header_lines + row_segs + note_lines + trailing_gap_lines;
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
        for (i, item) in self.flat.iter().enumerate() {
            if div_at.contains_key(&i) {
                vis_i += 1;
            }
            if item.is_file_header {
                if i == self.row_idx {
                    return vis_i;
                }
                vis_i += 1;
            } else {
                let seg_count = self.row_seg_count(i, content_width);
                if i == self.row_idx {
                    return vis_i + self.vis_line.min(seg_count.saturating_sub(1));
                }
                vis_i += seg_count;
            }
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
