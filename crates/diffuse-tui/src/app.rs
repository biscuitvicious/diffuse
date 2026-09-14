//! TUI App State Machine, Layout Engine & Flat Visual Row Navigation.

use std::{collections::HashSet, path::PathBuf};

use crate::{
    core::{
        DEFAULT_EXPAND_LINES,
        notes::{NotesDB, load_notes},
        wire::{OutputFile, PipelineOutput},
    },
    render::DiffuseTheme,
    types::{FlatRow, HunkMeta, HunkStart, Selection},
};

mod anchor;
mod collapse;
mod context;
mod dispatch;
mod finder;
mod flat;
mod geometry;
mod navigation;
mod notes;
mod query;
mod render;
mod selection;
mod sync;

pub struct AppState {
    pub output: PipelineOutput,
    pub flat: Vec<FlatRow>,
    pub row_idx: usize,
    pub vis_line: usize,
    pub scroll: usize,
    pub col_offset: usize,
    pub num_buffer: String,
    pub syntax_on: bool,
    pub word_diff: bool,
    pub raw_diff: bool,
    pub show_help: bool,
    pub note_draft: Option<String>,
    pub anchor: Option<usize>,
    pub sel_pivot: Option<usize>,
    pub confirmed_sel: Option<Selection>,
    pub theme: DiffuseTheme,
    pub repo: String,
    pub should_quit: bool,
    pub status_message: Option<(String, bool)>, // (message, is_error)
    pub viewport_height: usize,
    pub screen_height: usize,
    pub notes: NotesDB,
    pub notes_path: PathBuf,
    pub hunk_starts: Vec<HunkStart>,
    hunk_meta: Vec<Vec<HunkMeta>>,
    note_id_counter: u64,
    pub repo_root: PathBuf,
    pub git_args: Vec<String>,
    pub pending_editor: Option<crate::editor::EditorRequest>,
    pub file_finder: Option<crate::finder::FileFinderState>,
    pub collapsed_files: HashSet<usize>,
    pub loading_files: HashSet<usize>,
    pub on_demand_tx: Option<std::sync::mpsc::Sender<(usize, OutputFile)>>,
    pub on_demand_rx: Option<std::sync::mpsc::Receiver<(usize, OutputFile)>>,
    pub last_content_width: usize,
    pub terminal_width: usize,
    pub is_dragging_scrollbar: bool,
    pub last_viewport_start: usize,
    pub is_streaming: bool,
    pub commit_range: Option<String>,
    pub last_stream_activity: Option<std::time::Instant>,
    pub start_time: std::time::Instant,
    pub expand_lines: usize,
    /// Viewport pin set by programmatic view pivots (context expansion):
    /// `(scroll, row_idx_at_pin_time)`. Every `render_frame` applies it
    /// verbatim instead of re-clamping around the cursor — until the cursor
    /// moves or new user input arrives (see `execute`).
    scroll_pin: Option<(usize, usize)>,
}

impl AppState {
    pub fn new(output: PipelineOutput, repo: String, notes_path: PathBuf) -> Self {
        let mut collapsed_files = std::collections::HashSet::new();
        for (fi, f) in output.files.iter().enumerate() {
            let has_content_changes = f.counts.adds > 0 || f.counts.dels > 0 || f.counts.mods > 0;
            if f.is_binary
                || f.status == "untracked"
                || f.status == "deleted"
                || ((f.status == "renamed" || f.status == "mode changed") && !has_content_changes)
            {
                collapsed_files.insert(fi);
            }
        }

        let (flat, hunk_starts) = Self::build_visual_flat(&output, &collapsed_files);

        // Start on first coordinate row if available, or first row (e.g.
        // header)
        let start_row = flat
            .iter()
            .position(|r| Self::flat_coord(&output, false, r).is_some())
            .unwrap_or(0);

        let hunk_meta = Self::build_hunk_meta(&output);

        Self {
            output,
            flat,
            row_idx: start_row,
            vis_line: 0,
            scroll: 0,
            col_offset: 0,
            num_buffer: String::new(),
            syntax_on: true,
            word_diff: true,
            raw_diff: false,
            show_help: false,
            note_draft: None,
            anchor: None,
            sel_pivot: None,
            confirmed_sel: None,
            theme: DiffuseTheme::from_env(),
            repo,
            should_quit: false,
            status_message: None,
            viewport_height: 20,
            screen_height: 24,
            notes: load_notes(&notes_path),
            notes_path,
            hunk_starts,
            hunk_meta,
            note_id_counter: 0,
            repo_root: PathBuf::from("."),
            git_args: Vec::new(),
            pending_editor: None,
            file_finder: None,
            collapsed_files,
            loading_files: HashSet::new(),
            on_demand_tx: None,
            on_demand_rx: None,
            last_content_width: 80,
            terminal_width: 80,
            is_dragging_scrollbar: false,
            last_viewport_start: 2,
            is_streaming: false,
            commit_range: None,
            last_stream_activity: None,
            start_time: std::time::Instant::now(),
            expand_lines: DEFAULT_EXPAND_LINES,
            scroll_pin: None,
        }
    }

    pub fn with_expand_lines(mut self, expand_lines: usize) -> Self {
        self.expand_lines = expand_lines;
        self
    }

    pub fn with_theme(mut self, theme: DiffuseTheme) -> Self {
        self.theme = theme;
        self
    }

    pub fn with_commit_range(mut self, commit_range: Option<String>) -> Self {
        self.commit_range = commit_range;
        self
    }

    pub fn with_repo_context(mut self, repo_root: PathBuf, git_args: Vec<String>) -> Self {
        self.commit_range = diffuse_core::git::resolve_commit_range(&repo_root, &git_args);
        self.repo_root = repo_root;
        self.git_args = git_args;
        self
    }

    pub fn record_stream_activity(&mut self) {
        self.last_stream_activity = Some(std::time::Instant::now());
    }

    pub fn record_stream_activity_at(&mut self, now: std::time::Instant) {
        self.last_stream_activity = Some(now);
    }

    pub fn streaming_dot(&self) -> &'static str {
        self.streaming_dot_at(std::time::Instant::now())
    }

    pub fn streaming_dot_at(&self, now: std::time::Instant) -> &'static str {
        if !self.is_streaming {
            return "●";
        }
        let Some(last_activity) = self.last_stream_activity else {
            return "●";
        };
        const DELAY_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(500);
        const BLINK_INTERVAL_MS: u128 = 200;

        if now.duration_since(last_activity) < DELAY_THRESHOLD {
            let phase = (now.duration_since(self.start_time).as_millis() / BLINK_INTERVAL_MS) % 2;
            if phase == 0 { "●" } else { " " }
        } else {
            "●"
        }
    }

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
        let (tx, rx) = std::sync::mpsc::channel();
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
            .with_repo(self.repo_root.to_str().map(String::from))
            .with_notes(Some(self.notes.clone()), Some(self.repo.clone()))
            .with_expand_lines(self.expand_lines);
        diffuse_core::pipeline::render_file_from_source(
            source,
            Some(&self.repo_root),
            &self.git_args,
            diffuse_core::ENGINE_CONTEXT,
            &opts,
        )
    }

    pub fn ensure_file_loaded_sync(&mut self, fi: usize) {
        if !self.is_file_loaded(fi)
            && let Some(rendered) = self.render_file_on_demand_sync(fi)
        {
            self.output.files[fi] = rendered;
            if fi < self.hunk_meta.len() {
                self.hunk_meta[fi] = Self::build_file_hunk_meta(&self.output.files[fi]);
            }
            if !self.collapsed_files.contains(&fi) {
                self.rebuild_flat_preserving_cursor(fi);
            }
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
            let git_args = self.git_args.clone();
            let notes = self.notes.clone();
            let repo = self.repo.clone();
            let expand_lines = self.expand_lines;

            std::thread::spawn(move || {
                let opts = diffuse_core::PipelineOptions::default()
                    .with_repo(repo_root.to_str().map(String::from))
                    .with_notes(Some(notes), Some(repo))
                    .with_expand_lines(expand_lines);
                if let Some(rendered) = diffuse_core::pipeline::render_file_from_source(
                    &source,
                    Some(&repo_root),
                    &git_args,
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
            if fi < self.hunk_meta.len() {
                self.hunk_meta[fi] = Self::build_file_hunk_meta(&self.output.files[fi]);
            }
            if !self.collapsed_files.contains(&fi) {
                self.rebuild_flat_preserving_cursor(fi);
            }
        }
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

#[cfg(test)]
pub(crate) fn temp_notes_path() -> PathBuf {
    PathBuf::from("/nonexistent-diffuse-test-notes.json")
}

#[cfg(test)]
mod tests;
