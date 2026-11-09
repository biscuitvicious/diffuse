//! High-level CLI diff rendering entry points and pipeline formatters.

use std::collections::HashMap;

use crate::{
    Badge,
    ansi::{SGR_RESET, optimize_ansi},
    core::{
        notes::{ReviewNote, note_anchors_in_rows},
        parse::RowKind,
        wire::{OutputFile, OutputHunk, OutputNote, OutputRow, PipelineOutput},
    },
    notes::{DEFAULT_CALLOUT_WIDTH, render_output_note_callout},
    row::{styled_prefix_spans, write_raw_row},
    theme::DiffuseTheme,
};

use super::{
    header::{write_file_header_bar_with_width, write_raw_file_header},
    hunk::render_hunk_divider,
    row::render_row_line,
};

pub(crate) fn partition_file_notes<'a>(
    file: &'a OutputFile,
    rows: &[&OutputRow],
) -> (Vec<&'a OutputNote>, HashMap<usize, Vec<&'a OutputNote>>) {
    let mut banner_notes = Vec::new();
    let mut row_notes: HashMap<usize, Vec<&'a OutputNote>> = HashMap::new();

    for note in &file.notes {
        // `File` and unresolvable coordinates never bind to a row.
        let line_coords = match &note.location {
            crate::core::notes::NoteLocation::File => None,
            crate::core::notes::NoteLocation::Line { coord } => Some((*coord, *coord)),
            crate::core::notes::NoteLocation::Range { start, end } => Some((*start, *end)),
        };
        let Some((_start_coord, end_coord)) = line_coords else {
            banner_notes.push(note);
            continue;
        };
        let rn = ReviewNote::from(note);
        if note_anchors_in_rows(&rn, rows.iter().copied()) {
            // Precise row pair matching: both line numbers must agree.
            let target_row = rows.iter().position(|r| {
                r.old_line == end_coord.old_line_num.map(|n| n.get())
                    && r.new_line == end_coord.new_line_num.map(|n| n.get())
            });
            if let Some(idx) = target_row {
                row_notes.entry(idx).or_default().push(note);
                continue;
            }
        }
        banner_notes.push(note);
    }

    (banner_notes, row_notes)
}

/// Render complete pipeline output to terminal ANSI strings.
pub fn render_pipeline_output(
    output: &PipelineOutput,
    theme: &DiffuseTheme,
    styled_prefixes: bool,
) -> String {
    render_pipeline_output_with_width(output, theme, styled_prefixes, DEFAULT_CALLOUT_WIDTH)
}

/// Render complete pipeline output to terminal ANSI strings with a custom note
/// wrapping width.
pub fn render_pipeline_output_with_width(
    output: &PipelineOutput,
    theme: &DiffuseTheme,
    styled_prefixes: bool,
    width: usize,
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
        out.push_str(&render_file_diff_with_width(
            f,
            Some(Badge {
                idx: fi,
                total: Some(total),
            }),
            theme,
            styled_prefixes,
            width,
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
    render_file_diff_with_width(file, badge, theme, styled_prefixes, DEFAULT_CALLOUT_WIDTH)
}

/// Render a single file's header bar and colored hunks with a custom note
/// wrapping width.
pub fn render_file_diff_with_width(
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
    styled_prefixes: bool,
    width: usize,
) -> String {
    let total_rows: usize = file.hunks.iter().map(|h| h.rows.len()).sum();
    let mut out = String::with_capacity(total_rows * 128 + 256);

    let (banner_notes, row_notes) = if !file.notes.is_empty() {
        let all_rows: Vec<&OutputRow> = if !file.rows.is_empty() {
            file.rows.iter().collect()
        } else {
            file.hunks.iter().flat_map(|h| &h.rows).collect()
        };
        partition_file_notes(file, &all_rows)
    } else {
        (Vec::new(), HashMap::new())
    };

    let _ = write_file_header_bar_with_width(&mut out, file, badge, theme, Some(width));

    let emit_note = |note: &OutputNote, out: &mut String| {
        for line in render_output_note_callout(note, false, true, theme, width) {
            out.push_str(&line);
            out.push_str(SGR_RESET);
            out.push('\n');
        }
    };

    let emit_hunk_divider = |hi: usize, hunk: &OutputHunk, out: &mut String| {
        let h_badge = Badge {
            idx: hi,
            total: Some(file.hunks.len()),
        };
        let div = render_hunk_divider(h_badge, hunk, 0, width, theme);
        out.push_str(&div);
        out.push('\n');
    };

    for note in &banner_notes {
        emit_note(note, &mut out);
    }

    let mut row_idx = 0;
    if !file.rows.is_empty() && !file.hunks.is_empty() {
        for (hi, h) in file.hunks.iter().enumerate() {
            emit_hunk_divider(hi, h, &mut out);
            for _ in 0..h.rows.len() {
                if row_idx < file.rows.len() {
                    let r = &file.rows[row_idx];
                    render_row_line(&mut out, r, theme, width, styled_prefixes);
                    if let Some(notes) = row_notes.get(&row_idx) {
                        for note in notes {
                            emit_note(note, &mut out);
                        }
                    }
                    row_idx += 1;
                }
            }
        }
        while row_idx < file.rows.len() {
            let r = &file.rows[row_idx];
            render_row_line(&mut out, r, theme, width, styled_prefixes);
            if let Some(notes) = row_notes.get(&row_idx) {
                for note in notes {
                    emit_note(note, &mut out);
                }
            }
            row_idx += 1;
        }
    } else if !file.rows.is_empty() {
        for r in &file.rows {
            render_row_line(&mut out, r, theme, width, styled_prefixes);
            if let Some(notes) = row_notes.get(&row_idx) {
                for note in notes {
                    emit_note(note, &mut out);
                }
            }
            row_idx += 1;
        }
    } else {
        for (hi, h) in file.hunks.iter().enumerate() {
            emit_hunk_divider(hi, h, &mut out);
            for r in &h.rows {
                render_row_line(&mut out, r, theme, width, styled_prefixes);
                if let Some(notes) = row_notes.get(&row_idx) {
                    for note in notes {
                        emit_note(note, &mut out);
                    }
                }
                row_idx += 1;
            }
        }
    }

    // Drop redundant SGR state transitions per line: identical terminal
    // effect, smaller payload. Wire runs are untouched.
    optimize_ansi(&out).into_owned()
}

/// Render complete pipeline output in raw mode: flat green/red text with no
/// subword highlighting and no mod rows, equivalent to the TUI's `R` (raw diff)
/// toggle.
pub fn render_raw_pipeline_output(output: &PipelineOutput, theme: &DiffuseTheme) -> String {
    render_raw_pipeline_output_with_width(output, theme, DEFAULT_CALLOUT_WIDTH)
}

/// Render complete pipeline output in raw mode with a custom note wrapping
/// width.
pub fn render_raw_pipeline_output_with_width(
    output: &PipelineOutput,
    theme: &DiffuseTheme,
    width: usize,
) -> String {
    let mut out = String::new();
    let total = output.files.len();
    for (fi, f) in output.files.iter().enumerate() {
        out.push_str(&render_raw_file_diff_with_width(
            f,
            Some(Badge {
                idx: fi,
                total: Some(total),
            }),
            theme,
            width,
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
    render_raw_file_diff_with_width(file, badge, theme, DEFAULT_CALLOUT_WIDTH)
}

/// Render a single file's diff in raw mode with a custom width. Review
/// notes never render here: raw mode is plain unified diff, so notes stay
/// in the rich renderer and the interactive reviewer.
pub fn render_raw_file_diff_with_width(
    file: &OutputFile,
    _badge: Option<Badge>,
    theme: &DiffuseTheme,
    _width: usize,
) -> String {
    let hunks = if !file.raw_hunks.is_empty() {
        &file.raw_hunks
    } else {
        &file.hunks
    };
    let total_rows: usize = hunks.iter().map(|h| h.rows.len() + 1).sum();
    let mut out = String::with_capacity(total_rows * 128 + 256);

    let _ = write_raw_file_header(&mut out, file, theme);

    for h in hunks {
        let sep_text = format!(
            "@@ -{},{} +{},{} @@",
            h.old_start, h.old_count, h.new_start, h.new_count
        );
        let _ = write_raw_row(
            &mut out,
            RowKind::Ctx,
            &[],
            &sep_text,
            false,
            false,
            true,
            theme,
        );
        out.push('\n');

        for r in &h.rows {
            let pfx = styled_prefix_spans(
                r.kind, r.mov_side, None, r.group, r.new_line, r.old_line, &r.runs, true, None,
                theme,
            );
            let text = r.text();
            let _ = write_raw_row(&mut out, r.kind, &pfx, &text, false, false, true, theme);
            out.push('\n');
        }
    }

    optimize_ansi(&out).into_owned()
}
