//! TUI App State Machine, Layout Engine & Flat Visual Row Navigation.

use std::{collections::HashSet, path::PathBuf, sync::mpsc, time::Instant};

use crate::{
    core::{
        notes::NotesDB,
        wire::{OutputFile, PipelineOutput},
    },
    render::DiffuseTheme,
    types::{FlatRow, HunkMeta, HunkStart, ModRowKey, Selection},
};

mod anchor;
mod collapse;
mod context;
mod dispatch;
mod finder;
mod flat;
mod geometry;
mod init;
mod load;
mod navigation;
mod notes;
mod query;
mod render;
mod selection;
mod status;
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
    pub note_editor: Option<crate::editor::Editor>,
    pub editing_note_id: Option<String>,
    pub anchor: Option<usize>,
    pub sel_pivot: Option<usize>,
    pub confirmed_sel: Option<Selection>,
    pub theme: DiffuseTheme,
    pub repo: String,
    /// Repository lookup key for notes storage. None disables notes.
    pub repo_key: Option<String>,
    /// Revision lookup key for notes storage.
    pub rev_key: Option<String>,
    pub target: diffuse_core::ReviewTarget,
    pub should_quit: bool,
    /// Ephemeral action feedback rendered at the front of the secondary
    /// status bar: `(message, is_ok, created_at)`. All messages expire
    /// (errors linger longer); expiry is polled by the event loop and
    /// lazily on render so the reserved secondary slot simply goes blank.
    pub status_message: Option<(String, bool, Instant)>,
    pub viewport_height: usize,
    pub screen_height: usize,
    pub show_resolved_notes: bool,
    /// Whether note callouts display the note's original source snapshot.
    pub show_note_source: bool,
    pub notes: NotesDB,
    pub notes_path: PathBuf,
    pub hunk_starts: Vec<HunkStart>,
    hunk_meta: Vec<Vec<HunkMeta>>,
    note_id_counter: u64,
    pub repo_root: PathBuf,
    pub git_revs: Vec<String>,
    pub pending_editor: Option<crate::editor::EditorRequest>,
    pub file_finder: Option<crate::finder::FileFinderState>,
    pub collapsed_files: HashSet<usize>,
    pub expanded_mods: HashSet<ModRowKey>,
    /// Global modification row expansion state (Some(true) = expanded,
    /// Some(false) = collapsed, None = mode ignores mod expansion).
    pub expand_mods: Option<bool>,
    pub loading_files: HashSet<usize>,
    pub on_demand_tx: Option<mpsc::Sender<(usize, OutputFile)>>,
    pub on_demand_rx: Option<mpsc::Receiver<(usize, OutputFile)>>,
    pub last_content_width: usize,
    pub terminal_width: usize,
    pub is_dragging_scrollbar: bool,
    pub scrollbar_drag_grab: Option<usize>,
    pub last_viewport_start: usize,
    pub is_streaming: bool,
    pub pending_force_reload: bool,
    /// Targeted notes re-stream requested by an in-process note mutation, with
    /// the file paths noted *before* the mutation (so deleting the last note on
    /// a synthetic file still re-streams it for removal). The event loop
    /// consumes it to re-stream only the affected files.
    pub pending_notes_reload: Option<Vec<String>>,
    pub commit_range: Option<String>,
    pub last_stream_activity: Option<Instant>,
    pub start_time: Instant,
    pub expand_lines: usize,
    /// Viewport pin set by programmatic view pivots (context expansion):
    /// `(scroll, row_idx_at_pin_time)`. Every `render_frame` applies it
    /// verbatim instead of re-clamping around the cursor — until the cursor
    /// moves or new user input arrives (see `execute`).
    scroll_pin: Option<(usize, usize)>,
    /// Pending auto-scroll on expansion load: `(file_idx, row_idx, scroll)`.
    /// Preserved while streaming, invalidated if the user moves cursor or makes
    /// any UI state change.
    pub(crate) auto_scroll_on_load: Option<(usize, usize, usize)>,
    /// Cached visual geometry (see geometry.rs); cleared by bump_content /
    /// bump_notes on any content or notes mutation.
    pub(crate) geo_cache: [Option<geometry::CachedGeometry>; 2],
}

#[cfg(test)]
pub(crate) fn temp_notes_path() -> PathBuf {
    PathBuf::from("/nonexistent-diffuse-test-notes.json")
}

#[cfg(test)]
mod tests;
