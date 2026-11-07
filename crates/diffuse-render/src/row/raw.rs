//! Raw unified-diff row formatting with gutter numbers and trailing whitespace
//! highlighting.

use std::{fmt, fmt::Write};

use super::{gutter::write_gutter_prefix, segment::RowSegmentLine};
use crate::{
    ansi::{Color, ColorExt},
    core::parse::RowKind,
    span::StyledSpan,
    theme::{Cursor, DiffKind, DiffuseTheme},
};

/// Background escape for an optional line color: [`None`] and
/// [`Color::Primary`] inherit (no escape), matching the ANSI path where a
/// primary background resolves to an empty code.
fn bg_code(bg: Option<Color>) -> String {
    match bg.filter(|color| *color != Color::Primary) {
        Some(color) => color.bg_code(),
        None => String::new(),
    }
}

/// Write a row in the raw unified-diff representation with theme support
/// directly to a buffer. Style decisions come from [`raw_row_spans`];
/// emission keeps the exact skeleton (lead background, gutter prefix,
/// foreground, bespoke trailing fill, reset framing) so output stays
/// byte-identical.
#[allow(clippy::too_many_arguments)]
pub fn write_raw_row<W: Write>(
    out: &mut W,
    kind: RowKind,
    prefix: &[StyledSpan],
    text: &str,
    selected: bool,
    band: bool,
    highlight_trailing: bool,
    theme: &DiffuseTheme,
) -> fmt::Result {
    let row = raw_row_spans(
        kind,
        prefix.to_vec(),
        text,
        selected,
        band,
        highlight_trailing,
        theme,
    );
    let line_bg_code = bg_code(row.line_bg);
    let body = &row.spans[prefix.len().min(row.spans.len())..];
    let fg_code = match body
        .first()
        .and_then(|span| span.fg)
        .filter(|color| *color != Color::Primary)
    {
        Some(color) => color.fg_code(),
        None => String::new(),
    };
    // Raw gutter prefixes carry no styled background of their own; the
    // line background rides ahead of them, as on the ANSI path.
    out.write_str(&line_bg_code)?;
    // Raw prefixes are first-segment spellings: deletion rows keep the
    // mid-prefix italic-off, always closed with the trailing italic-off.
    write_gutter_prefix(out, prefix, kind == RowKind::Del, true)?;
    out.write_str(&fg_code)?;
    if let Some(main) = body.first() {
        out.write_str(&main.text)?;
    }
    // A split-off trailing fill repaints its sub-background; deletion
    // rows strike it and close without a foreground reset, exactly as
    // the ANSI path does.
    if let Some(trailing) = body.get(1) {
        out.write_str(&bg_code(trailing.bg))?;
        if trailing.strikethrough && trailing.sgrs.is_none() {
            out.write_str(crate::ansi::STRIKE_ON)?;
        }
        if let Some(color) = trailing.fg.filter(|color| *color != Color::Primary) {
            color.write_fg(out)?;
        }
        out.write_str(&trailing.text)?;
        if trailing.strikethrough && trailing.sgrs.is_none() {
            out.write_str(crate::ansi::STRIKE_OFF)?;
        }
        if line_bg_code.is_empty() {
            out.write_str("\x1b[49m")?;
        } else {
            out.write_str(&line_bg_code)?;
        }
        out.write_str(&fg_code)?;
    }
    if selected || band {
        out.write_str(&line_bg_code)?;
    } else {
        out.write_str("\x1b[49m")?;
    }
    out.write_str("\x1b[39m\x1b[22m")
}

/// Render a row in the raw unified-diff representation with theme support:
/// the 6-character line-number gutter plus the line text in flat
/// green/red/muted text color on a default black background (or cursor/band
/// background when selected).
pub fn render_raw_row(
    kind: RowKind,
    prefix: &[StyledSpan],
    text: &str,
    selected: bool,
    band: bool,
    highlight_trailing: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let _ = write_raw_row(
        &mut out,
        kind,
        prefix,
        text,
        selected,
        band,
        highlight_trailing,
        theme,
    );
    out
}

/// Build the span-native twin of [`render_raw_row`]: identical text,
/// foregrounds, and trailing-whitespace fills as ordered [`StyledSpan`]s.
/// Branching mirrors [`write_raw_row`] (including the byte-based trailing
/// split and the struck trailing fill on deletion rows); the ANSI function
/// stays the canonical batch serializer.
pub fn raw_row_spans(
    kind: RowKind,
    prefix: Vec<StyledSpan>,
    text: &str,
    selected: bool,
    band: bool,
    highlight_trailing: bool,
    theme: &DiffuseTheme,
) -> RowSegmentLine {
    let palette = theme.palette;
    let (fg_color, sub_bg_color, sub_fg_color) = match kind {
        RowKind::Add => (
            palette[Cursor::Rest][DiffKind::Add].gutter.icon.color,
            palette[Cursor::Rest][DiffKind::Add]
                .surface
                .fill
                .unwrap_or(Color::Primary),
            palette[Cursor::Rest][DiffKind::Add].gutter.num.color,
        ),
        RowKind::Del => (
            palette[Cursor::Rest][DiffKind::Del].gutter.icon.color,
            palette[Cursor::Row][DiffKind::Del].surface.bg,
            palette[Cursor::Rest][DiffKind::Del].gutter.num.color,
        ),
        _ => (
            palette[Cursor::Hunk][DiffKind::Ctx].gutter.num.color,
            palette[Cursor::Row][DiffKind::Ctx].surface.bg,
            palette[Cursor::Rest][DiffKind::Ctx].gutter.num.color,
        ),
    };

    let line_bg = if selected {
        Some(palette[Cursor::Row][DiffKind::Ctx].surface.bg)
    } else if band {
        Some(palette[Cursor::Row].band.bg)
    } else {
        Some(palette[Cursor::Rest][DiffKind::Ctx].surface.bg)
    };

    let mut spans: Vec<StyledSpan> = prefix;
    for span in &mut spans {
        span.bg = line_bg;
    }

    let trailing_len = if highlight_trailing {
        text.len() - text.trim_end_matches(|c: char| c.is_whitespace()).len()
    } else {
        0
    };

    if trailing_len > 0 {
        let (body, trailing) = text.split_at(text.len() - trailing_len);
        spans.push(StyledSpan::styled(body, Some(fg_color), line_bg));
        spans.push(StyledSpan {
            text: trailing.to_string(),
            fg: Some(sub_fg_color),
            bg: Some(sub_bg_color),
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            strikethrough: kind == RowKind::Del,
            sgrs: None,
        });
    } else {
        spans.push(StyledSpan::styled(text, Some(fg_color), line_bg));
    }

    RowSegmentLine { spans, line_bg }
}
