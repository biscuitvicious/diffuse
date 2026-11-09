use crate::{app::AppState, types::CursorSnapshot};

impl AppState {
    pub(crate) fn rebuild_flat_preserving_cursor(&mut self, target_fi: usize) {
        let prev_item = self.flat.get(self.row_idx).cloned();
        let prev_vis_line = self.vis_line;

        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files, &self.expanded_mods)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;

        // If target file is now collapsed, jump to its header row
        if self.collapsed_files.contains(&target_fi) {
            self.row_idx = self.first_row_of_file(target_fi);
        } else if self.row_idx >= self.flat.len() {
            self.row_idx = self.flat.len().saturating_sub(1);
        }

        if let Some(item) = prev_item
            && item.is_file_header()
            && self
                .flat
                .get(self.row_idx)
                .is_some_and(|r| r.is_file_header() && r.file_idx == item.file_idx)
        {
            let cw = self.effective_content_width();
            let max_vis = self.row_seg_count(self.row_idx, cw).saturating_sub(1);
            self.vis_line = prev_vis_line.min(max_vis);
        } else {
            self.vis_line = 0;
        }

        self.adjust_scroll();
        self.bump_content();
    }

    /// Capture the active cursor coordinate anchor and screen Y offset.
    pub fn capture_cursor_snapshot(&self) -> CursorSnapshot {
        let cur_fi = self.current_file_idx();
        let cur_path = self
            .current_file()
            .map(|f| f.path.clone())
            .unwrap_or_default();
        let old_vis = self.cursor_visual_line();
        let screen_y = old_vis.saturating_sub(self.scroll);

        let row = self.current_flat_row();
        let is_file_header = row.is_some_and(|r| r.is_file_header());
        let is_hunk_divider = row.is_some_and(|r| r.is_hunk_divider());
        let hunk_idx = self.current_hunk();

        let (target_new, target_old) = if let Some(fr) = row
            && let Some(r) = self.row_of(fr)
            && (r.new_line.is_some() || r.old_line.is_some())
        {
            match fr.kind {
                crate::types::FlatRowKind::ExpandedMod {
                    side: diffuse_core::ModSide::Del,
                    ..
                } => (None, r.old_line),
                crate::types::FlatRowKind::ExpandedMod {
                    side: diffuse_core::ModSide::Add,
                    ..
                } => (r.new_line, None),
                _ => (r.new_line, r.old_line),
            }
        } else {
            let mut neighbor_coord = None;
            for offset in 1..self.flat.len() {
                if self.row_idx >= offset {
                    let prev = &self.flat[self.row_idx - offset];
                    if prev.file_idx == cur_fi
                        && let Some(c) = self.flat_row_coord(prev)
                    {
                        neighbor_coord = Some(c);
                        break;
                    }
                }
                if self.row_idx + offset < self.flat.len() {
                    let next = &self.flat[self.row_idx + offset];
                    if next.file_idx == cur_fi
                        && let Some(c) = self.flat_row_coord(next)
                    {
                        neighbor_coord = Some(c);
                        break;
                    }
                }
            }
            (neighbor_coord, neighbor_coord)
        };

        CursorSnapshot {
            file_path: cur_path,
            file_idx: cur_fi,
            is_file_header,
            is_hunk_divider,
            hunk_idx,
            target_new,
            target_old,
            screen_y,
        }
    }

    /// Rebuild flat rows and hunk metadata while restoring cursor and viewport
    /// position from `snapshot`.
    pub fn rebuild_flat_preserving_snapshot(&mut self, snapshot: &CursorSnapshot) {
        if self.output.files.is_empty() {
            self.flat.clear();
            self.hunk_starts.clear();
            self.hunk_meta.clear();
            self.row_idx = 0;
            self.scroll = 0;
            self.vis_line = 0;
            return;
        }

        self.hunk_meta = Self::build_hunk_meta(&self.output);
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files, &self.expanded_mods)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;
        self.anchor = None;
        self.confirmed_sel = None;
        self.bump_content();

        let file_removed = !self
            .output
            .files
            .iter()
            .any(|f| f.path == snapshot.file_path);
        let target_fi = self
            .output
            .files
            .iter()
            .position(|f| f.path == snapshot.file_path)
            .unwrap_or_else(|| {
                snapshot
                    .file_idx
                    .min(self.output.files.len().saturating_sub(1))
            });

        // When the snapshot file was removed, cursor should land on the next
        // file's header if there is one, otherwise on the previous file's
        // bottom gap divider (last row).
        let mut idx = None;
        if file_removed {
            if snapshot.file_idx < self.output.files.len() {
                idx = Some(self.first_row_of_file(snapshot.file_idx));
            } else {
                idx = self
                    .flat
                    .iter()
                    .enumerate()
                    .rfind(|(_, item)| item.file_idx == target_fi)
                    .map(|(i, _)| i);
            }
        }

        if !file_removed && snapshot.is_file_header {
            idx = Some(self.first_row_of_file(target_fi));
        }

        if idx.is_none()
            && !file_removed
            && snapshot.is_hunk_divider
            && let Some(hi) = snapshot.hunk_idx
        {
            if let Some(hs) = self
                .hunk_starts
                .iter()
                .find(|h| h.file == target_fi && h.hunk == hi)
            {
                idx = Some(hs.flat);
            } else if let Some(hs) = self.hunk_starts.iter().rfind(|h| h.file == target_fi) {
                idx = Some(hs.flat);
            }
        }

        // Priority 1: Match on old_line if coming from a deletion
        if idx.is_none()
            && snapshot.target_new.is_none()
            && let Some(target_ol) = snapshot.target_old
        {
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx == target_fi
                    && self.row_of(item).and_then(|r| r.old_line) == Some(target_ol)
                {
                    idx = Some(i);
                    break;
                }
            }
        }

        // Priority 2: Match on new_line if coming from addition/context/mod
        if idx.is_none()
            && let Some(target_nl) = snapshot.target_new
        {
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx == target_fi
                    && item.mod_side() != Some(diffuse_core::ModSide::Del)
                    && self.row_of(item).and_then(|r| r.new_line) == Some(target_nl)
                {
                    idx = Some(i);
                    break;
                }
            }
        }

        // Priority 3: Match on old_line as fallback
        if idx.is_none()
            && let Some(target_ol) = snapshot.target_old
        {
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx == target_fi
                    && item.mod_side() != Some(diffuse_core::ModSide::Add)
                    && self.row_of(item).and_then(|r| r.old_line) == Some(target_ol)
                {
                    idx = Some(i);
                    break;
                }
            }
        }

        // Priority 4: Cross-side fallback
        if idx.is_none() {
            let coord = snapshot.target_new.or(snapshot.target_old);
            if let Some(c) = coord {
                for (i, item) in self.flat.iter().enumerate() {
                    if item.file_idx == target_fi
                        && self
                            .row_of(item)
                            .is_some_and(|r| r.new_line == Some(c) || r.old_line == Some(c))
                    {
                        idx = Some(i);
                        break;
                    }
                }
            }
        }

        // Priority 5: Proximity fallback (closest line in the same file)
        if idx.is_none() {
            let target = snapshot.target_new.or(snapshot.target_old).unwrap_or(0);
            let mut best = None;
            let mut best_diff = u32::MAX;
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx != target_fi || !item.is_diff_row() {
                    continue;
                }
                if let Some(c) = self.flat_row_coord(item) {
                    let diff = target.abs_diff(c);
                    if diff < best_diff {
                        best_diff = diff;
                        best = Some(i);
                    }
                }
            }
            idx = best;
        }

        let target_row = idx.unwrap_or_else(|| self.first_row_of_file(target_fi));
        self.row_idx = target_row.min(self.flat.len().saturating_sub(1));
        self.vis_line = 0;

        let new_vis = self.cursor_visual_line();
        let width = if self.terminal_width > 0 {
            self.terminal_width
        } else if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let content_width = if self.raw_diff {
            width
        } else {
            width.saturating_sub(1)
        };
        let (_, _, _, total_vis_lines) = self.cached_geometry_tuple(content_width);
        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = new_vis.saturating_sub(snapshot.screen_y).min(max_scroll);
    }
}
