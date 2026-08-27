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
        "\x1b[1m\x1b[38;5;220mhunk {}/{}\x1b[0m",
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

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Render the review header bar as a styled Ratatui [`Line`].
pub fn header_bar_line(repo: &str, file: &OutputFile) -> Line<'static> {
    let counts = &file.counts;
    let note_count = file.notes.len();
    let note_s = format!(
        "{note_count} note{}",
        if note_count == 1 { "" } else { "s" }
    );
    let mut spans = vec![
        Span::styled(
            " Review: ",
            Style::default()
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            repo.to_string(),
            Style::default()
                .fg(Color::Indexed(117))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::default().fg(Color::Indexed(240))),
        Span::styled(
            file.path.clone(),
            Style::default()
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({}) · ", file.status),
            Style::default().fg(Color::Indexed(244)),
        ),
        Span::styled(
            format!("+{}", counts.adds),
            Style::default().fg(Color::Indexed(120)),
        ),
    ];
    if counts.dels > 0 {
        spans.push(Span::styled(
            format!(" −{}", counts.dels),
            Style::default().fg(Color::Indexed(203)),
        ));
    }
    if counts.mods > 0 {
        spans.push(Span::styled(
            format!(" ~{}", counts.mods),
            Style::default().fg(Color::Indexed(220)),
        ));
    }
    if counts.movs > 0 {
        spans.push(Span::styled(
            format!(" ≈{} moved", counts.movs),
            Style::default().fg(Color::Indexed(183)),
        ));
    }
    spans.push(Span::styled(
        format!(" · {note_s}"),
        Style::default().fg(Color::Indexed(244)),
    ));

    Line::from(spans).style(Style::default().bg(Color::Indexed(237)))
}

/// Render the secondary status bar as a styled Ratatui [`Line`].
pub fn status_bar_line(ctx: StatusBarContext<'_>) -> Line<'static> {
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            format!("{}/{} files", ctx.cur_fi + 1, ctx.total_files),
            Style::default()
                .fg(Color::Indexed(183))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if ctx.is_streaming {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.push(Span::styled(
            "● streaming...",
            Style::default()
                .fg(Color::Indexed(117))
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        " · ",
        Style::default().fg(Color::Indexed(240)),
    ));
    spans.push(Span::styled(
        format!("hunk {}/{}", ctx.cur_hunk + 1, ctx.total_hunks),
        Style::default()
            .fg(Color::Indexed(220))
            .add_modifier(Modifier::BOLD),
    ));
    if ctx.raw_diff {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.push(Span::styled(
            "raw diff",
            Style::default()
                .fg(Color::Indexed(220))
                .add_modifier(Modifier::BOLD),
        ));
    }
    if ctx.col_offset > 0 {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.push(Span::styled(
            format!("col {}+", ctx.col_offset),
            Style::default().fg(Color::Indexed(244)),
        ));
    }
    if !ctx.num_buffer.is_empty() {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.push(Span::styled(
            format!("go {}", ctx.num_buffer),
            Style::default()
                .fg(Color::Indexed(221))
                .add_modifier(Modifier::BOLD),
        ));
    }
    if !ctx.branch.is_empty() {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.push(Span::styled(
            "branch:",
            Style::default().fg(Color::Indexed(244)),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            ctx.branch.to_string(),
            Style::default()
                .fg(Color::Indexed(252))
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

/// Render the bottom navigation key-bindings ribbon as styled Ratatui
/// [`Line`]s.
pub fn bottom_ribbon_lines(width: usize) -> (Line<'static>, Line<'static>) {
    let divider = Line::from(Span::styled(
        format!(" {}", "─".repeat(width.saturating_sub(2).min(60))),
        Style::default().fg(Color::Indexed(244)),
    ));
    let ribbon_text = truncate_pad(
        " [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]stage [x]discard [q]quit [?]help",
        width.saturating_sub(1),
    );
    let ribbon = Line::from(Span::styled(
        ribbon_text,
        Style::default().fg(Color::Indexed(244)),
    ));
    (divider, ribbon)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::counts::FileCounts;

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
            rendered.contains("38;5;220m"),
            "hunk count in status bar must have golden-amber tint 220"
        );
    }

    #[test]
    fn test_ratatui_header_and_status_lines() {
        let file = OutputFile {
            path: "src/main.rs".to_string(),
            status: "modified".to_string(),
            new_line_count: 10,
            old_line_count: 10,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![],
            raw_hunks: vec![],
            counts: FileCounts {
                adds: 5,
                dels: 2,
                mods: 1,
                movs: 0,
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
        };

        let header_line = header_bar_line("diffuse", &file);
        assert!(header_line.to_string().contains("Review: diffuse"));
        assert!(header_line.to_string().contains("src/main.rs"));
        assert!(header_line.to_string().contains("+5 −2 ~1"));

        let ctx = StatusBarContext {
            cur_fi: 0,
            total_files: 2,
            cur_hunk: 0,
            total_hunks: 1,
            raw_diff: false,
            col_offset: 4,
            num_buffer: "12",
            branch: "main",
            width: 80,
            is_streaming: true,
        };
        let status_line = status_bar_line(ctx);
        assert!(status_line.to_string().contains("1/2 files"));
        assert!(status_line.to_string().contains("streaming..."));
        assert!(status_line.to_string().contains("col 4+"));
        assert!(status_line.to_string().contains("go 12"));
        assert!(status_line.to_string().contains("branch: main"));

        let (div, rib) = bottom_ribbon_lines(80);
        assert!(div.to_string().contains("──"));
        assert!(rib.to_string().contains("[j/k]move"));
    }
}
