use crate::core::{
    DEFAULT_CONTEXT_LINES,
    parse::{RowKind, Spans},
    wire::{OutputFile, OutputHunk, OutputRow},
};

use super::{
    AppState,
    anchor::{CursorAnchor, DividerAnchors, DividerFallback},
};

impl AppState {
    /// Expand ALL hidden lines across all hunks in the repository.
    pub fn expand_all_hidden(&mut self) {
        let (anchor, screen_y) = self.capture_cursor_anchor();
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
        self.restore_cursor_anchor(anchor, screen_y, 0);
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
        let (anchor, screen_y) = self.capture_cursor_anchor();
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
        self.restore_cursor_anchor(anchor, screen_y, 0);
        count > 0
    }

    /// Reset all hunks in all files back to the default context width.
    pub fn reset_all_context(&mut self) {
        let (anchor, screen_y) = self.capture_cursor_anchor();
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
        self.restore_cursor_anchor(anchor, screen_y, 0);
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

    fn count_output_hunk_ctx(h: &OutputHunk) -> (u32, u32) {
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
                && item.is_hunk_divider
            {
                return Some((item.file_idx, item.hunk_idx));
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
                    text,
                    del_text: None,
                    spans: Spans::default(),
                    runs,
                    wholesale: false,
                });
            }

            let mut new_h0_rows = down_rows.clone();
            new_h0_rows.extend(std::mem::take(&mut f.hunks[0].rows));
            f.hunks[0].rows = new_h0_rows;
            f.hunks[0].old_start -= down_count as u32;
            f.hunks[0].new_start -= down_count as u32;
            f.hunks[0].old_count += down_count as u32;
            f.hunks[0].new_count += down_count as u32;
            if fi < self.hunk_meta.len() && !self.hunk_meta[fi].is_empty() {
                self.hunk_meta[fi][0].above += down_count;
            }

            f.rows.splice(0..0, down_rows);

            let (flat, hunk_starts) = Self::build_visual_flat(&self.output, &self.collapsed_files);
            self.flat = flat;
            self.bump_content();
            self.hunk_starts = hunk_starts;

            if let Some(hs) = self
                .hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == 0)
            {
                self.row_idx = hs.flat;
                self.vis_line = 0;
            }

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
                    text,
                    del_text: None,
                    spans: Spans::default(),
                    runs,
                    wholesale: false,
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
                new_h2_rows.extend(std::mem::take(&mut f.hunks[next_hi].rows));
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
        enum LastHunkState {
            Divider(DividerAnchors),
            Cursor(CursorAnchor, usize),
        }
        let is_last_hunk = self
            .output
            .files
            .get(fi)
            .is_some_and(|f| hi + 1 >= f.hunks.len());
        let last_hunk_state = if is_last_hunk {
            let (anchor, screen_y) = self.capture_cursor_anchor();
            LastHunkState::Cursor(anchor, screen_y)
        } else {
            LastHunkState::Divider(self.capture_divider_anchor(fi, hi + 1))
        };
        let expanded = self.expand_hunk_below_mutate(fi, hi);
        if !expanded {
            return false;
        }
        match last_hunk_state {
            LastHunkState::Cursor(anchor, screen_y) => {
                self.restore_cursor_anchor(anchor, screen_y, 0);
            }
            LastHunkState::Divider(anchors) => {
                self.restore_divider_pivot(anchors, fi, hi + 1, DividerFallback::BelowRow);
            }
        }
        true
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
            let h1_new_end = h1.new_start.saturating_add(h1.new_count.saturating_sub(1));
            let h1_old_end = h1.old_start.saturating_add(h1.old_count.saturating_sub(1));
            let total_lines = f.new_line_count.max(f.old_line_count) as u32;
            if h1_new_end >= total_lines && h1_old_end >= total_lines {
                return false;
            }
            let gap_count = (total_lines - h1_new_end.max(h1_old_end)) as usize;
            if gap_count == 0 {
                return false;
            }
            let up_count = gap_count.min(step);

            let mut up_rows = Vec::with_capacity(up_count);
            for k in 0..up_count {
                let old_num = h1_old_end + 1 + k as u32;
                let new_num = h1_new_end + 1 + k as u32;
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
                    text,
                    del_text: None,
                    spans: Spans::default(),
                    runs,
                    wholesale: false,
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
                    text,
                    del_text: None,
                    spans: Spans::default(),
                    runs,
                    wholesale: false,
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
                text,
                del_text: None,
                spans: Spans::default(),
                runs,
                wholesale: false,
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
            new_h2_rows.extend(std::mem::take(&mut f.hunks[hi + 1].rows));
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

        // Viewport pivot: the boundary between the upward half and the
        // downward half stays exactly where the divider was on screen. The
        // top half unfolds above it (pushing the content above up), the
        // bottom half below it.
        let (flat, hunk_starts) = Self::build_visual_flat(&self.output, &self.collapsed_files);
        self.flat = flat;
        self.bump_content();
        self.hunk_starts = hunk_starts;
        let (starts, _counts, _seg, total_vis_lines) =
            self.cached_geometry_tuple(self.last_content_width);
        // The boundary is the first downward-side row; when the divider
        // survived it is that divider's own leading segments.
        let boundary_vis = if merged {
            bottom_rows
                .first()
                .and_then(|r| r.new_line)
                .and_then(|nl| self.flat_pos_of_coord(fi, Some(nl), None))
                .map(|pos| starts[pos])
        } else {
            self.hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == hi + 1)
                .map(|hs| starts[hs.flat])
        };
        if let Some(boundary_vis) = boundary_vis {
            let max_rows = self.viewport_height.max(1);
            let max_scroll = total_vis_lines.saturating_sub(max_rows);
            let new_scroll = boundary_vis
                .saturating_sub(anchors.div_screen_y)
                .min(max_scroll);
            self.scroll = new_scroll;
            self.scroll_pin = Some((new_scroll, self.row_idx));
        }
        true
    }
}
