//! Interactive note drafting, inline editor instantiation, and note draft
//! commits.

use std::process;

use super::target::VisualTarget;
use crate::{
    app::AppState,
    core::notes::{
        DiffCoord, LineHash, NoteLocation, ReviewNote, add_note, find_note_in_repo, now_millis,
        save_notes, update_note_text,
    },
};

impl AppState {
    /// Open the note draft modal. If focused on an existing note, pre-populates
    /// with that note's text for in-place editing; otherwise starts an empty
    /// draft.
    pub fn start_note_draft(&mut self) {
        let note_info = self
            .visual_target_at(self.row_idx, self.vis_line)
            .and_then(|target| match target {
                VisualTarget::Note { note, .. } => Some((note.id.clone(), note.text.clone())),
                _ => None,
            });
        let text = if let Some((id, text)) = note_info {
            self.editing_note_id = Some(id);
            text
        } else {
            self.editing_note_id = None;
            String::new()
        };
        let mut editor = crate::editor::Editor::new(&text)
            .with_highlighter(crate::editor::markdown_highlighter(self.theme));
        let r = editor.lines().len().saturating_sub(1);
        let c = editor.lines()[r].chars().count();
        editor.set_cursor(r, c);
        self.note_editor = Some(editor);
        self.note_draft = Some(text);
    }

    /// Commit the note draft onto the current row (or active selection) and
    /// persist it through the notes DB. Mirrors the reference
    /// `commitNoteAction`.
    pub fn commit_note_draft(&mut self) -> Result<NoteLocation, &'static str> {
        let Some(repo_key) = self.repo_key.clone() else {
            self.note_editor = None;
            self.note_draft = None;
            self.editing_note_id = None;
            self.set_status_message(
                "notes disabled: not in a git repository (specify --repo-key to enable)"
                    .to_string(),
                false,
            );
            return Err("notes disabled: no repository key");
        };

        let text = if let Some(ref draft) = self.note_draft {
            if let Some(ref ed) = self.note_editor {
                if draft != &ed.text() {
                    self.note_draft.take()
                } else {
                    self.note_editor.take().map(|e| e.text())
                }
            } else {
                self.note_draft.take()
            }
        } else {
            self.note_editor.take().map(|e| e.text())
        };
        self.note_editor = None;
        self.note_draft = None;
        let text = match text {
            Some(t) => t.trim().to_string(),
            None => {
                self.editing_note_id = None;
                return Err("no draft text");
            }
        };
        let editing_id = self.editing_note_id.take();
        if text.is_empty() {
            return Err("empty note");
        }
        if let Some(id) = editing_id {
            if let Some((rev_k, file_k, note)) = find_note_in_repo(&self.notes, &repo_key, &id) {
                let loc = note.location;
                let file = file_k.to_string();
                let rev = rev_k.to_string();
                let updated = update_note_text(&mut self.notes, &repo_key, &rev, &file, &id, text);
                if updated {
                    let _ = save_notes(&self.notes_path, &self.notes);
                    if let Some(fi) = self.output.files.iter().position(|f| f.path == file) {
                        self.sync_file_notes(fi);
                    }
                    self.bump_notes();
                    self.pending_notes_reload = Some(vec![file.clone()]);
                    self.set_status_message("updated note".to_string(), true);
                }
                return Ok(loc);
            }
            return Err("note to edit not found");
        }
        let (file, selected_rows) = if let Some(sel) = self.compute_selection() {
            let file = self
                .output
                .files
                .get(sel.file_idx)
                .expect("selection file_idx out of bounds in output.files");
            let selected_rows: Vec<_> = self
                .flat
                .iter()
                .filter(|fr| fr.file_idx == sel.file_idx)
                .filter_map(|fr| self.row_of(fr))
                .filter(|r| sel.contains_row(r))
                .cloned()
                .collect();
            (file.path.clone(), selected_rows)
        } else {
            let Some(file) = self.current_file() else {
                return Err("no current file");
            };
            let Some(row) = self.current_flat_row() else {
                return Err("no current row");
            };
            if let crate::types::FlatRowKind::FileHeader = row.kind {
                // File-level note — no rows selected.
                (file.path.clone(), Vec::new())
            } else {
                let Some(r) = self.row_of(row) else {
                    return Err("row not found");
                };
                if DiffCoord::from_row(r).is_none() {
                    return Err("row has no line coordinate");
                }
                (file.path.clone(), vec![r.clone()])
            }
        };
        self.note_id_counter += 1;
        let created_at = now_millis();
        // The complete structural hash over the captured rows: verified at
        // anchor time against the rows found at the note's coordinates.
        let content_hash = LineHash::of_rows(&selected_rows);
        let note_file = file.clone();
        // Build location from the row coords: both old and new are captured
        // where the row carries them. Single row → Line; multi-row → Range;
        // no rows → File-level note.
        let location = match selected_rows.as_slice() {
            [] => NoteLocation::File,
            [row] => match DiffCoord::from_row(row) {
                Some(coord) => NoteLocation::Line { coord },
                None => NoteLocation::File,
            },
            rows => {
                let first = DiffCoord::from_row(&rows[0]);
                let last = DiffCoord::from_row(rows.last().unwrap());
                match (first, last) {
                    (Some(start), Some(end)) => NoteLocation::Range { start, end },
                    (Some(coord), None) | (None, Some(coord)) => NoteLocation::Line { coord },
                    (None, None) => NoteLocation::File,
                }
            }
        };
        let note = ReviewNote {
            id: format!(
                "{:x}-{:x}-{:x}",
                created_at,
                process::id(),
                self.note_id_counter
            ),
            file,
            location,
            text,
            created_at,
            content_hash,
            status: diffuse_core::notes::NoteStatus::Open,
            // One-time capture of the selection's output rows: the original
            // source the note was written against.
            original_source_rows: selected_rows,
        };
        let rev_key = self.rev_key.as_deref().unwrap_or("");
        add_note(&mut self.notes, &repo_key, rev_key, note);
        let _ = save_notes(&self.notes_path, &self.notes);
        if let Some(fi) = self.output.files.iter().position(|f| f.path == note_file) {
            self.sync_file_notes(fi);
        }
        self.bump_notes();
        self.pending_notes_reload = Some(vec![note_file]);
        self.confirmed_sel = None;
        self.anchor = None;
        Ok(location)
    }
}
