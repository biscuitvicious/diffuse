//! Symmetrical bidirectional context expansion around hunk dividers.

use std::mem;

use crate::{
    app::{
        AppState,
        anchor::{CursorAnchor, DividerFallback},
    },
    core::{
        parse::RowKind,
        wire::{OutputFile, OutputRow},
    },
};

impl AppState {
    /// Expand the hidden gap around the divider between hunks `hi` and
    /// `hi + 1`, splitting it equally: `(n+1)/2` lines upwards and
    /// `n - (n+1)/2` downwards (each side capped by `self.expand_lines`).
    /// The boundary between the two halves pivots at the divider's old screen
    /// position: the top half unfolds above it (pushing prior content up),
    /// the bottom half below it. When the whole gap is consumed the hunks
    /// merge into one.
    pub fn expand_hunk_both(&mut self, fi: usize, hi: usize) -> bool {
        let anchors = self.capture_divider_anchor(fi, hi + 1);
        let Some(f) = self.output.files.get_mut(fi) else {
            return false;
        };
        if hi + 1 >= f.hunks.len() {
            return false;
        }

        let step = self.expand_lines.max(1);
        let (h1_new_end, h1_old_end) = {
            let h1 = &f.hunks[hi];
            (
                h1.new_start.saturating_add(h1.new_count.saturating_sub(1)),
                h1.old_start.saturating_add(h1.old_count.saturating_sub(1)),
            )
        };
        let (h2_old_start, h2_new_start) = (f.hunks[hi + 1].old_start, f.hunks[hi + 1].new_start);

        let gap_old_start = h1_old_end + 1;
        let gap_old_end = h2_old_start.saturating_sub(1);
        let gap_new_start = h1_new_end + 1;
        let gap_new_end = h2_new_start.saturating_sub(1);

        let gap_count = if gap_old_end >= gap_old_start {
            (gap_old_end - gap_old_start + 1) as usize
        } else if gap_new_end >= gap_new_start {
            (gap_new_end - gap_new_start + 1) as usize
        } else {
            0
        };
        if gap_count == 0 {
            return false;
        }

        let n = gap_count as u32;
        let top_half = (n.saturating_add(1) / 2) as usize;
        let top_take = top_half.min(step);
        let bottom_take = (n as usize).saturating_sub(top_half).min(step);

        let make_row = |old_num: u32, new_num: u32, f: &OutputFile| -> OutputRow {
            let text = f
                .new_content
                .get((new_num.saturating_sub(1)) as usize)
                .cloned()
                .or_else(|| {
                    f.old_content
                        .get((old_num.saturating_sub(1)) as usize)
                        .cloned()
                })
                .unwrap_or_default();
            let runs = Self::get_line_runs(f, new_num, &text);
            OutputRow {
                kind: RowKind::Ctx,
                mov_side: None,
                new_line: Some(new_num),
                old_line: Some(old_num),
                runs,
                wholesale: false,
                group: None,
            }
        };

        // Upward side: rows appended to the END of hunk `hi`.
        let mut top_rows = Vec::with_capacity(top_take);
        for k in 0..top_take {
            top_rows.push(make_row(
                gap_old_start + k as u32,
                gap_new_start + k as u32,
                f,
            ));
        }
        // Downward side: rows prepended to the START of hunk `hi + 1`.
        let mut bottom_rows = Vec::with_capacity(bottom_take);
        for k in 0..bottom_take {
            bottom_rows.push(make_row(
                gap_old_end.saturating_sub((bottom_take - k - 1) as u32),
                gap_new_end.saturating_sub((bottom_take - k - 1) as u32),
                f,
            ));
        }

        let merged = top_take + bottom_take == gap_count;

        f.hunks[hi].rows.extend(top_rows.clone());
        {
            let mut new_h2_rows = bottom_rows.clone();
            new_h2_rows.extend(mem::take(&mut f.hunks[hi + 1].rows));
            f.hunks[hi + 1].rows = new_h2_rows;
        }

        if merged {
            let h2 = f.hunks.remove(hi + 1);
            f.hunks[hi].rows.extend(h2.rows);
            f.hunks[hi].old_count =
                (h2.old_start + h2.old_count).saturating_sub(f.hunks[hi].old_start);
            f.hunks[hi].new_count =
                (h2.new_start + h2.new_count).saturating_sub(f.hunks[hi].new_start);
            if fi < self.hunk_meta.len() && hi + 1 < self.hunk_meta[fi].len() {
                let h2_meta = self.hunk_meta[fi].remove(hi + 1);
                if hi < self.hunk_meta[fi].len() {
                    self.hunk_meta[fi][hi].below = h2_meta.below;
                }
            }
        } else {
            f.hunks[hi].old_count += top_take as u32;
            f.hunks[hi].new_count += top_take as u32;
            f.hunks[hi + 1].old_start -= bottom_take as u32;
            f.hunks[hi + 1].new_start -= bottom_take as u32;
            f.hunks[hi + 1].old_count += bottom_take as u32;
            f.hunks[hi + 1].new_count += bottom_take as u32;
            if fi < self.hunk_meta.len() && hi < self.hunk_meta[fi].len() {
                if !self.hunk_meta[fi].is_empty() {
                    self.hunk_meta[fi][hi].below += top_take;
                }
                if hi + 1 < self.hunk_meta[fi].len() {
                    self.hunk_meta[fi][hi + 1].above += bottom_take;
                }
            }
        }
        f.rows = f.hunks.iter().flat_map(|h| h.rows.clone()).collect();

        let fallback = if merged {
            bottom_rows
                .first()
                .map(|r| CursorAnchor {
                    file_idx: fi,
                    new_line: r.new_line,
                    old_line: r.old_line,
                })
                .map(DividerFallback::Coord)
                .unwrap_or(DividerFallback::BelowRow)
        } else {
            DividerFallback::BelowRow
        };
        self.restore_divider_pivot(anchors, fi, hi + 1, fallback);
        true
    }
}
