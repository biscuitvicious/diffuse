//! Header, status bar, and bottom ribbon rendering.

use crate::{
    core::{
        counts::{CountKind, FileCounts},
        wire::OutputFile,
    },
    render::{
        slice_by_visual_width,
        width::{char_width, display_width, grapheme_width},
    },
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use smallvec::SmallVec;
use unicode_segmentation::UnicodeSegmentation;

/// Calculate the visible column width of a string by ignoring ANSI escape
/// sequences and measuring grapheme display widths (wide chars count two).
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
            count += char_width(c);
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

        let gw = grapheme_width(&g);
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
        let gw = grapheme_width(g);
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

/// Highlight palette for mod-count segments by bucket.
fn count_color(kind: CountKind) -> u8 {
    match kind {
        CountKind::Dels => 203,
        CountKind::Adds => 120,
        CountKind::Mods => 214,
        CountKind::Movs => 147,
        CountKind::Notes => 220,
    }
}

/// Format mod counts as a plain text string (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`), omitting zero counts. Returns an empty string when
/// all counts are zero.
pub fn format_counts_plain(counts: &FileCounts) -> String {
    counts
        .segments()
        .iter()
        .map(|seg| format!("{}{}", seg.marker, seg.value))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Format mod counts as an ANSI-highlighted string (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`), omitting zero counts. Returns an empty string when
/// all counts are zero.
pub fn format_counts_ansi(counts: &FileCounts) -> String {
    counts
        .segments()
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let sep = if i == 0 { "" } else { " " };
            format!(
                "{sep}\x1b[1m\x1b[38;5;{}m{}{}\x1b[0m",
                count_color(seg.kind),
                seg.marker,
                seg.value
            )
        })
        .collect::<String>()
}

/// Format mod counts as styled Ratatui [`Span`]s (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`), omitting zero counts. At most five spans are ever
/// produced, stored inline without heap allocation. Returns an empty vec
/// when all counts are zero.
pub fn format_counts_spans(counts: &FileCounts) -> SmallVec<[Span<'static>; 5]> {
    let mut spans: SmallVec<[Span<'static>; 5]> = SmallVec::new();
    for (i, seg) in counts.segments().iter().enumerate() {
        let prefix = if i == 0 { "" } else { " " };
        spans.push(Span::styled(
            format!("{prefix}{}{}", seg.marker, seg.value),
            Style::default()
                .fg(Color::Indexed(count_color(seg.kind)))
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

/// Render the top review header bar: repo name, branch/commit-range glyph,
/// global counts, and common directory prefix. Per-file path and counts live
/// in the secondary status bar.
pub fn render_header_bar(
    repo: &str,
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
    // No dot before the global counts: the branch glyph (or repo name)
    // already separates them.
    let global_counts_seg = match global_counts {
        Some(ref gc) if !gc.is_empty() => {
            format!(" {}", format_counts_ansi(gc))
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

    let head_raw =
        format!(" \x1b[1m\x1b[38;5;117m{repo}\x1b[0m{branch_seg}{global_counts_seg}{prefix_seg}");
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
    /// Current file whose sub-path and counts lead the status bar.
    pub file: Option<&'a OutputFile>,
    /// Change counts of the current hunk, shown after the hunk badge.
    pub hunk_counts: Option<FileCounts>,
}

/// Current file path stripped of the shared directory prefix, rendering
/// renames as `old -> new`.
fn stripped_file_path(file: &OutputFile, common_prefix: Option<&str>) -> String {
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
    if let Some(old) = stripped_old
        && old != stripped_path
    {
        format!("{old} -> {stripped_path}")
    } else {
        stripped_path
    }
}

/// File status tag; routine `modified`/`renamed` states stay unadorned.
fn file_status_tag(file: &OutputFile) -> String {
    if file.status == "renamed" || file.status == "modified" {
        String::new()
    } else {
        format!(" ({})", file.status)
    }
}

/// Render the secondary status bar: file badge, current file sub-path and
/// counts, hunk badge and hunk counts, plus mode indicators.
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
    let file_seg = if let Some(f) = ctx.file {
        let path = stripped_file_path(f, ctx.common_prefix);
        let status = file_status_tag(f);
        let counts_txt = if f.counts.is_empty() {
            String::new()
        } else {
            format!(" ({})", format_counts_ansi(&f.counts))
        };
        format!(
            " \x1b[38;5;240m·\x1b[0m \x1b[1m\x1b[38;5;231m{path}\x1b[0m\x1b[38;5;244m{status}\x1b[0m{counts_txt}"
        )
    } else {
        String::new()
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
    let hunk_counts_txt = match ctx.hunk_counts {
        Some(c) if !c.is_empty() => format!(" ({})", format_counts_ansi(&c)),
        _ => String::new(),
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
        " {files_badge}{file_seg}{stream_txt} \x1b[38;5;240m·\x1b[0m {hunk_badge}{hunk_counts_txt}{mode_txt}{col_txt}{go_txt}"
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
/// support: repo name, branch/commit-range glyph, global counts, and common
/// directory prefix.
pub fn header_bar_line_themed(
    repo: &str,
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
        // No dot separator: the branch glyph (or repo name) already
        // separates the global counts.
        spans.push(Span::raw(" "));
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
    Line::from(spans).style(Style::default().bg(bg))
}

/// Render the review header bar as a styled Ratatui [`Line`].
pub fn header_bar_line(
    repo: &str,
    common_prefix: Option<&str>,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
) -> Line<'static> {
    header_bar_line_themed(
        repo,
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
    let mut spans = vec![Span::raw(" ")];
    spans.push(Span::styled(
        file_text,
        Style::default()
            .fg(Color::Indexed(220))
            .add_modifier(Modifier::BOLD),
    ));
    if let Some(f) = ctx.file {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(Color::Indexed(240)),
        ));
        spans.push(Span::styled(
            stripped_file_path(f, ctx.common_prefix),
            Style::default()
                .fg(Color::Indexed(231))
                .add_modifier(Modifier::BOLD),
        ));
        let status = file_status_tag(f);
        if !status.is_empty() {
            spans.push(Span::styled(
                status,
                Style::default().fg(Color::Indexed(244)),
            ));
        }
        if !f.counts.is_empty() {
            spans.push(Span::raw(" ("));
            spans.extend(format_counts_spans(&f.counts));
            spans.push(Span::raw(")"));
        }
    }
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
    if let Some(c) = ctx.hunk_counts
        && !c.is_empty()
    {
        spans.push(Span::raw(" ("));
        spans.extend(format_counts_spans(&c));
        spans.push(Span::raw(")"));
    }
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
    let used_w: usize = spans.iter().map(|s| display_width(&s.content)).sum();
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
