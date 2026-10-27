//! Row queries — coordinate and content access over the flat list.

use crate::{
    core::{
        pipeline::context_line_runs,
        wire::{OutputFile, OutputRow, OutputRun, PipelineOutput},
    },
    types::{FlatRow, FlatRowKind},
};

use super::AppState;

impl AppState {
    pub fn row_of(&self, fr: &FlatRow) -> Option<&OutputRow> {
        Self::row_from_output(&self.output, self.raw_diff, fr)
    }

    pub fn row_from_output<'a>(
        output: &'a PipelineOutput,
        raw_diff: bool,
        fr: &FlatRow,
    ) -> Option<&'a OutputRow> {
        let FlatRowKind::DiffRow { hunk_idx, row_idx } = fr.kind else {
            return None;
        };
        let file = output.files.get(fr.file_idx)?;
        if raw_diff {
            let hunks = if !file.raw_hunks.is_empty() {
                &file.raw_hunks
            } else {
                &file.hunks
            };
            let hunk = hunks.get(hunk_idx).unwrap_or_else(|| {
                panic!(
                    "FlatRow hunk_idx {} out of bounds for '{}' (has {} hunks)",
                    hunk_idx,
                    file.path,
                    hunks.len()
                )
            });
            Some(hunk.rows.get(row_idx).unwrap_or_else(|| {
                panic!(
                    "FlatRow row_idx {} out of bounds for '{}' hunk {} (has {} rows)",
                    row_idx,
                    file.path,
                    hunk_idx,
                    hunk.rows.len()
                )
            }))
        } else if !file.rows.is_empty() {
            Some(file.rows.get(row_idx).unwrap_or_else(|| {
                panic!(
                    "FlatRow row_idx {} out of bounds for '{}' (has {} visible rows)",
                    row_idx,
                    file.path,
                    file.rows.len()
                )
            }))
        } else {
            let hunk = file.hunks.get(hunk_idx).unwrap_or_else(|| {
                panic!(
                    "FlatRow hunk_idx {} out of bounds for '{}' (has {} hunks)",
                    hunk_idx,
                    file.path,
                    file.hunks.len()
                )
            });
            Some(hunk.rows.get(row_idx).unwrap_or_else(|| {
                panic!(
                    "FlatRow row_idx {} out of bounds for '{}' hunk {} (has {} rows)",
                    row_idx,
                    file.path,
                    hunk_idx,
                    hunk.rows.len()
                )
            }))
        }
    }

    pub fn flat_coord(output: &PipelineOutput, raw_diff: bool, fr: &FlatRow) -> Option<u32> {
        Self::row_from_output(output, raw_diff, fr).and_then(Self::row_coord)
    }

    pub fn flat_row_coord(&self, fr: &FlatRow) -> Option<u32> {
        self.row_of(fr).and_then(Self::row_coord)
    }

    pub fn row_coord(row: &OutputRow) -> Option<u32> {
        row.new_line.or(row.old_line)
    }

    pub(crate) fn get_line_runs(f: &OutputFile, line_num: u32, line_text: &str) -> Vec<OutputRun> {
        let line_idx = (line_num.saturating_sub(1)) as usize;
        if let Some(runs) = f.content_runs.get(line_idx)
            && !runs.is_empty()
        {
            return runs.clone();
        }
        let ext = f.path.rsplit('.').next().unwrap_or("");
        context_line_runs(line_text, Some(line_num), ext)
    }

    pub(crate) fn flat_pos_of_coord(
        &self,
        file_idx: usize,
        new_line: Option<u32>,
        old_line: Option<u32>,
    ) -> Option<usize> {
        self.flat.iter().position(|fr| {
            fr.file_idx == file_idx
                && fr.is_diff_row()
                && self.row_of(fr).is_some_and(|r| {
                    if let Some(nl) = new_line {
                        r.new_line == Some(nl)
                    } else if let Some(ol) = old_line {
                        r.old_line == Some(ol)
                    } else {
                        false
                    }
                })
        })
    }

    pub(crate) fn next_coord_row(&self, from: usize) -> usize {
        for i in from..self.flat.len() {
            if self.flat_row_coord(&self.flat[i]).is_some() {
                return i;
            }
        }
        from
    }

    pub(crate) fn prev_coord_row(&self, from: usize) -> usize {
        let hi = from.min(self.flat.len().saturating_sub(1));
        for i in (0..=hi).rev() {
            if self.flat_row_coord(&self.flat[i]).is_some() {
                return i;
            }
        }
        from
    }
}
