//! Raw unified-diff row formatting with gutter numbers and trailing whitespace
//! highlighting.

use std::{fmt, fmt::Write};

use super::segment::RowSegmentLine;
use crate::{
    ansi::{Color, ColorExt},
    core::parse::RowKind,
    span::StyledSpan,
    theme::{Cursor, DiffKind, DiffuseTheme},
};

/// Write a row in the raw unified-diff representation with theme support
/// directly to a buffer.
#[allow(clippy::too_many_arguments)]
pub fn write_raw_row<W: Write>(
    out: &mut W,
    kind: RowKind,
    prefix: &str,
    text: &str,
    selected: bool,
    band: bool,
    highlight_trailing: bool,
    theme: &DiffuseTheme,
) -> fmt::Result {
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
    let fg = fg_color.fg_code();
    let sub_bg = sub_bg_color.bg_code();
    let sub_fg = sub_fg_color.fg_code();

    let bg_color = if selected {
        palette[Cursor::Row][DiffKind::Ctx].surface.bg
    } else if band {
        palette[Cursor::Row].band.bg
    } else {
        palette[Cursor::Rest][DiffKind::Ctx].surface.bg
    };
    let bg = match bg_color {
        Color::Primary => String::new(),
        color => color.bg_code(),
    };
    let reset_bg = if bg.is_empty() { "\x1b[49m" } else { &bg };

    let trailing_len = if highlight_trailing {
        text.len() - text.trim_end_matches(|c: char| c.is_whitespace()).len()
    } else {
        0
    };

    write!(out, "{bg}{prefix}{fg}")?;
    if trailing_len > 0 {
        let (body, trailing) = text.split_at(text.len() - trailing_len);
        out.write_str(body)?;
        if kind == RowKind::Del {
            write!(
                out,
                "{sub_bg}\x1b[9m{sub_fg}{trailing}\x1b[29m{reset_bg}{fg}"
            )?;
        } else {
            write!(out, "{sub_bg}{sub_fg}{trailing}{reset_bg}{fg}")?;
        }
    } else {
        out.write_str(text)?;
    }

    if selected || band {
        out.write_str(&bg)?;
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
    prefix: &str,
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
        });
    } else {
        spans.push(StyledSpan::styled(text, Some(fg_color), line_bg));
    }

    RowSegmentLine { spans, line_bg }
}
