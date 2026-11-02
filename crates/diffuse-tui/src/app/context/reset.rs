//! Context reset, slicing, and divider hit testing.

use crate::{
    app::AppState,
    core::{DEFAULT_CONTEXT_LINES, parse::RowKind, wire::OutputHunk},
    types::FlatRowKind,
};

impl AppState {
    /// Expand ALL hidden lines across all hunks in the repository.
    pub fn expand_all_hidden(&mut self) {
        let cursor_screen_y: isize = (self.cursor_visual_line() as isize) - (self.scroll as isize);
        for fi in 0..self.output.files.len() {
            let mut made_progress = true;
            while made_progress {
                made_progress = false;
                let num_hunks = self.output.files[fi].hunks.len();
                for hi in (0..num_hunks).rev() {
                    if self.expand_hunk_above(fi, hi) {
                        made_progress = true;
                    }
                    if self.expand_hunk_below(fi, hi) {
                        made_progress = true;
                    }
                }
            }
        }
        let (flat, hunk_starts) = Self::build_visual_flat(&self.output, &self.collapsed_files);
        self.flat = flat;
        self.bump_content();
        self.hunk_starts = hunk_starts;
        self.retain_cursor_visual_position(cursor_screen_y);
    }

    /// Reset the current hunk's context lines back to the default width (4),
    /// splitting it back into separate hunks with dividers if any interior
    /// context run exceeds 2 * DEFAULT_CONTEXT_LINES.
    pub fn reset_current_hunk(&mut self) -> bool {
        let fi = self.current_file_idx();
        let Some(hi) = self.current_hunk() else {
            return false;
        };
        self.reset_hunk_at(fi, hi)
    }

    /// Reset hunk `hi` in file `fi`.
    pub fn reset_hunk_at(&mut self, fi: usize, hi: usize) -> bool {
        let cursor_screen_y: isize = (self.cursor_visual_line() as isize) - (self.scroll as isize);
        let Some(f) = self.output.files.get_mut(fi) else {
            return false;
        };
        if hi >= f.hunks.len() {
            return false;
        }

        let old_hunk = f.hunks.remove(hi);
        let new_hunks = Self::reset_hunk_slices(&old_hunk, DEFAULT_CONTEXT_LINES);
        let count = new_hunks.len();
        f.hunks.splice(hi..hi, new_hunks);

        // Rebuild f.rows from f.hunks
        f.rows = f.hunks.iter().flat_map(|h| h.rows.clone()).collect();

        // Rebuild hunk_meta for file
        if fi < self.hunk_meta.len() {
            let mut new_meta = Vec::with_capacity(f.hunks.len());
            for h in &f.hunks {
                let (above, below) = Self::count_output_hunk_ctx(h);
                new_meta.push(crate::types::HunkMeta {
                    above: above as usize,
                    below: below as usize,
                });
            }
            self.hunk_meta[fi] = new_meta;
        }

        let (flat, hunk_starts) = Self::build_visual_flat(&self.output, &self.collapsed_files);
        self.flat = flat;
        self.bump_content();
        self.hunk_starts = hunk_starts;
        self.retain_cursor_visual_position(cursor_screen_y);
        count > 0
    }

    /// Reset all hunks in all files back to the default context width.
    pub fn reset_all_context(&mut self) {
        let cursor_screen_y: isize = (self.cursor_visual_line() as isize) - (self.scroll as isize);
        for fi in 0..self.output.files.len() {
            let num_hunks = self.output.files[fi].hunks.len();
            for hi in (0..num_hunks).rev() {
                self.reset_hunk_at(fi, hi);
            }
        }
        let (flat, hunk_starts) = Self::build_visual_flat(&self.output, &self.collapsed_files);
        self.flat = flat;
        self.bump_content();
        self.hunk_starts = hunk_starts;
        self.retain_cursor_visual_position(cursor_screen_y);
    }

    /// Split and trim an `OutputHunk`'s rows around change blocks so that every
    /// change has at most `default_ctx` surrounding context lines, and any
    /// interior context run > 2 * default_ctx forms a new hunk divider gap.
    pub fn reset_hunk_slices(hunk: &OutputHunk, default_ctx: usize) -> Vec<OutputHunk> {
        let change_indices: Vec<usize> = hunk
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind != RowKind::Ctx)
            .map(|(i, _)| i)
            .collect();

        if change_indices.is_empty() {
            let mut trimmed_rows = hunk.rows.clone();
            if trimmed_rows.len() > default_ctx {
                trimmed_rows.truncate(default_ctx);
            }
            return vec![OutputHunk {
                old_start: hunk.old_start,
                old_count: trimmed_rows.len() as u32,
                new_start: hunk.new_start,
                new_count: trimmed_rows.len() as u32,
                rows: trimmed_rows,
            }];
        }

        let mut hunk_slices: Vec<(usize, usize)> = Vec::new();
        let first_change = change_indices[0];
        let mut current_hunk_start = first_change.saturating_sub(default_ctx);

        for pair in change_indices.windows(2) {
            let prev_change = pair[0];
            let next_change = pair[1];
            let ctx_between = next_change.saturating_sub(prev_change + 1);

            if ctx_between > 2 * default_ctx {
                let current_hunk_end = (prev_change + default_ctx).min(next_change - 1);
                hunk_slices.push((current_hunk_start, current_hunk_end));
                current_hunk_start = next_change.saturating_sub(default_ctx);
            }
        }

        let last_change = *change_indices.last().unwrap();
        let last_hunk_end = (last_change + default_ctx).min(hunk.rows.len().saturating_sub(1));
        hunk_slices.push((current_hunk_start, last_hunk_end));

        let mut new_hunks = Vec::new();
        for (s_idx, e_idx) in hunk_slices {
            let rows = hunk.rows[s_idx..=e_idx].to_vec();
            let first_row = rows.first().unwrap();
            let last_row = rows.last().unwrap();
            let old_start = first_row.old_line.unwrap_or(hunk.old_start);
            let new_start = first_row.new_line.unwrap_or(hunk.new_start);
            let old_end = last_row.old_line.unwrap_or(old_start);
            let new_end = last_row.new_line.unwrap_or(new_start);
            let old_count = old_end.saturating_sub(old_start) + 1;
            let new_count = new_end.saturating_sub(new_start) + 1;

            new_hunks.push(OutputHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                rows,
            });
        }

        new_hunks
    }

    pub(crate) fn count_output_hunk_ctx(h: &OutputHunk) -> (u32, u32) {
        let mut above = 0u32;
        for r in &h.rows {
            if r.kind == RowKind::Ctx {
                above += 1;
            } else {
                break;
            }
        }
        let mut below = 0u32;
        for r in h.rows.iter().rev() {
            if r.kind == RowKind::Ctx {
                below += 1;
            } else {
                break;
            }
        }
        (above, below)
    }

    /// Check if the cursor is positioned on a hunk divider.
    pub fn is_cursor_on_hunk_divider(&self) -> Option<(usize, usize)> {
        if self.raw_diff {
            if let Some(item) = self.flat.get(self.row_idx)
                && let FlatRowKind::HunkDivider { hunk_idx } = item.kind
            {
                return Some((item.file_idx, hunk_idx));
            }
            return None;
        }
        for hs in &self.hunk_starts {
            if hs.flat == self.row_idx {
                let cw = if self.last_content_width > 0 {
                    self.last_content_width
                } else {
                    80
                };
                let div_segs = self.hunk_div_seg_count(hs.file, hs.hunk, cw);
                if self.vis_line < div_segs {
                    return Some((hs.file, hs.hunk));
                }
            }
        }
        None
    }

    /// Check if the cursor is positioned on a bottom gap divider.
    pub fn is_cursor_on_bottom_divider(&self) -> Option<usize> {
        if self.raw_diff {
            return None;
        }
        self.flat
            .get(self.row_idx)
            .filter(|item| item.is_bottom_gap_divider())
            .map(|item| item.file_idx)
    }
}
