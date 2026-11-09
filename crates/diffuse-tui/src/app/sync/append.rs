use crate::{
    app::AppState,
    core::{parse::RowKind, wire::OutputFile},
    types::{FlatRow, HunkStart, ModRowKey},
};

impl AppState {
    pub fn append_file(&mut self, file: OutputFile) {
        self.record_stream_activity();
        let fi = self.output.files.len();
        let has_content_changes =
            file.counts.adds > 0 || file.counts.dels > 0 || file.counts.mods > 0;
        if file.is_binary
            || file.status == "untracked"
            || file.status == "deleted"
            || ((file.status == "renamed" || file.status == "mode changed") && !has_content_changes)
        {
            self.collapsed_files.insert(fi);
        }

        let is_collapsed = self.collapsed_files.contains(&fi);
        let meta = Self::build_file_hunk_meta(&file);
        self.hunk_meta.push(meta);

        if self.expand_mods == Some(true) {
            if !file.rows.is_empty() {
                for r in &file.rows {
                    if r.kind == RowKind::Mod {
                        self.expanded_mods.insert(ModRowKey::new(fi, r));
                    }
                }
            } else {
                for h in &file.hunks {
                    for r in &h.rows {
                        if r.kind == RowKind::Mod {
                            self.expanded_mods.insert(ModRowKey::new(fi, r));
                        }
                    }
                }
            }
        }

        let file_header_idx = self.flat.len();
        self.flat.push(FlatRow::file_header(fi));

        if !is_collapsed {
            if self.raw_diff {
                let hunks = if !file.raw_hunks.is_empty() {
                    &file.raw_hunks
                } else {
                    &file.hunks
                };
                for (hi, h) in hunks.iter().enumerate() {
                    self.flat.push(FlatRow::hunk_divider(fi, hi));
                    self.hunk_starts.push(HunkStart {
                        flat: self.flat.len() - 1,
                        file: fi,
                        hunk: hi,
                    });
                    for ri in 0..h.rows.len() {
                        self.flat.push(FlatRow::diff_row(fi, hi, ri));
                    }
                }
            } else if !file.rows.is_empty() {
                crate::app::flat::push_file_content_rows(
                    &mut self.flat,
                    fi,
                    &file,
                    &self.expanded_mods,
                );
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
                    crate::app::flat::push_hunk_rows(
                        &mut self.flat,
                        fi,
                        hi,
                        &h.rows,
                        0,
                        &self.expanded_mods,
                    );
                }
            }
            if !self.raw_diff && crate::app::flat::file_trailing_gap(&file) > 0 {
                let last_hi = file.hunks.len().saturating_sub(1);
                self.flat.push(FlatRow::bottom_gap(fi, last_hi));
            }
        }

        self.output.files.push(file);
        self.sync_file_notes(fi);

        if self.flat.len() == 1
            && self.flat[0].is_file_header()
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
        self.bump_content();
    }
}
