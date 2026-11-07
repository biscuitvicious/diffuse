//! Renderer-agnostic styled text spans.
//!
//! A [`StyledSpan`] is plain text plus palette colors and attribute flags,
//! shared by the ANSI batch path and the Ratatui grid path so their styling
//! decisions cannot drift apart.

use std::fmt::{self, Write};

use unicode_segmentation::UnicodeSegmentation;
use yansi::{Color, Style};

use crate::{
    ansi::{ColorExt, FG_RESET, STRIKE_OFF, STRIKE_ON, sgr_off, sgr_on_str},
    row::slice_by_visual_width,
    width::grapheme_width,
};

/// Modifier flags decoded from a wire SGR id. The mapping mirrors
/// [`crate::ansi::sgr_on`]: 1 is bold, 2 is dim, 3 is italic, 4 is
/// underline, 9 is dim plus strikethrough, and 14 is bold plus underline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SgrModifiers {
    /// Bold intensity flag.
    pub bold: bool,
    /// Dim intensity flag.
    pub dim: bool,
    /// Italic slant flag.
    pub italic: bool,
    /// Underline flag.
    pub underline: bool,
    /// Strikethrough flag.
    pub strikethrough: bool,
}

/// One set modifier flag, in canonical emission order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flag {
    Bold,
    Dim,
    Italic,
    Underline,
    Strike,
}

/// Decode a wire SGR id into modifier flags. Unknown ids carry no
/// modifiers, matching the unstyled fallback of the ANSI path.
#[must_use]
pub fn modifiers_for_sgr(sgr: u32) -> SgrModifiers {
    match sgr {
        1 => SgrModifiers {
            bold: true,
            ..SgrModifiers::default()
        },
        2 => SgrModifiers {
            dim: true,
            ..SgrModifiers::default()
        },
        3 => SgrModifiers {
            italic: true,
            ..SgrModifiers::default()
        },
        4 => SgrModifiers {
            underline: true,
            ..SgrModifiers::default()
        },
        9 => SgrModifiers {
            dim: true,
            strikethrough: true,
            ..SgrModifiers::default()
        },
        14 => SgrModifiers {
            bold: true,
            underline: true,
            ..SgrModifiers::default()
        },
        _ => SgrModifiers::default(),
    }
}

/// One ordered run of styled text: plain content plus optional palette
/// foreground/background and SGR-style attribute flags. [`Color::Primary`]
/// means "inherit" (no escape is emitted), matching the ANSI path where a
/// primary background resolves to an empty code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledSpan {
    /// Plain text content with no escape sequences.
    pub text: String,
    /// Foreground palette color, or [`None`] to inherit.
    pub fg: Option<Color>,
    /// Background palette color, or [`None`] to inherit.
    pub bg: Option<Color>,
    /// Bold intensity flag.
    pub bold: bool,
    /// Dim intensity flag.
    pub dim: bool,
    /// Italic slant flag.
    pub italic: bool,
    /// Underline flag.
    pub underline: bool,
    /// Strikethrough flag.
    pub strikethrough: bool,
    /// Exact source SGR id list, when the modifiers came from wire SGR
    /// ids. [`None`] means the flags were set directly (banner, gutter,
    /// del runs) and serialize with the plain per-flag spellings. The
    /// Ratatui converter reads only the flags, so this never affects the
    /// grid path — it exists so ANSI serialization reproduces stacked and
    /// raw-fallback spellings byte-for-byte.
    pub sgrs: Option<Box<[u32]>>,
}

impl StyledSpan {
    /// An unstyled span inheriting its surroundings.
    #[must_use]
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            fg: None,
            bg: None,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            strikethrough: false,
            sgrs: None,
        }
    }

    /// A span with palette foreground/background and no modifiers.
    #[must_use]
    pub fn styled(text: impl Into<String>, fg: Option<Color>, bg: Option<Color>) -> Self {
        Self {
            text: text.into(),
            fg,
            bg,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            strikethrough: false,
            sgrs: None,
        }
    }

    /// Set the foreground palette color.
    #[must_use]
    pub fn with_fg(mut self, fg: Color) -> Self {
        self.fg = Some(fg);
        self
    }

    /// Set the background palette color.
    #[must_use]
    pub fn with_bg(mut self, bg: Color) -> Self {
        self.bg = Some(bg);
        self
    }

    /// Set the bold intensity flag.
    #[must_use]
    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }

    /// Set the dim intensity flag.
    #[must_use]
    pub fn with_dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }

    /// Set the italic slant flag.
    #[must_use]
    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// Set the underline flag.
    #[must_use]
    pub fn with_underline(mut self, underline: bool) -> Self {
        self.underline = underline;
        self
    }

    /// Set the strikethrough flag.
    #[must_use]
    pub fn with_strikethrough(mut self, strikethrough: bool) -> Self {
        self.strikethrough = strikethrough;
        self
    }

    /// Apply the modifier flags of a wire SGR id, preserving flags already
    /// set (stacked emphasis accumulates, as on the ANSI path). The id is
    /// retained so ANSI serialization reproduces its exact spelling
    /// (stacked, combined, or raw fallback).
    #[must_use]
    pub fn with_sgr(mut self, sgr: u32) -> Self {
        self = self.with_sgrs(&[sgr]);
        self
    }

    /// Apply the modifier flags of several wire SGR ids, retaining the
    /// exact list for ANSI serialization (combined `\x1b[a;b;…m` emission
    /// with per-modifier resets in reverse, as on the ANSI path).
    #[must_use]
    pub fn with_sgrs(mut self, sgrs: &[u32]) -> Self {
        for sgr in sgrs {
            let flags = modifiers_for_sgr(*sgr);
            self.bold |= flags.bold;
            self.dim |= flags.dim;
            self.italic |= flags.italic;
            self.underline |= flags.underline;
            self.strikethrough |= flags.strikethrough;
        }
        if !sgrs.is_empty() {
            self.sgrs = Some(sgrs.into());
        }
        self
    }

    /// One set modifier flag, in canonical emission order.
    ///
    /// SGR on-sequence for one modifier flag: the plain per-flag spelling
    /// (`\x1b[1m`, `\x1b[2m`, `\x1b[3m`, `\x1b[4m`, or plain
    /// strikethrough — never the stacked id-9 dim+strike spelling, which
    /// only the retained-id path may emit).
    fn flag_on(flag: Flag) -> &'static str {
        match flag {
            Flag::Bold => sgr_on_str(1).unwrap_or("\x1b[1m"),
            Flag::Dim => sgr_on_str(2).unwrap_or("\x1b[2m"),
            Flag::Italic => sgr_on_str(3).unwrap_or("\x1b[3m"),
            Flag::Underline => sgr_on_str(4).unwrap_or("\x1b[4m"),
            Flag::Strike => STRIKE_ON,
        }
    }

    /// SGR off-sequence for one modifier flag, mirroring the ANSI path's
    /// per-modifier resets.
    fn flag_off(flag: Flag) -> &'static str {
        match flag {
            Flag::Bold => sgr_off(1).unwrap_or("\x1b[22m"),
            Flag::Dim => sgr_off(2).unwrap_or("\x1b[22m"),
            Flag::Italic => sgr_off(3).unwrap_or("\x1b[23m"),
            Flag::Underline => sgr_off(4).unwrap_or("\x1b[24m"),
            Flag::Strike => STRIKE_OFF,
        }
    }

    /// Modifier flags set on this span, in canonical emission order.
    fn flags(&self) -> Vec<Flag> {
        let mut out = Vec::with_capacity(5);
        if self.bold {
            out.push(Flag::Bold);
        }
        if self.dim {
            out.push(Flag::Dim);
        }
        if self.italic {
            out.push(Flag::Italic);
        }
        if self.underline {
            out.push(Flag::Underline);
        }
        if self.strikethrough {
            out.push(Flag::Strike);
        }
        out
    }

    /// Write this span's SGR on-sequence: retained ids reproduce their
    /// exact spelling (single, combined stacked, or raw fallback), while
    /// directly-set flags use the plain per-flag spellings.
    fn write_sgr_on<W: Write>(&self, out: &mut W) -> fmt::Result {
        if let Some(ids) = &self.sgrs {
            if ids.len() > 1 {
                out.write_str("\x1b[")?;
                for (i, s) in ids.iter().enumerate() {
                    if i > 0 {
                        out.write_str(";")?;
                    }
                    write!(out, "{s}")?;
                }
                return out.write_str("m");
            }
            if let Some(s) = ids.first() {
                if let Some(on) = sgr_on_str(*s) {
                    return out.write_str(on);
                }
                return write!(out, "\x1b[{s}m");
            }
        }
        for flag in self.flags() {
            out.write_str(Self::flag_on(flag))?;
        }
        Ok(())
    }

    /// Write this span's SGR off-sequence, in reverse (stacked resets
    /// unwind in reverse, as on the ANSI path). Unknown retained ids have
    /// no off-sequence, matching `write_token`.
    fn write_sgr_off<W: Write>(&self, out: &mut W) -> fmt::Result {
        if let Some(ids) = &self.sgrs {
            for s in ids.iter().rev() {
                if let Some(off) = sgr_off(*s) {
                    out.write_str(off)?;
                }
            }
            return Ok(());
        }
        for flag in self.flags().into_iter().rev() {
            out.write_str(Self::flag_off(flag))?;
        }
        Ok(())
    }

    /// Whether this span emits an SGR on-sequence (unknown retained ids
    /// still emit their raw fallback).
    fn has_sgr_on(&self) -> bool {
        if let Some(ids) = &self.sgrs {
            return !ids.is_empty();
        }
        !self.flags().is_empty()
    }

    /// Whether this span emits an SGR off-sequence.
    fn has_sgr_off(&self) -> bool {
        if let Some(ids) = &self.sgrs {
            return ids.iter().any(|s| sgr_off(*s).is_some());
        }
        !self.flags().is_empty()
    }

    /// Project this span onto a [`yansi::Style`]. [`None`] and
    /// [`Color::Primary`] colors stay unset so the span inherits its
    /// surroundings, matching the ANSI path where primary resolves to an
    /// empty code.
    #[must_use]
    pub fn yansi_style(&self) -> Style {
        let mut style = Style::new();
        if let Some(fg) = self.fg.filter(|color| *color != Color::Primary) {
            style = style.fg(fg);
        }
        if let Some(bg) = self.bg.filter(|color| *color != Color::Primary) {
            style = style.bg(bg);
        }
        if self.bold {
            style = style.bold();
        }
        if self.dim {
            style = style.dim();
        }
        if self.italic {
            style = style.italic();
        }
        if self.underline {
            style = style.underline();
        }
        if self.strikethrough {
            style = style.strike();
        }
        style
    }
}

/// Slice styled spans to the visual column window `start..start + width`,
/// preserving each piece's styling. A wide grapheme straddling the window
/// edge is dropped, matching [`slice_by_visual_width`].
#[must_use]
pub fn slice_spans(spans: &[StyledSpan], start: usize, width: usize) -> Vec<StyledSpan> {
    if width == 0 {
        return Vec::new();
    }
    let end = start + width;
    let mut out = Vec::new();
    let mut col = 0usize;
    for span in spans {
        let span_width: usize = span.text.graphemes(true).map(grapheme_width).sum();
        let span_start = col;
        col += span_width;
        if col <= start || span_start >= end {
            continue;
        }
        let from = start.max(span_start);
        let to = end.min(col);
        let piece = slice_by_visual_width(&span.text, from - span_start, to - from);
        if piece.is_empty() {
            continue;
        }
        out.push(StyledSpan {
            text: piece,
            ..span.clone()
        });
    }
    out
}

/// Total display width of styled spans in terminal columns.
#[must_use]
pub fn spans_width(spans: &[StyledSpan]) -> usize {
    spans
        .iter()
        .map(|span| crate::width::display_width(&span.text))
        .sum()
}

/// Write one span as styled ANSI text: the span's SGR on-sequence, its
/// foreground, its text, then `FG_RESET` (iff a foreground was emitted)
/// and the SGR off-sequence. No background is emitted — callers paint the
/// span background around the token, exactly as `write_token` callers do.
/// [`None`] and [`Color::Primary`] foregrounds emit nothing, matching the
/// ANSI path where primary resolves to an empty code.
pub fn write_span_token<W: Write>(out: &mut W, span: &StyledSpan) -> fmt::Result {
    if span.has_sgr_on() {
        span.write_sgr_on(out)?;
    }
    let fg = span.fg.filter(|color| *color != Color::Primary);
    if let Some(color) = fg {
        color.write_fg(out)?;
    }
    out.write_str(&span.text)?;
    if fg.is_some() {
        out.write_str(FG_RESET)?;
    }
    if span.has_sgr_off() {
        span.write_sgr_off(out)?;
    }
    Ok(())
}

/// Truncate spans wider than `width`, or pad narrower spans with spaces on
/// `bg` so the row fills `width` exactly. Padding carries no foreground so
/// it inherits the surrounding text color.
#[must_use]
pub fn pad_spans(spans: Vec<StyledSpan>, width: usize, bg: Option<Color>) -> Vec<StyledSpan> {
    let used = spans_width(&spans);
    if used >= width {
        return slice_spans(&spans, 0, width);
    }
    let mut out = spans;
    out.push(StyledSpan::plain(" ".repeat(width - used)).with_bg(bg.unwrap_or(Color::Primary)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use yansi::Style;

    #[test]
    fn test_plain_span_carries_no_styling() {
        let span = StyledSpan::plain("hi");
        assert_eq!(span.text, "hi");
        assert_eq!(span.fg, None);
        assert_eq!(span.bg, None);
        assert!(!span.bold);
        assert!(!span.dim);
        assert!(!span.italic);
        assert!(!span.underline);
        assert!(!span.strikethrough);
        assert_eq!(span.yansi_style(), Style::default());
    }

    #[test]
    fn test_styled_span_projects_to_yansi() {
        let span = StyledSpan::styled("hi", Some(Color::Fixed(120)), Some(Color::Fixed(233)));
        assert_eq!(
            span.yansi_style(),
            Style::new().fg(Color::Fixed(120)).bg(Color::Fixed(233))
        );
    }

    #[test]
    fn test_modifier_flags_project_to_yansi() {
        let span = StyledSpan::plain("hi")
            .with_bold(true)
            .with_dim(true)
            .with_italic(true)
            .with_underline(true)
            .with_strikethrough(true);
        assert_eq!(
            span.yansi_style(),
            Style::new().bold().dim().italic().underline().strike()
        );
        // Clearing a flag drops only that attribute.
        let cleared = span.with_bold(false);
        assert_eq!(
            cleared.yansi_style(),
            Style::new().dim().italic().underline().strike()
        );
    }

    #[test]
    fn test_with_fg_and_with_bg_set_palette_colors() {
        let span = StyledSpan::plain("hi")
            .with_fg(Color::Fixed(120))
            .with_bg(Color::Fixed(233));
        assert_eq!(span.fg, Some(Color::Fixed(120)));
        assert_eq!(span.bg, Some(Color::Fixed(233)));
    }

    #[test]
    fn test_primary_colors_project_to_inherit() {
        // `Color::Primary` emits no escape on the ANSI path, so it must
        // project to an unset style (inheriting the surroundings).
        let span =
            StyledSpan::styled("hi", Some(Color::Primary), Some(Color::Primary)).with_bold(true);
        assert_eq!(span.yansi_style(), Style::new().bold());
    }

    #[test]
    fn test_with_sgr_records_exact_id_and_flags() {
        let span = StyledSpan::plain("hi").with_sgr(9);
        assert_eq!(span.sgrs.as_deref(), Some(&[9][..]));
        assert!(span.dim && span.strikethrough);
        assert!(!span.bold && !span.italic && !span.underline);
        // Unknown ids are retained verbatim with no flags, matching the
        // raw-fallback emission of the ANSI path.
        let unknown = StyledSpan::plain("hi").with_sgr(99);
        assert_eq!(unknown.sgrs.as_deref(), Some(&[99][..]));
        assert!(!unknown.bold && !unknown.strikethrough);
    }

    #[test]
    fn test_with_sgrs_records_stacked_ids() {
        let span = StyledSpan::plain("hi").with_sgrs(&[1, 4]);
        assert_eq!(span.sgrs.as_deref(), Some(&[1, 4][..]));
        assert!(span.bold && span.underline);
    }

    fn token_span(fg: Option<Color>, text: &str) -> StyledSpan {
        StyledSpan::styled(text, fg, None)
    }

    #[test]
    fn test_write_span_token_plain() {
        let mut out = String::new();
        write_span_token(&mut out, &token_span(None, "hi")).unwrap();
        assert_eq!(out, "hi");
    }

    #[test]
    fn test_write_span_token_fg() {
        let mut out = String::new();
        write_span_token(&mut out, &token_span(Some(Color::Fixed(120)), "hi")).unwrap();
        assert_eq!(out, "\x1b[38;5;120mhi\x1b[39m");
    }

    #[test]
    fn test_write_span_token_primary_fg_emits_nothing() {
        let mut out = String::new();
        write_span_token(&mut out, &token_span(Some(Color::Primary), "hi")).unwrap();
        assert_eq!(out, "hi");
    }

    #[test]
    fn test_write_span_token_single_sgr() {
        let mut out = String::new();
        let span = token_span(Some(Color::Fixed(120)), "hi").with_sgr(1);
        write_span_token(&mut out, &span).unwrap();
        assert_eq!(out, "\x1b[1m\x1b[38;5;120mhi\x1b[39m\x1b[22m");
    }

    #[test]
    fn test_write_span_token_stacked_sgrs() {
        let mut out = String::new();
        let span = token_span(Some(Color::Fixed(120)), "hi").with_sgrs(&[1, 4]);
        write_span_token(&mut out, &span).unwrap();
        assert_eq!(out, "\x1b[1;4m\x1b[38;5;120mhi\x1b[39m\x1b[24m\x1b[22m");
    }

    #[test]
    fn test_write_span_token_unknown_sgr_raw_fallback() {
        let mut out = String::new();
        let span = token_span(Some(Color::Fixed(120)), "hi").with_sgr(99);
        write_span_token(&mut out, &span).unwrap();
        assert_eq!(out, "\x1b[99m\x1b[38;5;120mhi\x1b[39m");
    }

    #[test]
    fn test_write_span_token_sgr9_stacked_spelling() {
        // Id 9 keeps its bespoke dim+strike spelling, not plain strike.
        let mut out = String::new();
        let span = token_span(Some(Color::Fixed(120)), "hi").with_sgr(9);
        write_span_token(&mut out, &span).unwrap();
        assert_eq!(out, "\x1b[2;9m\x1b[38;5;120mhi\x1b[39m\x1b[29m\x1b[22m");
    }

    #[test]
    fn test_write_span_token_flag_strike_plain_spelling() {
        // Flag-derived strike (del runs) uses plain 9/29, never id 9's
        // dim+strike spelling.
        let mut out = String::new();
        let span = token_span(Some(Color::Fixed(203)), "hi").with_strikethrough(true);
        write_span_token(&mut out, &span).unwrap();
        assert_eq!(out, "\x1b[9m\x1b[38;5;203mhi\x1b[39m\x1b[29m");
    }

    #[test]
    fn test_write_span_token_flag_bold_underline() {
        let mut out = String::new();
        let span = token_span(None, "hi").with_bold(true).with_underline(true);
        write_span_token(&mut out, &span).unwrap();
        assert_eq!(out, "\x1b[1m\x1b[4mhi\x1b[24m\x1b[22m");
    }

    #[test]
    fn test_modifiers_for_sgr_covers_known_ids() {
        assert_eq!(
            modifiers_for_sgr(1),
            SgrModifiers {
                bold: true,
                ..SgrModifiers::default()
            }
        );
        assert_eq!(
            modifiers_for_sgr(2),
            SgrModifiers {
                dim: true,
                ..SgrModifiers::default()
            }
        );
        assert_eq!(
            modifiers_for_sgr(3),
            SgrModifiers {
                italic: true,
                ..SgrModifiers::default()
            }
        );
        assert_eq!(
            modifiers_for_sgr(4),
            SgrModifiers {
                underline: true,
                ..SgrModifiers::default()
            }
        );
        // SGR 9 emits dim plus strikethrough (`sgr_on(9)` is `\x1b[2;9m`).
        assert_eq!(
            modifiers_for_sgr(9),
            SgrModifiers {
                dim: true,
                strikethrough: true,
                ..SgrModifiers::default()
            }
        );
        // SGR 14 emits bold plus underline (`sgr_on(14)` is `\x1b[1;4m`).
        assert_eq!(
            modifiers_for_sgr(14),
            SgrModifiers {
                bold: true,
                underline: true,
                ..SgrModifiers::default()
            }
        );
        assert_eq!(modifiers_for_sgr(99), SgrModifiers::default());
    }

    #[test]
    fn test_with_sgr_applies_known_ids() {
        let span = StyledSpan::plain("hi").with_sgr(9);
        assert!(span.dim);
        assert!(span.strikethrough);
        assert!(!span.bold && !span.italic && !span.underline);
    }

    #[test]
    fn test_slice_spans_by_visual_columns() {
        let spans = vec![
            StyledSpan::styled("ab", Some(Color::Fixed(1)), None),
            StyledSpan::styled("cde", Some(Color::Fixed(2)), None),
        ];
        let sliced = slice_spans(&spans, 1, 3);
        assert_eq!(sliced.len(), 2);
        assert_eq!(sliced[0].text, "b");
        assert_eq!(sliced[0].fg, Some(Color::Fixed(1)));
        assert_eq!(sliced[1].text, "cd");
        assert_eq!(sliced[1].fg, Some(Color::Fixed(2)));
        // Slicing past the end yields nothing.
        assert!(slice_spans(&spans, 9, 3).is_empty());
        // Zero width yields nothing.
        assert!(slice_spans(&spans, 0, 0).is_empty());
    }

    #[test]
    fn test_slice_spans_keeps_wide_grapheme_atomic() {
        let spans = vec![StyledSpan::plain("a💬b")];
        // The emoji occupies columns 1..3; a window starting inside it drops
        // it, matching `slice_by_visual_width`.
        assert_eq!(slice_spans(&spans, 1, 2)[0].text, "💬");
        assert_eq!(slice_spans(&spans, 2, 2)[0].text, "b");
    }

    #[test]
    fn test_spans_width_sums_display_columns() {
        let spans = vec![StyledSpan::plain("ab"), StyledSpan::plain("💬")];
        assert_eq!(spans_width(&spans), 4);
        assert_eq!(spans_width(&[]), 0);
    }

    #[test]
    fn test_pad_spans_fills_to_width_with_background() {
        let spans = vec![StyledSpan::styled("ab", None, Some(Color::Fixed(233)))];
        let padded = pad_spans(spans, 5, Some(Color::Fixed(233)));
        assert_eq!(spans_width(&padded), 5);
        assert_eq!(padded.last().unwrap().text, "   ");
        assert_eq!(padded.last().unwrap().bg, Some(Color::Fixed(233)));
        // Already-wide spans are truncated, never padded.
        let wide = vec![StyledSpan::plain("abcdef")];
        let truncated = pad_spans(wide, 4, None);
        assert_eq!(spans_width(&truncated), 4);
        assert_eq!(truncated[0].text, "abcd");
    }
}
