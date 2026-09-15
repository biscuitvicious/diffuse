//! Notes — draft commit/delete and callout generation.

use std::collections::HashMap;

use crate::core::notes::{
    ReviewNote, Side, add_note, note_runs, notes_for_file, now_millis, remove_notes, save_notes,
};

use super::AppState;

impl AppState {
    /// Commit the note draft onto the current row (or active selection) and
    /// persist it through the notes DB. Mirrors the reference
    /// `commitNoteAction`.
    pub fn commit_note_draft(&mut self) {
        let text = match self.note_draft.take() {
            Some(t) => t.trim().to_string(),
            None => return,
        };
        if text.is_empty() {
            return;
        }
        let (file, start, end, is_new) = if let Some(sel) = self.compute_selection() {
            let file = self
                .output
                .files
                .get(sel.file_idx)
                .expect("selection file_idx out of bounds in output.files");
            (file.path.clone(), sel.start, sel.end, sel.side_is_new)
        } else {
            let Some(file) = self.current_file() else {
                return;
            };
            let Some(row) = self.current_flat_row() else {
                return;
            };
            let Some(r) = self.row_of(row) else {
                return;
            };
            let Some(c) = Self::row_coord(r) else {
                return;
            };
            (file.path.clone(), c, c, r.new_line.is_some())
        };
        self.note_id_counter += 1;
        let created_at = now_millis();
        let note = ReviewNote {
            id: format!(
                "{:x}-{:x}-{:x}",
                created_at,
                std::process::id(),
                self.note_id_counter
            ),
            file,
            side: if is_new { Side::New } else { Side::Old },
            start,
            end,
            text,
            created_at,
        };
        add_note(&mut self.notes, &self.repo, note);
        let _ = save_notes(&self.notes_path, &self.notes);
        self.confirmed_sel = None;
        self.anchor = None;
    }

    /// Delete notes overlapping the current row or active selection.
    /// Persists immediately.
    pub fn delete_notes(&mut self) {
        let (file, start, end) = if let Some(sel) = self.compute_selection() {
            let file = self
                .output
                .files
                .get(sel.file_idx)
                .expect("selection file_idx out of bounds in output.files");
            (file.path.clone(), sel.start, sel.end)
        } else {
            let Some(file) = self.current_file() else {
                return;
            };
            let Some(row) = self.current_flat_row() else {
                return;
            };
            let Some(c) = self.flat_row_coord(row) else {
                return;
            };
            (file.path.clone(), c, c)
        };
        let new_removed = remove_notes(&mut self.notes, &self.repo, &file, Side::New, start, end);
        let old_removed = remove_notes(&mut self.notes, &self.repo, &file, Side::Old, start, end);
        let removed = new_removed + old_removed;
        let _ = save_notes(&self.notes_path, &self.notes);
        self.confirmed_sel = None;
        self.anchor = None;
        self.status_message = Some((
            if removed > 0 {
                format!(
                    "deleted {removed} note{}",
                    if removed == 1 { "" } else { "s" }
                )
            } else {
                "no note here".to_string()
            },
            removed > 0,
        ));
    }

    /// Map of flat-row index → pre-rendered note callout lines for the
    /// current file (`💬 <markdown>`). A note anchors to the LAST row whose
    /// coordinate falls in its range, else the file's first row — mirroring
    /// the reference `noteByRow`.
    pub(crate) fn note_callouts(&self) -> HashMap<usize, Vec<String>> {
        let Some(f) = self.current_file() else {
            return HashMap::new();
        };
        let file_idx = self.current_file_idx();
        let mut out: HashMap<usize, Vec<String>> = HashMap::new();
        for n in notes_for_file(&self.notes, &self.repo, &f.path) {
            let mut at: Option<usize> = None;
            for (i, item) in self.flat.iter().enumerate() {
                if item.file_idx != file_idx {
                    continue;
                }
                if let Some(c) = self.flat_row_coord(item)
                    && c >= n.start
                    && c <= n.end
                {
                    at = Some(i);
                }
            }
            let at = at.unwrap_or_else(|| {
                self.flat
                    .iter()
                    .position(|it| it.file_idx == file_idx)
                    .unwrap_or(0)
            });
            let mut body = String::new();
            for r in note_runs(&n.text) {
                let color = r.color.as_deref().unwrap_or("warning");
                body.push_str(&self.theme.render_token(color, &r.text, "ctx", None, r.sgr));
            }
            let line = format!("\x1b[38;5;220m   💬 \x1b[39m{}", body);
            out.entry(at).or_default().push(line);
        }
        out
    }
}

#[cfg(test)]
mod tests;
