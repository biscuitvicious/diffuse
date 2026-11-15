pub use diffuse_render::hunk_divider_text;
use ratatui::text::Line;

use crate::{
    core::wire::OutputHunk,
    render::{Badge, DiffuseTheme, slice_by_visual_width, span::StyledSpan, width::display_width},
    view::span::line_from_spans,
};

/// Visual segment count for a divider text: ceiling division, except a
/// trailing segment holding only bar dashes (no salient content) never
/// wraps alone — as long as the labels fit, the bar is dropped.
#[must_use]
pub fn divider_seg_count(txt: &str, content_width: usize) -> usize {
    let text_cols = content_width.max(1);
    let len = display_width(txt);
    if len == 0 {
        return 1;
    }
    let mut count = len.div_ceil(text_cols);
    while count > 1 {
        let piece = slice_by_visual_width(txt, (count - 1) * text_cols, text_cols);
        if piece.chars().all(|c| c == '─' || c.is_whitespace()) {
            count -= 1;
        } else {
            break;
        }
    }
    count.max(1)
}

/// Build the raw text of a bottom gap divider with format
/// `──── {hidden_gap} more {unit} ────`.
pub fn bottom_gap_divider_text(hidden_gap: usize) -> String {
    let unit = if hidden_gap == 1 { "line" } else { "lines" };
    format!("──── {hidden_gap} more {unit} ────")
}

/// Style one divider span: bold foreground, inheriting the canvas when
/// the asset leaves the background primary.
fn divider_span(text: String, divider: &crate::render::SurfaceColors) -> StyledSpan {
    let mut span = StyledSpan::styled(text, Some(divider.fg), None).with_bold(true);
    if divider.bg != crate::render::Color::Primary {
        span.bg = Some(divider.bg);
    }
    span
}

/// Render a specific visual segment of a hunk header divider as a styled
/// Ratatui [`Line`]: divider text sliced at `seg_start`, padded with the
/// ruler to `content_width`. The direct grid path: no ANSI round-trip.
#[allow(clippy::too_many_arguments)]
pub fn hunk_divider_segment_line(
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    is_selected: bool,
    seg_start: usize,
    content_width: usize,
    theme: &DiffuseTheme,
) -> Line<'static> {
    let txt = hunk_divider_text(badge, hunk, hidden_gap);
    let divider = theme.palette[crate::render::Cursor::from_banner_state(is_selected, is_current)]
        .hunk_divider;
    let total_w = display_width(&txt);
    if seg_start >= total_w {
        return line_from_spans(vec![divider_span("─".repeat(content_width), &divider)]);
    }
    let mut piece = slice_by_visual_width(&txt, seg_start, content_width);
    let piece_w = display_width(&piece);
    if piece_w < content_width {
        piece.push_str(&"─".repeat(content_width - piece_w));
    }
    line_from_spans(vec![divider_span(piece, &divider)])
}

/// Render a hunk header divider as a styled Ratatui [`Line`] with theme
/// support.
pub fn hunk_divider_line(
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
    theme: &DiffuseTheme,
) -> Line<'static> {
    hunk_divider_segment_line(
        badge,
        hunk,
        hidden_gap,
        is_current,
        false,
        0,
        content_width,
        theme,
    )
}

/// Render a specific visual segment of a bottom gap divider as a styled
/// Ratatui [`Line`]: gap text sliced at `seg_start`, padded with the ruler
/// to `content_width`. The direct grid path: no ANSI round-trip.
pub fn bottom_gap_divider_segment_line(
    hidden_gap: usize,
    is_current: bool,
    is_selected: bool,
    seg_start: usize,
    content_width: usize,
    theme: &DiffuseTheme,
) -> Line<'static> {
    let txt = bottom_gap_divider_text(hidden_gap);
    let divider = theme.palette[crate::render::Cursor::from_banner_state(is_selected, is_current)]
        .hunk_divider;
    let total_w = display_width(&txt);
    if seg_start >= total_w {
        return line_from_spans(vec![divider_span("─".repeat(content_width), &divider)]);
    }
    let mut piece = slice_by_visual_width(&txt, seg_start, content_width);
    let piece_w = display_width(&piece);
    if piece_w < content_width {
        piece.push_str(&"─".repeat(content_width - piece_w));
    }
    line_from_spans(vec![divider_span(piece, &divider)])
}

/// Render a bottom gap divider as a styled Ratatui [`Line`] with theme support.
pub fn bottom_gap_divider_line(
    hidden_gap: usize,
    is_current: bool,
    is_selected: bool,
    content_width: usize,
    theme: &DiffuseTheme,
) -> Line<'static> {
    bottom_gap_divider_segment_line(hidden_gap, is_current, is_selected, 0, content_width, theme)
}
