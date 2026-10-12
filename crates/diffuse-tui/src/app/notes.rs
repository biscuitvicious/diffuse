//! Notes — draft commit/delete and callout generation.

use std::{collections::HashMap, process};

use crate::{render::ColorExt, types::FlatRowKind};

use crate::core::notes::{
    LineHash, ReviewNote, Side, add_note, note_anchors_in_file, note_runs, notes_for_file,
    now_millis, remove_note_by_id, remove_notes, save_notes, update_note_text,
};

use super::AppState;

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
/// left-edge stripe from amber to active cyan, and if `is_current_line` is
/// true, highlights the line background with cursor background (`Fixed(237)`).
pub(crate) fn style_focused_note_line(nline: &str, is_current_line: bool) -> String {
    let gutter_fg = crate::render::Color::Rgb(220, 220, 20).fg_code();
    let glyph_fg = crate::render::Color::Fixed(255).fg_code();
    let amber_fg = crate::render::Color::Rgb(250, 75, 40).fg_code();
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

    let first_prefix = format!(
        "{note_bg}{amber_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}💬 {fg_reset}"
    );
    let line_prefix = format!(
        "{note_bg}{amber_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}│  {fg_reset}"
    );
    let final_prefix = format!(
        "{note_bg}{amber_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}╰  {fg_reset}"
    );

    let focused_first = format!(
        "{active_bg}{cyan_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}💬 {fg_reset}"
    );
    let focused_line = format!(
        "{active_bg}{cyan_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}│  {fg_reset}"
    );
    let focused_final = format!(
        "{active_bg}{cyan_fg}{STRIPE_LEFT}{gutter_fg}{BAND_BLOCK}{BAND_BLOCK}{BAND_BLOCK}{fg_reset} {glyph_fg}╰  {fg_reset}"
    );

    if let Some(rest) = nline.strip_prefix(&first_prefix) {
        format!("{focused_first}{rest}")
    } else if let Some(rest) = nline.strip_prefix(&line_prefix) {
        format!("{focused_line}{rest}")
    } else if let Some(rest) = nline.strip_prefix(&final_prefix) {
        format!("{focused_final}{rest}")
    } else {
        nline.to_string()
    }
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
    pub fn commit_note_draft(&mut self) {
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
                return;
            }
        };
        let editing_id = self.editing_note_id.take();
        if text.is_empty() {
            return;
        }
        if let Some(id) = editing_id {
            if let Some(file) = self.current_file().map(|f| f.path.clone()) {
                let updated = update_note_text(&mut self.notes, &self.notes_key, &file, &id, text);
                if updated {
                    let _ = save_notes(&self.notes_path, &self.notes);
                    if let Some(fi) = self.output.files.iter().position(|f| f.path == file) {
                        self.sync_file_notes(fi);
                    }
                    self.bump_notes();
                    self.set_status_message("updated note".to_string(), true);
                }
            }
            return;
        }
        let (file, start, end, is_new, selected_lines, source_lines) =
            if let Some(sel) = self.compute_selection() {
                let file = self
                    .output
                    .files
                    .get(sel.file_idx)
                    .expect("selection file_idx out of bounds in output.files");
                let mut selected_lines = Vec::new();
                for fr in &self.flat {
                    if fr.file_idx != sel.file_idx {
                        continue;
                    }
                    if let Some(r) = self.row_of(fr) {
                        let coord = if sel.side_is_new {
                            r.new_line
                        } else {
                            r.old_line
                        };
                        if let Some(c) = coord
                            && c >= sel.start
                            && c <= sel.end
                        {
                            let text = if sel.side_is_new {
                                r.new_text()
                            } else {
                                r.old_text()
                            };
                            selected_lines.push(text);
                        }
                    }
                }
                let source = (!selected_lines.is_empty()).then(|| selected_lines.join("\n"));
                (
                    file.path.clone(),
                    sel.start,
                    sel.end,
                    sel.side_is_new,
                    selected_lines,
                    source,
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
                let text = if is_new { r.new_text() } else { r.old_text() };
                (
                    file.path.clone(),
                    c,
                    c,
                    is_new,
                    vec![text.clone()],
                    Some(text),
                )
            };
        self.note_id_counter += 1;
        let created_at = now_millis();
        let line_hash = if !selected_lines.is_empty() {
            Some(LineHash::from_lines(
                selected_lines.iter().map(|s| s.as_str()),
            ))
        } else {
            None
        };
        let note_file = file.clone();
        let note = ReviewNote {
            id: format!(
                "{:x}-{:x}-{:x}",
                created_at,
                process::id(),
                self.note_id_counter
            ),
            file,
            side: if is_new { Side::New } else { Side::Old },
            start,
            end,
            text,
            created_at,
            line_hash,
            status: diffuse_core::notes::NoteStatus::Open,
            source_lines,
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

    /// Delete note focused under the cursor or across the active selection
    /// range. If the cursor is on a diff line without a selection, deletion
    /// is disabled to prevent accidental removal while navigating code.
    pub fn delete_note(&mut self) {
        if let Some(sel) = self.compute_selection() {
            let file = self
                .output
                .files
                .get(sel.file_idx)
                .expect("selection file_idx out of bounds in output.files");
            let file_path = file.path.clone();
            let new_removed = remove_notes(
                &mut self.notes,
                &self.notes_key,
                &file_path,
                Side::New,
                sel.start,
                sel.end,
            );
            let old_removed = remove_notes(
                &mut self.notes,
                &self.notes_key,
                &file_path,
                Side::Old,
                sel.start,
                sel.end,
            );
            let removed = new_removed + old_removed;
            let _ = save_notes(&self.notes_path, &self.notes);
            if let Some(fi) = self.output.files.iter().position(|f| f.path == file_path) {
                self.sync_file_notes(fi);
            }
            self.bump_notes();
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
            if let Some((file, note_id)) = note_info {
                let removed = remove_note_by_id(&mut self.notes, &self.notes_key, &file, &note_id);
                if removed {
                    let _ = save_notes(&self.notes_path, &self.notes);
                    if let Some(fi) = self.output.files.iter().position(|f| f.path == file) {
                        self.sync_file_notes(fi);
                    }
                    self.bump_notes();
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
        let focused_note = if let Some(target) = self.visual_target_at(self.row_idx, self.vis_line)
        {
            match target {
                VisualTarget::Note { note, .. } => Some((note.file.clone(), note.id.clone())),
                _ => None,
            }
        } else {
            None
        };

        let (file_path, note_id) = if let Some(info) = focused_note {
            info
        } else if let Some(sel) = self.compute_selection() {
            let file = &self.output.files[sel.file_idx];
            if let Some(n) = notes_for_file(&self.notes, &self.notes_key, &file.path)
                .find(|n| n.start <= sel.end && n.end >= sel.start)
            {
                (n.file.clone(), n.id.clone())
            } else {
                return false;
            }
        } else if let Some(file) = self.current_file()
            && let Some(row) = self.current_flat_row()
            && let Some(c) = self.flat_row_coord(row)
        {
            if let Some(n) = notes_for_file(&self.notes, &self.notes_key, &file.path)
                .find(|n| n.start <= c && n.end >= c)
            {
                (n.file.clone(), n.id.clone())
            } else {
                return false;
            }
        } else {
            return false;
        };

        let next_status = {
            let existing =
                notes_for_file(&self.notes, &self.notes_key, &file_path).find(|n| n.id == note_id);
            match existing {
                Some(n) => n.status.cycle(),
                None => return false,
            }
        };

        let updated = crate::core::notes::set_note_status_by_id(
            &mut self.notes,
            &self.notes_key,
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
        let msg = if self.show_resolved_notes {
            "showing resolved notes"
        } else {
            "hiding resolved notes"
        };
        self.set_status_message(msg.to_string(), true);
    }

    /// Toggle displaying original source lines in note callouts.
    pub fn toggle_show_source_lines(&mut self) {
        self.show_source_lines = !self.show_source_lines;
        self.bump_notes();
        let msg = if self.show_source_lines {
            "showing note source lines"
        } else {
            "hiding note source lines"
        };
        self.set_status_message(msg.to_string(), true);
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
                        status: n.status,
                        source_lines: n.source_lines.clone(),
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

    /// Determine which flat row a note anchors to.
    ///
    /// If the note anchors cleanly against the file's content lines, returns
    /// the row index corresponding to `note.end`. Otherwise, falls back to
    /// the file header banner row.
    pub fn note_target_row(&self, file_idx: usize, note: &ReviewNote) -> usize {
        let banner_idx = self.first_row_of_file(file_idx);
        let Some(f) = self.output.files.get(file_idx) else {
            return banner_idx;
        };
        let content_lines = match note.side {
            Side::New => &f.new_content,
            Side::Old => &f.old_content,
        };
        if !note_anchors_in_file(note, content_lines) {
            return banner_idx;
        }
        let target_coord = note.start.max(note.end);
        for (i, item) in self.flat.iter().enumerate() {
            if item.file_idx != file_idx {
                continue;
            }
            let Some(r) = self.row_of(item) else {
                continue;
            };
            let coord = match note.side {
                Side::New => r.new_line,
                Side::Old => r.old_line,
            };
            if coord == Some(target_coord) {
                return i;
            }
        }
        banner_idx
    }

    /// Map of flat-row index → pre-rendered note callout lines (`💬
    /// <markdown>`). Notes that have a line hash and anchor cleanly to
    /// their target line display inline on the last row in their coordinate
    /// range. Notes that do not anchor cleanly display attached to the file
    /// banner.
    pub(crate) fn note_callouts(&self, content_width: usize) -> HashMap<usize, Vec<String>> {
        let mut out: HashMap<usize, Vec<String>> = HashMap::new();
        for (file_idx, f) in self.output.files.iter().enumerate() {
            for n in notes_for_file(&self.notes, &self.notes_key, &f.path) {
                if !self.show_resolved_notes && n.status.is_resolved() {
                    continue;
                }
                let target_row = self.note_target_row(file_idx, n);
                let rendered_lines = crate::render::render_note_callout_lines(
                    n.status,
                    &n.text,
                    n.source_lines.as_deref(),
                    self.show_source_lines,
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
            for n in notes_for_file(&self.notes, &self.notes_key, &f.path) {
                if !self.show_resolved_notes && n.status.is_resolved() {
                    continue;
                }
                let target_row = self.note_target_row(file_idx, n);
                if target_row == row_idx {
                    let lines = crate::render::render_note_callout_lines(
                        n.status,
                        &n.text,
                        n.source_lines.as_deref(),
                        self.show_source_lines,
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
