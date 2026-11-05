//! Inline Review Note Callout Rendering.
//!
//! Formats inline boxed bubble callouts (`▌███ 💬`, `│`, `╰`) for review
//! notes attached to code rows or file banners.

use diffuse_core::{
    notes::NoteStatus,
    wire::{OutputNote, OutputRow},
};

use crate::{
    ansi::{Color, ColorExt, FG_RESET, strip_ansi},
    diff::render_source_rows,
    markdown::{MarkdownLine, markdown_lines, render_markdown_line},
    row::expand_tabs,
    theme::DiffuseTheme,
    width::display_width,
};

/// Default line wrap width for review note callout formatting.
pub const DEFAULT_CALLOUT_WIDTH: usize = 80;

/// Render the formatted ANSI lines of a single review note callout box.
///
/// When `show_source` is set and the note carries `original_source_rows`,
/// the captured source snapshot is prepended above the callout box, rendered
/// with the standard diff row coloring for each row's kind and language
/// token runs, mimicking the original selected text placement when the note was
/// created.
pub fn render_note_callout_lines(
    status: NoteStatus,
    text: &str,
    source_rows: &[OutputRow],
    show_source: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
    width: usize,
) -> Vec<String> {
    let gutter_fg = Color::Rgb(220, 220, 20).fg_code();
    let glyph_fg = Color::Fixed(255).fg_code();
    let note_bg = Color::Fixed(234).bg_code();
    let fg_reset = FG_RESET;
    const STRIPE_LEFT: &str = "\u{258C}";
    const BAND_BLOCK: &str = "\u{2588}";
    let text_cols = width.saturating_sub(8).max(1);

    let (stripe_color, glyph) = match status {
        NoteStatus::Open => (Color::Rgb(250, 75, 40), "💬"),
        NoteStatus::InProgress => (Color::Rgb(60, 180, 240), "🔄"),
        NoteStatus::Resolved => (Color::Rgb(40, 140, 60), "✅"),
    };
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

    let lines = markdown_lines(text, text_cols);
    let lines = if lines.is_empty() {
        vec![MarkdownLine { runs: Vec::new() }]
    } else {
        lines
    };

    let mut rendered = Vec::new();
    if show_source && !source_rows.is_empty() {
        rendered.extend(render_source_rows(source_rows, theme, width, true));
    }

    let total = lines.len();
    rendered.extend(lines.iter().enumerate().map(|(visual_idx, line)| {
        let is_last = visual_idx + 1 == total;
        let prefix = match visual_idx {
            0 => first_prefix.as_str(),
            _ if total > 1 && is_last => final_prefix.as_str(),
            _ => line_prefix.as_str(),
        };
        let mut rendered = render_markdown_line(line, prefix, syntax_on, theme);
        if is_last && width > 0 {
            let plain = strip_ansi(&rendered);
            let exp = expand_tabs(&plain, 4);
            let vis_w = display_width(&exp);
            let rem = vis_w % width;
            let pad = if rem == 0 && vis_w > 0 {
                0
            } else {
                width - rem
            };
            for _ in 0..pad {
                rendered.push(' ');
            }
        }
        rendered
    }));

    rendered
}

/// Render the formatted ANSI lines of an [`OutputNote`].
pub fn render_output_note_callout(
    note: &OutputNote,
    show_source: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
    width: usize,
) -> Vec<String> {
    render_note_callout_lines(
        note.status,
        &note.text,
        &note.original_source_rows,
        show_source,
        syntax_on,
        theme,
        width,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::strip_ansi;

    #[test]
    fn test_render_note_callout_statuses() {
        let theme = DiffuseTheme::dark();

        let open =
            render_note_callout_lines(NoteStatus::Open, "open note", &[], false, true, &theme, 80);
        assert_eq!(open.len(), 1);
        assert!(strip_ansi(&open[0]).contains("▌███ 💬 open note"));

        let in_progress = render_note_callout_lines(
            NoteStatus::InProgress,
            "working on it",
            &[],
            false,
            true,
            &theme,
            80,
        );
        assert_eq!(in_progress.len(), 1);
        assert!(strip_ansi(&in_progress[0]).contains("▌███ 🔄 working on it"));

        let resolved = render_note_callout_lines(
            NoteStatus::Resolved,
            "fixed now",
            &[],
            false,
            true,
            &theme,
            80,
        );
        assert_eq!(resolved.len(), 1);
        assert!(strip_ansi(&resolved[0]).contains("▌███ ✅ fixed now"));
    }

    #[test]
    fn test_render_note_callout_multiline_prefixes() {
        let theme = DiffuseTheme::dark();
        let note = "- line one\n- line two\n- line three";
        let lines = render_note_callout_lines(NoteStatus::Open, note, &[], false, true, &theme, 80);
        assert_eq!(lines.len(), 3);
        let plain: Vec<_> = lines.iter().map(|l| strip_ansi(l).to_string()).collect();
        assert!(plain[0].starts_with("▌███ 💬 • line one"));
        assert!(plain[1].starts_with("▌███ │  • line two"));
        assert!(plain[2].starts_with("▌███ ╰  • line three"));
    }

    #[test]
    fn test_render_note_callout_empty_text() {
        let theme = DiffuseTheme::dark();
        let lines = render_note_callout_lines(NoteStatus::Open, "", &[], false, true, &theme, 80);
        assert_eq!(lines.len(), 1);
        let plain = strip_ansi(&lines[0]);
        assert!(plain.starts_with("▌███ 💬 "));
        assert_eq!(crate::width::display_width(&plain), 80);
    }

    #[test]
    fn test_render_output_note_callout() {
        let theme = DiffuseTheme::dark();
        let out_note = OutputNote {
            id: "test-note".to_string(),
            file: "foo.rs".to_string(),
            location: diffuse_core::notes::NoteLocation::line(
                diffuse_core::notes::DiffCoord::new_side(1),
            ),
            text: "test note content".to_string(),
            created_at: 100,
            runs: vec![],
            content_hash: None,
            status: NoteStatus::Open,
            original_source_rows: Vec::new(),
        };
        let lines = render_output_note_callout(&out_note, false, true, &theme, 80);
        assert_eq!(lines.len(), 1);
        assert!(strip_ansi(&lines[0]).contains("▌███ 💬 test note content"));
    }

    #[test]
    fn test_render_note_callout_prepends_source_rows() {
        let theme = DiffuseTheme::dark();
        let rows = vec![diffuse_core::wire::OutputRow {
            kind: diffuse_core::parse::RowKind::Add,
            mov_side: None,
            new_line: Some(2),
            old_line: None,
            runs: vec![diffuse_core::wire::OutputRun::new(
                diffuse_core::merge::RunKind::Add,
                "let x = 42;".to_string(),
            )],
            wholesale: false,
            group: None,
        }];

        // Hidden: only the callout box.
        let hidden =
            render_note_callout_lines(NoteStatus::Open, "note", &rows, false, true, &theme, 80);
        assert_eq!(hidden.len(), 1);
        assert!(!strip_ansi(&hidden[0]).contains("let x = 42;"));

        // Shown: snapshot rows appear *above* the callout box mimicking original code
        // placement.
        let shown =
            render_note_callout_lines(NoteStatus::Open, "note", &rows, true, true, &theme, 80);
        assert_eq!(shown.len(), 2);
        let src_plain = strip_ansi(&shown[0]);
        assert!(
            src_plain.contains('+'),
            "add row renders with its sign: {src_plain:?}"
        );
        assert!(src_plain.contains("let x = 42;"));

        let note_plain = strip_ansi(&shown[1]);
        assert!(
            note_plain.contains("💬 note"),
            "callout box follows beneath the original source lines: {note_plain:?}"
        );
    }

    #[test]
    fn test_render_note_callout_multiline_pads_last_line_to_width() {
        let theme = DiffuseTheme::dark();
        let note = "- line one\n- line two";
        let width = 80;
        let lines =
            render_note_callout_lines(NoteStatus::Open, note, &[], false, true, &theme, width);
        assert_eq!(lines.len(), 2);

        let first_plain = strip_ansi(&lines[0]);
        assert!(crate::width::display_width(&first_plain) < width);

        let last_plain = strip_ansi(&lines[1]);
        assert_eq!(crate::width::display_width(&last_plain), width);
    }

    #[test]
    fn test_render_note_callout_single_line_pads_to_width() {
        let theme = DiffuseTheme::dark();
        let note = "single line";
        let width = 80;
        let lines =
            render_note_callout_lines(NoteStatus::Open, note, &[], false, true, &theme, width);
        assert_eq!(lines.len(), 1);

        let plain = strip_ansi(&lines[0]);
        assert_eq!(crate::width::display_width(&plain), width);
    }
}
