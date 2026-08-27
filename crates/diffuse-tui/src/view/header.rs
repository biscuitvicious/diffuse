//! Header, status bar, and bottom ribbon rendering.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{core::wire::OutputFile, render::slice_by_visual_width};

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

/// Wrap text into segments of `max_cols` visual width.
pub fn wrap_text_by_visual_width(text: &str, max_cols: usize) -> Vec<String> {
    if max_cols == 0 {
        return vec![text.to_string()];
    }
    let total_w = visible_width(text);
    if total_w <= max_cols {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut start_col = 0;
    while start_col < total_w {
        let piece = slice_by_visual_width(text, start_col, max_cols);
        if piece.is_empty() {
            break;
        }
        start_col += max_cols;
        lines.push(piece);
    }
    lines
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

/// Render the top review header bar with file path, status, and line counts,
/// wrapping across lines if wider than `width`.
pub fn render_header_bar(repo: &str, file: &OutputFile, width: usize) -> Vec<String> {
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
    let wrapped = wrap_text_by_visual_width(&head_raw, width);
    wrapped
        .into_iter()
        .map(|piece| {
            let head_str = truncate_pad(&piece, width);
            format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;231m{head_str}\x1b[0m")
        })
        .collect()
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
    pub is_streaming: bool,
}

/// Render the secondary status bar with file index, hunk index, and mode
/// indicators, wrapping across lines if wider than `width`.
pub fn render_status_bar(ctx: StatusBarContext<'_>) -> Vec<String> {
    let files_badge = format!(
        "\x1b[1m\x1b[38;5;183m{}/{} files\x1b[0m",
        ctx.cur_fi + 1,
        ctx.total_files
    );
    let hunk_badge = format!(
        "\x1b[1m\x1b[38;5;214mhunk {}/{}\x1b[0m",
        ctx.cur_hunk + 1,
        ctx.total_hunks
    );
    let stream_txt = if ctx.is_streaming {
        " \x1b[38;5;240m·\x1b[0m \x1b[1m\x1b[38;5;117m● streaming...\x1b[0m"
    } else {
        ""
    };
    let mode_txt = if ctx.raw_diff {
        " \x1b[38;5;240m·\x1b[0m \x1b[1m\x1b[38;5;220mraw diff\x1b[0m"
    } else {
        ""
    };
    let col_txt = if ctx.col_offset > 0 {
        format!(
            " \x1b[38;5;240m·\x1b[0m \x1b[38;5;244mcol {}+\x1b[0m",
            ctx.col_offset
        )
    } else {
        String::new()
    };
    let go_txt = if !ctx.num_buffer.is_empty() {
        format!(
            " \x1b[38;5;240m·\x1b[0m \x1b[1m\x1b[38;5;221mgo {}\x1b[0m",
            ctx.num_buffer
        )
    } else {
        String::new()
    };
    let branch_txt = if !ctx.branch.is_empty() {
        format!(
            " \x1b[38;5;240m·\x1b[0m \x1b[38;5;244mbranch:\x1b[0m \x1b[1m\x1b[38;5;252m{}\x1b[0m",
            ctx.branch
        )
    } else {
        String::new()
    };
    let line = format!(
        " {files_badge}{stream_txt} \x1b[38;5;240m·\x1b[0m {hunk_badge}{mode_txt}{col_txt}{go_txt}{branch_txt}"
    );
    let vw = visible_width(&line);
    if vw <= ctx.width {
        vec![format!(
            "{line}{}\x1b[0m",
            " ".repeat(ctx.width.saturating_sub(vw))
        )]
    } else {
        let wrapped = wrap_text_by_visual_width(&line, ctx.width);
        wrapped
            .into_iter()
            .map(|piece| {
                let pw = visible_width(&piece);
                if pw < ctx.width {
                    format!("{piece}{}\x1b[0m", " ".repeat(ctx.width - pw))
                } else {
                    format!("{piece}\x1b[0m")
                }
            })
            .collect()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_status_bar_styling() {
        let ctx = StatusBarContext {
            cur_fi: 0,
            total_files: 11,
            cur_hunk: 0,
            total_hunks: 3,
            raw_diff: false,
            col_offset: 0,
            num_buffer: "",
            branch: "main",
            width: 80,
            is_streaming: false,
        };
        let rendered = render_status_bar(ctx).join("\n");
        assert!(
            rendered.contains("1/11 files"),
            "status bar must show 1/11 files"
        );
        assert!(
            rendered.contains("38;5;183m") || rendered.contains("38;5;225m"),
            "file stats in status bar must have purple tint"
        );
        assert!(
            rendered.contains("38;5;214m"),
            "hunk count in status bar must have orange tint"
        );
    }
}
