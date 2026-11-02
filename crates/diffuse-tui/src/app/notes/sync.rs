//! Review notes database synchronization, delta detection, and file note
//! updates.

use std::collections::HashMap;

use crate::{
    app::AppState,
    core::{
        notes::{NotesDB, ReviewNote, note_runs, query_notes_for_file},
        wire::OutputNote,
    },
};

/// Summary of changes detected between two revisions of the review notes DB.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotesChangeSummary {
    pub added: usize,
    pub deleted: usize,
    pub edited: usize,
    pub active_delta: isize,
    pub resolved_delta: isize,
    pub in_progress_delta: isize,
}

impl NotesChangeSummary {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added == 0
            && self.deleted == 0
            && self.edited == 0
            && self.active_delta == 0
            && self.resolved_delta == 0
            && self.in_progress_delta == 0
    }

    #[must_use]
    pub fn format_message(&self) -> String {
        let mut parts = Vec::new();
        if self.active_delta != 0 && self.resolved_delta != 0 {
            let active_sign = if self.active_delta > 0 { "+" } else { "" };
            let resolved_sign = if self.resolved_delta > 0 { "+" } else { "" };
            parts.push(format!("{active_sign}{} active", self.active_delta));
            parts.push(format!("{resolved_sign}{} resolved", self.resolved_delta));
        } else if self.resolved_delta != 0 {
            let resolved_sign = if self.resolved_delta > 0 { "+" } else { "" };
            parts.push(format!("{resolved_sign}{} resolved", self.resolved_delta));
        } else if self.added > 0 || self.deleted > 0 {
            if self.added > 0 {
                parts.push(format!(
                    "+{} note{}",
                    self.added,
                    if self.added == 1 { "" } else { "s" }
                ));
            }
            if self.deleted > 0 {
                parts.push(format!(
                    "-{} note{}",
                    self.deleted,
                    if self.deleted == 1 { "" } else { "s" }
                ));
            }
        } else if self.active_delta != 0 {
            let active_sign = if self.active_delta > 0 { "+" } else { "" };
            parts.push(format!("{active_sign}{} active", self.active_delta));
        } else if self.in_progress_delta != 0 {
            let ip_sign = if self.in_progress_delta > 0 { "+" } else { "" };
            parts.push(format!("{ip_sign}{} in progress", self.in_progress_delta));
        }

        if self.edited > 0 {
            parts.push(format!(
                "{} note{} edited",
                self.edited,
                if self.edited == 1 { "" } else { "s" }
            ));
        }

        if parts.is_empty() {
            "↻ notes updated".to_string()
        } else {
            format!("↻ notes updated ({})", parts.join(", "))
        }
    }
}

impl AppState {
    /// Synchronize review notes and counts on an [`OutputFile`] in
    /// `self.output.files` with the active state in `self.notes`.
    pub(crate) fn sync_file_notes(&mut self, file_idx: usize) {
        if let Some(f) = self.output.files.get_mut(file_idx) {
            let notes: Vec<OutputNote> = query_notes_for_file(
                &self.notes,
                self.repo_key.as_deref(),
                self.rev_key.as_deref(),
                &f.path,
            )
            .into_iter()
            .map(|n| OutputNote {
                id: n.id.clone(),
                file: n.file.clone(),
                location: n.location,
                text: n.text.clone(),
                created_at: n.created_at,
                runs: note_runs(&n.text),
                content_hash: n.content_hash,
                status: n.status,
                original_source_rows: n.original_source_rows.clone(),
            })
            .collect();
            let mut open_notes = 0;
            let mut in_progress_notes = 0;
            let mut resolved_notes = 0;
            for n in &notes {
                match n.status {
                    diffuse_core::notes::NoteStatus::Open => open_notes += 1,
                    diffuse_core::notes::NoteStatus::InProgress => in_progress_notes += 1,
                    diffuse_core::notes::NoteStatus::Resolved => resolved_notes += 1,
                }
            }
            f.counts.notes = open_notes;
            f.counts.in_progress_notes = in_progress_notes;
            f.counts.resolved_notes = resolved_notes;
            f.notes = notes;
        }
    }

    /// Synchronize review notes and counts across all files in
    /// `self.output.files`.
    pub(crate) fn sync_all_notes(&mut self) {
        for fi in 0..self.output.files.len() {
            self.sync_file_notes(fi);
        }
    }

    /// Compare two versions of the notes database for `repo_key` and compute
    /// an aggregated delta summary.
    pub fn diff_notes_db(
        old: &NotesDB,
        new: &NotesDB,
        repo_key: Option<&str>,
    ) -> NotesChangeSummary {
        fn collect_notes<'a>(
            db: &'a NotesDB,
            repo_key: Option<&str>,
        ) -> HashMap<&'a str, &'a ReviewNote> {
            let Some(key) = repo_key else {
                return HashMap::new();
            };
            db.get(key)
                .into_iter()
                .flat_map(|rev_map| rev_map.values())
                .flat_map(|file_map| file_map.values().flatten())
                .map(|note| (note.id.as_str(), note))
                .collect()
        }

        let old_notes = collect_notes(old, repo_key);
        let new_notes = collect_notes(new, repo_key);
        let mut summary = NotesChangeSummary::default();

        for (id, old_note) in &old_notes {
            if let Some(new_note) = new_notes.get(id) {
                if old_note.status != new_note.status {
                    if old_note.status.counts_as_open() && new_note.status.is_resolved() {
                        summary.active_delta -= 1;
                        summary.resolved_delta += 1;
                    } else if old_note.status.is_resolved() && new_note.status.counts_as_open() {
                        summary.active_delta += 1;
                        summary.resolved_delta -= 1;
                    } else if old_note.status.is_open() && new_note.status.is_in_progress() {
                        summary.in_progress_delta += 1;
                    } else if old_note.status.is_in_progress() && new_note.status.is_open() {
                        summary.in_progress_delta -= 1;
                    }
                }
                if old_note.text != new_note.text {
                    summary.edited += 1;
                }
            } else {
                summary.deleted += 1;
                if old_note.status.counts_as_open() {
                    summary.active_delta -= 1;
                } else if old_note.status.is_resolved() {
                    summary.resolved_delta -= 1;
                }
            }
        }

        for (id, new_note) in &new_notes {
            if !old_notes.contains_key(id) {
                summary.added += 1;
                if new_note.status.counts_as_open() {
                    summary.active_delta += 1;
                } else if new_note.status.is_resolved() {
                    summary.resolved_delta += 1;
                }
            }
        }

        summary
    }

    /// Reload the review notes database from disk, detect modifications,
    /// re-synchronize note callouts and counts across all files, and preserve
    /// the current cursor and viewport screen-Y position.
    pub fn reload_notes_from_disk(&mut self) -> Option<String> {
        let new_notes = crate::core::notes::load_notes(&self.notes_path);
        if new_notes == self.notes {
            return None;
        }

        let summary = Self::diff_notes_db(&self.notes, &new_notes, self.repo_key.as_deref());

        let old_vis = self.cursor_visual_line();
        let screen_y = old_vis.saturating_sub(self.scroll);

        self.notes = new_notes;
        self.sync_all_notes();
        self.bump_notes();

        let cw = self.effective_content_width();
        let max_nav = self.row_nav_vis_count(self.row_idx, cw);
        if self.vis_line >= max_nav {
            self.vis_line = max_nav.saturating_sub(1);
        }

        let new_vis = self.cursor_visual_line();
        let (_, _, _, total_vis_lines) = self.cached_geometry_tuple(cw);
        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = new_vis.saturating_sub(screen_y).min(max_scroll);

        if !summary.is_empty() {
            let msg = summary.format_message();
            self.set_status_message(msg.clone(), true);
            Some(msg)
        } else {
            None
        }
    }
}
