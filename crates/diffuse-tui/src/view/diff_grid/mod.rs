//! Diff grid row, hunk divider, and file header line formatting.

use super::header::truncate_pad;
use crate::{
    core::{
        parse::RowKind,
        wire::{OutputFile, OutputHunk},
    },
    render::slice_by_visual_width,
};

/// Char column of the collapse/expand triangle in [`file_header_text`].
pub const FILE_HEADER_INDICATOR_COL: usize = 12;

/// Build the raw text of a file header banner row.
pub fn file_header_text(file: &OutputFile, is_collapsed: bool) -> String {
    let f_counts = &file.counts;
    let indicator = if is_collapsed { "▶" } else { "▼" };
    let collapse_tag = if is_collapsed { " [collapsed]" } else { "" };
    format!(
        "──────────── {} {} ({}) -{} +{} ~{}{} ────────────",
        indicator,
        file.path,
        file.status,
        f_counts.dels,
        f_counts.adds,
        f_counts.mods,
        collapse_tag
    )
}

/// Render a specific visual segment of a file header banner row.
pub fn render_file_header_segment(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    let bar = file_header_text(file, is_collapsed);
    let piece = slice_by_visual_width(&bar, seg_start, content_width);
    let bar_str = truncate_pad(&piece, content_width);
    if is_selected {
        format!("\x1b[48;5;241m\x1b[1m\x1b[38;5;219m{bar_str}\x1b[0m")
    } else if in_band || is_current {
        format!("\x1b[48;5;239m\x1b[1m\x1b[38;5;219m{bar_str}\x1b[0m")
    } else {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;183m{bar_str}\x1b[0m")
    }
}

/// Render a file header banner row with collapse status indicator and counts.
pub fn render_file_header(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
) -> String {
    render_file_header_segment(
        file,
        is_collapsed,
        is_selected,
        is_current,
        in_band,
        0,
        content_width,
    )
}

/// Build the raw text of a hunk header divider with format
/// `{rows} lines (-dels +adds ~mods)` plus the hidden-gap tail.
pub fn hunk_divider_text(hi: usize, hunk: &OutputHunk, hidden_gap: usize) -> String {
    let mut dels: usize = 0;
    let mut adds: usize = 0;
    let mut mods: usize = 0;
    for r in &hunk.rows {
        match r.kind {
            RowKind::Del => dels += 1,
            RowKind::Add => adds += 1,
            RowKind::Mod => mods += 1,
            _ => {}
        }
    }
    let total = hunk.rows.len();
    let unit = if total == 1 { "line" } else { "lines" };
    let summary = format!("{total} {unit} (-{dels} +{adds} ~{mods})");

    let gap_part = if hidden_gap > 0 {
        let arrows = if hi == 0 {
            "▼▼"
        } else {
            "▲▼ ▲▲ ▼▼"
        };
        format!(" · {hidden_gap} hidden {arrows}")
    } else {
        String::new()
    };
    format!("───────── H{} · {summary}{gap_part} ─────────", hi + 1)
}

/// Render a specific visual segment of a hunk header divider.
pub fn render_hunk_divider_segment(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    let txt = hunk_divider_text(hi, hunk, hidden_gap);
    let piece = slice_by_visual_width(&txt, seg_start, content_width);
    let txt_str = truncate_pad(&piece, content_width);
    if is_current {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    } else {
        format!("\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    }
}

/// Render a hunk header divider with format `x-y → a-b`, hidden lines gap,
/// and context expansion buttons (`▲▼ ▲▲ ▼▼` for interior hunks, or `▼▼` for H1
/// at top of file).
pub fn render_hunk_divider(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> String {
    render_hunk_divider_segment(hi, hunk, hidden_gap, is_current, 0, content_width)
}

/// Build the raw text of a bottom gap divider.
pub fn bottom_gap_divider_text(hidden_gap: usize) -> String {
    let unit = if hidden_gap == 1 { "line" } else { "lines" };
    format!("───────── {hidden_gap} {unit} hidden · ▲▲ ─────────")
}

/// Render a specific visual segment of a bottom gap divider.
pub fn render_bottom_gap_divider_segment(
    hidden_gap: usize,
    is_current: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    let txt = bottom_gap_divider_text(hidden_gap);
    let piece = slice_by_visual_width(&txt, seg_start, content_width);
    let txt_str = truncate_pad(&piece, content_width);
    if is_current {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    } else {
        format!("\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    }
}

/// Render a clean bottom divider for trailing hidden lines after the last hunk.
pub fn render_bottom_gap_divider(
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> String {
    render_bottom_gap_divider_segment(hidden_gap, is_current, 0, content_width)
}

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Render a file header banner row as a styled Ratatui [`Line`].
pub fn file_header_line(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
) -> Line<'static> {
    let bar = file_header_text(file, is_collapsed);
    let bar_str = truncate_pad(&bar, content_width);
    let style = if is_selected {
        Style::default()
            .bg(Color::Indexed(241))
            .fg(Color::Indexed(219))
            .add_modifier(Modifier::BOLD)
    } else if in_band || is_current {
        Style::default()
            .bg(Color::Indexed(239))
            .fg(Color::Indexed(219))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .bg(Color::Indexed(237))
            .fg(Color::Indexed(183))
            .add_modifier(Modifier::BOLD)
    };
    Line::from(Span::styled(bar_str, style))
}

/// Render a hunk header divider as a styled Ratatui [`Line`].
pub fn hunk_divider_line(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> Line<'static> {
    let txt = hunk_divider_text(hi, hunk, hidden_gap);
    let txt_str = truncate_pad(&txt, content_width);
    let style = if is_current {
        Style::default()
            .bg(Color::Indexed(237))
            .fg(Color::Indexed(220))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Indexed(220))
            .add_modifier(Modifier::BOLD)
    };
    Line::from(Span::styled(txt_str, style))
}

/// Render a bottom gap divider as a styled Ratatui [`Line`].
pub fn bottom_gap_divider_line(
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> Line<'static> {
    let txt = bottom_gap_divider_text(hidden_gap);
    let txt_str = truncate_pad(&txt, content_width);
    let style = if is_current {
        Style::default()
            .bg(Color::Indexed(237))
            .fg(Color::Indexed(220))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Indexed(220))
            .add_modifier(Modifier::BOLD)
    };
    Line::from(Span::styled(txt_str, style))
}

#[cfg(test)]
mod tests;
