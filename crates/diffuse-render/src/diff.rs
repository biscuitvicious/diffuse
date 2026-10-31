//! High-level CLI Diff Pretty-Printer.
//!
//! Formats full file diff summaries and colored hunks directly to terminal
//! output.

use std::{
    collections::HashMap,
    fmt::{self, Write},
};

use crate::{
    Badge,
    ansi::{ColorExt, FG_RESET, SGR_RESET, optimize_ansi, strip_ansi},
    core::{
        notes::{ReviewNote, note_anchors_in_rows},
        parse::RowKind,
        wire::{OutputFile, OutputHunk, OutputNote, OutputRow, PipelineOutput},
    },
    counts::{count_cells, format_counts_plain},
    notes::{DEFAULT_CALLOUT_WIDTH, render_output_note_callout},
    row::{display_text, expand_tabs, slice_by_visual_width},
    width::display_width,
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
    let bg_code = theme.palette.header_bar_bg.bg_code();
    let fg_code = theme.palette.header_bar_fg.fg_code();
    let path_white = theme.palette.file_header_path_current.fg_code();

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
    for cell in count_cells(counts, &theme.palette) {
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
            Some(Badge { idx: fi, total }),
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

fn partition_file_notes<'a>(
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

/// Compute the hidden line count before hunk `hi` in `file`.
#[must_use]
pub fn hunk_leading_gap(file: &OutputFile, hi: usize) -> usize {
    let Some(hunk) = file.hunks.get(hi) else {
        return 0;
    };
    let hunk_new_start = hunk.rows.iter().find_map(|r| r.new_line);
    let hunk_old_start = hunk.rows.iter().find_map(|r| r.old_line);

    if hi == 0 {
        if let Some(ns) = hunk_new_start {
            ns.saturating_sub(1) as usize
        } else if let Some(os) = hunk_old_start {
            os.saturating_sub(1) as usize
        } else {
            (hunk.new_start.max(hunk.old_start)).saturating_sub(1) as usize
        }
    } else {
        let Some(prev) = file.hunks.get(hi - 1) else {
            return 0;
        };
        let prev_new_end = prev.rows.iter().rev().find_map(|r| r.new_line);
        let prev_old_end = prev.rows.iter().rev().find_map(|r| r.old_line);

        if let (Some(cur_new), Some(prev_new)) = (hunk_new_start, prev_new_end) {
            cur_new.saturating_sub(prev_new + 1) as usize
        } else if let (Some(cur_old), Some(prev_old)) = (hunk_old_start, prev_old_end) {
            cur_old.saturating_sub(prev_old + 1) as usize
        } else {
            let cur_s = hunk.new_start.max(hunk.old_start) as usize;
            let prev_e = if prev.new_count > 0 {
                (prev.new_start + prev.new_count.saturating_sub(1)) as usize
            } else {
                (prev.old_start + prev.old_count.saturating_sub(1)) as usize
            };
            cur_s.saturating_sub(prev_e + 1)
        }
    }
}

/// Build the raw text of a hunk header divider with format
/// `──── H2/3 · 6 lines: −1 +4 ~1 ────` plus optional hidden-gap tail.
#[must_use]
pub fn hunk_divider_text(badge: Badge, hunk: &OutputHunk, hidden_gap: usize) -> String {
    let counts = hunk.counts();
    let (dels, adds, mods, movs) = (counts.dels, counts.adds, counts.mods, counts.movs);
    let total = dels + adds + mods + movs;
    let summary_part = if total > 0 {
        let unit = if total == 1 { "line" } else { "lines" };
        let s = format!("{total} {unit}");
        let counts_str = format_counts_plain(&counts);
        if counts_str.is_empty() {
            format!(" · {s}")
        } else {
            format!(" · {s}: {counts_str}")
        }
    } else {
        String::new()
    };

    let gap_part = if hidden_gap > 0 {
        let gap_unit = if hidden_gap == 1 { "line" } else { "lines" };
        let arrows = if badge.idx == 0 {
            "▼"
        } else {
            "⬍ · ▲ · ▼"
        };
        format!(" · {hidden_gap} more {gap_unit}: {arrows}")
    } else {
        String::new()
    };
    format!("──── {}{summary_part}{gap_part} ────", badge.hunk_text())
}

/// Render a hunk header divider filling the specified width with horizontal bar
/// characters.
#[must_use]
pub fn render_hunk_divider(
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    width: usize,
    theme: &DiffuseTheme,
) -> String {
    let txt = hunk_divider_text(badge, hunk, hidden_gap);
    let (bg, fg) = theme.palette.hunk_divider_colors(false, false);
    let bg_code = match bg {
        yansi::Color::Primary => String::new(),
        color => color.bg_code(),
    };
    let fg_code = fg.fg_code();

    let piece = slice_by_visual_width(&txt, 0, width);
    let piece_w = display_width(&piece);
    let bar_str = if piece_w < width {
        let ruler_len = width - piece_w;
        format!("{piece}{}", "─".repeat(ruler_len))
    } else {
        piece
    };
    format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m")
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

/// Render one diff row (gutter prefix, run coloring, wrapping, padding)
/// exactly as `render_file_diff` does, appending complete lines to `out`.
fn render_row_line(
    out: &mut String,
    r: &OutputRow,
    theme: &DiffuseTheme,
    width: usize,
    styled_prefixes: bool,
) {
    let pfx = if styled_prefixes {
        styled_prefix(
            r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs, false, theme,
        )
    } else {
        let plain = prefix_of(r.kind, r.mov_side, r.group, r.new_line, r.old_line, &r.runs);
        format!("{plain} ")
    };

    let cont_pfx = if styled_prefixes {
        "\x1b[38;5;242m      \x1b[39m".to_string()
    } else {
        "      ".to_string()
    };

    let text = r.text();

    if width == 0 {
        let mut row_buf = String::with_capacity(text.len() + 64);
        let _ = write_row_segment(
            &mut row_buf,
            &text,
            r.is_aux(),
            r.kind,
            r.mov_side,
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
        out.push_str(&row_buf);
        out.push_str(SGR_RESET);
        out.push('\n');
        return;
    }

    let text_cols = width.saturating_sub(6).max(1);
    let disp = display_text(&text, r.is_aux(), r.kind, &r.runs, 0, true);
    let exp = expand_tabs(&disp, 4);
    let len = display_width(&exp);
    let seg_count = if len == 0 { 1 } else { len.div_ceil(text_cols) };

    if seg_count == 1 {
        let mut row_buf = String::with_capacity(text.len() + 64);
        let _ = write_row_segment(
            &mut row_buf,
            &text,
            r.is_aux(),
            r.kind,
            r.mov_side,
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
        let plain = strip_ansi(&row_buf);
        let exp = expand_tabs(&plain, 4);
        let vis_w = display_width(&exp);
        out.push_str(&row_buf);
        if vis_w < width {
            for _ in 0..(width - vis_w) {
                out.push(' ');
            }
        }
        out.push_str(SGR_RESET);
        out.push('\n');
        return;
    }

    for s_idx in 0..seg_count {
        let seg_pfx = if s_idx == 0 { &pfx } else { &cont_pfx };
        let seg_start = s_idx * text_cols;
        let seg_end = seg_start + text_cols;
        let mut row_buf = String::with_capacity(text_cols + 64);
        let _ = write_row_segment(
            &mut row_buf,
            &text,
            r.is_aux(),
            r.kind,
            r.mov_side,
            &r.runs,
            seg_start,
            seg_end,
            seg_pfx,
            false,
            false,
            0,
            true,
            true,
            theme,
        );
        let plain = strip_ansi(&row_buf);
        let exp = expand_tabs(&plain, 4);
        let vis_w = display_width(&exp);
        out.push_str(&row_buf);
        if vis_w < width {
            for _ in 0..(width - vis_w) {
                out.push(' ');
            }
        }
        out.push_str(SGR_RESET);
        out.push('\n');
    }
}

/// Render original source rows with the standard diff row coloring for
/// their kind and language-derived token runs — the presentation used for
/// note `original_source_rows` snapshots.
pub fn render_source_rows(
    rows: &[OutputRow],
    theme: &DiffuseTheme,
    width: usize,
    styled_prefixes: bool,
) -> Vec<String> {
    let mut out = String::with_capacity(rows.len() * 128);
    for r in rows {
        render_row_line(&mut out, r, theme, width, styled_prefixes);
    }
    out.trim_end_matches('\n')
        .split('\n')
        .map(str::to_string)
        .collect()
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
            total: file.hunks.len(),
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
            Some(Badge { idx: fi, total }),
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

/// Render a single file's diff in raw mode with a custom note wrapping width.
pub fn render_raw_file_diff_with_width(
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
    width: usize,
) -> String {
    let hunks = if !file.raw_hunks.is_empty() {
        &file.raw_hunks
    } else {
        &file.hunks
    };
    let total_rows: usize = hunks.iter().map(|h| h.rows.len() + 1).sum();
    let mut out = String::with_capacity(total_rows * 128 + 256);

    let (banner_notes, row_notes) = if !file.notes.is_empty() {
        let all_rows: Vec<&OutputRow> = hunks.iter().flat_map(|h| &h.rows).collect();
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

    for note in &banner_notes {
        emit_note(note, &mut out);
    }

    let mut row_idx = 0;
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
            let text = r.text();
            let _ = write_raw_row(&mut out, r.kind, &pfx, &text, false, false, true, theme);
            out.push('\n');

            if let Some(notes) = row_notes.get(&row_idx) {
                for note in notes {
                    emit_note(note, &mut out);
                }
            }
            row_idx += 1;
        }
    }

    optimize_ansi(&out).into_owned()
}

#[cfg(test)]
mod tests;
