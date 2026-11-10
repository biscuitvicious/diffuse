use crate::{
    app::AppState,
    core::wire::{OutputFile, PipelineOutput},
    types::CursorSnapshot,
};

impl AppState {
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
        let init_w = self.explicit_line_num_width.unwrap_or(0);
        self.old_line_num = crate::types::LineNumTracking::new(0, init_w);
        self.new_line_num = crate::types::LineNumTracking::new(0, init_w);
        if self.output.files.is_empty() {
            self.flat.clear();
            self.hunk_starts.clear();
            self.hunk_meta.clear();
            return;
        }

        if self.expand_mods == Some(true) {
            self.populate_all_expanded_mods();
        }
        self.rebuild_flat_preserving_snapshot(&snapshot);
        self.recalculate_line_num_width();
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

        if self.expand_mods == Some(true) {
            self.populate_all_expanded_mods();
        }
        self.rebuild_flat_preserving_snapshot(&snapshot);
        self.recalculate_line_num_width();
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
        if self.expand_mods == Some(true) {
            self.populate_all_expanded_mods();
        }
        self.rebuild_flat_preserving_snapshot(snapshot);
        self.recalculate_line_num_width();
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
        let init_w = self.explicit_line_num_width.unwrap_or(0);
        self.old_line_num = crate::types::LineNumTracking::new(0, init_w);
        self.new_line_num = crate::types::LineNumTracking::new(0, init_w);
        self.anchor = None;
        self.confirmed_sel = None;
        self.scroll_pin = None;
        self.auto_scroll_on_load = None;
        self.is_streaming = true;
        self.record_stream_activity();
        self.bump_content();
    }
}
