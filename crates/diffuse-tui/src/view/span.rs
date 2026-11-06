//! Ratatui projection of renderer-agnostic styled spans.
//!
//! Converts [`StyledSpan`](crate::render::span::StyledSpan) slices into
//! Ratatui [`Line`]s, slicing and padding by visual columns so grid rows
//! fill their viewport exactly without an ANSI round-trip.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use yansi::Color;

use crate::render::span::{StyledSpan, pad_spans};

/// Project a [`StyledSpan`] onto a Ratatui [`Style`]. Unset and
/// [`Color::Primary`] colors stay unset so the span inherits the paragraph
/// base, matching the ANSI path where primary resolves to an empty code.
#[must_use]
pub fn span_style(span: &StyledSpan) -> Style {
    let mut style = Style::default();
    if let Some(fg) = span.fg.filter(|color| *color != Color::Primary) {
        style = style.fg(crate::view::to_color(fg));
    }
    if let Some(bg) = span.bg.filter(|color| *color != Color::Primary) {
        style = style.bg(crate::view::to_color(bg));
    }
    if span.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if span.dim {
        style = style.add_modifier(Modifier::DIM);
    }
    if span.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if span.underline {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    if span.strikethrough {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

/// Build a Ratatui [`Line`] from styled spans, dropping empty pieces so
/// adjacent styling never produces zero-width spans.
#[must_use]
pub fn line_from_spans(spans: Vec<StyledSpan>) -> Line<'static> {
    Line::from(
        spans
            .into_iter()
            .filter(|span| !span.text.is_empty())
            .map(|span| {
                let style = span_style(&span);
                Span::styled(span.text, style)
            })
            .collect::<Vec<_>>(),
    )
}

/// Build a Ratatui [`Line`] from styled spans truncated or padded with
/// spaces on `pad_bg` to exactly `width` visual columns.
#[must_use]
pub fn fit_line(spans: Vec<StyledSpan>, width: usize, pad_bg: Option<Color>) -> Line<'static> {
    line_from_spans(pad_spans(spans, width, pad_bg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::span::modifiers_for_sgr;

    #[test]
    fn test_span_style_maps_all_modifier_flags() {
        let span = StyledSpan::plain("hi")
            .with_bold(true)
            .with_dim(true)
            .with_italic(true)
            .with_underline(true)
            .with_strikethrough(true);
        let style = span_style(&span);
        for modifier in [
            Modifier::BOLD,
            Modifier::DIM,
            Modifier::ITALIC,
            Modifier::UNDERLINED,
            Modifier::CROSSED_OUT,
        ] {
            assert!(
                style.add_modifier.contains(modifier),
                "missing {modifier:?}"
            );
        }
        assert_eq!(style.fg, None);
        assert_eq!(style.bg, None);
    }

    #[test]
    fn test_span_style_maps_palette_colors() {
        let span = StyledSpan::styled("hi", Some(Color::Fixed(120)), Some(Color::Rgb(1, 2, 3)));
        let style = span_style(&span);
        assert_eq!(style.fg, Some(crate::view::to_color(Color::Fixed(120))));
        assert_eq!(style.bg, Some(crate::view::to_color(Color::Rgb(1, 2, 3))));
    }

    #[test]
    fn test_span_style_treats_primary_as_inherit() {
        // `Color::Primary` means "no escape emitted" on the ANSI path, so it
        // must project to an unset Ratatui color (inheriting the paragraph
        // base) rather than an explicit reset.
        let span = StyledSpan::styled("hi", Some(Color::Primary), None);
        let style = span_style(&span);
        assert_eq!(style.fg, None);
        assert_eq!(style.bg, None);
    }

    #[test]
    fn test_sgr_ids_project_to_ratatui_modifiers() {
        let span = StyledSpan::plain("hi").with_sgr(9);
        let style = span_style(&span);
        assert!(style.add_modifier.contains(Modifier::CROSSED_OUT));
        assert!(style.add_modifier.contains(Modifier::DIM));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
        assert!(modifiers_for_sgr(14).bold && modifiers_for_sgr(14).underline);
    }

    #[test]
    fn test_line_from_spans_preserves_text_and_styles() {
        let spans = vec![
            StyledSpan::styled("ab", Some(Color::Fixed(1)), None),
            StyledSpan::plain("cd"),
        ];
        let line = line_from_spans(spans);
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content, "ab");
        assert_eq!(
            line.spans[0].style.fg,
            Some(crate::view::to_color(Color::Fixed(1)))
        );
        assert_eq!(line.spans[1].content, "cd");
        assert_eq!(line.spans[1].style.fg, None);
        // Empty pieces never become empty spans.
        let line = line_from_spans(vec![StyledSpan::plain(String::new())]);
        assert!(line.spans.is_empty());
    }

    #[test]
    fn test_fit_line_truncates_and_pads_to_width() {
        // Truncation keeps the leading columns with styles intact.
        let spans = vec![StyledSpan::styled("abcdef", Some(Color::Fixed(2)), None)];
        let line = fit_line(spans, 4, None);
        assert_eq!(line.width(), 4);
        assert_eq!(line.to_string(), "abcd");
        assert_eq!(
            line.spans[0].style.fg,
            Some(crate::view::to_color(Color::Fixed(2)))
        );
        // Padding fills with the given background.
        let spans = vec![StyledSpan::plain("ab")];
        let line = fit_line(spans, 5, Some(Color::Fixed(233)));
        assert_eq!(line.width(), 5);
        assert_eq!(line.to_string(), "ab   ");
        // Wide graphemes count two columns.
        let line = fit_line(vec![StyledSpan::plain("💬")], 4, None);
        assert_eq!(line.width(), 4);
        assert_eq!(line.to_string(), "💬  ");
    }
}
