//! Note mutations: deletion, status cycling, and display visibility toggles.

use super::target::VisualTarget;
use crate::{
    app::AppState,
    core::notes::{
        NotesDB, find_note_in_repo, query_notes_for_file, remove_note_by_id, remove_notes_in_range,
        save_notes,
    },
};

impl AppState {
    /// Delete note focused under the cursor or across the active selection
    /// range. If the cursor is on a diff line without a selection, deletion
    /// is disabled to prevent accidental removal while navigating code.
    pub fn delete_note(&mut self) {
        let Some(repo_key) = self.repo_key.clone() else {
            self.set_status_message(
                "notes disabled: not in a git repository (specify --repo-key to enable)"
                    .to_string(),
                false,
            );
            return;
        };
        if let Some(sel) = self.compute_selection() {
            let file = self
                .output
                .files
                .get(sel.file_idx)
                .expect("selection file_idx out of bounds in output.files");
            let file_path = file.path.clone();
            let rev_key = self.rev_key.as_deref().unwrap_or("");
            let removed = remove_notes_in_range(
                &mut self.notes,
                &repo_key,
                rev_key,
                &file_path,
                sel.start
                    .new_line_num
                    .or(sel.start.old_line_num)
                    .map_or(0, |n| n.get()),
                sel.end
                    .new_line_num
                    .or(sel.end.old_line_num)
                    .map_or(0, |n| n.get()),
            );
            let _ = save_notes(&self.notes_path, &self.notes);
            if let Some(fi) = self.output.files.iter().position(|f| f.path == file_path) {
                self.sync_file_notes(fi);
            }
            self.bump_notes();
            self.pending_notes_reload = Some(vec![file_path.clone()]);
            let (note_msg, note_ok) = if removed > 0 {
                (
                    format!(
                        "deleted {removed} note{}",
                        if removed == 1 { "" } else { "s" }
                    ),
                    true,
                )
            } else {
                ("no note here".to_string(), false)
            };
            self.set_status_message(note_msg, note_ok);
        } else if let Some(target) = self.visual_target_at(self.row_idx, self.vis_line) {
            let note_info = match target {
                VisualTarget::Note { note, .. } => Some((note.file.clone(), note.id.clone())),
                _ => None,
            };
            if let Some((_, note_id)) = note_info {
                if let Some((rev_k, f_k, _)) = find_note_in_repo(&self.notes, &repo_key, &note_id) {
                    let file = f_k.to_string();
                    let rev = rev_k.to_string();
                    let removed =
                        remove_note_by_id(&mut self.notes, &repo_key, &rev, &file, &note_id);
                    if removed {
                        let _ = save_notes(&self.notes_path, &self.notes);
                        if let Some(fi) = self.output.files.iter().position(|f| f.path == file) {
                            self.sync_file_notes(fi);
                        }
                        self.bump_notes();
                        self.pending_notes_reload = Some(vec![file.clone()]);
                        let cw = if self.last_content_width > 0 {
                            self.last_content_width
                        } else {
                            80
                        };
                        let max_nav = self.row_nav_vis_count(self.row_idx, cw);
                        if self.vis_line >= max_nav {
                            self.vis_line = max_nav.saturating_sub(1);
                        }
                        self.set_status_message("deleted note".to_string(), true);
                    }
                }
            } else {
                self.set_status_message("no note here".to_string(), false);
            }
        } else {
            self.set_status_message("no note here".to_string(), false);
        }
        self.confirmed_sel = None;
        self.anchor = None;
    }

    /// Cycle the status of the focused review note: Open -> InProgress ->
    /// Resolved -> Open. Returns true if a note status was cycled, false if
    /// no note was focused.
    pub fn cycle_note_status(&mut self) -> bool {
        let Some(repo_key) = self.repo_key.clone() else {
            return false;
        };

        let focused_note = if let Some(target) = self.visual_target_at(self.row_idx, self.vis_line)
        {
            match target {
                VisualTarget::Note { note, .. } => Some((note.file.clone(), note.id.clone())),
                _ => None,
            }
        } else {
            None
        };

        let (_file_path, note_id) = if let Some(info) = focused_note {
            info
        } else if let Some(sel) = self.compute_selection() {
            let file = &self.output.files[sel.file_idx];
            let sel_start = sel
                .start
                .new_line_num
                .or(sel.start.old_line_num)
                .map_or(0, |n| n.get());
            let sel_end = sel
                .end
                .new_line_num
                .or(sel.end.old_line_num)
                .map_or(0, |n| n.get());
            if let Some(n) = query_notes_for_file(
                &self.notes,
                Some(&repo_key),
                self.rev_key.as_deref(),
                &file.path,
            )
            .into_iter()
            .find(|n| n.start() <= sel_end && n.end() >= sel_start)
            {
                (n.file.clone(), n.id.clone())
            } else {
                return false;
            }
        } else if let Some(file) = self.current_file()
            && let Some(row) = self.current_flat_row()
            && let Some(c) = self.flat_row_coord(row)
        {
            if let Some(n) = query_notes_for_file(
                &self.notes,
                Some(&repo_key),
                self.rev_key.as_deref(),
                &file.path,
            )
            .into_iter()
            .find(|n| n.start() <= c && n.end() >= c)
            {
                (n.file.clone(), n.id.clone())
            } else {
                return false;
            }
        } else {
            return false;
        };

        let Some((rev_k, f_path, existing)) = find_note_in_repo(&self.notes, &repo_key, &note_id)
        else {
            return false;
        };
        let next_status = existing.status.cycle();
        let rev_k = rev_k.to_string();
        let file_path = f_path.to_string();

        let updated = crate::core::notes::set_note_status_by_id(
            &mut self.notes,
            &repo_key,
            &rev_k,
            &file_path,
            &note_id,
            next_status,
        );

        if updated {
            let _ = save_notes(&self.notes_path, &self.notes);
            if let Some(fi) = self.output.files.iter().position(|f| f.path == file_path) {
                self.sync_file_notes(fi);
            }
            self.bump_notes();
            self.pending_notes_reload = Some(vec![file_path.clone()]);
            let label = match next_status {
                diffuse_core::notes::NoteStatus::Open => "open",
                diffuse_core::notes::NoteStatus::InProgress => "in progress",
                diffuse_core::notes::NoteStatus::Resolved => "resolved",
            };
            self.set_status_message(format!("note marked {label} {}", next_status.glyph()), true);
            true
        } else {
            false
        }
    }

    /// Toggle displaying resolved notes in the diff view.
    pub fn toggle_show_resolved_notes(&mut self) {
        self.show_resolved_notes = !self.show_resolved_notes;
        self.bump_notes();
        self.pending_notes_reload = Some(Vec::new());
        let msg = if self.show_resolved_notes {
            "showing resolved notes"
        } else {
            "hiding resolved notes"
        };
        self.set_status_message(msg.to_string(), true);
    }

    /// Notes database scoped to the notes that are currently visible.
    ///
    /// Resolved notes are present only when resolved-note display is enabled;
    /// reload paths use this to avoid loading content for notes that would
    /// render behind a hidden resolved-note callout.
    #[must_use]
    pub fn visible_notes_db(&self) -> NotesDB {
        if self.show_resolved_notes {
            self.notes.clone()
        } else {
            crate::core::notes::without_resolved(&self.notes)
        }
    }

    /// Toggle displaying the original source snapshot in note callouts.
    pub fn toggle_show_note_source(&mut self) {
        self.show_note_source = !self.show_note_source;
        self.bump_notes();
        let msg = if self.show_note_source {
            "showing note original source"
        } else {
            "hiding note original source"
        };
        self.set_status_message(msg.to_string(), true);
    }
}
