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
    core::parse::RowKind,
    diff::render_source_rows,
    markdown::{MarkdownLine, MarkdownRun, markdown_lines, render_markdown_line},
    row::{
        continuation_prefix_spans, display_text, expand_tabs, row_segment_spans,
        styled_prefix_spans,
    },
    span::{StyledSpan, pad_spans},
    theme::{Cursor, DiffKind, DiffuseTheme},
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
    let text_cols = width.saturating_sub(8).max(1);

    let (stripe_color, glyph) = note_stripe(status);
    // Style decisions come from `note_prefix_spans` (shared with the span
    // path); the serializer below keeps the exact prefix skeleton — one
    // leading background, each piece's foreground (an uncolored piece
    // resets instead), and a single trailing reset — so output stays
    // byte-identical.
    let prefix_for = |marker: &str| -> String {
        let spans = note_prefix_spans(marker, stripe_color, Some(NOTE_BG));
        let mut prefix =
            String::with_capacity(spans.iter().map(|span| span.text.len()).sum::<usize>() + 64);
        let bg = spans
            .first()
            .and_then(|span| span.bg)
            .map(|color| color.bg_code())
            .unwrap_or_default();
        prefix.push_str(&bg);
        for span in &spans {
            match span.fg.filter(|color| *color != Color::Primary) {
                Some(color) => prefix.push_str(&color.fg_code()),
                None => prefix.push_str(FG_RESET),
            }
            prefix.push_str(&span.text);
        }
        prefix.push_str(FG_RESET);
        prefix
    };
    let first_prefix = prefix_for(glyph);
    let line_prefix = prefix_for("│  ");
    let final_prefix = prefix_for("╰  ");

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

/// Review note callout background and gutter colors, shared by the ANSI
/// and span paths so both paint identical stripes.
const NOTE_BG: Color = Color::Fixed(234);
const NOTE_GUTTER: Color = Color::Rgb(220, 220, 20);
const NOTE_GLYPH: Color = Color::Fixed(255);
const NOTE_FOCUSED_STRIPE: Color = Color::Fixed(81);

/// Stripe color and first-line marker (glyph plus its trailing space) for
/// a note status.
fn note_stripe(status: NoteStatus) -> (Color, &'static str) {
    match status {
        NoteStatus::Open => (Color::Rgb(250, 75, 40), "💬 "),
        NoteStatus::InProgress => (Color::Rgb(60, 180, 240), "🔄 "),
        NoteStatus::Resolved => (Color::Rgb(40, 140, 60), "✅ "),
    }
}

/// Focus highlight for note callout markdown lines: the line range (within
/// the markdown part, before any source rows) rendered with the active
/// stripe, and the cursor line on the cursor background.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteCalloutFocus {
    /// First focused markdown line.
    pub start: usize,
    /// Focused markdown line count.
    pub len: usize,
    /// Cursor markdown line, painted on the cursor background.
    pub cursor: Option<usize>,
}

/// Gutter stripe prefix pieces for one note callout line: the status
/// stripe, the gutter blocks, a space, and the position marker (which
/// carries its own trailing spacing, matching the ANSI prefixes where only
/// the first line has a trailing space).
fn note_prefix_spans(marker: &str, stripe: Color, line_bg: Option<Color>) -> Vec<StyledSpan> {
    vec![
        StyledSpan::styled("▌", Some(stripe), line_bg),
        StyledSpan::styled("███", Some(NOTE_GUTTER), line_bg),
        StyledSpan::styled(" ", None, line_bg),
        StyledSpan::styled(marker, Some(NOTE_GLYPH), line_bg),
    ]
}

/// Project one Markdown run onto a styled span, stacking the run flags
/// with its SGR id exactly as [`crate::markdown::write_markdown_line`]
/// combines their escapes.
fn markdown_run_span(
    run: &MarkdownRun,
    line_bg: Option<Color>,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> StyledSpan {
    if !syntax_on {
        return StyledSpan::styled(run.text.to_string(), None, line_bg);
    }
    let mut span = StyledSpan::styled(
        run.text.to_string(),
        theme.token_color(run.color, RowKind::Ctx),
        line_bg,
    );
    if run.underline {
        span = span.with_underline(true);
    }
    if run.strike {
        span = span.with_strikethrough(true);
    }
    if run.dim {
        span = span.with_dim(true);
    }
    if let Some(sgr) = run.sgr {
        span = span.with_sgr(sgr);
    }
    span
}

/// Render captured source rows as span lines mirroring
/// [`render_source_rows`]: styled prefixes, wrapping at `width`, and
/// padding to `width` on each row's line background.
fn render_note_source_spans(
    rows: &[OutputRow],
    theme: &DiffuseTheme,
    width: usize,
) -> Vec<Vec<StyledSpan>> {
    let mut out = Vec::with_capacity(rows.len() * 2);
    for row in rows {
        let prefix = styled_prefix_spans(
            row.kind,
            row.mov_side,
            None,
            None,
            row.group,
            row.new_line,
            row.old_line,
            &row.runs,
            false,
            None,
            theme,
        );
        let text = row.text();
        if width == 0 {
            let line = row_segment_spans(
                &text,
                row.is_peer(),
                row.kind,
                row.mov_side,
                None,
                &row.runs,
                0,
                usize::MAX,
                prefix,
                false,
                false,
                0,
                true,
                true,
                theme,
            );
            out.push(line.spans);
            continue;
        }
        let text_cols = width.saturating_sub(6).max(1);
        let disp = display_text(&text, row.is_peer(), row.kind, &row.runs, 0, true);
        let exp = expand_tabs(&disp, 4);
        let len = display_width(&exp);
        let seg_count = if len == 0 { 1 } else { len.div_ceil(text_cols) };
        if seg_count == 1 {
            let line = row_segment_spans(
                &text,
                row.is_peer(),
                row.kind,
                row.mov_side,
                None,
                &row.runs,
                0,
                usize::MAX,
                prefix,
                false,
                false,
                0,
                true,
                true,
                theme,
            );
            out.push(pad_spans(line.spans, width, line.line_bg));
            continue;
        }
        for seg_idx in 0..seg_count {
            let seg_prefix = if seg_idx == 0 {
                prefix.clone()
            } else {
                continuation_prefix_spans(None)
            };
            let seg_start = seg_idx * text_cols;
            let line = row_segment_spans(
                &text,
                row.is_peer(),
                row.kind,
                row.mov_side,
                None,
                &row.runs,
                seg_start,
                seg_start + text_cols,
                seg_prefix,
                false,
                false,
                0,
                true,
                true,
                theme,
            );
            out.push(pad_spans(line.spans, width, line.line_bg));
        }
    }
    out
}

/// Render the formatted note callout box as span lines: the span-native
/// twin of [`render_note_callout_lines`]. Branching (source rows, stripe
/// prefixes, markdown runs, last-line padding) mirrors the ANSI function
/// arm-for-arm; every line is padded to exactly `width` on its own
/// background. `focus` highlights the markdown line range with the active
/// stripe, painting the cursor line on the cursor background.
#[allow(clippy::too_many_arguments)]
pub fn render_note_callout_spans(
    status: NoteStatus,
    text: &str,
    source_rows: &[OutputRow],
    show_source: bool,
    syntax_on: bool,
    theme: &DiffuseTheme,
    width: usize,
    focus: Option<NoteCalloutFocus>,
) -> Vec<Vec<StyledSpan>> {
    let note_bg = Some(NOTE_BG);
    let cursor_bg = Some(theme.palette[Cursor::Row][DiffKind::Ctx].surface.bg);
    let (stripe_color, glyph) = note_stripe(status);

    let mut out: Vec<Vec<StyledSpan>> = Vec::new();
    if show_source && !source_rows.is_empty() {
        out.extend(render_note_source_spans(source_rows, theme, width));
    }

    let text_cols = width.saturating_sub(8).max(1);
    let lines = markdown_lines(text, text_cols);
    let lines = if lines.is_empty() {
        vec![MarkdownLine { runs: Vec::new() }]
    } else {
        lines
    };

    let total = lines.len();
    for (visual_idx, line) in lines.iter().enumerate() {
        let is_last = visual_idx + 1 == total;
        let focused = focus
            .is_some_and(|focus| visual_idx >= focus.start && visual_idx < focus.start + focus.len);
        let line_bg = if focused && focus.is_some_and(|focus| focus.cursor == Some(visual_idx)) {
            cursor_bg
        } else {
            note_bg
        };
        let stripe = if focused {
            NOTE_FOCUSED_STRIPE
        } else {
            stripe_color
        };
        let marker = match visual_idx {
            0 => glyph,
            _ if total > 1 && is_last => "╰  ",
            _ => "│  ",
        };
        let mut spans = note_prefix_spans(marker, stripe, line_bg);
        for run in &line.runs {
            spans.push(markdown_run_span(run, line_bg, syntax_on, theme));
        }
        if is_last && width > 0 {
            let plain: String = spans.iter().map(|span| span.text.as_str()).collect();
            let vis_w = display_width(&plain);
            let rem = vis_w % width;
            let pad = if rem == 0 && vis_w > 0 {
                0
            } else {
                width - rem
            };
            for _ in 0..pad {
                spans.push(StyledSpan::styled(" ", None, line_bg));
            }
        }
        out.push(spans);
    }
    out
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

        // Shown: snapshot rows appear *above* the callout box mimicking
        // original code placement.
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

#[cfg(test)]
mod span_tests {
    use super::*;
    use crate::{span::spans_width, width::display_width};

    fn span_text(spans: &[crate::span::StyledSpan]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn test_note_callout_spans_match_ansi_text() {
        let theme = DiffuseTheme::dark();
        let note = "- line one\n- line two\n- line three";
        for syntax_on in [false, true] {
            let ansi = render_note_callout_lines(
                NoteStatus::Open,
                note,
                &[],
                false,
                syntax_on,
                &theme,
                80,
            );
            let spans = render_note_callout_spans(
                NoteStatus::Open,
                note,
                &[],
                false,
                syntax_on,
                &theme,
                80,
                None,
            );
            assert_eq!(
                spans.len(),
                ansi.len(),
                "span lines must match ANSI line count"
            );
            for (line, ansi_line) in spans.iter().zip(ansi.iter()) {
                assert_eq!(
                    span_text(line),
                    strip_ansi(ansi_line).to_string(),
                    "span text must match ANSI text"
                );
                assert_eq!(
                    spans_width(line),
                    display_width(strip_ansi(ansi_line).as_str())
                );
            }
        }
    }

    #[test]
    fn test_note_callout_spans_carry_stripe_and_focus() {
        let theme = DiffuseTheme::dark();
        let lines = render_note_callout_spans(
            NoteStatus::Open,
            "open note",
            &[],
            false,
            true,
            &theme,
            80,
            None,
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0][0].text, "▌");
        assert_eq!(lines[0][0].fg, Some(crate::ansi::Color::Rgb(250, 75, 40)));
        // Every piece of an unfocused line rides on the note background.
        assert!(
            lines[0]
                .iter()
                .all(|span| span.bg == Some(crate::ansi::Color::Fixed(234)))
        );

        let focused = render_note_callout_spans(
            NoteStatus::Open,
            "open note",
            &[],
            false,
            true,
            &theme,
            80,
            Some(NoteCalloutFocus {
                start: 0,
                len: 1,
                cursor: Some(0),
            }),
        );
        assert_eq!(focused[0][0].fg, Some(crate::ansi::Color::Fixed(81)));
        assert_eq!(
            focused[0][0].bg,
            Some(
                theme.palette[crate::theme::Cursor::Row][crate::theme::DiffKind::Ctx]
                    .surface
                    .bg
            )
        );
    }
}
