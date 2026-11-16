use super::{counts::format_counts_spans, text::truncate_pad};
use crate::{
    core::counts::FileCounts,
    render::{DiffuseTheme, UiPalette, width::display_width},
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// File count badge in the primary header bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderFileCount {
    /// Exact count of reviewed files (e.g. `4 files` or `1 file`).
    Exact(usize),
    /// Count is unknown while streaming the diff (`? files`).
    Streaming,
    /// Count badge is omitted (e.g. empty diff or dropped on narrow screens).
    None,
}

impl HeaderFileCount {
    /// Whether any file count badge is presented.
    #[must_use]
    pub fn is_some(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Format the badge text (e.g. `" 4 files"`, `" 1 file"`, `" ? files"`), or
    /// `None` if omitted.
    #[must_use]
    pub fn badge_text(&self) -> Option<String> {
        match *self {
            Self::Exact(n) => Some(format!(" {n} {}", if n == 1 { "file" } else { "files" })),
            Self::Streaming => Some(" ? files".to_string()),
            Self::None => None,
        }
    }
}

impl From<usize> for HeaderFileCount {
    fn from(n: usize) -> Self {
        Self::Exact(n)
    }
}

impl From<Option<usize>> for HeaderFileCount {
    fn from(opt: Option<usize>) -> Self {
        match opt {
            Some(n) => Self::Exact(n),
            None => Self::None,
        }
    }
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
    total_files: impl Into<HeaderFileCount>,
    width: usize,
) -> Line<'static> {
    let file_count = total_files.into();
    let mut spans = header_bar_spans(
        repo,
        theme,
        global_counts,
        branch,
        commit_range,
        is_staged,
        file_count,
    );
    // The change counts say more than the file count, so the file count is what
    // gives way first when the bar cannot hold the whole header.
    if file_count.is_some() && spans_width(&spans) > width {
        spans = header_bar_spans(
            repo,
            theme,
            global_counts,
            branch,
            commit_range,
            is_staged,
            HeaderFileCount::None,
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
    file_count: HeaderFileCount,
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

    if let Some(text) = file_count.badge_text() {
        spans.push(Span::styled(
            text,
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

/// Render the bottom navigation key-bindings ribbon as a styled Ratatui
/// [`Line`].
pub fn bottom_ribbon_line(width: usize, ui: &UiPalette) -> Line<'static> {
    let bg = crate::view::to_color(ui.bg_base);
    let ribbon_text = truncate_pad(
        " [?]help [q]uit [j/k]move [n/p]file [J/K]hunk [Enter]toggle [v]select [m]note [o]edit [s]tage [t]staged [T]theme [x]discard",
        width,
    );

    Line::from(Span::styled(
        ribbon_text,
        Style::default().fg(crate::view::to_color(ui.ribbon_text)),
    ))
    .style(Style::default().bg(bg))
}
