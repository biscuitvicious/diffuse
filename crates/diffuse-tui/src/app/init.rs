use std::{collections::HashSet, path::PathBuf, time::Instant};

use super::AppState;
use crate::{
    core::{DEFAULT_EXPAND_LINES, notes::load_notes, wire::PipelineOutput},
    render::DiffuseTheme,
};

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

        let (flat, hunk_starts) =
            Self::build_visual_flat(&output, &collapsed_files, &HashSet::new());

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
        let target = if repo.is_empty() || repo == "<unnamed>" {
            diffuse_core::ReviewTarget::from_repo(diffuse_core::Repo::default())
        } else {
            diffuse_core::ReviewTarget::from_repo(diffuse_core::Repo::from_key(repo.clone()))
        };
        let repo_display = target.display_name().to_string();
        let repo_key = target.repo_key().map(ToString::to_string);
        let rev_key = target.rev_key().map(ToString::to_string);
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
            repo: repo_display,
            repo_key,
            rev_key,
            target,
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
            repo_root: PathBuf::new(),
            git_revs: Vec::new(),
            pending_editor: None,
            file_finder: None,
            collapsed_files,
            expanded_mods: HashSet::new(),
            expand_mods: Some(false),
            old_line_num: crate::types::LineNumTracking::default(),
            new_line_num: crate::types::LineNumTracking::default(),
            show_old_line_gutter: None,
            explicit_line_num_width: None,
            side_by_side: false,
            explicit_side_by_side: Some(false),
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
        state.recalculate_line_num_width();
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

    pub fn with_line_num_width(mut self, width: Option<usize>) -> Self {
        self.explicit_line_num_width = width.map(|w| w.min(diffuse_render::MAX_LINE_NUM_WIDTH));
        self.recalculate_line_num_width();
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

    pub fn with_expand_mods(mut self, expand_mods: Option<bool>) -> Self {
        self.expand_mods = expand_mods;
        if expand_mods == Some(true) {
            self.populate_all_expanded_mods();
            let (flat, hunk_starts) = self.build_current_flat();
            self.flat = flat;
            self.hunk_starts = hunk_starts;
        }
        self
    }

    pub fn with_side_by_side(mut self, side_by_side: bool) -> Self {
        self.side_by_side = side_by_side;
        self.explicit_side_by_side = Some(side_by_side);
        let (flat, hunk_starts) = self.build_current_flat();
        self.flat = flat;
        self.hunk_starts = hunk_starts;
        self
    }

    pub fn with_explicit_side_by_side(mut self, explicit: Option<bool>) -> Self {
        self.explicit_side_by_side = explicit;
        if let Some(sbs) = explicit {
            self.side_by_side = sbs;
            let (flat, hunk_starts) = self.build_current_flat();
            self.flat = flat;
            self.hunk_starts = hunk_starts;
        }
        self
    }

    /// Set the review target, synchronizing repo, repo_key, rev_key, repo_root,
    /// commit_range, and git_revs fields.
    pub fn with_target(mut self, target: diffuse_core::ReviewTarget) -> Self {
        self.repo = target.display_name().to_string();
        self.repo_key = target.repo_key().map(ToString::to_string);
        self.rev_key = target.rev_key().map(ToString::to_string);
        if let Some(root) = target.repo_root() {
            self.repo_root = root.to_path_buf();
        } else {
            self.repo_root = PathBuf::new();
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
        let repo = diffuse_core::Repo::from_root(repo_root).with_key_opt(self.repo_key.clone());
        let mut target = diffuse_core::ReviewTarget::from_repo_args(repo, git_revs, staged);
        if let Some(rev_key) = self.rev_key.clone() {
            target = target.with_rev_key(rev_key);
        }
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

    /// Explicitly override the repository key for review notes.
    pub fn with_repo_key(mut self, repo_key: Option<String>) -> Self {
        self.repo_key = repo_key;
        self.sync_all_notes();
        self
    }

    /// Explicitly override the revision key for review notes.
    pub fn with_rev_key(mut self, rev_key: Option<String>) -> Self {
        self.rev_key = rev_key;
        self.sync_all_notes();
        self
    }
}
