//! High-level CLI Diff Pretty-Printer.
//!
//! Formats full file diff summaries and colored hunks directly to terminal
//! output.

use crate::core::wire::{OutputFile, OutputRow, PipelineOutput};

use super::{
    row::{prefix_of, render_row_segment, styled_prefix},
    theme::DiffuseTheme,
};

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

    for f in &output.files {
        out.push_str(&render_file_diff(f, theme, styled_prefixes));
    }

    // Rows end with their row tint background active (the TUI relies on it
    // for width fill); the CLI stream must not leave the terminal tinted
    // after it exits.
    out.push_str("\x1b[0m");

    out
}

/// Render a single file's header bar and colored hunks.
pub fn render_file_diff(file: &OutputFile, theme: &DiffuseTheme, styled_prefixes: bool) -> String {
    let total_rows: usize = file.hunks.iter().map(|h| h.rows.len()).sum();
    let mut out = String::with_capacity(total_rows * 128 + 256);

    let counts = &file.counts;
    let note_str = format!(
        "{} note{}",
        file.notes.len(),
        if file.notes.len() == 1 { "" } else { "s" }
    );
    let mov_str = if counts.movs > 0 {
        format!(" \x1b[38;5;117m≈{}\x1b[39m moved", counts.movs)
    } else {
        String::new()
    };
    let mod_str = if counts.mods > 0 {
        format!(" \x1b[38;5;214m~{}\x1b[39m", counts.mods)
    } else {
        String::new()
    };
    let del_str = if counts.dels > 0 {
        format!(" \x1b[38;5;167m-{}\x1b[39m", counts.dels)
    } else {
        String::new()
    };
    let add_str = format!("\x1b[38;5;120m+{}\x1b[39m", counts.adds);

    let bar = format!(
        "\x1b[48;5;237m\x1b[1m\x1b[38;5;231m {} ({}) · {}{}{}{} · {} \x1b[0m\n",
        file.path, file.status, add_str, del_str, mod_str, mov_str, note_str
    );
    out.push_str(&bar);

    let render_row = |r: &OutputRow, out: &mut String| {
        let pfx = if styled_prefixes {
            styled_prefix(r.kind, r.new_line, r.old_line, r.moved, &r.runs, false)
        } else {
            let plain = prefix_of(r.kind, r.new_line, r.old_line, r.moved, &r.runs);
            format!("{plain} ")
        };

        let seg = render_row_segment(
            &r.text,
            r.del_text.as_deref(),
            &r.spans,
            r.kind,
            r.moved,
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
        out.push_str(&seg);
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

#[cfg(test)]
mod tests;
