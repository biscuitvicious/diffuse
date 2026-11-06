//! Inline review note callout formatting, gutter striping, and focus
//! highlighting.

use std::collections::HashMap;

use ratatui::text::Line;

use crate::{
    app::AppState,
    core::notes::{ReviewNote, query_notes_for_file},
    render::{NoteCalloutFocus, render_note_callout_lines, render_note_callout_spans},
    view::span::line_from_spans,
};

/// Focus highlight for one row's note block: the cursor sits on the
/// `note_idx`-th attached note at its `line_idx`-th line.
pub(crate) struct NoteRowFocus {
    /// Index of the focused note among the row's attached notes.
    pub(crate) note_idx: usize,
    /// Cursor line within the focused note's full lines.
    pub(crate) line_idx: usize,
}

impl AppState {
    /// Map of flat-row index → pre-rendered note callout lines (`💬
    /// <markdown>`). Notes that anchor cleanly to their target line display
    /// inline on the last row in their coordinate range. Notes that do not
    /// anchor cleanly display attached to the file banner. Lines render
    /// directly from spans: no ANSI round-trip.
    pub(crate) fn note_callouts(&self, content_width: usize) -> HashMap<usize, Vec<Line<'static>>> {
        let mut out: HashMap<usize, Vec<Line<'static>>> = HashMap::new();
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
                let rendered_lines = render_note_callout_spans(
                    n.status,
                    &n.text,
                    &n.original_source_rows,
                    show_source,
                    self.syntax_on,
                    &self.theme,
                    content_width,
                    None,
                );
                out.entry(target_row)
                    .or_default()
                    .extend(rendered_lines.into_iter().map(line_from_spans));
            }
        }
        out
    }

    /// Re-render the note block attached to `row_idx` as styled lines,
    /// highlighting the focused note (cursor stripe plus cursor-background
    /// cursor line) when `focus` is set. Unfocused rows use the cached
    /// [`Self::note_callouts`] lines instead.
    pub(crate) fn note_block_lines(
        &self,
        row_idx: usize,
        content_width: usize,
        focus: Option<NoteRowFocus>,
    ) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        let mut note_idx = 0usize;
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
                if target_row != row_idx {
                    continue;
                }
                let is_unanchored = target_row == banner_idx;
                let show_source = self.show_note_source && is_unanchored;
                // The whole focused note keeps the active stripe; only the
                // cursor line takes the cursor background.
                let note_focus = focus.as_ref().and_then(|focus| {
                    (focus.note_idx == note_idx).then_some(NoteCalloutFocus {
                        start: 0,
                        len: usize::MAX,
                        cursor: Some(focus.line_idx),
                    })
                });
                let rendered_lines = render_note_callout_spans(
                    n.status,
                    &n.text,
                    &n.original_source_rows,
                    show_source,
                    self.syntax_on,
                    &self.theme,
                    content_width,
                    note_focus,
                );
                out.extend(rendered_lines.into_iter().map(line_from_spans));
                note_idx += 1;
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
                    let lines = render_note_callout_lines(
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
}
