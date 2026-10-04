//! High-level CLI Diff Pretty-Printer.
//!
//! Formats full file diff summaries and colored hunks directly to terminal
//! output.

use std::fmt::Write;

use crate::{
    Badge,
    ansi::{ColorExt, FG_RESET, SGR_RESET},
    core::{
        parse::RowKind,
        wire::{OutputFile, OutputRow, PipelineOutput},
    },
    counts::count_cells,
};

use super::{
    row::{prefix_of, styled_prefix, write_raw_row, write_row_segment},
    theme::DiffuseTheme,
};

/// Format a file's header bar directly into an output buffer without
/// intermediate string allocations. Mirrors the TUI file banner order:
/// badge, filename, change counts, then status parens. `badge` carries the
/// file position (`F3/17`); pass `None` while streaming, where the total is
/// unknown. The filename and status parens render in the current-file path
/// white (the pager has no cursor states).
pub fn write_file_header_bar<W: Write>(
    out: &mut W,
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
) -> std::fmt::Result {
    let counts = &file.counts;
    let path_display = file.display_path();
    let status_display = file.display_status();
    let bg_code = theme.palette.header_bar_bg.bg_code();
    let fg_code = theme.palette.header_bar_fg.fg_code();
    let path_white = theme.palette.file_header_path_current.fg_code();

    let status_suffix = if status_display.is_empty() || status_display == "modified" {
        String::new()
    } else {
        format!(" ({status_display})")
    };

    write!(out, "{bg_code}\x1b[1m{fg_code} ")?;
    if let Some(badge) = badge {
        write!(out, "{} ", badge.file_text())?;
    }
    write!(
        out,
        "\x1b[22m{path_white}{path_display}\x1b[39m\x1b[1m{fg_code}"
    )?;
    // Shared count cells: the same segments, order, and colors as the
    // status bar and the TUI banners (the ≈ mov count converges on the
    // shared 147; the gutter icon keeps its own color).
    for (i, cell) in count_cells(counts, &theme.palette).iter().enumerate() {
        let sep = if i == 0 { "" } else { " " };
        let fg = cell.color.fg_code();
        write!(
            out,
            "{sep}\x1b[1m{fg}{}{}{FG_RESET}",
            cell.marker, cell.value
        )?;
    }

    if counts.movs > 0 {
        write!(out, " moved")?;
    }
    if !status_suffix.is_empty() {
        write!(
            out,
            "\x1b[22m{path_white}{status_suffix}\x1b[39m\x1b[1m{fg_code}"
        )?;
    }
    if !file.notes.is_empty() {
        write!(
            out,
            " · {} note{}",
            file.notes.len(),
            if file.notes.len() == 1 { "" } else { "s" }
        )?;
    }
    writeln!(out, " {SGR_RESET}")
}

/// Render complete pipeline output to terminal ANSI strings.
pub fn render_pipeline_output(
    output: &PipelineOutput,
    theme: &DiffuseTheme,
    styled_prefixes: bool,
) -> String {
    let total_rows: usize = output
        .files
        .iter()
        .map(|f| {
            if !f.rows.is_empty() {
                f.rows.len()
            } else {
                f.hunks.iter().map(|h| h.rows.len()).sum()
            }
        })
        .sum();
    let mut out = String::with_capacity(total_rows * 128 + output.files.len() * 256 + 16);

    let total = output.files.len();
    for (fi, f) in output.files.iter().enumerate() {
        out.push_str(&render_file_diff(
            f,
            Some(Badge { idx: fi, total }),
            theme,
            styled_prefixes,
        ));
    }

    // Rows end with their row tint background active (the TUI relies on it
    // for width fill); the CLI stream must not leave the terminal tinted
    // after it exits.
    out.push_str("\x1b[0m");

    out
}

/// Render a single file's header bar and colored hunks.
pub fn render_file_diff(
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
    styled_prefixes: bool,
) -> String {
    let total_rows: usize = file.hunks.iter().map(|h| h.rows.len()).sum();
    let mut out = String::with_capacity(total_rows * 128 + 256);

    let _ = write_file_header_bar(&mut out, file, badge, theme);

    let render_row = |r: &OutputRow, out: &mut String| {
        let pfx = if styled_prefixes {
            styled_prefix(
                r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs, false, theme,
            )
        } else {
            let plain = prefix_of(r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs);
            format!("{plain} ")
        };

        let _ = write_row_segment(
            out,
            &r.text,
            r.del_text.as_deref(),
            &r.spans,
            r.kind,
            &r.runs,
            0,
            usize::MAX,
            &pfx,
            false,
            false,
            0,
            true,
            true,
            theme,
        );
        out.push_str(SGR_RESET);
        out.push('\n');
    };

    if !file.rows.is_empty() {
        for r in &file.rows {
            render_row(r, &mut out);
        }
    } else {
        for h in &file.hunks {
            for r in &h.rows {
                render_row(r, &mut out);
            }
        }
    }

    out
}

/// Render complete pipeline output in raw mode: flat green/red text with no
/// subword highlighting and no mod rows, equivalent to the TUI's `R` (raw diff)
/// toggle.
pub fn render_raw_pipeline_output(output: &PipelineOutput, theme: &DiffuseTheme) -> String {
    let mut out = String::new();
    let total = output.files.len();
    for (fi, f) in output.files.iter().enumerate() {
        out.push_str(&render_raw_file_diff(
            f,
            Some(Badge { idx: fi, total }),
            theme,
        ));
    }
    out.push_str("\x1b[0m");
    out
}

/// Render a single file's diff in raw mode: flat green/red text on default
/// black background, colorized number gutter, only - and + rows (no mod rows),
/// and `@@` hunk headers, equivalent to the TUI's `R` toggle.
pub fn render_raw_file_diff(
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
) -> String {
    let hunks = if !file.raw_hunks.is_empty() {
        &file.raw_hunks
    } else {
        &file.hunks
    };
    let total_rows: usize = hunks.iter().map(|h| h.rows.len() + 1).sum();
    let mut out = String::with_capacity(total_rows * 128 + 256);

    let _ = write_file_header_bar(&mut out, file, badge, theme);

    for h in hunks {
        let sep_text = format!(
            "@@ -{},{} +{},{} @@",
            h.old_start, h.old_count, h.new_start, h.new_count
        );
        let _ = write_raw_row(
            &mut out,
            RowKind::Ctx,
            "",
            &sep_text,
            false,
            false,
            true,
            theme,
        );
        out.push('\n');

        for r in &h.rows {
            let pfx = styled_prefix(
                r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs, true, theme,
            );
            let _ = write_raw_row(&mut out, r.kind, &pfx, &r.text, false, false, true, theme);
            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests;
