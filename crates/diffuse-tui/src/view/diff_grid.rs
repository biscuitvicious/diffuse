//! Diff grid row, hunk divider, and file header line formatting.

use super::header::truncate_pad;
use diffuse_core::wire::{OutputFile, OutputHunk};

/// Render a file header banner row with collapse status indicator and counts.
pub fn render_file_header(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    in_band: bool,
    content_width: usize,
) -> String {
    let f_counts = &file.counts;
    let indicator = if is_collapsed { "▸" } else { "▾" };
    let collapse_tag = if is_collapsed { " [collapsed]" } else { "" };
    let bar = format!(
        "─── {} {} ({}) +{} -{} ~{}{} ───",
        indicator,
        file.path,
        file.status,
        f_counts.adds,
        f_counts.dels,
        f_counts.mods,
        collapse_tag
    );
    let bar_str = truncate_pad(&bar, content_width);
    if is_selected {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;231m{bar_str}\x1b[0m")
    } else if in_band {
        format!("\x1b[48;5;240m\x1b[1m\x1b[38;5;231m{bar_str}\x1b[0m")
    } else {
        format!("\x1b[38;5;244m{bar_str}\x1b[39m")
    }
}

/// Render a hunk header divider with context envelope markers (`ctx N▴ M▾`).
pub fn render_hunk_divider(
    hi: usize,
    hunk: &OutputHunk,
    ctx: (usize, usize),
    is_current: bool,
    content_width: usize,
) -> String {
    let span = format!(
        "−{},{} +{},{}",
        hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
    );
    let txt = format!(
        "───── H{} · {span} · ctx {}▴ {}▾ ─────",
        hi + 1,
        ctx.0,
        ctx.1
    );
    let txt_str = truncate_pad(&txt, content_width);
    if is_current {
        format!("\x1b[48;5;236m\x1b[1m\x1b[38;5;231m{txt_str}\x1b[0m")
    } else {
        format!("\x1b[38;5;240m{txt_str}\x1b[39m")
    }
}
