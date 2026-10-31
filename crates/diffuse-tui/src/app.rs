//! TUI App State Machine, Layout Engine & Flat Visual Row Navigation.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

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
    pub note_editor: Option<crate::editor::Editor>,
    pub editing_note_id: Option<String>,
    pub anchor: Option<usize>,
    pub sel_pivot: Option<usize>,
    pub confirmed_sel: Option<Selection>,
    pub theme: DiffuseTheme,
    pub repo: String,
    /// HEAD-scoped notes-DB key (`{repo}@{head}`); the display name in
    /// `repo` stays human-readable while notes isolate per commit.
    /// Resolved once at startup and never re-resolved mid-session, so all
    /// notes filed while the TUI is open land under the commit it opened
    /// with even if HEAD moves outside diffuse.
    pub notes_key: String,
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

impl AppState {
    pub fn new(
        output: PipelineOutput,
        repo: String,
        notes_path: PathBuf,
        theme: DiffuseTheme,
    ) -> Self {
        let mut collapsed_files = HashSet::new();
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
        let start_vis_line =
            if start_row > 0 && !output.files.is_empty() && !output.files[0].hunks.is_empty() {
                1
            } else {
                0
            };

        let hunk_meta = Self::build_hunk_meta(&output);

        let loaded_notes = load_notes(&notes_path);
        let mut state = Self {
            output,
            flat,
            row_idx: start_row,
            vis_line: start_vis_line,
            scroll: 0,
            col_offset: 0,
            num_buffer: String::new(),
            syntax_on: true,
            word_diff: true,
            raw_diff: false,
            show_help: false,
            note_draft: None,
            note_editor: None,
            editing_note_id: None,
            anchor: None,
            sel_pivot: None,
            confirmed_sel: None,
            theme,
            repo: repo.clone(),
            notes_key: repo.clone(),
            target: diffuse_core::ReviewTarget::from_repo(diffuse_core::Repo::from_name(repo)),
            should_quit: false,
            status_message: None,
            viewport_height: 20,
            screen_height: 24,
            show_resolved_notes: false,
            show_note_source: false,
            notes: loaded_notes,
            notes_path,
            hunk_starts,
            hunk_meta,
            note_id_counter: 0,
            repo_root: PathBuf::from("."),
            git_revs: Vec::new(),
            pending_editor: None,
            file_finder: None,
            collapsed_files,
            loading_files: HashSet::new(),
            on_demand_tx: None,
            on_demand_rx: None,
            last_content_width: 80,
            terminal_width: 80,
            is_dragging_scrollbar: false,
            scrollbar_drag_grab: None,
            last_viewport_start: 2,
            is_streaming: false,
            pending_force_reload: false,
            pending_notes_reload: None,
            commit_range: None,
            last_stream_activity: None,
            start_time: Instant::now(),
            expand_lines: DEFAULT_EXPAND_LINES,
            scroll_pin: None,
            auto_scroll_on_load: None,
            geo_cache: Default::default(),
        };
        state.sync_all_notes();
        state
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

    pub fn with_show_resolved_notes(mut self, show_resolved: bool) -> Self {
        self.show_resolved_notes = show_resolved;
        self
    }

    pub fn with_show_note_source(mut self, show_source: bool) -> Self {
        self.show_note_source = show_source;
        self
    }

    /// Set the review target, synchronizing repo, notes_key, repo_root,
    /// commit_range, and git_revs fields.
    pub fn with_target(mut self, target: diffuse_core::ReviewTarget) -> Self {
        self.repo = target.repo_name().to_string();
        self.notes_key = target.notes_key();
        if let Some(root) = target.repo_root() {
            self.repo_root = root.to_path_buf();
        }
        self.commit_range = target.commit_range();
        self.git_revs = target.git_revs.clone();
        self.target = target;
        self.sync_all_notes();
        self
    }

    pub fn with_repo_context(
        self,
        repo_root: PathBuf,
        git_revs: Vec<String>,
        staged: bool,
    ) -> Self {
        let repo_name = self.repo.clone();
        let target = diffuse_core::ReviewTarget::from_repo_args(
            diffuse_core::Repo::from_root(repo_root).with_name(repo_name),
            git_revs,
            staged,
        );
        self.with_target(target)
    }

    /// Returns true if the user is in an active interaction modal or drafting
    /// state where reloading would disrupt ongoing user input.
    #[must_use]
    pub fn is_reload_deferred(&self) -> bool {
        self.note_draft.is_some()
            || self.file_finder.is_some()
            || self.show_help
            || self.is_dragging_scrollbar
            || self.anchor.is_some()
            || self.pending_editor.is_some()
    }

    /// Scope note storage/lookup to the live HEAD of `repo_root`
    /// (`{repo}@{head}`); falls back to the bare repo name outside git.
    pub fn with_notes_key(mut self, notes_key: String) -> Self {
        self.notes_key = notes_key;
        self.sync_all_notes();
        self
    }

    pub fn record_stream_activity(&mut self) {
        self.last_stream_activity = Some(Instant::now());
    }

    pub fn record_stream_activity_at(&mut self, now: Instant) {
        self.last_stream_activity = Some(now);
    }

    pub fn streaming_dot(&self) -> &'static str {
        self.streaming_dot_at(Instant::now())
    }

    pub fn streaming_dot_at(&self, now: Instant) -> &'static str {
        if !self.is_streaming {
            return "●";
        }
        let Some(last_activity) = self.last_stream_activity else {
            return "●";
        };
        const DELAY_THRESHOLD: Duration = Duration::from_millis(500);
        const BLINK_INTERVAL_MS: u128 = 200;

        if now.duration_since(last_activity) < DELAY_THRESHOLD {
            let phase = (now.duration_since(self.start_time).as_millis() / BLINK_INTERVAL_MS) % 2;
            if phase == 0 { "●" } else { " " }
        } else {
            "●"
        }
    }

    /// Time-to-live for status-bar feedback: ok confirmations vanish
    /// quickly, errors linger longer. Both expire.
    pub const STATUS_OK_TTL: Duration = Duration::from_secs(4);
    pub const STATUS_ERR_TTL: Duration = Duration::from_secs(10);

    pub fn status_ttl(is_ok: bool) -> Duration {
        if is_ok {
            Self::STATUS_OK_TTL
        } else {
            Self::STATUS_ERR_TTL
        }
    }

    /// Record ephemeral feedback for the secondary status bar.
    pub fn set_status_message(&mut self, msg: String, is_ok: bool) {
        self.set_status_message_at(msg, is_ok, Instant::now());
    }

    pub fn set_status_message_at(&mut self, msg: String, is_ok: bool, at: Instant) {
        self.status_message = Some((msg, is_ok, at));
    }

    /// The message as of `now`, or `None` when absent/expired (pure).
    pub fn status_message_at(&self, now: Instant) -> Option<(&str, bool)> {
        let (msg, is_ok, at) = self.status_message.as_ref()?;
        if now.duration_since(*at) >= Self::status_ttl(*is_ok) {
            return None;
        }
        Some((msg.as_str(), *is_ok))
    }

    /// The currently visible message, if any.
    pub fn visible_status_message(&self) -> Option<(&str, bool)> {
        self.status_message_at(Instant::now())
    }

    /// Drop the message when past its TTL. Returns true when cleared.
    pub fn poll_status_expiry(&mut self, now: Instant) -> bool {
        let expired = match self.status_message.as_ref() {
            Some((_, is_ok, at)) => now.duration_since(*at) >= Self::status_ttl(*is_ok),
            None => false,
        };
        if expired {
            self.status_message = None;
            return true;
        }
        false
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
            .with_repo(self.repo_root.to_str().map(String::from))
            .with_notes(Some(self.notes.clone()), Some(self.notes_key.clone()))
            .with_expand_lines(self.expand_lines);
        diffuse_core::pipeline::render_file_from_source(
            source,
            Some(&self.repo_root),
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
            let repo = self.notes_key.clone();
            let expand_lines = self.expand_lines;

            thread::spawn(move || {
                let opts = diffuse_core::PipelineOptions::default()
                    .with_repo(repo_root.to_str().map(String::from))
                    .with_notes(Some(notes), Some(repo))
                    .with_expand_lines(expand_lines);
                if let Some(rendered) = diffuse_core::pipeline::render_file_from_source(
                    &source,
                    Some(&repo_root),
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

#[cfg(test)]
pub(crate) fn temp_notes_path() -> PathBuf {
    PathBuf::from("/nonexistent-diffuse-test-notes.json")
}

#[cfg(test)]
mod tests;
