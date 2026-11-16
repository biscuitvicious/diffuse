//! Hunk header divider rendering and leading gap calculation.

use crate::{
    Badge,
    ansi::ColorExt,
    core::wire::{OutputFile, OutputHunk},
    counts::{count_cells, format_counts_plain},
    span::{StyledSpan, slice_spans, spans_width},
    theme::{DiffuseTheme, SurfaceColors, UiPalette},
};
use yansi::Color;

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
/// `──── Hunk 2/3 −1 +4 ~1 ────` plus optional hidden-gap tail.
#[must_use]
pub fn hunk_divider_text(badge: Badge, hunk: &OutputHunk, hidden_gap: usize) -> String {
    let counts = hunk.counts();
    let counts_str = format_counts_plain(&counts);
    let summary_part = if counts_str.is_empty() {
        String::new()
    } else {
        format!(" {counts_str}")
    };

    let gap_part = if hidden_gap > 0 {
        let gap_unit = if hidden_gap == 1 { "line" } else { "lines" };
        format!(" · {hidden_gap} more {gap_unit}")
    } else {
        String::new()
    };
    format!("──── {}{summary_part}{gap_part} ────", badge.hunk_text())
}

/// Build the structured spans of a hunk header divider. Base spans (rule
/// prefix, separators, gap text, rule postfix) use `divider.fg`.
/// Count spans use their bucket colors from `ui`. All spans use `bg` and are
/// bold.
#[must_use]
pub fn hunk_divider_spans(
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    divider: &SurfaceColors,
    ui: &UiPalette,
    bg: Option<Color>,
) -> Vec<StyledSpan> {
    let mut spans = Vec::new();

    // 1. Prefix: `──── {badge}`
    let prefix = format!("──── {}", badge.hunk_text());
    spans.push(StyledSpan::styled(prefix, Some(divider.fg), bg).with_bold(true));

    // 2. Change counts (if any)
    let counts = hunk.counts();
    let cells = count_cells(&counts, ui);
    if !cells.is_empty() {
        for cell in &cells {
            spans.push(StyledSpan::styled(" ", Some(divider.fg), bg).with_bold(true));
            let cell_text = format!("{}{}", cell.marker, cell.value);
            spans.push(StyledSpan::styled(cell_text, Some(cell.color), bg).with_bold(true));
        }
    }

    // 3. Hidden gap (if any)
    if hidden_gap > 0 {
        let gap_unit = if hidden_gap == 1 { "line" } else { "lines" };
        let gap_str = format!(" · {hidden_gap} more {gap_unit}");
        spans.push(StyledSpan::styled(gap_str, Some(divider.fg), bg).with_bold(true));
    }

    // 4. Suffix: ` ────`
    spans.push(StyledSpan::styled(" ────", Some(divider.fg), bg).with_bold(true));

    spans
}

/// Render a hunk header divider filling the specified width with horizontal bar
/// characters. Style decisions come from [`hunk_divider_spans`]; emission
/// keeps the exact skeleton (explicit background, bold set, foreground,
/// ruler text, full-reset close) so output stays byte-identical.
#[must_use]
pub fn render_hunk_divider(
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    width: usize,
    theme: &DiffuseTheme,
) -> String {
    let divider = &theme.palette[crate::theme::Cursor::Rest].hunk_divider;
    let bg = match divider.bg {
        Color::Primary => theme.palette.ui.bg_base,
        color => color,
    };
    let mut spans = hunk_divider_spans(
        badge,
        hunk,
        hidden_gap,
        divider,
        &theme.palette.ui,
        Some(bg),
    );
    let total_w = spans_width(&spans);
    if total_w < width {
        let ruler_len = width - total_w;
        spans.push(
            StyledSpan::styled("─".repeat(ruler_len), Some(divider.fg), Some(bg)).with_bold(true),
        );
    } else if total_w > width {
        spans = slice_spans(&spans, 0, width);
    }

    let mut out = String::with_capacity(width + 64);
    out.push_str(&bg.bg_code());
    out.push_str("\x1b[1m");
    let mut current_fg: Option<Color> = None;
    for span in &spans {
        if span.fg != current_fg {
            if let Some(color) = span.fg {
                out.push_str(&color.fg_code());
            } else {
                out.push_str("\x1b[39m");
            }
            current_fg = span.fg;
        }
        out.push_str(&span.text);
    }
    out.push_str("\x1b[0m");
    out
}
