//! Hunk divider, gap divider, and trailing context line calculations.

use crate::app::AppState;

impl AppState {
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
