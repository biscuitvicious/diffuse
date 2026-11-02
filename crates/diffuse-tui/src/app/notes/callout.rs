//! Inline review note callout formatting, gutter striping, and focus
//! highlighting.

use std::collections::HashMap;

use crate::{
    app::AppState,
    core::notes::{ReviewNote, query_notes_for_file},
    render::{Color, ColorExt, FG_RESET, render_note_callout_lines},
};

/// Apply cursor focus highlight to a rendered note callout line: switches the
/// left-edge stripe from the status color to active cyan, and if
/// `is_current_line` is true, highlights the line background with cursor
/// background (`Fixed(237)`).
pub(crate) fn style_focused_note_line(nline: &str, is_current_line: bool) -> String {
    let gutter_fg = Color::Rgb(220, 220, 20).fg_code();
    let glyph_fg = Color::Fixed(255).fg_code();
    let cyan_fg = "\x1b[38;5;81m";
    let note_bg = Color::Fixed(234).bg_code();
    let cursor_bg = Color::Fixed(237).bg_code();
    let active_bg = if is_current_line {
        cursor_bg.as_str()
    } else {
        note_bg.as_str()
    };
    let fg_reset = FG_RESET;
    const STRIPE_LEFT: &str = "\u{258C}";
    const BAND_BLOCK: &str = "\u{2588}";

    const STATUS_PAIRS: [(Color, &str); 3] = [
        (Color::Rgb(250, 75, 40), "💬"),
        (Color::Rgb(60, 180, 240), "🔄"),
        (Color::Rgb(40, 140, 60), "✅"),
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
                let rendered_lines = render_note_callout_lines(
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
