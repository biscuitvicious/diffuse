//! Flat synchronization — streaming append and reload with cursor preservation.

use crate::{
    core::wire::{OutputFile, PipelineOutput},
    types::{CursorSnapshot, FlatRow, HunkStart},
};

use super::AppState;

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
                let hunk_count = file.hunks.len();
                let mut ri = 0;
                for (hi, h) in file.hunks.iter().enumerate() {
                    for _ in 0..h.rows.len() {
                        if ri < file.rows.len() {
                            self.flat.push(FlatRow::diff_row(fi, hi, ri));
                            ri += 1;
                        }
                    }
                }
                while ri < file.rows.len() {
                    self.flat
                        .push(FlatRow::diff_row(fi, hunk_count.saturating_sub(1), ri));
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
                        self.flat.push(FlatRow::diff_row(fi, hi, ri));
                    }
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

    pub(crate) fn rebuild_flat_preserving_cursor(&mut self, target_fi: usize) {
        let prev_item = self.flat.get(self.row_idx).cloned();
        let prev_vis_line = self.vis_line;

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

    pub(crate) fn adjust_scroll(&mut self) {
        if self.flat.is_empty() {
            self.scroll = 0;
            return;
        }
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let (starts, counts, _, total_vis_lines) = self.cached_geometry_tuple(cw);
        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);

        let cur_vis = starts.get(self.row_idx).copied().unwrap_or(0);
        let cur_count = counts.get(self.row_idx).copied().unwrap_or(1);

        if cur_vis < self.scroll {
            self.scroll = cur_vis;
        } else if cur_vis + cur_count > self.scroll + max_rows {
            self.scroll = (cur_vis + cur_count).saturating_sub(max_rows);
        }
        self.scroll = self.scroll.min(max_scroll);
    }

    pub fn adjust_scroll_for_expanded_file(&mut self, fi: usize) {
        if self.collapsed_files.contains(&fi) || !self.is_file_loaded(fi) {
            return;
        }
        let vh = self.viewport_height;
        if vh == 0 {
            return;
        }

        let header_flat = self.first_row_of_file(fi);
        let last_flat = self.last_row_of_file(fi);

        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let (starts, counts, _, total_vis_lines) = self.cached_geometry_tuple(cw);
        let Some(&header_vis) = starts.get(header_flat) else {
            return;
        };
        let Some(&last_vis) = starts.get(last_flat) else {
            return;
        };
        let last_count = counts.get(last_flat).copied().unwrap_or(1);
        let file_end_vis = last_vis + last_count;
        let file_span = file_end_vis.saturating_sub(header_vis);

        // Header must be within the visible viewport to be considered
        // "far down to the bottom of the view".
        if header_vis < self.scroll || header_vis >= self.scroll + vh {
            return;
        }

        let screen_row = header_vis - self.scroll;
        let lines_below = vh - screen_row;

        let target_fill = (vh / 5).max(1);
        let desired_visible = target_fill.min(file_span);

        if lines_below < desired_visible {
            let needed_scroll_down = desired_visible - lines_below;
            let max_rows = vh.max(1);
            let max_scroll = total_vis_lines.saturating_sub(max_rows);
            self.scroll = (self.scroll + needed_scroll_down).min(max_scroll);

            // Keep cursor in view if it was above the new scroll position
            if self.row_idx < header_flat
                && starts.get(self.row_idx).copied().unwrap_or(0) < self.scroll
            {
                self.row_idx = header_flat;
                self.vis_line = 0;
            }
        }
    }

    pub fn reload(&mut self, output: PipelineOutput) {
        let snapshot = self.capture_cursor_snapshot();

        for (fi, f) in output.files.iter().enumerate() {
            let has_content_changes = f.counts.adds > 0 || f.counts.dels > 0 || f.counts.mods > 0;
            if (f.is_binary
                || f.status == "untracked"
                || f.status == "deleted"
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
            return;
        }

        self.rebuild_flat_preserving_snapshot(&snapshot);
    }

    /// Update or remove specific files in `self.output.files` without reloading
    /// unaffected files across the entire repository.
    pub fn reload_files(&mut self, mut updated_files: Vec<OutputFile>, checked_paths: &[String]) {
        let snapshot = self.capture_cursor_snapshot();

        for path in checked_paths {
            if let Some(pos) = updated_files.iter().position(|f| &f.path == path) {
                let new_file = updated_files.swap_remove(pos);
                if let Some(fi) = self.output.files.iter().position(|f| &f.path == path) {
                    self.output.files[fi] = new_file;
                } else {
                    let insert_pos = self
                        .output
                        .files
                        .binary_search_by(|f| f.path.cmp(path))
                        .unwrap_or_else(|pos| pos);
                    self.output.files.insert(insert_pos, new_file);
                }
            } else if let Some(fi) = self.output.files.iter().position(|f| &f.path == path) {
                self.output.files.remove(fi);
            }
        }

        if self.output.files.is_empty() {
            self.flat.clear();
            self.hunk_starts.clear();
            self.hunk_meta.clear();
            return;
        }

        self.rebuild_flat_preserving_snapshot(&snapshot);
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
            (r.new_line, r.old_line)
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
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.hunk_starts = hunk_starts;
        self.anchor = None;
        self.confirmed_sel = None;
        self.bump_content();

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

        let mut idx = None;
        if snapshot.is_file_header {
            idx = Some(self.first_row_of_file(target_fi));
        }

        if idx.is_none()
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

    /// Replace an existing file in `self.output.files` or insert a new file in
    /// sorted path order, rebuilding flat rows while maintaining the cursor
    /// at `snapshot`.
    pub fn replace_or_insert_file(&mut self, file: OutputFile, snapshot: &CursorSnapshot) {
        self.record_stream_activity();
        let path = file.path.clone();
        if let Some(pos) = self.output.files.iter().position(|f| f.path == path) {
            self.output.files[pos] = file;
        } else {
            let insert_pos = self
                .output
                .files
                .binary_search_by(|f| f.path.cmp(&path))
                .unwrap_or_else(|pos| pos);
            self.output.files.insert(insert_pos, file);
        }
        self.rebuild_flat_preserving_snapshot(snapshot);
    }

    /// Drop all current diff output and state back to a clean starting baseline
    /// to prepare for streaming reload.
    pub fn reset_for_streaming_reload(&mut self) {
        self.output.files.clear();
        self.output.branch.clear();
        self.output.untracked_skipped.clear();
        self.flat.clear();
        self.hunk_starts.clear();
        self.hunk_meta.clear();
        self.collapsed_files.clear();
        self.loading_files.clear();
        self.row_idx = 0;
        self.vis_line = 0;
        self.scroll = 0;
        self.col_offset = 0;
        self.anchor = None;
        self.confirmed_sel = None;
        self.scroll_pin = None;
        self.auto_scroll_on_load = None;
        self.is_streaming = true;
        self.record_stream_activity();
        self.bump_content();
    }
}

#[cfg(test)]
mod tests;
