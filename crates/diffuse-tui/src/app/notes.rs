//! Notes — draft commit/delete and callout generation.

use std::collections::HashMap;

use crate::core::notes::{
    ReviewNote, Side, add_note, hash_line_content, note_anchors_cleanly, note_runs, notes_for_file,
    now_millis, remove_notes, save_notes,
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
        let (file, start, end, is_new, line_content) = if let Some(sel) = self.compute_selection() {
            let file = self
                .output
                .files
                .get(sel.file_idx)
                .expect("selection file_idx out of bounds in output.files");
            let pos = if sel.side_is_new {
                self.flat_pos_of_coord(sel.file_idx, Some(sel.start), None)
            } else {
                self.flat_pos_of_coord(sel.file_idx, None, Some(sel.start))
            };
            let content = pos
                .and_then(|p| self.flat.get(p))
                .and_then(|fr| self.row_of(fr))
                .map(|r| {
                    if sel.side_is_new {
                        r.text.clone()
                    } else {
                        r.del_text.as_deref().unwrap_or(&r.text).to_string()
                    }
                });
            (
                file.path.clone(),
                sel.start,
                sel.end,
                sel.side_is_new,
                content,
            )
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
            let is_new = r.new_line.is_some();
            let text = if is_new {
                r.text.clone()
            } else {
                r.del_text.as_deref().unwrap_or(&r.text).to_string()
            };
            (file.path.clone(), c, c, is_new, Some(text))
        };
        self.note_id_counter += 1;
        let created_at = now_millis();
        let line_hash = line_content.as_deref().map(hash_line_content);
        let note_file = file.clone();
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
            line_hash,
        };
        add_note(&mut self.notes, &self.notes_key, note);
        let _ = save_notes(&self.notes_path, &self.notes);
        if let Some(fi) = self.output.files.iter().position(|f| f.path == note_file) {
            self.sync_file_notes(fi);
        }
        self.bump_notes();
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
        let new_removed = remove_notes(
            &mut self.notes,
            &self.notes_key,
            &file,
            Side::New,
            start,
            end,
        );
        let old_removed = remove_notes(
            &mut self.notes,
            &self.notes_key,
            &file,
            Side::Old,
            start,
            end,
        );
        let removed = new_removed + old_removed;
        let _ = save_notes(&self.notes_path, &self.notes);
        if let Some(fi) = self.output.files.iter().position(|f| f.path == file) {
            self.sync_file_notes(fi);
        }
        self.bump_notes();
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

    /// Synchronize review notes and counts on an [`OutputFile`] in
    /// `self.output.files` with the active state in `self.notes`.
    pub(crate) fn sync_file_notes(&mut self, file_idx: usize) {
        if let Some(f) = self.output.files.get_mut(file_idx) {
            let notes: Vec<crate::core::wire::OutputNote> =
                notes_for_file(&self.notes, &self.notes_key, &f.path)
                    .map(|n| crate::core::wire::OutputNote {
                        id: n.id.clone(),
                        file: n.file.clone(),
                        side: n.side,
                        start: n.start,
                        end: n.end,
                        text: n.text.clone(),
                        created_at: n.created_at,
                        runs: note_runs(&n.text),
                        line_hash: n.line_hash,
                    })
                    .collect();
            f.counts.notes = notes.len();
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

    /// Map of flat-row index → pre-rendered note callout lines (`💬
    /// <markdown>`). Notes that have a line hash and anchor cleanly to
    /// their target line display inline on the last row in their coordinate
    /// range. Notes that do not anchor cleanly, or do not have a line hash,
    /// display attached to the file banner.
    pub(crate) fn note_callouts(&self) -> HashMap<usize, Vec<String>> {
        let mut out: HashMap<usize, Vec<String>> = HashMap::new();
        for (file_idx, f) in self.output.files.iter().enumerate() {
            let banner_idx = self.first_row_of_file(file_idx);
            for n in notes_for_file(&self.notes, &self.notes_key, &f.path) {
                let mut at: Option<usize> = None;
                let mut anchor_line_content: Option<&str> = None;
                for (i, item) in self.flat.iter().enumerate() {
                    if item.file_idx != file_idx {
                        continue;
                    }
                    let Some(r) = self.row_of(item) else {
                        continue;
                    };
                    let (line_num, text) = match n.side {
                        Side::New => (r.new_line, r.text.as_str()),
                        Side::Old => (r.old_line, r.del_text.as_deref().unwrap_or(&r.text)),
                    };
                    if let Some(c) = line_num {
                        if c == n.start {
                            anchor_line_content = Some(text);
                        }
                        if c >= n.start && c <= n.end {
                            at = Some(i);
                        }
                    }
                }

                let target_row = if n.line_hash.is_some()
                    && let Some(content) = anchor_line_content
                    && note_anchors_cleanly(n, content)
                    && let Some(inline_idx) = at
                {
                    inline_idx
                } else {
                    banner_idx
                };

                let mut body = String::new();
                for r in note_runs(&n.text) {
                    let color = r.color.as_deref().unwrap_or("warning");
                    body.push_str(&self.theme.render_token(color, &r.text, "ctx", None, r.sgr));
                }
                let line = format!("\x1b[38;5;220m   💬 \x1b[39m{}", body);
                out.entry(target_row).or_default().push(line);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
