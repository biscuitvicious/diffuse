//! Header, status bar, and bottom ribbon rendering.

use crate::{
    core::{counts::FileCounts, wire::OutputFile},
    render::{
        ColorExt, slice_by_visual_width,
        width::{display_width, grapheme_width},
    },
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use smallvec::SmallVec;
use unicode_segmentation::UnicodeSegmentation;

/// Calculate the visible column width of a string by ignoring ANSI escape
/// sequences and measuring grapheme display widths (wide chars count two).
pub fn visible_width(s: &str) -> usize {
    let mut in_escape = false;
    let mut count = 0;
    let mut text_start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if c == '\x1b' {
            if let Some(start) = text_start.take() {
                count += display_width(&s[start..i]);
            }
            in_escape = true;
        } else if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\t' {
            if let Some(start) = text_start.take() {
                count += display_width(&s[start..i]);
            }
            let tab_spaces = 4 - (count % 4);
            count += tab_spaces;
        } else if text_start.is_none() {
            text_start = Some(i);
        }
    }
    if let Some(start) = text_start {
        count += display_width(&s[start..]);
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
/// Shorten a status-bar path to fit `budget` columns, keeping the end and
/// marking the cut with a leading `…` (grapheme-safe for wide glyphs).
/// Returns `None` when the path fits or the budget cannot hold even the
/// ellipsis.
#[must_use]
pub fn truncate_status_path(path: &str, budget: usize) -> Option<String> {
    let path_w = display_width(path);
    if path_w <= budget || budget == 0 {
        return None;
    }
    let keep = budget.saturating_sub(1);
    let tail = slice_by_visual_width(path, path_w.saturating_sub(keep), keep);
    Some(format!("…{tail}"))
}

pub use crate::render::counts::{CountCell, count_cells, count_color};

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
pub fn format_counts_ansi(counts: &FileCounts, palette: &crate::render::ThemePalette) -> String {
    count_cells(counts, palette)
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let sep = if i == 0 { "" } else { " " };
            format!(
                "{sep}\x1b[1m{}{}{}\x1b[0m",
                cell.color.fg_code(),
                cell.marker,
                cell.value
            )
        })
        .collect::<String>()
}

/// Format mod counts as styled Ratatui [`Span`]s (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬 notes`), omitting zero counts. At most five spans are ever
/// produced, stored inline without heap allocation. Returns an empty vec
/// when all counts are zero.
pub fn format_counts_spans(
    counts: &FileCounts,
    palette: &crate::render::ThemePalette,
) -> SmallVec<[Span<'static>; 5]> {
    let mut spans: SmallVec<[Span<'static>; 5]> = SmallVec::new();
    for (i, cell) in count_cells(counts, palette).iter().enumerate() {
        let prefix = if i == 0 { "" } else { " " };
        spans.push(Span::styled(
            format!("{prefix}{}{}", cell.marker, cell.value),
            Style::default()
                .fg(crate::view::to_color(cell.color))
                .add_modifier(Modifier::BOLD),
        ));
    }
    spans
}

/// Render the top review header bar: repo name, branch/commit-range glyph,
/// global counts, and common directory prefix. Per-file path and counts live
/// in the secondary status bar.
#[expect(clippy::too_many_arguments)]
pub fn render_header_bar(
    repo: &str,
    width: usize,
    common_prefix: Option<&str>,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
    is_staged: bool,
    palette: &crate::render::ThemePalette,
) -> Vec<String> {
    let branch_seg = if let Some(range) = commit_range
        && !range.is_empty()
    {
        format!(
            " {}\u{2387}\x1b[0m\x1b[1m{}{range}\x1b[0m",
            palette.header_dim.fg_code(),
            palette.header_branch.fg_code()
        )
    } else if let Some(b) = branch.filter(|b| !b.is_empty()) {
        let staged_badge = if is_staged {
            format!(
                " {}(\x1b[1m{}staged\x1b[0m{})",
                palette.header_dim.fg_code(),
                palette.header_staged.fg_code(),
                palette.header_dim.fg_code()
            )
        } else {
            String::new()
        };
        format!(
            " {}\u{2387}\x1b[0m\x1b[1m{}{b}\x1b[0m{staged_badge}",
            palette.header_dim.fg_code(),
            palette.header_branch.fg_code()
        )
    } else if is_staged {
        format!(
            " {}\u{2387}(\x1b[1m{}staged\x1b[0m{})",
            palette.header_dim.fg_code(),
            palette.header_staged.fg_code(),
            palette.header_dim.fg_code()
        )
    } else {
        String::new()
    };
    // No dot before the global counts: the branch glyph (or repo name)
    // already separates them.
    let global_counts_seg = match global_counts {
        Some(ref gc) if !gc.is_empty() => {
            format!(" {}", format_counts_ansi(gc, palette))
        }
        _ => String::new(),
    };

    let prefix_seg = if let Some(pfx) = common_prefix
        && !pfx.is_empty()
    {
        format!(
            " {}·\x1b[0m \x1b[1m{}{pfx}\x1b[0m",
            palette.header_dim.fg_code(),
            palette.header_prefix.fg_code()
        )
    } else {
        String::new()
    };

    let head_raw = format!(
        " \x1b[1m{}{repo}\x1b[0m{branch_seg}{global_counts_seg}{prefix_seg}",
        palette.header_repo.fg_code()
    );
    let wrapped = wrap_text_by_visual_width(&head_raw, width);
    wrapped
        .into_iter()
        .map(|piece| {
            let piece_vw = visible_width(&piece);
            let pad = " ".repeat(width.saturating_sub(piece_vw));
            format!(
                "{}\x1b[1m{}{piece}{pad}\x1b[0m",
                palette.header_bar_bg.bg_code(),
                palette.header_bar_fg.fg_code()
            )
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
    /// Active theme for status bar styling.
    pub theme: &'a crate::render::DiffuseTheme,
}

/// Current file path stripped of the shared directory prefix, rendering
/// renames as `old -> new`.
pub(crate) fn stripped_file_path(file: &OutputFile, common_prefix: Option<&str>) -> String {
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
pub(crate) fn file_status_tag(file: &OutputFile) -> String {
    if file.status == "renamed" || file.status == "modified" {
        String::new()
    } else {
        format!(" ({})", file.status)
    }
}

/// Render the secondary status bar as ANSI strings: the shared
/// [`status_layout`](super::status_layout) cascade (full badges →
/// abbreviated badges → path truncation → wrap) emitted as escape runs.
pub fn render_status_bar(ctx: StatusBarContext<'_>) -> Vec<String> {
    use super::status_layout::{StatusSegment, status_layout};

    let bg_code = ctx.theme.palette.status_bar_bg.bg_code();

    let seg_ansi = |seg: &StatusSegment| -> String {
        let bold = if seg.bold { "\x1b[1m" } else { "" };
        let bold_off = if seg.bold { "\x1b[22m" } else { "" };
        format!(
            "{}{bold}{}{bold_off}\x1b[39m",
            seg.color.fg_code(),
            seg.text
        )
    };

    let layout = status_layout(&ctx);
    let line: String = layout.segments.iter().map(seg_ansi).collect();
    if !layout.wraps {
        let vw = visible_width(&line);
        return vec![format!(
            "{bg_code}{line}{}\x1b[0m",
            " ".repeat(ctx.width.saturating_sub(vw))
        )];
    }
    wrap_text_by_visual_width(&line, ctx.width)
        .into_iter()
        .map(|piece| {
            let piece_vw = visible_width(&piece);
            let pad = " ".repeat(ctx.width.saturating_sub(piece_vw));
            format!("{bg_code}{piece}{pad}\x1b[0m")
        })
        .collect()
}

/// Render the bottom navigation key-bindings ribbon.
pub fn render_bottom_ribbon(
    width: usize,
    palette: &crate::render::ThemePalette,
) -> (String, String) {
    let div_str = format!(" {}", "─".repeat(width.saturating_sub(2).min(60)));
    let divider = format!("{}{div_str}\x1b[39m", palette.ribbon_divider.fg_code());
    let ribbon_text = truncate_pad(
        " [?]help [q]uit [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]tage [t]staged [T]theme [x]discard",
        width.saturating_sub(1),
    );

    let ribbon = format!("{}{ribbon_text}\x1b[39m", palette.ribbon_text.fg_code());
    (divider, ribbon)
}

/// Render the review header bar as a styled Ratatui [`Line`] with theme
/// support: repo name, branch/commit-range glyph, global counts, and common
/// directory prefix.
pub fn header_bar_line(
    repo: &str,
    theme: &crate::render::DiffuseTheme,
    common_prefix: Option<&str>,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
    is_staged: bool,
) -> Line<'static> {
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            repo.to_string(),
            Style::default()
                .fg(crate::view::to_color(theme.palette.header_repo))
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(range) = commit_range
        && !range.is_empty()
    {
        spans.push(Span::styled(
            " \u{2387}",
            Style::default().fg(crate::view::to_color(theme.palette.header_dim)),
        ));
        spans.push(Span::styled(
            range.to_string(),
            Style::default()
                .fg(crate::view::to_color(theme.palette.header_branch))
                .add_modifier(Modifier::BOLD),
        ));
    } else if let Some(b) = branch.filter(|b| !b.is_empty()) {
        spans.push(Span::styled(
            " \u{2387}",
            Style::default().fg(crate::view::to_color(theme.palette.header_dim)),
        ));
        spans.push(Span::styled(
            b.to_string(),
            Style::default()
                .fg(crate::view::to_color(theme.palette.header_branch))
                .add_modifier(Modifier::BOLD),
        ));
        if is_staged {
            spans.push(Span::styled(
                " (",
                Style::default().fg(crate::view::to_color(theme.palette.header_dim)),
            ));
            spans.push(Span::styled(
                "staged",
                Style::default()
                    .fg(crate::view::to_color(theme.palette.header_staged))
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                ")",
                Style::default().fg(crate::view::to_color(theme.palette.header_dim)),
            ));
        }
    } else if is_staged {
        spans.push(Span::styled(
            " \u{2387}",
            Style::default().fg(crate::view::to_color(theme.palette.header_dim)),
        ));
        spans.push(Span::styled(
            "(",
            Style::default().fg(crate::view::to_color(theme.palette.header_dim)),
        ));
        spans.push(Span::styled(
            "staged",
            Style::default()
                .fg(crate::view::to_color(theme.palette.header_staged))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            ")",
            Style::default().fg(crate::view::to_color(theme.palette.header_dim)),
        ));
    }

    if let Some(ref gc) = global_counts
        && !gc.is_empty()
    {
        // No dot separator: the branch glyph (or repo name) already
        // separates the global counts.
        spans.push(Span::raw(" "));
        spans.extend(format_counts_spans(gc, &theme.palette));
    }

    if let Some(pfx) = common_prefix
        && !pfx.is_empty()
    {
        spans.push(Span::styled(
            " · ",
            Style::default().fg(crate::view::to_color(theme.palette.header_dim)),
        ));
        spans.push(Span::styled(
            pfx.to_string(),
            Style::default()
                .fg(crate::view::to_color(theme.palette.header_prefix))
                .add_modifier(Modifier::BOLD),
        ));
    }

    let bg = crate::view::to_color(theme.palette.header_bar_bg);
    Line::from(spans).style(Style::default().bg(bg))
}

/// Render the secondary status bar as a styled Ratatui [`Line`], consuming
/// the same shared [`status_layout`](super::status_layout) cascade as the
/// ANSI path.
pub fn status_bar_line(ctx: StatusBarContext<'_>) -> Line<'static> {
    use super::status_layout::{StatusSegment, status_layout};

    let seg_span = |seg: &StatusSegment| -> Span<'static> {
        let mut style = Style::default().fg(crate::view::to_color(seg.color));
        if seg.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        Span::styled(seg.text.to_string(), style)
    };

    let layout = status_layout(&ctx);
    let spans: Vec<Span<'static>> = layout.segments.iter().map(seg_span).collect();
    let bg = crate::view::to_color(ctx.theme.palette.status_bar_bg);
    padded_line(spans, ctx.width).style(Style::default().bg(bg))
}

/// Pad spans with trailing spaces to `width` and wrap into a [`Line`].
fn padded_line(mut spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let used_w: usize = spans.iter().map(|s| display_width(&s.content)).sum();
    if used_w < width {
        spans.push(Span::raw(" ".repeat(width - used_w)));
    }
    Line::from(spans)
}

/// Render the bottom navigation key-bindings ribbon as styled Ratatui
/// [`Line`]s.
pub fn bottom_ribbon_lines(
    width: usize,
    palette: &crate::render::ThemePalette,
) -> (Line<'static>, Line<'static>) {
    let bg = crate::view::to_color(palette.bg_base);
    let div_str = truncate_pad(
        &format!(" {}", "─".repeat(width.saturating_sub(2).min(60))),
        width,
    );
    let divider = Line::from(Span::styled(
        div_str,
        Style::default().fg(crate::view::to_color(palette.ribbon_divider)),
    ))
    .style(Style::default().bg(bg));
    let ribbon_text = truncate_pad(
        " [?]help [q]uit [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]tage [t]staged [T]theme [x]discard",
        width,
    );

    let ribbon = Line::from(Span::styled(
        ribbon_text,
        Style::default().fg(crate::view::to_color(palette.ribbon_text)),
    ))
    .style(Style::default().bg(bg));
    (divider, ribbon)
}

#[cfg(test)]
mod tests;
