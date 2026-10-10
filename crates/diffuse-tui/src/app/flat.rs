//! Flat model — construction and indexing of the visual flat list.

use std::collections::HashSet;

use crate::{
    core::{
        parse::RowKind,
        wire::{OutputFile, PipelineOutput},
    },
    types::{FlatRow, HunkMeta, HunkStart},
};

use super::AppState;

impl AppState {
    pub(crate) fn build_visual_flat(
        output: &PipelineOutput,
        collapsed_files: &HashSet<usize>,
    ) -> (Vec<FlatRow>, Vec<HunkStart>) {
        let flat = Self::build_flat(output, collapsed_files);
        let hunk_starts = Self::build_hunk_starts(output, &flat);
        (flat, hunk_starts)
    }

    /// The raw unified-diff flat sequence: per file a header banner, then per
    /// hunk an `@@` header row and the hunk rows exactly as loaded in the diff.
    /// Hunk navigation jumps to the `@@` rows.
    pub(crate) fn build_raw_flat(
        output: &PipelineOutput,
        collapsed_files: &HashSet<usize>,
    ) -> (Vec<FlatRow>, Vec<HunkStart>) {
        let mut flat = Vec::new();
        let mut hunk_starts = Vec::new();

        for (fi, f) in output.files.iter().enumerate() {
            flat.push(Self::file_header_row(fi));
            if collapsed_files.contains(&fi) {
                continue;
            }
            let hunks = if !f.raw_hunks.is_empty() {
                &f.raw_hunks
            } else {
                &f.hunks
            };
            for (hi, h) in hunks.iter().enumerate() {
                flat.push(FlatRow {
                    file_idx: fi,
                    hunk_idx: hi,
                    row_idx: 0,
                    is_file_header: false,
                    is_hunk_divider: true,
                });
                hunk_starts.push(HunkStart {
                    flat: flat.len() - 1,
                    file: fi,
                    hunk: hi,
                });
                for ri in 0..h.rows.len() {
                    flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: hi,
                        row_idx: ri,
                        is_file_header: false,
                        is_hunk_divider: false,
                    });
                }
            }
        }
        (flat, hunk_starts)
    }

    /// Build the flat row list (shared by the visual and raw representations).
    pub(crate) fn build_flat(
        output: &PipelineOutput,
        collapsed_files: &HashSet<usize>,
    ) -> Vec<FlatRow> {
        let mut flat = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            flat.push(Self::file_header_row(fi));
            if collapsed_files.contains(&fi) {
                continue;
            }
            if !f.rows.is_empty() {
                let hunk_count = f.hunks.len();
                let mut ri = 0;
                for (hi, h) in f.hunks.iter().enumerate() {
                    for _ in 0..h.rows.len() {
                        if ri < f.rows.len() {
                            flat.push(FlatRow {
                                file_idx: fi,
                                hunk_idx: hi,
                                row_idx: ri,
                                is_file_header: false,
                                is_hunk_divider: false,
                            });
                            ri += 1;
                        }
                    }
                }
                while ri < f.rows.len() {
                    flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: hunk_count.saturating_sub(1),
                        row_idx: ri,
                        is_file_header: false,
                        is_hunk_divider: false,
                    });
                    ri += 1;
                }
            } else {
                for (hi, h) in f.hunks.iter().enumerate() {
                    for ri in 0..h.rows.len() {
                        flat.push(FlatRow {
                            file_idx: fi,
                            hunk_idx: hi,
                            row_idx: ri,
                            is_file_header: false,
                            is_hunk_divider: false,
                        });
                    }
                }
            }
        }
        flat
    }

    pub(crate) fn file_header_row(fi: usize) -> FlatRow {
        FlatRow {
            file_idx: fi,
            hunk_idx: 0,
            row_idx: 0,
            is_file_header: true,
            is_hunk_divider: false,
        }
    }

    /// Locate each hunk's first content row in the flat sequence by file index
    /// and hunk boundary offsets.
    pub(crate) fn build_file_hunk_starts_for_file(
        flat: &[FlatRow],
        fi: usize,
        f: &OutputFile,
        file_header_idx: usize,
    ) -> Vec<HunkStart> {
        let mut starts = Vec::new();
        if f.hunks.is_empty() {
            return starts;
        }
        let search_from = file_header_idx + 1;
        if search_from >= flat.len() || flat[search_from].file_idx != fi {
            return starts;
        }
        let mut offset = 0;
        for (hi, h) in f.hunks.iter().enumerate() {
            let hunk_flat_idx = search_from + offset;
            if hunk_flat_idx < flat.len() && flat[hunk_flat_idx].file_idx == fi {
                starts.push(HunkStart {
                    flat: hunk_flat_idx,
                    file: fi,
                    hunk: hi,
                });
            }
            offset += h.rows.len();
        }
        starts
    }

    pub(crate) fn build_hunk_starts(output: &PipelineOutput, flat: &[FlatRow]) -> Vec<HunkStart> {
        let mut starts = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            let file_header_idx = flat
                .iter()
                .position(|fr| fr.file_idx == fi && fr.is_file_header)
                .unwrap_or(0);
            starts.extend(Self::build_file_hunk_starts_for_file(
                flat,
                fi,
                f,
                file_header_idx,
            ));
        }
        starts
    }

    /// Per-file per-hunk context counts — the leading/trailing context rows
    /// carried through the wire on each hunk.
    pub(crate) fn build_file_hunk_meta(f: &OutputFile) -> Vec<HunkMeta> {
        f.hunks
            .iter()
            .map(|h| {
                let mut above = 0usize;
                for r in &h.rows {
                    if r.kind == RowKind::Ctx {
                        above += 1;
                    } else {
                        break;
                    }
                }
                let mut below = 0usize;
                for r in h.rows.iter().rev() {
                    if r.kind == RowKind::Ctx {
                        below += 1;
                    } else {
                        break;
                    }
                }
                HunkMeta { above, below }
            })
            .collect()
    }

    pub(crate) fn build_hunk_meta(output: &PipelineOutput) -> Vec<Vec<HunkMeta>> {
        output
            .files
            .iter()
            .map(Self::build_file_hunk_meta)
            .collect()
    }

    pub fn file_header_row_idx(&self, fi: usize) -> Option<usize> {
        self.flat
            .iter()
            .position(|r| r.file_idx == fi && r.is_file_header)
    }

    pub fn first_row_of_file(&self, fi: usize) -> usize {
        self.file_header_row_idx(fi).unwrap_or(0)
    }

    pub fn last_row_of_file(&self, fi: usize) -> usize {
        match self.flat.iter().position(|r| r.file_idx > fi) {
            Some(next) => next - 1,
            None => self.flat.len().saturating_sub(1),
        }
    }
}

#[cfg(test)]
mod tests;
