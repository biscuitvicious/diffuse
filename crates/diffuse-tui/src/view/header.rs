//! Header, status bar, and bottom ribbon rendering.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use diffuse_core::wire::OutputFile;

/// Calculate the visible column width of a string by ignoring ANSI escape
/// sequences and using unicode-width for wide chars (emojis, CJK, etc.).
pub fn visible_width(s: &str) -> usize {
    let mut in_escape = false;
    let mut count = 0;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            count += c.width().unwrap_or(0);
        }
    }
    count
}

/// Truncate or right-pad a string with spaces to exactly `target_width`
/// terminal columns (accounting for wide characters/emojis).
pub fn truncate_pad(s: &str, target_width: usize) -> String {
    let mut current_width = 0;
    let mut out = String::new();
    for g in s.graphemes(true) {
        let gw = g.width();
        if current_width + gw > target_width {
            break;
        }
        out.push_str(g);
        current_width += gw;
    }
    if current_width < target_width {
        out.push_str(&" ".repeat(target_width - current_width));
    }
    out
}

/// Render the top review header bar with file path, status, and line counts.
pub fn render_header_bar(repo: &str, file: &OutputFile, width: usize) -> String {
    let counts = &file.counts;
    let note_count = file.notes.len();
    let note_s = format!(
        "{note_count} note{}",
        if note_count == 1 { "" } else { "s" }
    );
    let counts_str = format!(
        "+{}{}{}{} · {}",
        counts.adds,
        if counts.dels > 0 {
            format!(" −{}", counts.dels)
        } else {
            String::new()
        },
        if counts.mods > 0 {
            format!(" ~{}", counts.mods)
        } else {
            String::new()
        },
        if counts.movs > 0 {
            format!(" ≈{} moved", counts.movs)
        } else {
            String::new()
        },
        note_s
    );

    let head_raw = format!(
        " Review: {} · {} ({}) · {}",
        repo, file.path, file.status, counts_str
    );
    let head_str = truncate_pad(&head_raw, width);
    format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;231m{head_str}\x1b[0m")
}

/// Parameters for rendering the secondary status bar.
pub struct StatusBarContext<'a> {
    pub cur_fi: usize,
    pub total_files: usize,
    pub cur_hunk: usize,
    pub total_hunks: usize,
    pub raw_diff: bool,
    pub col_offset: usize,
    pub num_buffer: &'a str,
    pub branch: &'a str,
    pub width: usize,
}

/// Render the secondary status bar with file index, hunk index, and mode
/// indicators.
pub fn render_status_bar(ctx: StatusBarContext<'_>) -> String {
    let mode_txt = if ctx.raw_diff {
        " · raw diff".to_string()
    } else {
        String::new()
    };
    let go_txt = if !ctx.num_buffer.is_empty() {
        format!(" · go {}", ctx.num_buffer)
    } else {
        String::new()
    };
    let col_txt = if ctx.col_offset > 0 {
        format!(" · col {}+", ctx.col_offset)
    } else {
        String::new()
    };
    let branch_txt = if !ctx.branch.is_empty() {
        format!(" · branch: {}", ctx.branch)
    } else {
        String::new()
    };
    let status_raw = format!(
        " {}/{} files · hunk {}/{}{}{}{}{}",
        ctx.cur_fi + 1,
        ctx.total_files,
        ctx.cur_hunk + 1,
        ctx.total_hunks,
        mode_txt,
        col_txt,
        go_txt,
        branch_txt
    );
    let status_str = truncate_pad(&status_raw, ctx.width);
    format!("\x1b[38;5;244m{status_str}\x1b[39m")
}

/// Render the bottom navigation key-bindings ribbon.
pub fn render_bottom_ribbon(width: usize) -> (String, String) {
    let divider = format!(
        "\x1b[38;5;244m {}\x1b[39m",
        "─".repeat(width.saturating_sub(2).min(60))
    );
    let ribbon_text = truncate_pad(
        " [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]stage [x]discard [q]quit [?]help",
        width.saturating_sub(1),
    );
    let ribbon = format!("\x1b[38;5;244m{ribbon_text}\x1b[39m");
    (divider, ribbon)
}
