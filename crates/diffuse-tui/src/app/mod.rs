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
    pub last_content_width: usize,
    pub terminal_width: usize,
    pub is_dragging_scrollbar: bool,
    pub last_viewport_start: usize,
    pub is_streaming: bool,
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
            if f.status == "untracked" {
                collapsed_files.insert(fi);
            }
        }

        let (flat, hunk_starts) = Self::build_visual_flat(&output, &collapsed_files);

        // Start on first coordinate row if available, or first row (e.g. header)
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
            theme: DiffuseTheme::MAGIC,
            repo,
            should_quit: false,
            status_message: None,
            viewport_height: 20,
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
            last_content_width: 80,
            terminal_width: 80,
            is_dragging_scrollbar: false,
            last_viewport_start: 2,
            is_streaming: false,
            expand_lines: DEFAULT_EXPAND_LINES,
            scroll_pin: None,
        }
    }

    pub fn with_expand_lines(mut self, expand_lines: usize) -> Self {
        self.expand_lines = expand_lines;
        self
    }

    pub fn with_repo_context(mut self, repo_root: PathBuf, git_args: Vec<String>) -> Self {
        self.repo_root = repo_root;
        self.git_args = git_args;
        self
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
}

#[cfg(test)]
pub(crate) fn temp_notes_path() -> PathBuf {
    PathBuf::from("/nonexistent-diffuse-test-notes.json")
}

#[cfg(test)]
mod tests;
