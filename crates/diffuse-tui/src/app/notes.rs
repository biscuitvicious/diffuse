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
    /// current file (`💬 [start-end] <markdown>`). A note anchors to the
    /// LAST row whose coordinate falls in its range, else the file's first
    /// row — mirroring the reference `noteByRow`.
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
            let where_str = if n.end == n.start {
                format!("{}", n.start)
            } else {
                format!("{}-{}", n.start, n.end)
            };
            let mut body = String::new();
            for r in note_runs(&n.text) {
                let color = r.color.as_deref().unwrap_or("warning");
                body.push_str(&self.theme.render_token(color, &r.text, "ctx", None, r.sgr));
            }
            let line = format!("\x1b[38;5;220m   💬 [{}] \x1b[39m{}", where_str, body);
            out.entry(at).or_default().push(line);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        app::AppState,
        core::{
            PipelineOptions,
            notes::{load_notes, notes_for_file},
            pipeline::run_pipeline_full,
            wire::PipelineOutput,
        },
    };

    fn sample_output() -> PipelineOutput {
        let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
      let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
        let new_content = [("filesystem.rs".to_string(), "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n".to_string())].into_iter().collect();
        let old_content = [(
            "filesystem.rs".to_string(),
            "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
        )]
        .into_iter()
        .collect();
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        )
    }

    #[test]
    fn test_note_commit_persists_and_deletes() {
        let notes_path = std::env::temp_dir().join(format!(
            "diffuse-test-note-lifecycle-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&notes_path);
        let mut state = AppState::new(sample_output(), "test_repo".into(), notes_path.clone());

        let file = state.current_file().unwrap().path.clone();
        state.note_draft = Some("check this **carefully**".into());
        state.commit_note_draft();
        assert!(state.note_draft.is_none());

        let notes: Vec<_> = notes_for_file(&state.notes, "test_repo", &file).collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "check this **carefully**");
        assert!(
            notes[0].start > 0,
            "note must anchor to a real row coordinate"
        );

        let on_disk = load_notes(&notes_path);
        assert_eq!(
            notes_for_file(&on_disk, "test_repo", &file).count(),
            1,
            "commit must persist through the notes DB file"
        );

        let frame = state.render_frame(100, 30);
        assert!(
            frame.contains("💬"),
            "a committed note must render a callout: {:.200}",
            frame
        );

        state.delete_notes();
        assert!(
            notes_for_file(&state.notes, "test_repo", &file)
                .next()
                .is_none(),
            "delete must remove the note from the DB"
        );
        let on_disk2 = load_notes(&notes_path);
        assert!(
            notes_for_file(&on_disk2, "test_repo", &file)
                .next()
                .is_none(),
            "delete must persist the removal"
        );

        let _ = std::fs::remove_file(notes_path);
    }
}
