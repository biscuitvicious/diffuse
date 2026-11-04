use super::{
    counts::{format_counts_ansi, format_counts_spans},
    text::{truncate_pad, visible_width, wrap_text_by_visual_width},
};
use crate::{
    core::counts::FileCounts,
    render::{ColorExt, DiffuseTheme, UiPalette},
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// Render the top review header bar: repo name, branch/commit-range glyph,
/// and global counts.
pub fn render_header_bar(
    repo: &str,
    width: usize,
    global_counts: Option<FileCounts>,
    branch: Option<&str>,
    commit_range: Option<&str>,
    is_staged: bool,
    ui: &UiPalette,
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

    let head_raw = format!(
        " \x1b[1m{}{repo}\x1b[0m{branch_seg}{global_counts_seg}",
        ui.header_repo.fg_code()
    );
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
/// support: repo name, branch/commit-range glyph, and global counts.
pub fn header_bar_line(
    repo: &str,
    theme: &DiffuseTheme,
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
        spans.extend(format_counts_spans(gc, &theme.scoped_palette().ui));
    }

    let bg = crate::view::to_color(theme.palette.header_bar_bg);
    Line::from(spans).style(Style::default().bg(bg))
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
