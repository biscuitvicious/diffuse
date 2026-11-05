//! File banner and header bar formatting.

use std::fmt::{self, Write};

use crate::{
    Badge,
    ansi::{ColorExt, FG_RESET, SGR_RESET, strip_ansi},
    core::wire::OutputFile,
    counts::count_cells,
    theme::{Cursor, DiffuseTheme},
    width::display_width,
};

/// Format a file's header bar directly into an output buffer without
/// intermediate string allocations. Mirrors the TUI file banner order:
/// badge, filename, change counts, then status parens. `badge` carries the
/// file position (`File 3/17`); pass `None` while streaming, where the total is
/// unknown. The filename and status parens render in the current-file path
/// white (the pager has no cursor states).
pub fn write_file_header_bar<W: Write>(
    out: &mut W,
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
) -> fmt::Result {
    write_file_header_bar_with_width(out, file, badge, theme, None)
}

/// Format a file's header bar directly into an output buffer, extending the
/// background color out to `width` when provided.
pub fn write_file_header_bar_with_width<W: Write>(
    out: &mut W,
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
    width: Option<usize>,
) -> fmt::Result {
    let counts = &file.counts;
    let path_display = file.display_path();
    let status_display = file.display_status();
    let ui = &theme.palette.ui;
    let bg_code = ui.header_bar_bg.bg_code();
    let fg_code = ui.header_bar_fg.fg_code();
    // The current-file path white resolves at the `Cursor::File` banner
    // tier (`path` surface color; unset paths fall back to the foreground
    // at cascade time).
    let path_white = theme.palette[Cursor::File]
        .file_header
        .fg_or_path()
        .fg_code();

    let status_suffix = if status_display.is_empty()
        || status_display == "modified"
        || status_display == "unmodified"
    {
        String::new()
    } else {
        format!(" ({status_display})")
    };

    let mut bar_buf = String::with_capacity(128);
    write!(bar_buf, "{bg_code}\x1b[1m{fg_code} ")?;
    if let Some(badge) = badge {
        write!(bar_buf, "{} ", badge.file_text())?;
    }
    write!(
        bar_buf,
        "\x1b[22m{path_white}{path_display}\x1b[39m\x1b[1m{fg_code}"
    )?;
    // Shared count cells: the same segments, order, and colors as the
    // status bar and the TUI banners (the ≈ mov count converges on the
    // shared 147; the gutter icon keeps its own color).
    for cell in count_cells(counts, &theme.palette.ui) {
        let fg = cell.color.fg_code();
        write!(
            bar_buf,
            " \x1b[1m{fg}{}{}{FG_RESET}",
            cell.marker, cell.value
        )?;
    }

    if counts.movs > 0 {
        write!(bar_buf, " moved")?;
    }
    if !status_suffix.is_empty() {
        write!(
            bar_buf,
            "\x1b[22m{path_white}{status_suffix}\x1b[39m\x1b[1m{fg_code}"
        )?;
    }
    if !file.notes.is_empty() {
        write!(
            bar_buf,
            " · {} note{}",
            file.notes.len(),
            if file.notes.len() == 1 { "" } else { "s" }
        )?;
    }

    let plain = strip_ansi(&bar_buf);
    let vis_w = display_width(&plain);
    let pad = match width {
        Some(w) if w > 0 => {
            let rem = vis_w % w;
            if rem == 0 && vis_w > 0 { 0 } else { w - rem }
        }
        _ => 0,
    };
    for _ in 0..pad {
        bar_buf.push(' ');
    }
    writeln!(bar_buf, "{SGR_RESET}")?;
    out.write_str(&bar_buf)
}
