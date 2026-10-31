//! Notes — draft commit/delete and callout generation.

use std::{collections::HashMap, process};

use crate::{render::ColorExt, types::FlatRowKind};

use crate::core::notes::{
    DiffCoord, LineHash, NoteLocation, NotesDB, ReviewNote, add_note, find_note_in_repo,
    note_anchors_in_rows, note_runs, now_millis, query_notes_for_file, remove_note_by_id,
    remove_notes_in_range, save_notes, update_note_text,
};

use super::AppState;

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

/// Target entity addressed by cursor coordinate `(row_idx, vis_line)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisualTarget<'a> {
    FileHeader,
    HunkDivider {
        file_idx: usize,
        hunk_idx: usize,
    },
    DiffRow {
        seg_idx: usize,
        total_segs: usize,
    },
    Note {
        note: &'a ReviewNote,
        note_idx: usize,
        line_idx: usize,
        total_note_lines: usize,
    },
    BottomGapDivider {
        file_idx: usize,
    },
}

/// Apply cursor focus highlight to a rendered note callout line: switches the
/// left-edge stripe from the status color to active cyan, and if
/// `is_current_line` is true, highlights the line background with cursor
/// background (`Fixed(237)`).
pub(crate) fn style_focused_note_line(nline: &str, is_current_line: bool) -> String {
    let gutter_fg = crate::render::Color::Rgb(220, 220, 20).fg_code();
    let glyph_fg = crate::render::Color::Fixed(255).fg_code();
    let cyan_fg = "\x1b[38;5;81m";
    let note_bg = crate::render::Color::Fixed(234).bg_code();
    let cursor_bg = crate::render::Color::Fixed(237).bg_code();
    let active_bg = if is_current_line {
        cursor_bg.as_str()
    } else {
        note_bg.as_str()
    };
    let fg_reset = crate::render::FG_RESET;
    const STRIPE_LEFT: &str = "\u{258C}";
    const BAND_BLOCK: &str = "\u{2588}";

    const STATUS_PAIRS: [(crate::render::Color, &str); 3] = [
        (crate::render::Color::Rgb(250, 75, 40), "💬"),
        (crate::render::Color::Rgb(60, 180, 240), "🔄"),
        (crate::render::Color::Rgb(40, 140, 60), "✅"),
    ];

    for (stripe_color, glyph) in STATUS_PAIRS {
        let stripe_fg = stripe_color.fg_code();
        let first_prefix = format!(
            "{note_bg}{stripe_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}{glyph} {fg_reset}"
        );
        let line_prefix = format!(
            "{note_bg}{stripe_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}│  {fg_reset}"
        );
        let final_prefix = format!(
            "{note_bg}{stripe_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}╰  {fg_reset}"
        );

        let focused_first = format!(
            "{active_bg}{cyan_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}{glyph} {fg_reset}"
        );
        let focused_line = format!(
            "{active_bg}{cyan_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}│  {fg_reset}"
        );
        let focused_final = format!(
            "{active_bg}{cyan_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}╰  {fg_reset}"
        );

        if let Some(rest) = nline.strip_prefix(&first_prefix) {
            return format!("{focused_first}{rest}");
        } else if let Some(rest) = nline.strip_prefix(&line_prefix) {
            return format!("{focused_line}{rest}");
        } else if let Some(rest) = nline.strip_prefix(&final_prefix) {
            return format!("{focused_final}{rest}");
        }
    }

    nline.to_string()
}

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
    pub fn commit_note_draft(&mut self) -> Result<crate::core::notes::NoteLocation, &'static str> {
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
                if crate::core::notes::DiffCoord::from_row(r).is_none() {
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

    /// Synchronize review notes and counts on an [`OutputFile`] in
    /// `self.output.files` with the active state in `self.notes`.
    pub(crate) fn sync_file_notes(&mut self, file_idx: usize) {
        if let Some(f) = self.output.files.get_mut(file_idx) {
            let notes: Vec<crate::core::wire::OutputNote> = query_notes_for_file(
                &self.notes,
                self.repo_key.as_deref(),
                self.rev_key.as_deref(),
                &f.path,
            )
            .into_iter()
            .map(|n| crate::core::wire::OutputNote {
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
        old: &crate::core::notes::NotesDB,
        new: &crate::core::notes::NotesDB,
        repo_key: Option<&str>,
    ) -> NotesChangeSummary {
        fn collect_notes<'a>(
            db: &'a crate::core::notes::NotesDB,
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

    /// Determine which flat row a note anchors to.
    ///
    /// Anchoring is coordinate-exact: the note must anchor against the file's
    /// output rows (exact both-side coordinate match plus complete hash), and
    /// the flat row whose coordinates exactly match the note's end coordinate
    /// receives the callout. Otherwise, the note falls back to the file
    /// header banner row.
    pub fn note_target_row(&self, file_idx: usize, note: &ReviewNote) -> usize {
        let banner_idx = self.first_row_of_file(file_idx);
        let Some(f) = self.output.files.get(file_idx) else {
            return banner_idx;
        };
        // The end coordinate of the location: where the callout attaches.
        let end_coord = match &note.location {
            NoteLocation::File => return banner_idx,
            NoteLocation::Line { coord } => *coord,
            NoteLocation::Range { end, .. } => *end,
        };
        // Collect the file's visible output rows for anchor verification —
        // the same side-projected rows the user sees and the note's rows
        // were captured from (render's `partition_file_notes` uses the same
        // view). Hunks are the fallback when no visible rows exist.
        let file_rows: Vec<_> = if f.rows.is_empty() {
            f.hunks.iter().flat_map(|h| h.rows.iter()).collect()
        } else {
            f.rows.iter().collect()
        };
        if !note_anchors_in_rows(note, file_rows.iter().copied()) {
            return banner_idx;
        }
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != file_idx {
                continue;
            }
            let Some(r) = self.row_of(item) else {
                continue;
            };
            if r.old_line == end_coord.old_line_num.map(|n| n.get())
                && r.new_line == end_coord.new_line_num.map(|n| n.get())
            {
                return i;
            }
        }
        banner_idx
    }

    /// Map of flat-row index → pre-rendered note callout lines (`💬
    /// <markdown>`). Notes that anchor cleanly to their target line display
    /// inline on the last row in their coordinate range. Notes that do not
    /// anchor cleanly display attached to the file banner.
    pub(crate) fn note_callouts(&self, content_width: usize) -> HashMap<usize, Vec<String>> {
        let mut out: HashMap<usize, Vec<String>> = HashMap::new();
        for (file_idx, f) in self.output.files.iter().enumerate() {
            let banner_idx = self.first_row_of_file(file_idx);
            for n in query_notes_for_file(
                &self.notes,
                self.repo_key.as_deref(),
                self.rev_key.as_deref(),
                &f.path,
            ) {
                if !self.show_resolved_notes && n.status.is_resolved() {
                    continue;
                }
                let target_row = self.note_target_row(file_idx, n);
                let is_unanchored = target_row == banner_idx;
                let show_source = self.show_note_source && is_unanchored;
                let rendered_lines = crate::render::render_note_callout_lines(
                    n.status,
                    &n.text,
                    &n.original_source_rows,
                    show_source,
                    self.syntax_on,
                    &self.theme,
                    content_width,
                );
                out.entry(target_row).or_default().extend(rendered_lines);
            }
        }
        out
    }

    /// Sequence of notes attached to `row_idx` along with their rendered line
    /// count.
    pub(crate) fn note_callout_layout_for_row(
        &self,
        row_idx: usize,
        content_width: usize,
    ) -> Vec<(&ReviewNote, usize)> {
        let mut results = Vec::new();
        for (file_idx, f) in self.output.files.iter().enumerate() {
            let banner_idx = self.first_row_of_file(file_idx);
            for n in query_notes_for_file(
                &self.notes,
                self.repo_key.as_deref(),
                self.rev_key.as_deref(),
                &f.path,
            ) {
                if !self.show_resolved_notes && n.status.is_resolved() {
                    continue;
                }
                let target_row = self.note_target_row(file_idx, n);
                if target_row == row_idx {
                    let is_unanchored = target_row == banner_idx;
                    let show_source = self.show_note_source && is_unanchored;
                    let lines = crate::render::render_note_callout_lines(
                        n.status,
                        &n.text,
                        &n.original_source_rows,
                        show_source,
                        self.syntax_on,
                        &self.theme,
                        content_width,
                    );
                    results.push((n, lines.len()));
                }
            }
        }
        results
    }

    /// Map a coordinate `(row_idx, vis_line)` to its semantic visual target.
    pub fn visual_target_at(&self, row_idx: usize, vis_line: usize) -> Option<VisualTarget<'_>> {
        let item = self.flat.get(row_idx)?;
        let cw = if self.last_content_width > 0 {
            self.last_content_width
        } else {
            80
        };
        let div_segs = if !self.raw_diff
            && let Some(hs) = self.hunk_starts.iter().find(|h| h.flat == row_idx)
        {
            self.hunk_div_seg_count(hs.file, hs.hunk, cw)
        } else {
            0
        };

        if vis_line < div_segs
            && let Some(hs) = self.hunk_starts.iter().find(|h| h.flat == row_idx)
        {
            return Some(VisualTarget::HunkDivider {
                file_idx: hs.file,
                hunk_idx: hs.hunk,
            });
        }

        match item.kind {
            FlatRowKind::FileHeader => {
                let segs = self.row_seg_count(row_idx, cw);
                if vis_line < div_segs + segs {
                    return Some(VisualTarget::FileHeader);
                }
            }
            FlatRowKind::HunkDivider { hunk_idx } => {
                return Some(VisualTarget::HunkDivider {
                    file_idx: item.file_idx,
                    hunk_idx,
                });
            }
            FlatRowKind::BottomGapDivider { .. } => {
                let segs = self.row_seg_count(row_idx, cw);
                if vis_line < div_segs + segs {
                    return Some(VisualTarget::BottomGapDivider {
                        file_idx: item.file_idx,
                    });
                }
            }
            FlatRowKind::DiffRow { .. } => {
                let segs = self.row_seg_count(row_idx, cw);
                if vis_line < div_segs + segs {
                    return Some(VisualTarget::DiffRow {
                        seg_idx: vis_line.saturating_sub(div_segs),
                        total_segs: segs,
                    });
                }
            }
        }

        let segs = self.row_seg_count(row_idx, cw);
        let note_start = div_segs + segs;
        if vis_line >= note_start {
            let mut note_rel = vis_line - note_start;
            let note_layout = self.note_callout_layout_for_row(row_idx, cw);
            for (n_idx, (note, note_total_lines)) in note_layout.into_iter().enumerate() {
                if note_rel < note_total_lines {
                    return Some(VisualTarget::Note {
                        note,
                        note_idx: n_idx,
                        line_idx: note_rel,
                        total_note_lines: note_total_lines,
                    });
                }
                note_rel -= note_total_lines;
            }
        }

        None
    }
}

#[cfg(test)]
mod tests;
