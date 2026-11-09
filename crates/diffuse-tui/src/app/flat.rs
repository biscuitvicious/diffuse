//! Flat model — construction and indexing of the visual flat list.

use std::collections::HashSet;

use crate::{
    core::{
        ModGlyph, ModSide,
        parse::RowKind,
        wire::{OutputFile, OutputRow, PipelineOutput},
    },
    types::{FlatRow, HunkMeta, HunkStart, ModRowKey},
};

use super::AppState;

impl AppState {
    pub(crate) fn build_visual_flat(
        output: &PipelineOutput,
        collapsed_files: &HashSet<usize>,
        expanded_mods: &HashSet<ModRowKey>,
    ) -> (Vec<FlatRow>, Vec<HunkStart>) {
        let flat = Self::build_flat(output, collapsed_files, expanded_mods);
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
            flat.push(FlatRow::file_header(fi));
            if collapsed_files.contains(&fi) {
                continue;
            }
            let hunks = if !f.raw_hunks.is_empty() {
                &f.raw_hunks
            } else {
                &f.hunks
            };
            for (hi, h) in hunks.iter().enumerate() {
                flat.push(FlatRow::hunk_divider(fi, hi));
                hunk_starts.push(HunkStart {
                    flat: flat.len() - 1,
                    file: fi,
                    hunk: hi,
                });
                for ri in 0..h.rows.len() {
                    flat.push(FlatRow::diff_row(fi, hi, ri));
                }
            }
        }
        (flat, hunk_starts)
    }

    /// Build the flat row list (shared by the visual and raw representations).
    pub(crate) fn build_flat(
        output: &PipelineOutput,
        collapsed_files: &HashSet<usize>,
        expanded_mods: &HashSet<ModRowKey>,
    ) -> Vec<FlatRow> {
        let mut flat = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            flat.push(FlatRow::file_header(fi));
            if collapsed_files.contains(&fi) {
                continue;
            }
            push_file_content_rows(&mut flat, fi, f, expanded_mods);
            if file_trailing_gap(f) > 0 {
                flat.push(FlatRow::bottom_gap(fi, f.hunks.len().saturating_sub(1)));
            }
        }
        flat
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
        let mut cur = search_from;
        for (hi, _) in f.hunks.iter().enumerate() {
            while cur < flat.len() && flat[cur].file_idx == fi && flat[cur].hunk_idx() < Some(hi) {
                cur += 1;
            }
            if cur < flat.len() && flat[cur].file_idx == fi && flat[cur].hunk_idx() == Some(hi) {
                starts.push(HunkStart {
                    flat: cur,
                    file: fi,
                    hunk: hi,
                });
            }
        }
        starts
    }

    pub(crate) fn build_hunk_starts(output: &PipelineOutput, flat: &[FlatRow]) -> Vec<HunkStart> {
        let mut starts = Vec::new();
        for (fi, f) in output.files.iter().enumerate() {
            let file_header_idx = flat
                .iter()
                .position(|fr| fr.file_idx == fi && fr.is_file_header())
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
            .position(|r| r.file_idx == fi && r.is_file_header())
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

/// Return the number of hidden trailing lines after the last hunk in `f`.
pub fn file_trailing_gap(f: &OutputFile) -> usize {
    let Some(last_hunk) = f.hunks.last() else {
        return 0;
    };
    if f.new_line_count == 0 || f.old_line_count == 0 {
        return 0;
    }
    let last_new = last_hunk
        .rows
        .iter()
        .rev()
        .find_map(|r| r.new_line)
        .unwrap_or_else(|| {
            if last_hunk.new_count > 0 {
                last_hunk
                    .new_start
                    .saturating_add(last_hunk.new_count)
                    .saturating_sub(1)
            } else {
                last_hunk.new_start
            }
        });
    let last_old = last_hunk
        .rows
        .iter()
        .rev()
        .find_map(|r| r.old_line)
        .unwrap_or_else(|| {
            if last_hunk.old_count > 0 {
                last_hunk
                    .old_start
                    .saturating_add(last_hunk.old_count)
                    .saturating_sub(1)
            } else {
                last_hunk.old_start
            }
        });
    let new_gap = (f.new_line_count as u32).saturating_sub(last_new) as usize;
    let old_gap = (f.old_line_count as u32).saturating_sub(last_old) as usize;
    new_gap.min(old_gap)
}

/// Push all content rows of a file onto `flat`, honoring hunk boundaries.
pub(crate) fn push_file_content_rows(
    flat: &mut Vec<FlatRow>,
    fi: usize,
    f: &OutputFile,
    expanded_mods: &HashSet<ModRowKey>,
) {
    if !f.rows.is_empty() {
        let hunk_count = f.hunks.len();
        let mut ri = 0;
        for (hi, h) in f.hunks.iter().enumerate() {
            let hunk_len = h.rows.len();
            let end_ri = (ri + hunk_len).min(f.rows.len());
            push_hunk_rows(flat, fi, hi, &f.rows[ri..end_ri], ri, expanded_mods);
            ri = end_ri;
        }
        if ri < f.rows.len() {
            let hi = hunk_count.saturating_sub(1);
            push_hunk_rows(flat, fi, hi, &f.rows[ri..], ri, expanded_mods);
        }
    } else {
        for (hi, h) in f.hunks.iter().enumerate() {
            push_hunk_rows(flat, fi, hi, &h.rows, 0, expanded_mods);
        }
    }
}

/// Push rows for a hunk onto `flat`, merging contiguous expanded modification
/// rows into unified clusters of `n` deletions followed by `n` additions with
/// connected box glyphs (`┌`, `│`, `└`).
pub(crate) fn push_hunk_rows(
    flat: &mut Vec<FlatRow>,
    fi: usize,
    hi: usize,
    rows: &[OutputRow],
    start_ri: usize,
    expanded_mods: &HashSet<ModRowKey>,
) {
    let mut i = 0;
    while i < rows.len() {
        let r = &rows[i];
        let r_ri = start_ri + i;
        if r.kind == RowKind::Mod && expanded_mods.contains(&ModRowKey::new(fi, r)) {
            let mut j = i + 1;
            while j < rows.len()
                && rows[j].kind == RowKind::Mod
                && expanded_mods.contains(&ModRowKey::new(fi, &rows[j]))
            {
                j += 1;
            }
            let k = j - i;
            for idx in 0..k {
                let row_idx = start_ri + i + idx;
                let glyph = ModGlyph::for_cluster(ModSide::Del, idx, k);
                flat.push(FlatRow::expanded_mod(fi, hi, row_idx, ModSide::Del, glyph));
            }
            for idx in 0..k {
                let row_idx = start_ri + i + idx;
                let glyph = ModGlyph::for_cluster(ModSide::Add, idx, k);
                flat.push(FlatRow::expanded_mod(fi, hi, row_idx, ModSide::Add, glyph));
            }
            i = j;
        } else {
            flat.push(FlatRow::diff_row(fi, hi, r_ri));
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests;
