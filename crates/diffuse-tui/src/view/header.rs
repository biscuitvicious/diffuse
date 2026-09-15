//! Header, status bar, and bottom ribbon rendering.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    core::{counts::FileCounts, wire::OutputFile},
    render::slice_by_visual_width,
};

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
        } else if c == '\t' {
            let tab_spaces = 4 - (count % 4);
            count += tab_spaces;
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

/// Truncate or right-pad an ANSI-escaped string with spaces to exactly
/// `target_width` terminal columns, guaranteeing that the rendered line never
/// exceeds or falls short of `target_width`.
pub fn clamp_or_pad_ansi(s: &str, target_width: usize) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let mut vis_w = 0usize;

    let mut chars = s.chars().peekable();
    while let Some(c) = chars.peek().copied() {
        if c == '\x1b' {
            chars.next();
            out.push('\x1b');
            if chars.peek() == Some(&'[') {
                out.push(chars.next().unwrap());
                while let Some(&ch) = chars.peek() {
                    out.push(chars.next().unwrap());
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }

        if c == '\t' {
            chars.next();
            let tab_spaces = 4 - (vis_w % 4);
            if vis_w + tab_spaces <= target_width {
                out.push_str(&" ".repeat(tab_spaces));
                vis_w += tab_spaces;
            } else {
                let rem = target_width.saturating_sub(vis_w);
                if rem > 0 {
                    out.push_str(&" ".repeat(rem));
                    vis_w += rem;
                }
                break;
            }
            continue;
        }

        let mut g = String::new();
        g.push(chars.next().unwrap());
        while let Some(&next_c) = chars.peek() {
            if next_c == '\x1b' || next_c == '\t' {
                break;
            }
            let mut test_s = g.clone();
            test_s.push(next_c);
            if test_s.graphemes(true).count() == 1 {
                g.push(chars.next().unwrap());
            } else {
                break;
            }
        }

        let gw = g.width();
        if gw > 0 && vis_w + gw > target_width {
            let rem = target_width.saturating_sub(vis_w);
            if rem > 0 {
                out.push_str(&" ".repeat(rem));
                vis_w += rem;
            }
            break;
        }
        out.push_str(&g);
        vis_w += gw;
    }

    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            out.push(c);
            out.push(chars.next().unwrap());
            while let Some(&ch) = chars.peek() {
                out.push(chars.next().unwrap());
                if ch.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }

    if vis_w < target_width {
        out.push_str(&" ".repeat(target_width - vis_w));
    }
    out.push_str("\x1b[0m");
    out
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

/// Format mod counts as a plain text string (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`). Returns an empty string when all counts are zero.
pub fn format_counts_plain(counts: &FileCounts) -> String {
    if counts.dels == 0
        && counts.adds == 0
        && counts.mods == 0
        && counts.movs == 0
        && counts.notes == 0
    {
        return String::new();
    }
    let mut s = format!("−{}", counts.dels);
    if counts.adds > 0 {
        s.push_str(&format!(" +{}", counts.adds));
    }
    if counts.mods > 0 {
        s.push_str(&format!(" ~{}", counts.mods));
    }
    if counts.movs > 0 {
        s.push_str(&format!(" ≈{}", counts.movs));
    }
    if counts.notes > 0 {
        s.push_str(&format!(" 💬 {}", counts.notes));
    }
    s
}

/// Format mod counts as an ANSI-highlighted string (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`). Returns an empty string when all counts are zero.
pub fn format_counts_ansi(counts: &FileCounts) -> String {
    if counts.is_empty() {
        return String::new();
    }
    let mut s = format!("\x1b[1m\x1b[38;5;203m−{}\x1b[0m", counts.dels);
    if counts.adds > 0 {
        s.push_str(&format!(" \x1b[1m\x1b[38;5;120m+{}\x1b[0m", counts.adds));
    }
    if counts.mods > 0 {
        s.push_str(&format!(" \x1b[1m\x1b[38;5;214m~{}\x1b[0m", counts.mods));
    }
    if counts.movs > 0 {
        s.push_str(&format!(" \x1b[1m\x1b[38;5;147m≈{}\x1b[0m", counts.movs));
    }
    if counts.notes > 0 {
        s.push_str(&format!(" \x1b[1m\x1b[38;5;220m💬 {}\x1b[0m", counts.notes));
    }
    s
}

/// Format mod counts as styled Ratatui [`Span`]s (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`). Returns an empty vec when all counts are zero.
pub fn format_counts_spans(counts: &FileCounts) -> Vec<Span<'static>> {
    if counts.is_empty() {
        return Vec::new();
    }
    let mut spans = vec![Span::styled(
        format!("−{}", counts.dels),
        Style::default()
            .fg(Color::Indexed(203))
            .add_modifier(Modifier::BOLD),
    )];
    if counts.adds > 0 {
        spans.push(Span::styled(
            format!(" +{}", counts.adds),
            Style::default()
                .fg(Color::Indexed(120))
                .add_modifier(Modifier::BOLD),
        ));
    }
    if counts.mods > 0 {
        spans.push(Span::styled(
            format!(" ~{}", counts.mods),
            Style::default()
                .fg(Color::Indexed(214))
                .add_modifier(Modifier::BOLD),
        ));
    }
    if counts.movs > 0 {
        spans.push(Span::styled(
            format!(" ≈{}", counts.movs),
            Style::default()
                .fg(Color::Indexed(147))
                .add_modifier(Modifier::BOLD),
        ));
    }
    if counts.notes > 0 {
        spans.push(Span::styled(
            format!(" 💬 {}", counts.notes),
            Style::default()
                .fg(Color::Indexed(220))
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

/// Render the top review header bar with file path, status, and line counts,
/// wrapping across lines if wider than `width`.
pub fn render_header_bar(
    repo: &str,
    file: Option<&OutputFile>,
    width: usize,
    common_prefix: Option<&str>,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
) -> Vec<String> {
    let branch_seg = if let Some(range) = commit_range
        && !range.is_empty()
    {
        format!(" \x1b[38;5;244m\u{2387}\x1b[0m \x1b[1m\x1b[38;5;252m{range}\x1b[0m")
    } else if let Some(b) = branch.filter(|b| !b.is_empty()) {
        format!(" \x1b[38;5;244m\u{2387}\x1b[0m \x1b[1m\x1b[38;5;252m{b}\x1b[0m")
    } else {
        String::new()
    };
    let global_counts_seg = match global_counts {
        Some(ref gc) if !gc.is_empty() => {
            format!(" \x1b[38;5;240m·\x1b[0m {}", format_counts_ansi(gc))
        }
        _ => String::new(),
    };

    let prefix_seg = if let Some(pfx) = common_prefix
        && !pfx.is_empty()
    {
        format!(" \x1b[38;5;240m·\x1b[0m \x1b[1m\x1b[38;5;152m{pfx}\x1b[0m")
    } else {
        String::new()
    };

    let head_raw = if let Some(file) = file {
        let counts_ansi = format_counts_ansi(&file.counts);
        let counts_seg = if counts_ansi.is_empty() {
            String::new()
        } else {
            format!(" \x1b[38;5;240m·\x1b[0m {counts_ansi}")
        };

        let stripped_old = file.old_path.as_deref().map(|old| {
            if let Some(pfx) = common_prefix {
                old.strip_prefix(pfx).unwrap_or(old).to_string()
            } else {
                old.to_string()
            }
        });
        let stripped_path = if let Some(pfx) = common_prefix {
            file.path
                .strip_prefix(pfx)
                .unwrap_or(&file.path)
                .to_string()
        } else {
            file.path.clone()
        };

        let styled_path = if let Some(ref old) = stripped_old
            && old != &stripped_path
        {
            crate::core::wire::format_rename_path_styled(old, &stripped_path, "\x1b[38;5;231m")
        } else {
            stripped_path
        };

        let status_seg = if file.status == "renamed" || file.status == "modified" {
            String::new()
        } else {
            format!(" ({})", file.status)
        };
        format!(
            " \x1b[1m\x1b[38;5;117m{repo}\x1b[0m{branch_seg}{global_counts_seg}{prefix_seg} \x1b[38;5;240m·\x1b[0m {styled_path}{status_seg}{counts_seg}"
        )
    } else {
        format!(" \x1b[1m\x1b[38;5;117m{repo}\x1b[0m{branch_seg}{global_counts_seg}{prefix_seg}")
    };
    let wrapped = wrap_text_by_visual_width(&head_raw, width);
    wrapped
        .into_iter()
        .map(|piece| {
            let piece_vw = visible_width(&piece);
            let pad = " ".repeat(width.saturating_sub(piece_vw));
            format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;231m{piece}{pad}\x1b[0m")
        })
        .collect()
}

/// Parameters for rendering the secondary status bar.
#[derive(Debug, Clone, Copy)]
pub struct StatusBarContext<'a> {
    pub cur_fi: usize,
    pub total_files: usize,
    pub cur_hunk: usize,
    pub total_hunks: usize,
    pub raw_diff: bool,
    pub col_offset: usize,
    pub num_buffer: &'a str,
    pub common_prefix: Option<&'a str>,
    pub width: usize,
    pub is_streaming: bool,
    pub streaming_dot: &'a str,
}

/// Render the secondary status bar with file index, hunk index, and mode
/// indicators, wrapping across lines if wider than `width`.
pub fn render_status_bar(ctx: StatusBarContext<'_>) -> Vec<String> {
    let files_badge = if ctx.total_files == 0 {
        "\x1b[1m\x1b[38;5;220mfile 0/0\x1b[0m".to_string()
    } else {
        format!(
            "\x1b[1m\x1b[38;5;220mfile {}/{}\x1b[0m",
            ctx.cur_fi + 1,
            ctx.total_files
        )
    };
    let hunk_badge = if ctx.total_files == 0 || ctx.total_hunks == 0 {
        "\x1b[1m\x1b[38;5;213mhunk 0/0\x1b[0m".to_string()
    } else {
        format!(
            "\x1b[1m\x1b[38;5;213mhunk {}/{}\x1b[0m",
            ctx.cur_hunk + 1,
            ctx.total_hunks
        )
    };
    let stream_txt = if ctx.is_streaming {
        let dot = if ctx.streaming_dot.is_empty() {
            "●"
        } else {
            ctx.streaming_dot
        };
        format!(" \x1b[38;5;240m\u{00b7}\x1b[0m \x1b[1m\x1b[38;5;117m{dot} streaming...\x1b[0m")
    } else {
        String::new()
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
    let line = format!(
        " {files_badge}{stream_txt} \x1b[38;5;240m·\x1b[0m {hunk_badge}{mode_txt}{col_txt}{go_txt}"
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
                let piece_vw = visible_width(&piece);
                let pad = " ".repeat(ctx.width.saturating_sub(piece_vw));
                format!("{piece}{pad}\x1b[0m")
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
        " [?]help [q]uit [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]tage [x]discard",
        width.saturating_sub(1),
    );
    let ribbon = format!("\x1b[38;5;244m{ribbon_text}\x1b[39m");
    (divider, ribbon)
}

/// Render the review header bar as a styled Ratatui [`Line`] with theme
/// support.
pub fn header_bar_line_themed(
    repo: &str,
    file: Option<&OutputFile>,
    theme: &crate::render::DiffuseTheme,
    common_prefix: Option<&str>,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
) -> Line<'static> {
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            repo.to_string(),
            Style::default()
                .fg(Color::Indexed(117))
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(range) = commit_range
        && !range.is_empty()
    {
        spans.push(Span::styled(
            " \u{2387} ",
            Style::default().fg(Color::Indexed(244)),
        ));
        spans.push(Span::styled(
            range.to_string(),
            Style::default()
                .fg(Color::Indexed(252))
                .add_modifier(Modifier::BOLD),
        ));
    } else if let Some(b) = branch.filter(|b| !b.is_empty()) {
        spans.push(Span::styled(
            " \u{2387} ",
            Style::default().fg(Color::Indexed(244)),
        ));
        spans.push(Span::styled(
            b.to_string(),
            Style::default()
                .fg(Color::Indexed(252))
                .add_modifier(Modifier::BOLD),
        ));
    }

    if let Some(ref gc) = global_counts
        && !gc.is_empty()
    {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.extend(format_counts_spans(gc));
    }

    if let Some(pfx) = common_prefix
        && !pfx.is_empty()
    {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.push(Span::styled(
            pfx.to_string(),
            Style::default()
                .fg(Color::Indexed(152))
                .add_modifier(Modifier::BOLD),
        ));
    }

    let bg = crate::view::to_color(theme.palette.header_bar_bg);
    let Some(file) = file else {
        return Line::from(spans).style(Style::default().bg(bg));
    };

    spans.push(Span::styled(
        " · ",
        Style::default().fg(Color::Indexed(240)),
    ));

    let stripped_old = file.old_path.as_deref().map(|old| {
        if let Some(pfx) = common_prefix {
            old.strip_prefix(pfx).unwrap_or(old).to_string()
        } else {
            old.to_string()
        }
    });
    let stripped_path = if let Some(pfx) = common_prefix {
        file.path
            .strip_prefix(pfx)
            .unwrap_or(&file.path)
            .to_string()
    } else {
        file.path.clone()
    };

    if let Some(ref old) = stripped_old
        && old != &stripped_path
    {
        let old_parts: Vec<&str> = old.split('/').collect();
        let new_parts: Vec<&str> = stripped_path.split('/').collect();
        let mut common_len = 0;
        while common_len + 1 < old_parts.len()
            && common_len + 1 < new_parts.len()
            && old_parts[common_len] == new_parts[common_len]
        {
            common_len += 1;
        }
        if common_len > 0 {
            let pfx = format!("{}/", old_parts[..common_len].join("/"));
            let old_rem = old_parts[common_len..].join("/");
            let new_rem = new_parts[common_len..].join("/");
            spans.push(Span::styled(
                pfx,
                Style::default()
                    .fg(Color::Indexed(139))
                    .add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!("{old_rem} -> {new_rem}"),
                Style::default()
                    .fg(Color::Indexed(231))
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!("{old} -> {stripped_path}"),
                Style::default()
                    .fg(Color::Indexed(231))
                    .add_modifier(Modifier::BOLD),
            ));
        }
    } else {
        spans.push(Span::styled(
            stripped_path,
            Style::default()
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD),
        ));
    }

    if file.status != "renamed" && file.status != "modified" {
        spans.push(Span::styled(
            format!(" ({})", file.status),
            Style::default().fg(Color::Indexed(244)),
        ));
    }

    let count_spans = format_counts_spans(&file.counts);
    if !count_spans.is_empty() {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.extend(count_spans);
    }

    Line::from(spans).style(Style::default().bg(bg))
}

/// Render the review header bar as a styled Ratatui [`Line`].
pub fn header_bar_line(
    repo: &str,
    file: Option<&OutputFile>,
    common_prefix: Option<&str>,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
) -> Line<'static> {
    header_bar_line_themed(
        repo,
        file,
        &crate::render::DiffuseTheme::from_env(),
        common_prefix,
        global_counts,
        branch,
        commit_range,
    )
}

/// Render the secondary status bar as a styled Ratatui [`Line`].
pub fn status_bar_line(ctx: StatusBarContext<'_>) -> Line<'static> {
    let file_text = if ctx.total_files == 0 {
        "file 0/0".to_string()
    } else {
        format!("file {}/{}", ctx.cur_fi + 1, ctx.total_files)
    };
    let hunk_text = if ctx.total_files == 0 || ctx.total_hunks == 0 {
        "hunk 0/0".to_string()
    } else {
        format!("hunk {}/{}", ctx.cur_hunk + 1, ctx.total_hunks)
    };
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            file_text,
            Style::default()
                .fg(Color::Indexed(220))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if ctx.is_streaming {
        let dot = if ctx.streaming_dot.is_empty() {
            "●"
        } else {
            ctx.streaming_dot
        };
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.push(Span::styled(
            format!("{dot} streaming..."),
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
        hunk_text,
        Style::default()
            .fg(Color::Indexed(213))
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
    let used_w: usize = spans
        .iter()
        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    if used_w < ctx.width {
        spans.push(Span::raw(" ".repeat(ctx.width - used_w)));
    }
    Line::from(spans)
}

/// Render the bottom navigation key-bindings ribbon as styled Ratatui
/// [`Line`]s.
pub fn bottom_ribbon_lines(width: usize) -> (Line<'static>, Line<'static>) {
    let div_str = truncate_pad(
        &format!(" {}", "─".repeat(width.saturating_sub(2).min(60))),
        width,
    );
    let divider = Line::from(Span::styled(
        div_str,
        Style::default().fg(Color::Indexed(244)),
    ));
    let ribbon_text = truncate_pad(
        " [?]help [q]uit [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]tage [x]discard",
        width,
    );
    let ribbon = Line::from(Span::styled(
        ribbon_text,
        Style::default().fg(Color::Indexed(244)),
    ));
    (divider, ribbon)
}

#[cfg(test)]
mod tests;
