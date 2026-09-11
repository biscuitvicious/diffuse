//! Flat synchronization — streaming append and reload with cursor preservation.

use crate::{
    core::wire::{OutputFile, PipelineOutput},
    types::{FlatRow, HunkStart},
};

use super::AppState;

impl AppState {
    pub fn append_file(&mut self, file: OutputFile) {
        let fi = self.output.files.len();
        let has_content_changes =
            file.counts.adds > 0 || file.counts.dels > 0 || file.counts.mods > 0;
        if file.is_binary
            || file.status == "untracked"
            || ((file.status == "renamed" || file.status == "mode changed") && !has_content_changes)
        {
            self.collapsed_files.insert(fi);
        }

        let is_collapsed = self.collapsed_files.contains(&fi);
        let meta = Self::build_file_hunk_meta(&file);
        self.hunk_meta.push(meta);

        let file_header_idx = self.flat.len();
        self.flat.push(Self::file_header_row(fi));

        if !is_collapsed {
            if self.raw_diff {
                let hunks = if !file.raw_hunks.is_empty() {
                    &file.raw_hunks
                } else {
                    &file.hunks
                };
                for (hi, h) in hunks.iter().enumerate() {
                    self.flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: hi,
                        row_idx: 0,
                        is_file_header: false,
                        is_hunk_divider: true,
                    });
                    self.hunk_starts.push(HunkStart {
                        flat: self.flat.len() - 1,
                        file: fi,
                        hunk: hi,
                    });
                    for ri in 0..h.rows.len() {
                        self.flat.push(FlatRow {
                            file_idx: fi,
                            hunk_idx: hi,
                            row_idx: ri,
                            is_file_header: false,
                            is_hunk_divider: false,
                        });
                    }
                }
            } else if !file.rows.is_empty() {
                let hunk_count = file.hunks.len();
                let mut ri = 0;
                for (hi, h) in file.hunks.iter().enumerate() {
                    for _ in 0..h.rows.len() {
                        if ri < file.rows.len() {
                            self.flat.push(FlatRow {
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
                while ri < file.rows.len() {
                    self.flat.push(FlatRow {
                        file_idx: fi,
                        hunk_idx: hunk_count.saturating_sub(1),
                        row_idx: ri,
                        is_file_header: false,
                        is_hunk_divider: false,
                    });
                    ri += 1;
                }
                self.hunk_starts
                    .extend(Self::build_file_hunk_starts_for_file(
                        &self.flat,
                        fi,
                        &file,
                        file_header_idx,
                    ));
            } else {
                for (hi, h) in file.hunks.iter().enumerate() {
                    let hunk_start_flat = self.flat.len();
                    self.hunk_starts.push(HunkStart {
                        flat: hunk_start_flat,
                        file: fi,
                        hunk: hi,
                    });
                    for ri in 0..h.rows.len() {
                        self.flat.push(FlatRow {
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

        self.output.files.push(file);

        if self.flat.len() == 1
            && self.flat[0].is_file_header
            && self.row_idx == 0
            && self.output.files.len() == 1
            && let Some(pos) = self
                .flat
                .iter()
                .position(|r| Self::flat_coord(&self.output, self.raw_diff, r).is_some())
        {
            self.row_idx = pos;
        }
        self.vis_line = 0;
    }

    pub(crate) fn rebuild_flat_preserving_cursor(&mut self, target_fi: usize) {
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;

        // If target file is now collapsed, jump to its header row
        if self.collapsed_files.contains(&target_fi) {
            self.row_idx = self.first_row_of_file(target_fi);
        } else if self.row_idx >= self.flat.len() {
            self.row_idx = self.flat.len().saturating_sub(1);
        }
        self.adjust_scroll();
    }

    pub(crate) fn adjust_scroll(&mut self) {
        let max_rows = self.viewport_height.saturating_sub(2);
        if self.row_idx < self.scroll {
            self.scroll = self.row_idx;
        }
        if self.row_idx >= self.scroll + max_rows {
            self.scroll = self.row_idx.saturating_sub(max_rows.saturating_sub(1));
        }
    }

    pub fn reload(&mut self, output: PipelineOutput) {
        let (file_path, coord, prefer_new) = {
            let cur_path = self
                .current_file()
                .map(|f| f.path.clone())
                .unwrap_or_default();
            let row = self.current_flat_row();
            let c = row.and_then(|r| self.flat_row_coord(r)).unwrap_or(0);
            let prefer = row
                .and_then(|r| self.row_of(r))
                .map(|r| r.new_line.is_some())
                .unwrap_or(true);
            (cur_path, c, prefer)
        };

        for (fi, f) in output.files.iter().enumerate() {
            let has_content_changes = f.counts.adds > 0 || f.counts.dels > 0 || f.counts.mods > 0;
            if (f.is_binary
                || f.status == "untracked"
                || ((f.status == "renamed" || f.status == "mode changed") && !has_content_changes))
                && !self.output.files.iter().any(|old| old.path == f.path)
            {
                self.collapsed_files.insert(fi);
            }
        }

        self.output = output;
        if self.output.files.is_empty() {
            self.flat.clear();
            self.hunk_starts.clear();
            self.hunk_meta.clear();
            self.should_quit = true;
            return;
        }

        self.hunk_meta = Self::build_hunk_meta(&self.output);
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;
        self.anchor = None;
        self.confirmed_sel = None;

        let target_fi = self
            .output
            .files
            .iter()
            .position(|f| f.path == file_path)
            .unwrap_or(0);

        self.goto_coord(target_fi, coord, prefer_new);
    }

    /// Jump to the first flat row in `file` carrying `coord` (preferring the
    /// given side); fall back to the file's first coordinate row — the
    /// reference's coordinate-resume behavior.
    pub(crate) fn goto_coord(&mut self, file: usize, coord: u32, prefer_new: bool) {
        let mut idx = None;
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != file {
                continue;
            }
            let Some(r) = self.row_of(item) else {
                continue;
            };
            let side_ok = if prefer_new {
                r.new_line.is_some()
            } else {
                r.old_line.is_some()
            };
            if Self::row_coord(r) == Some(coord) && side_ok {
                idx = Some(i);
                break;
            }
        }
        let idx = idx.unwrap_or_else(|| self.first_row_of_file(file));
        self.row_idx = idx;
        self.scroll = 0;
    }
}

#[cfg(test)]
mod tests;
