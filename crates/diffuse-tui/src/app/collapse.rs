//! Collapse — file and modification line collapse toggling.

use crate::{
    core::{ModSide, parse::RowKind},
    types::ModRowKey,
};

use super::AppState;

impl AppState {
    pub fn toggle_file_collapse(&mut self, fi: usize) {
        if let Some(f) = self.output.files.get(fi)
            && f.is_binary
        {
            return;
        }
        if self.collapsed_files.contains(&fi) {
            self.collapsed_files.remove(&fi);
            if !self.is_file_loaded(fi) {
                self.auto_scroll_on_load = Some((fi, self.row_idx, self.scroll));
                self.request_file_load(fi);
                self.rebuild_flat_preserving_cursor(fi);
            } else {
                self.rebuild_flat_preserving_cursor(fi);
                self.adjust_scroll_for_expanded_file(fi);
            }
        } else {
            self.collapsed_files.insert(fi);
            self.auto_scroll_on_load = None;
            if let Some(f) = self.output.files.get_mut(fi) {
                f.release_rendered_memory();
            }
            self.rebuild_flat_preserving_cursor(fi);
        }
    }

    pub fn toggle_current_file_collapse(&mut self) {
        let fi = self.current_file_idx();
        self.toggle_file_collapse(fi);
    }

    pub fn toggle_current_mod_row(&mut self) {
        if self.raw_diff {
            return;
        }
        let Some(item) = self.flat.get(self.row_idx).copied() else {
            return;
        };
        let Some(row) = self.row_of(&item) else {
            return;
        };
        if row.kind != RowKind::Mod {
            return;
        }
        let fi = item.file_idx;
        let diff_idx = item.diff_row_idx();
        let key = ModRowKey::new(fi, row);

        let is_expanding = !self.expanded_mods.contains(&key);
        if is_expanding {
            self.expanded_mods.insert(key);
        } else {
            self.expanded_mods.remove(&key);
        }

        let (flat, hunk_starts) =
            Self::build_visual_flat(&self.output, &self.collapsed_files, &self.expanded_mods);
        self.flat = flat;
        self.hunk_starts = hunk_starts;

        // Position cursor:
        // Expanding: cursor lands on the Del row (the first row).
        // Collapsing: cursor lands on the single Mod row.
        if is_expanding {
            if let Some(pos) = self.flat.iter().position(|r| {
                r.file_idx == fi
                    && r.diff_row_idx() == diff_idx
                    && r.mod_side() == Some(ModSide::Del)
            }) {
                self.row_idx = pos;
            }
        } else if let Some(pos) = self
            .flat
            .iter()
            .position(|r| r.file_idx == fi && r.diff_row_idx() == diff_idx && !r.is_expanded_mod())
        {
            self.row_idx = pos;
        }

        self.vis_line = 0;
        self.adjust_scroll();
        self.bump_content();
    }

    pub fn toggle_expand_all_mods(&mut self) {
        if self.raw_diff {
            return;
        }
        let total_mods = self.total_mod_rows();
        if total_mods == 0 {
            return;
        }

        let cur_item = self.flat.get(self.row_idx).copied();
        let cur_diff_idx = cur_item.and_then(|it| it.diff_row_idx());
        let cur_fi = cur_item.map(|it| it.file_idx);
        let is_cur_mod = cur_item
            .and_then(|it| self.row_of(&it))
            .is_some_and(|r| r.kind == RowKind::Mod);

        let expand_all = self.expanded_mods.len() < total_mods;
        if expand_all {
            for (fi, f) in self.output.files.iter().enumerate() {
                if !f.rows.is_empty() {
                    for r in &f.rows {
                        if r.kind == RowKind::Mod {
                            self.expanded_mods.insert(ModRowKey::new(fi, r));
                        }
                    }
                } else {
                    for h in &f.hunks {
                        for r in &h.rows {
                            if r.kind == RowKind::Mod {
                                self.expanded_mods.insert(ModRowKey::new(fi, r));
                            }
                        }
                    }
                }
            }
        } else {
            self.expanded_mods.clear();
        }

        let (flat, hunk_starts) =
            Self::build_visual_flat(&self.output, &self.collapsed_files, &self.expanded_mods);
        self.flat = flat;
        self.hunk_starts = hunk_starts;

        // Position cursor:
        // If cursor was on a mod row:
        //   When expanding all: land on the Del row of that mod row.
        //   When collapsing all: land on the single Mod row of that mod row.
        // Otherwise: stay on the same logical row / header.
        if let Some(fi) = cur_fi {
            if let Some(diff_idx) = cur_diff_idx {
                if is_cur_mod && expand_all {
                    if let Some(pos) = self.flat.iter().position(|r| {
                        r.file_idx == fi
                            && r.diff_row_idx() == Some(diff_idx)
                            && r.mod_side() == Some(ModSide::Del)
                    }) {
                        self.row_idx = pos;
                    }
                } else if let Some(pos) = self.flat.iter().position(|r| {
                    r.file_idx == fi && r.diff_row_idx() == Some(diff_idx) && !r.is_expanded_mod()
                }) {
                    self.row_idx = pos;
                }
            } else if let Some(item) = cur_item {
                if item.is_file_header() {
                    if let Some(pos) = self
                        .flat
                        .iter()
                        .position(|r| r.file_idx == fi && r.is_file_header())
                    {
                        self.row_idx = pos;
                    }
                } else if item.is_bottom_gap_divider()
                    && let Some(pos) = self
                        .flat
                        .iter()
                        .position(|r| r.file_idx == fi && r.is_bottom_gap_divider())
                {
                    self.row_idx = pos;
                }
            }
        }

        if self.row_idx >= self.flat.len() {
            self.row_idx = self.flat.len().saturating_sub(1);
        }

        self.vis_line = 0;
        self.adjust_scroll();
        self.bump_content();
    }
}

#[cfg(test)]
mod tests;
