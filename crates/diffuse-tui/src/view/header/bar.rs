use super::{
    counts::{format_counts_ansi, format_counts_spans},
    text::{truncate_pad, visible_width, wrap_text_by_visual_width},
};
use crate::{
    core::counts::FileCounts,
    render::{ColorExt, DiffuseTheme, UiPalette, width::display_width},
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// Render the top review header bar: repo name, branch/commit-range glyph, the
/// number of reviewed files, and global counts.
#[allow(clippy::too_many_arguments)]
pub fn render_header_bar(
    repo: &str,
    width: usize,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
    is_staged: bool,
    ui: &UiPalette,
    total_files: Option<usize>,
) -> Vec<String> {
    let branch_seg = if let Some(range) = commit_range
        && !range.is_empty()
    {
        format!(
            " {}\u{2387}\x1b[0m\x1b[1m{}{range}\x1b[0m",
            ui.header_dim.fg_code(),
            ui.header_branch.fg_code()
        )
    } else if let Some(b) = branch.filter(|b| !b.is_empty()) {
        let staged_badge = if is_staged {
            format!(
                " {}(\x1b[1m{}staged\x1b[0m{})",
                ui.header_dim.fg_code(),
                ui.header_staged.fg_code(),
                ui.header_dim.fg_code()
            )
        } else {
            String::new()
        };
        format!(
            " {}\u{2387}\x1b[0m\x1b[1m{}{b}\x1b[0m{staged_badge}",
            ui.header_dim.fg_code(),
            ui.header_branch.fg_code()
        )
    } else if is_staged {
        format!(
            " {}\u{2387}(\x1b[1m{}staged\x1b[0m{})",
            ui.header_dim.fg_code(),
            ui.header_staged.fg_code(),
            ui.header_dim.fg_code()
        )
    } else {
        String::new()
    };
    // No dot before the global counts: the branch glyph (or repo name)
    // already separates them.
    let global_counts_seg = match global_counts {
        Some(ref gc) if !gc.is_empty() => {
            format!(" {}", format_counts_ansi(gc, ui))
        }
        _ => String::new(),
    };

    // The file count is omitted while the diff is streaming, where the number
    // of files seen so far is not the number of files under review.
    let files_seg = match total_files {
        Some(n) => format!(
            " {}{n} {}\x1b[0m",
            ui.header_dim.fg_code(),
            if n == 1 { "file" } else { "files" }
        ),
        None => String::new(),
    };

    let mut head_raw = format!(
        " \x1b[1m{}{repo}\x1b[0m{branch_seg}{files_seg}{global_counts_seg}",
        ui.header_repo.fg_code()
    );
    // The change counts say more than the file count, so the file count is what
    // gives way first when the bar cannot hold the whole header.
    if total_files.is_some() && visible_width(&head_raw) > width {
        head_raw = format!(
            " \x1b[1m{}{repo}\x1b[0m{branch_seg}{global_counts_seg}",
            ui.header_repo.fg_code()
        );
    }
    let wrapped = wrap_text_by_visual_width(&head_raw, width);
    wrapped
        .into_iter()
        .map(|piece| {
            let piece_vw = visible_width(&piece);
            let pad = " ".repeat(width.saturating_sub(piece_vw));
            format!(
                "{}\x1b[1m{}{piece}{pad}\x1b[0m",
                ui.header_bar_bg.bg_code(),
                ui.header_bar_fg.fg_code()
            )
        })
        .collect()
}

/// Render the bottom navigation key-bindings ribbon.
pub fn render_bottom_ribbon(width: usize, ui: &UiPalette) -> (String, String) {
    let div_str = format!(" {}", "─".repeat(width.saturating_sub(2).min(60)));
    let divider = format!("{}{div_str}\x1b[39m", ui.ribbon_divider.fg_code());
    let ribbon_text = truncate_pad(
        " [?]help [q]uit [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]tage [t]staged [T]theme [x]discard",
        width.saturating_sub(1),
    );

    let ribbon = format!("{}{ribbon_text}\x1b[39m", ui.ribbon_text.fg_code());
    (divider, ribbon)
}

/// Render the review header bar as a styled Ratatui [`Line`] with theme
/// support: repo name, branch/commit-range glyph, the number of reviewed
/// files, and global counts.
#[allow(clippy::too_many_arguments)]
pub fn header_bar_line(
    repo: &str,
    theme: &DiffuseTheme,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
    is_staged: bool,
    total_files: Option<usize>,
    width: usize,
) -> Line<'static> {
    let mut spans = header_bar_spans(
        repo,
        theme,
        global_counts,
        branch,
        commit_range,
        is_staged,
        total_files,
    );
    // The change counts say more than the file count, so the file count is what
    // gives way first when the bar cannot hold the whole header.
    if total_files.is_some() && spans_width(&spans) > width {
        spans = header_bar_spans(
            repo,
            theme,
            global_counts,
            branch,
            commit_range,
            is_staged,
            None,
        );
    }
    let bg = crate::view::to_color(theme.palette.ui.header_bar_bg);
    Line::from(spans).style(Style::default().bg(bg))
}

/// Visible width of a span sequence in terminal cells.
fn spans_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| display_width(&span.content)).sum()
}

/// Build the header bar spans: repo name, branch/commit-range glyph, the
/// reviewed file count, and global change counts.
fn header_bar_spans(
    repo: &str,
    theme: &DiffuseTheme,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
    is_staged: bool,
    total_files: Option<usize>,
) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            repo.to_string(),
            Style::default()
                .fg(crate::view::to_color(theme.palette.ui.header_repo))
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(range) = commit_range
        && !range.is_empty()
    {
        spans.push(Span::styled(
            " \u{2387}",
            Style::default().fg(crate::view::to_color(theme.palette.ui.header_dim)),
        ));
        spans.push(Span::styled(
            range.to_string(),
            Style::default()
                .fg(crate::view::to_color(theme.palette.ui.header_branch))
                .add_modifier(Modifier::BOLD),
        ));
    } else if let Some(b) = branch.filter(|b| !b.is_empty()) {
        spans.push(Span::styled(
            " \u{2387}",
            Style::default().fg(crate::view::to_color(theme.palette.ui.header_dim)),
        ));
        spans.push(Span::styled(
            b.to_string(),
            Style::default()
                .fg(crate::view::to_color(theme.palette.ui.header_branch))
                .add_modifier(Modifier::BOLD),
        ));
        if is_staged {
            spans.push(Span::styled(
                " (",
                Style::default().fg(crate::view::to_color(theme.palette.ui.header_dim)),
            ));
            spans.push(Span::styled(
                "staged",
                Style::default()
                    .fg(crate::view::to_color(theme.palette.ui.header_staged))
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                ")",
                Style::default().fg(crate::view::to_color(theme.palette.ui.header_dim)),
            ));
        }
    } else if is_staged {
        spans.push(Span::styled(
            " \u{2387}",
            Style::default().fg(crate::view::to_color(theme.palette.ui.header_dim)),
        ));
        spans.push(Span::styled(
            "(",
            Style::default().fg(crate::view::to_color(theme.palette.ui.header_dim)),
        ));
        spans.push(Span::styled(
            "staged",
            Style::default()
                .fg(crate::view::to_color(theme.palette.ui.header_staged))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            ")",
            Style::default().fg(crate::view::to_color(theme.palette.ui.header_dim)),
        ));
    }

    // The count is omitted while the diff is streaming, where the files seen so
    // far are not the files under review.
    if let Some(n) = total_files {
        spans.push(Span::styled(
            format!(" {n} {}", if n == 1 { "file" } else { "files" }),
            Style::default().fg(crate::view::to_color(theme.palette.ui.header_dim)),
        ));
    }

    if let Some(ref gc) = global_counts
        && !gc.is_empty()
    {
        // No dot separator: the branch glyph (or repo name) already
        // separates the global counts.
        spans.push(Span::raw(" "));
        spans.extend(format_counts_spans(gc, &theme.palette.ui));
    }

    spans
}

/// Render the bottom navigation key-bindings ribbon as styled Ratatui
/// [`Line`]s.
pub fn bottom_ribbon_lines(width: usize, ui: &UiPalette) -> (Line<'static>, Line<'static>) {
    let bg = crate::view::to_color(ui.bg_base);
    let div_str = truncate_pad(
        &format!(" {}", "─".repeat(width.saturating_sub(2).min(60))),
        width,
    );
    let divider = Line::from(Span::styled(
        div_str,
        Style::default().fg(crate::view::to_color(ui.ribbon_divider)),
    ))
    .style(Style::default().bg(bg));
    let ribbon_text = truncate_pad(
        " [?]help [q]uit [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]tage [t]staged [T]theme [x]discard",
        width,
    );

    let ribbon = Line::from(Span::styled(
        ribbon_text,
        Style::default().fg(crate::view::to_color(ui.ribbon_text)),
    ))
    .style(Style::default().bg(bg));
    (divider, ribbon)
}
