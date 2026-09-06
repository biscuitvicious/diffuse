//! High-level CLI Diff Pretty-Printer.
//!
//! Formats full file diff summaries and colored hunks directly to terminal
//! output.

use crate::core::{
    parse::RowKind,
    wire::{OutputFile, OutputRow, PipelineOutput},
};

use super::{
    row::{prefix_of, render_raw_row, render_row_segment, styled_prefix},
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
    let note_str = if file.notes.is_empty() {
        String::new()
    } else {
        format!(
            " · {} note{}",
            file.notes.len(),
            if file.notes.len() == 1 { "" } else { "s" }
        )
    };
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
    let path_display = file.display_path();

    let bar = format!(
        "\x1b[48;5;237m\x1b[1m\x1b[38;5;231m {} ({}) · {}{}{}{}{} \x1b[0m\n",
        path_display,
        file.display_status(),
        add_str,
        del_str,
        mod_str,
        mov_str,
        note_str
    );
    out.push_str(&bar);

    let render_row = |r: &OutputRow, out: &mut String| {
        let pfx = if styled_prefixes {
            styled_prefix(r.kind, r.mov_side, r.new_line, r.old_line, &r.runs, false)
        } else {
            let plain = prefix_of(r.kind, r.mov_side, r.new_line, r.old_line, &r.runs);
            format!("{plain} ")
        };

        let seg = render_row_segment(
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

/// Render complete pipeline output in raw mode: flat green/red text with no
/// subword highlighting and no mod rows, equivalent to the TUI's `R` (raw diff)
/// toggle.
pub fn render_raw_pipeline_output(output: &PipelineOutput) -> String {
    let mut out = String::new();
    for f in &output.files {
        out.push_str(&render_raw_file_diff(f));
    }
    out.push_str("\x1b[0m");
    out
}

/// Render a single file's diff in raw mode: flat green/red text on default
/// black background, colorized number gutter, only - and + rows (no mod rows),
/// and `@@` hunk headers, equivalent to the TUI's `R` toggle.
pub fn render_raw_file_diff(file: &OutputFile) -> String {
    let hunks = if !file.raw_hunks.is_empty() {
        &file.raw_hunks
    } else {
        &file.hunks
    };
    let total_rows: usize = hunks.iter().map(|h| h.rows.len() + 1).sum();
    let mut out = String::with_capacity(total_rows * 128 + 256);

    // Same file header as styled mode.
    let counts = &file.counts;
    let note_str = if file.notes.is_empty() {
        String::new()
    } else {
        format!(
            " · {} note{}",
            file.notes.len(),
            if file.notes.len() == 1 { "" } else { "s" }
        )
    };
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
    let path_display = file.display_path();
    let bar = format!(
        "\x1b[48;5;237m\x1b[1m\x1b[38;5;231m {} ({}) · {}{}{}{}{} \x1b[0m\n",
        path_display,
        file.display_status(),
        add_str,
        del_str,
        mod_str,
        mov_str,
        note_str
    );
    out.push_str(&bar);

    for h in hunks {
        let sep_text = format!(
            "@@ -{},{} +{},{} @@",
            h.old_start, h.old_count, h.new_start, h.new_count
        );
        let hunk_header = render_raw_row(RowKind::Ctx, "", &sep_text, false, false, true);
        out.push_str(&hunk_header);
        out.push('\n');

        for r in &h.rows {
            let pfx = styled_prefix(r.kind, r.mov_side, r.new_line, r.old_line, &r.runs, true);
            let row = render_raw_row(r.kind, &pfx, &r.text, false, false, true);
            out.push_str(&row);
            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests;
