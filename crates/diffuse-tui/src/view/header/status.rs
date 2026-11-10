use crate::{
    core::{counts::FileCounts, wire::OutputFile},
    render::width::display_width,
};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// Parameters for rendering the secondary status bar.
#[derive(Debug, Clone, Copy)]
pub struct StatusBarContext<'a> {
    pub cur_fi: usize,
    pub total_files: usize,
    pub cur_hunk: usize,
    pub total_hunks: usize,
    pub raw_diff: bool,
    pub side_by_side: bool,
    pub expanded_mods_count: usize,
    pub total_mods: usize,
    pub col_offset: usize,
    pub num_buffer: &'a str,
    pub common_prefix: Option<&'a str>,
    pub width: usize,
    pub is_streaming: bool,
    pub streaming_dot: &'a str,
    /// Ephemeral action feedback shown first when present.
    pub status_message: Option<(&'a str, bool)>,
    /// Current file (optional; file path and stats render in sticky file
    /// banners).
    pub file: Option<&'a OutputFile>,
    /// Change counts of the current hunk, shown after the hunk badge.
    pub hunk_counts: Option<FileCounts>,
    /// Active theme for status bar styling.
    pub theme: crate::render::DiffuseTheme,
}

/// Render the secondary status bar as a styled Ratatui [`Line`], consuming
/// the same shared [`status_layout`](crate::view::status_layout) segments as
/// the ANSI path.
pub fn status_bar_line(ctx: StatusBarContext<'_>) -> Line<'static> {
    use crate::view::status_layout::{StatusSegment, status_layout};

    let seg_span = |seg: &StatusSegment| -> Span<'static> {
        let mut style = Style::default().fg(crate::view::to_color(seg.color));
        if seg.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        Span::styled(seg.text.to_string(), style)
    };

    let layout = status_layout(&ctx);
    if layout.segments.is_empty() {
        return Line::default();
    }
    let spans: Vec<Span<'static>> = layout.segments.iter().map(seg_span).collect();
    let bg = crate::view::to_color(ctx.theme.palette.ui.status_bar_bg);
    padded_line(spans, ctx.width).style(Style::default().bg(bg))
}

/// Pad spans with trailing spaces to `width` and wrap into a [`Line`].
pub fn padded_line(mut spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let used_w: usize = spans.iter().map(|s| display_width(&s.content)).sum();
    if used_w < width {
        spans.push(Span::raw(" ".repeat(width - used_w)));
    }
    Line::from(spans)
}
