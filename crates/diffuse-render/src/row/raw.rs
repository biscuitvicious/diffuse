//! Raw unified-diff row formatting with gutter numbers and trailing whitespace
//! highlighting.

use std::{fmt, fmt::Write};

use crate::{
    ansi::{Color, ColorExt},
    core::parse::RowKind,
    theme::DiffuseTheme,
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
    let (fg_color, sub_bg_color, sub_fg_color) = match kind {
        RowKind::Add => (
            theme.palette.add_gutter_icon,
            theme.palette.add_fill_bg,
            theme.palette.add_gutter_num,
        ),
        RowKind::Del => (
            theme.palette.del_gutter_icon,
            theme.palette.del_cursor_bg,
            theme.palette.del_gutter_num,
        ),
        _ => (
            theme.palette.ctx_gutter_in_hunk,
            theme.palette.ctx_cursor_bg,
            theme.palette.ctx_gutter_out_hunk,
        ),
    };
    let fg = fg_color.fg_code();
    let sub_bg = sub_bg_color.bg_code();
    let sub_fg = sub_fg_color.fg_code();

    let bg_color = if selected {
        theme.palette.ctx_cursor_bg
    } else if band {
        theme.palette.band_bg
    } else {
        theme.palette.ctx_tint_bg
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
