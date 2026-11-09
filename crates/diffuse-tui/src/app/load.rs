use std::{sync::mpsc, thread};

use super::AppState;
use crate::{core::wire::OutputFile, types::FlatRow};

impl AppState {
    pub fn current_file_idx(&self) -> usize {
        self.flat.get(self.row_idx).map(|r| r.file_idx).unwrap_or(0)
    }

    pub fn current_file(&self) -> Option<&OutputFile> {
        self.output.files.get(self.current_file_idx())
    }

    pub fn current_flat_row(&self) -> Option<&FlatRow> {
        self.flat.get(self.row_idx)
    }

    pub fn enable_async_on_demand(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.on_demand_tx = Some(tx);
        self.on_demand_rx = Some(rx);
    }

    pub fn is_file_loaded(&self, fi: usize) -> bool {
        self.output.files.get(fi).is_some_and(|f| f.is_loaded())
    }

    pub fn render_file_on_demand_sync(&self, fi: usize) -> Option<OutputFile> {
        let f = self.output.files.get(fi)?;
        let source = f.raw_source.as_ref()?;
        let opts = diffuse_core::PipelineOptions::default()
            .with_notes(
                Some(self.notes.clone()),
                self.repo_key.clone(),
                self.rev_key.clone(),
            )
            .with_expand_lines(self.expand_lines);
        let repo_root_opt = if self.repo_root.as_os_str().is_empty() {
            None
        } else {
            Some(self.repo_root.as_path())
        };
        diffuse_core::pipeline::render_file_from_source(
            source,
            repo_root_opt,
            &self.git_revs,
            diffuse_core::ENGINE_CONTEXT,
            &opts,
        )
    }

    pub fn ensure_file_loaded_sync(&mut self, fi: usize) {
        if !self.is_file_loaded(fi)
            && let Some(rendered) = self.render_file_on_demand_sync(fi)
        {
            self.output.files[fi] = rendered;
            if self.expand_mods == Some(true) {
                self.populate_all_expanded_mods();
            }
            if fi < self.hunk_meta.len() {
                self.hunk_meta[fi] = Self::build_file_hunk_meta(&self.output.files[fi]);
            }
            if !self.collapsed_files.contains(&fi) {
                self.rebuild_flat_preserving_cursor(fi);
                if let Some((target_fi, expected_row, expected_scroll)) = self.auto_scroll_on_load
                    && target_fi == fi
                    && self.row_idx == expected_row
                    && self.scroll == expected_scroll
                {
                    self.adjust_scroll_for_expanded_file(fi);
                }
            }
            self.auto_scroll_on_load = None;
        }
    }

    pub fn request_file_load(&mut self, fi: usize) {
        if self.is_file_loaded(fi) || self.loading_files.contains(&fi) {
            return;
        }
        if let Some(tx) = self.on_demand_tx.clone() {
            let Some(f) = self.output.files.get(fi) else {
                return;
            };
            let Some(source) = f.raw_source.clone() else {
                return;
            };

            self.loading_files.insert(fi);
            self.is_streaming = true;
            self.record_stream_activity();

            let repo_root = self.repo_root.clone();
            let git_revs = self.git_revs.clone();
            let notes = self.notes.clone();
            let repo_key = self.repo_key.clone();
            let rev_key = self.rev_key.clone();
            let expand_lines = self.expand_lines;

            thread::spawn(move || {
                let opts = diffuse_core::PipelineOptions::default()
                    .with_notes(Some(notes), repo_key, rev_key)
                    .with_expand_lines(expand_lines);
                let repo_root_opt = if repo_root.as_os_str().is_empty() {
                    None
                } else {
                    Some(repo_root.as_path())
                };
                if let Some(rendered) = diffuse_core::pipeline::render_file_from_source(
                    &source,
                    repo_root_opt,
                    &git_revs,
                    diffuse_core::ENGINE_CONTEXT,
                    &opts,
                ) {
                    let _ = tx.send((fi, rendered));
                }
            });
        } else {
            self.ensure_file_loaded_sync(fi);
        }
    }

    pub fn finish_on_demand_expand(&mut self, fi: usize, rendered: OutputFile) {
        self.loading_files.remove(&fi);
        self.record_stream_activity();
        if fi < self.output.files.len() {
            self.output.files[fi] = rendered;
            self.sync_file_notes(fi);
            if self.expand_mods == Some(true) {
                self.populate_all_expanded_mods();
            }
            if fi < self.hunk_meta.len() {
                self.hunk_meta[fi] = Self::build_file_hunk_meta(&self.output.files[fi]);
            }
            if !self.collapsed_files.contains(&fi) {
                self.rebuild_flat_preserving_cursor(fi);
                if let Some((target_fi, expected_row, expected_scroll)) = self.auto_scroll_on_load
                    && target_fi == fi
                    && self.row_idx == expected_row
                    && self.scroll == expected_scroll
                {
                    self.adjust_scroll_for_expanded_file(fi);
                }
            }
        }
        self.auto_scroll_on_load = None;
        if self.loading_files.is_empty() {
            self.is_streaming = false;
        }
    }

    pub fn poll_on_demand(&mut self) -> bool {
        let mut items = Vec::new();
        if let Some(ref rx) = self.on_demand_rx {
            while let Ok(item) = rx.try_recv() {
                items.push(item);
            }
        }
        let loaded_any = !items.is_empty();
        for (fi, rendered) in items {
            self.finish_on_demand_expand(fi, rendered);
        }
        loaded_any
    }
}
