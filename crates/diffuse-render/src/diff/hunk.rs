//! Hunk header divider rendering and leading gap calculation.

use crate::{
    Badge,
    ansi::ColorExt,
    core::wire::{OutputFile, OutputHunk},
    counts::format_counts_plain,
    row::slice_by_visual_width,
    theme::DiffuseTheme,
    width::display_width,
};

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
/// `──── Hunk 2/3 · 6 lines: −1 +4 ~1 ────` plus optional hidden-gap tail.
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
    // Rest state resolves at the cursor baseline tier (asset bg is
    // `primary`, rendering as the empty-string background code).
    let divider = &theme.palette[crate::theme::Cursor::Rest].hunk_divider;
    let bg_code = match divider.bg {
        yansi::Color::Primary => String::new(),
        color => color.bg_code(),
    };
    let fg_code = divider.fg.fg_code();

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
