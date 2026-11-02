//! One-directional context expansion (above or below hunks).

use std::mem;

use crate::{
    app::{AppState, anchor::DividerFallback},
    core::{parse::RowKind, wire::OutputRow},
};

impl AppState {
    /// Expand context lines above hunk `hi` in `file[fi]` by
    /// `self.expand_lines`.
    pub fn expand_hunk_above(&mut self, fi: usize, hi: usize) -> bool {
        let anchors = self.capture_divider_anchor(fi, hi);
        let expanded = self.expand_hunk_above_mutate(fi, hi);
        if expanded {
            self.restore_divider_pivot(anchors, fi, hi, DividerFallback::AboveRow);
        }
        expanded
    }

    /// Mutate-only core of [`Self::expand_hunk_above`]: no viewport capture or
    /// restore.
    fn expand_hunk_above_mutate(&mut self, fi: usize, hi: usize) -> bool {
        let Some(f) = self.output.files.get_mut(fi) else {
            return false;
        };
        if hi >= f.hunks.len() {
            return false;
        }

        let step = self.expand_lines.max(1);

        if hi == 0 {
            let h0 = &f.hunks[0];
            let min_start = h0.new_start.min(h0.old_start);
            if min_start <= 1 {
                return false;
            }
            let gap_count = (min_start - 1) as usize;
            let down_count = gap_count.min(step);
            let old_start_line = (h0.old_start.saturating_sub(down_count as u32)).max(1);
            let new_start_line = (h0.new_start.saturating_sub(down_count as u32)).max(1);

            let mut down_rows = Vec::with_capacity(down_count);
            for k in 0..down_count {
                let old_num = old_start_line + k as u32;
                let new_num = new_start_line + k as u32;
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
                down_rows.push(OutputRow {
                    kind: RowKind::Ctx,
                    mov_side: None,
                    new_line: Some(new_num),
                    old_line: Some(old_num),
                    runs,
                    wholesale: false,
                    group: None,
                });
            }

            let mut new_h0_rows = down_rows.clone();
            new_h0_rows.extend(mem::take(&mut f.hunks[0].rows));
            f.hunks[0].rows = new_h0_rows;
            f.hunks[0].old_start -= down_count as u32;
            f.hunks[0].new_start -= down_count as u32;
            f.hunks[0].old_count += down_count as u32;
            f.hunks[0].new_count += down_count as u32;
            if fi < self.hunk_meta.len() && !self.hunk_meta[fi].is_empty() {
                self.hunk_meta[fi][0].above += down_count;
            }

            f.rows.splice(0..0, down_rows);
            return true;
        } else {
            let prev_hi = hi - 1;
            let next_hi = hi;
            let h1 = &f.hunks[prev_hi];
            let h2 = &f.hunks[next_hi];

            let h1_old_end = h1.old_start.saturating_add(h1.old_count.saturating_sub(1));
            let h1_new_end = h1.new_start.saturating_add(h1.new_count.saturating_sub(1));
            let h2_old_start = h2.old_start;
            let h2_new_start = h2.new_start;

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

            let down_count = gap_count.min(step);
            let remaining_gap = gap_count - down_count;

            let mut down_rows = Vec::with_capacity(down_count);
            for k in 0..down_count {
                let old_num = (gap_old_end - down_count as u32 + 1) + k as u32;
                let new_num = (gap_new_end - down_count as u32 + 1) + k as u32;
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
                down_rows.push(OutputRow {
                    kind: RowKind::Ctx,
                    mov_side: None,
                    new_line: Some(new_num),
                    old_line: Some(old_num),
                    runs,
                    wholesale: false,
                    group: None,
                });
            }

            if remaining_gap == 0 {
                let h2 = f.hunks.remove(next_hi);
                f.hunks[prev_hi].rows.extend(down_rows.clone());
                f.hunks[prev_hi].rows.extend(h2.rows);
                f.hunks[prev_hi].old_count =
                    (h2.old_start + h2.old_count).saturating_sub(f.hunks[prev_hi].old_start);
                f.hunks[prev_hi].new_count =
                    (h2.new_start + h2.new_count).saturating_sub(f.hunks[prev_hi].new_start);

                if fi < self.hunk_meta.len() && next_hi < self.hunk_meta[fi].len() {
                    let h2_meta = self.hunk_meta[fi].remove(next_hi);
                    if prev_hi < self.hunk_meta[fi].len() {
                        self.hunk_meta[fi][prev_hi].below = h2_meta.below;
                    }
                }
            } else {
                let mut new_h2_rows = down_rows.clone();
                new_h2_rows.extend(mem::take(&mut f.hunks[next_hi].rows));
                f.hunks[next_hi].rows = new_h2_rows;
                f.hunks[next_hi].old_start -= down_count as u32;
                f.hunks[next_hi].new_start -= down_count as u32;
                f.hunks[next_hi].old_count += down_count as u32;
                f.hunks[next_hi].new_count += down_count as u32;
                if fi < self.hunk_meta.len() && next_hi < self.hunk_meta[fi].len() {
                    self.hunk_meta[fi][next_hi].above += down_count;
                }
            }
            f.rows = f.hunks.iter().flat_map(|h| h.rows.clone()).collect();
        }

        true
    }

    /// Expand context lines below hunk `hi` in `file[fi]` by
    /// `self.expand_lines`.
    pub fn expand_hunk_below(&mut self, fi: usize, hi: usize) -> bool {
        let anchors = self.capture_divider_anchor(fi, hi + 1);
        let expanded = self.expand_hunk_below_mutate(fi, hi);
        if expanded {
            self.restore_divider_pivot(anchors, fi, hi + 1, DividerFallback::BelowRow);
        }
        expanded
    }

    /// Mutate-only core of [`Self::expand_hunk_below`]: no viewport capture or
    /// restore.
    fn expand_hunk_below_mutate(&mut self, fi: usize, hi: usize) -> bool {
        let _is_last_hunk = {
            let Some(f) = self.output.files.get(fi) else {
                return false;
            };
            hi + 1 >= f.hunks.len()
        };

        let step = self.expand_lines.max(1);

        let Some(f) = self.output.files.get_mut(fi) else {
            return false;
        };
        if hi >= f.hunks.len() {
            return false;
        }

        if hi + 1 == f.hunks.len() {
            let h1 = &f.hunks[hi];
            if f.new_line_count == 0 || f.old_line_count == 0 {
                return false;
            }
            let last_new = h1
                .rows
                .iter()
                .rev()
                .find_map(|r| r.new_line)
                .unwrap_or_else(|| {
                    if h1.new_count > 0 {
                        h1.new_start.saturating_add(h1.new_count).saturating_sub(1)
                    } else {
                        h1.new_start
                    }
                });
            let last_old = h1
                .rows
                .iter()
                .rev()
                .find_map(|r| r.old_line)
                .unwrap_or_else(|| {
                    if h1.old_count > 0 {
                        h1.old_start.saturating_add(h1.old_count).saturating_sub(1)
                    } else {
                        h1.old_start
                    }
                });
            let new_gap = (f.new_line_count as u32).saturating_sub(last_new) as usize;
            let old_gap = (f.old_line_count as u32).saturating_sub(last_old) as usize;
            let gap_count = new_gap.min(old_gap);
            if gap_count == 0 {
                return false;
            }
            let up_count = gap_count.min(step);

            let mut up_rows = Vec::with_capacity(up_count);
            for k in 0..up_count {
                let old_num = last_old + 1 + k as u32;
                let new_num = last_new + 1 + k as u32;
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
                up_rows.push(OutputRow {
                    kind: RowKind::Ctx,
                    mov_side: None,
                    new_line: Some(new_num),
                    old_line: Some(old_num),
                    runs,
                    wholesale: false,
                    group: None,
                });
            }

            f.hunks[hi].rows.extend(up_rows.clone());
            f.hunks[hi].old_count += up_count as u32;
            f.hunks[hi].new_count += up_count as u32;
            if fi < self.hunk_meta.len() && hi < self.hunk_meta[fi].len() {
                self.hunk_meta[fi][hi].below += up_count;
            }

            f.rows.extend(up_rows);
        } else {
            let prev_hi = hi;
            let next_hi = hi + 1;
            let h1 = &f.hunks[prev_hi];
            let h2 = &f.hunks[next_hi];

            let h1_old_end = h1.old_start.saturating_add(h1.old_count.saturating_sub(1));
            let h1_new_end = h1.new_start.saturating_add(h1.new_count.saturating_sub(1));
            let h2_old_start = h2.old_start;
            let h2_new_start = h2.new_start;

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

            let up_count = gap_count.min(step);
            let remaining_gap = gap_count - up_count;

            let mut up_rows = Vec::with_capacity(up_count);
            for k in 0..up_count {
                let old_num = gap_old_start + k as u32;
                let new_num = gap_new_start + k as u32;
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
                up_rows.push(OutputRow {
                    kind: RowKind::Ctx,
                    mov_side: None,
                    new_line: Some(new_num),
                    old_line: Some(old_num),
                    runs,
                    wholesale: false,
                    group: None,
                });
            }

            if remaining_gap == 0 {
                let h2 = f.hunks.remove(next_hi);
                f.hunks[prev_hi].rows.extend(up_rows.clone());
                f.hunks[prev_hi].rows.extend(h2.rows);
                f.hunks[prev_hi].old_count =
                    (h2.old_start + h2.old_count).saturating_sub(f.hunks[prev_hi].old_start);
                f.hunks[prev_hi].new_count =
                    (h2.new_start + h2.new_count).saturating_sub(f.hunks[prev_hi].new_start);

                if fi < self.hunk_meta.len() && next_hi < self.hunk_meta[fi].len() {
                    let h2_meta = self.hunk_meta[fi].remove(next_hi);
                    if prev_hi < self.hunk_meta[fi].len() {
                        self.hunk_meta[fi][prev_hi].below = h2_meta.below;
                    }
                }
            } else {
                f.hunks[prev_hi].rows.extend(up_rows.clone());
                f.hunks[prev_hi].old_count += up_count as u32;
                f.hunks[prev_hi].new_count += up_count as u32;
                if fi < self.hunk_meta.len() && prev_hi < self.hunk_meta[fi].len() {
                    self.hunk_meta[fi][prev_hi].below += up_count;
                }
            }
            f.rows = f.hunks.iter().flat_map(|h| h.rows.clone()).collect();
        }

        true
    }
}
