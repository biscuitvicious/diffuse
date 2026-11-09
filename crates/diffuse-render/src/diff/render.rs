//! High-level CLI diff rendering entry points and pipeline formatters.

use std::collections::HashMap;

use crate::{
    Badge,
    ansi::{SGR_RESET, optimize_ansi},
    core::{
        ModGlyph, ModSide,
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
    render_pipeline_output_with_options(output, theme, styled_prefixes, DEFAULT_CALLOUT_WIDTH, None)
}

/// Render complete pipeline output to terminal ANSI strings with a custom note
/// wrapping width.
pub fn render_pipeline_output_with_width(
    output: &PipelineOutput,
    theme: &DiffuseTheme,
    styled_prefixes: bool,
    width: usize,
) -> String {
    render_pipeline_output_with_options(output, theme, styled_prefixes, width, None)
}

/// Render complete pipeline output to terminal ANSI strings with options
/// including modification row expansion.
pub fn render_pipeline_output_with_options(
    output: &PipelineOutput,
    theme: &DiffuseTheme,
    styled_prefixes: bool,
    width: usize,
    expand_mods: Option<bool>,
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
        out.push_str(&render_file_diff_with_options(
            f,
            Some(Badge {
                idx: fi,
                total: Some(total),
            }),
            theme,
            styled_prefixes,
            width,
            expand_mods,
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
    render_file_diff_with_options(
        file,
        badge,
        theme,
        styled_prefixes,
        DEFAULT_CALLOUT_WIDTH,
        None,
    )
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
    render_file_diff_with_options(file, badge, theme, styled_prefixes, width, None)
}

/// Render a single file's header bar and colored hunks with custom options
/// including modification row expansion.
pub fn render_file_diff_with_options(
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
    styled_prefixes: bool,
    width: usize,
    expand_mods: Option<bool>,
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

    let emit_slice = |rows: &[OutputRow], start_idx: usize, out: &mut String| {
        let mut i = 0;
        while i < rows.len() {
            let r = &rows[i];
            if expand_mods == Some(true) && r.kind == RowKind::Mod {
                let mut j = i + 1;
                while j < rows.len() && rows[j].kind == RowKind::Mod {
                    j += 1;
                }
                let k = j - i;
                for (idx, row) in rows[i..j].iter().enumerate() {
                    let glyph = ModGlyph::for_cluster(ModSide::Del, idx, k);
                    render_row_line(
                        out,
                        row,
                        theme,
                        width,
                        styled_prefixes,
                        Some(ModSide::Del),
                        Some(glyph),
                    );
                }
                for (idx, row) in rows[i..j].iter().enumerate() {
                    let glyph = ModGlyph::for_cluster(ModSide::Add, idx, k);
                    render_row_line(
                        out,
                        row,
                        theme,
                        width,
                        styled_prefixes,
                        Some(ModSide::Add),
                        Some(glyph),
                    );
                }
                for row_idx in (start_idx + i)..(start_idx + j) {
                    if let Some(notes) = row_notes.get(&row_idx) {
                        for note in notes {
                            emit_note(note, out);
                        }
                    }
                }
                i = j;
            } else {
                render_row_line(out, r, theme, width, styled_prefixes, None, None);
                if let Some(notes) = row_notes.get(&(start_idx + i)) {
                    for note in notes {
                        emit_note(note, out);
                    }
                }
                i += 1;
            }
        }
    };

    for note in &banner_notes {
        emit_note(note, &mut out);
    }

    if !file.rows.is_empty() && !file.hunks.is_empty() {
        let mut row_idx = 0;
        for (hi, h) in file.hunks.iter().enumerate() {
            emit_hunk_divider(hi, h, &mut out);
            let hunk_len = h.rows.len();
            let end_idx = (row_idx + hunk_len).min(file.rows.len());
            emit_slice(&file.rows[row_idx..end_idx], row_idx, &mut out);
            row_idx = end_idx;
        }
        if row_idx < file.rows.len() {
            emit_slice(&file.rows[row_idx..], row_idx, &mut out);
        }
    } else if !file.rows.is_empty() {
        emit_slice(&file.rows, 0, &mut out);
    } else {
        let mut row_idx = 0;
        for (hi, h) in file.hunks.iter().enumerate() {
            emit_hunk_divider(hi, h, &mut out);
            emit_slice(&h.rows, row_idx, &mut out);
            row_idx += h.rows.len();
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
                r.kind, r.mov_side, None, None, r.group, r.new_line, r.old_line, &r.runs, true,
                None, theme,
            );
            let text = r.text();
            let _ = write_raw_row(&mut out, r.kind, &pfx, &text, false, false, true, theme);
            out.push('\n');
        }
    }

    optimize_ansi(&out).into_owned()
}
