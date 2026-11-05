pub use diffuse_render::hunk_divider_text;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::{
    core::wire::OutputHunk,
    render::{Badge, ColorExt, DiffuseTheme, slice_by_visual_width, width::display_width},
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

/// Render a specific visual segment of a hunk header divider with theme
/// support.
#[allow(clippy::too_many_arguments)]
pub fn render_hunk_divider_segment(
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    is_selected: bool,
    seg_start: usize,
    content_width: usize,
    theme: &DiffuseTheme,
) -> String {
    let txt = hunk_divider_text(badge, hunk, hidden_gap);

    let divider = theme.palette[crate::render::Cursor::from_banner_state(is_selected, is_current)]
        .hunk_divider;
    let (bg, fg) = (divider.bg, divider.fg);
    let bg_code = match bg {
        crate::render::Color::Primary => String::new(),
        color => color.bg_code(),
    };
    let fg_code = fg.fg_code();

    let total_txt_w = display_width(&txt);
    if seg_start >= total_txt_w {
        let bar_str = "─".repeat(content_width);
        return format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m");
    }

    let piece = slice_by_visual_width(&txt, seg_start, content_width);
    let piece_w = display_width(&piece);
    let bar_str = if piece_w < content_width {
        let ruler_len = content_width - piece_w;
        format!("{piece}{}", "─".repeat(ruler_len))
    } else {
        piece
    };
    format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m")
}

/// Render a hunk header divider with format `x-y → a-b`, hidden lines gap,
/// and context expansion buttons (`⬍ · ▲ · ▼` for interior hunks, or `▼` for H1
/// at top of file).
pub fn render_hunk_divider(
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
    theme: &DiffuseTheme,
) -> String {
    render_hunk_divider_segment(
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

/// Screen-space hit area for a divider arrow glyph: exactly the three
/// cells holding `[' ', glyph, ' ']` after wrapping — the glyph's unwrapped
/// strip neighbors mapped through the segment wrap. Content columns only;
/// the scrollbar column can never match, so the bar keeps its column.
#[must_use]
pub fn wrapped_glyph_hit(txt: &str, pat: &str, seg_off: usize, col: usize, cw: usize) -> bool {
    let Some(idx) = txt.find(pat) else {
        return false;
    };
    let c = txt[..idx].chars().count();
    let cw = cw.max(1);
    (c.saturating_sub(1)..=c + 1).any(|e| e / cw == seg_off && e % cw == col)
}

/// Build the raw text of a bottom gap divider with format
/// `──── {hidden_gap} more {unit}: ▲ ────`.
pub fn bottom_gap_divider_text(hidden_gap: usize) -> String {
    let unit = if hidden_gap == 1 { "line" } else { "lines" };
    format!("──── {hidden_gap} more {unit}: ▲ ────")
}

/// Render a specific visual segment of a bottom gap divider with theme support.
pub fn render_bottom_gap_divider_segment(
    hidden_gap: usize,
    is_current: bool,
    is_selected: bool,
    seg_start: usize,
    content_width: usize,
    theme: &DiffuseTheme,
) -> String {
    let txt = bottom_gap_divider_text(hidden_gap);

    let divider = theme.palette[crate::render::Cursor::from_banner_state(is_selected, is_current)]
        .hunk_divider;
    let (bg, fg) = (divider.bg, divider.fg);
    let bg_code = match bg {
        crate::render::Color::Primary => String::new(),
        color => color.bg_code(),
    };
    let fg_code = fg.fg_code();

    let total_txt_w = display_width(&txt);
    if seg_start >= total_txt_w {
        let bar_str = "─".repeat(content_width);
        return format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m");
    }

    let piece = slice_by_visual_width(&txt, seg_start, content_width);
    let piece_w = display_width(&piece);
    let bar_str = if piece_w < content_width {
        let ruler_len = content_width - piece_w;
        format!("{piece}{}", "─".repeat(ruler_len))
    } else {
        piece
    };
    format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m")
}

/// Render a clean bottom divider for trailing hidden lines after the last hunk.
pub fn render_bottom_gap_divider(
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
    theme: &DiffuseTheme,
) -> String {
    render_bottom_gap_divider_segment(hidden_gap, is_current, false, 0, content_width, theme)
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
    let txt = hunk_divider_text(badge, hunk, hidden_gap);
    let txt_w = display_width(&txt);
    let bar_str = if txt_w < content_width {
        let ruler_len = content_width - txt_w;
        format!("{txt}{}", "─".repeat(ruler_len))
    } else {
        slice_by_visual_width(&txt, 0, content_width)
    };

    let divider =
        theme.palette[crate::render::Cursor::from_banner_state(false, is_current)].hunk_divider;
    let (bg, fg) = (divider.bg, divider.fg);
    let mut style = Style::default()
        .fg(crate::view::to_color(fg))
        .add_modifier(Modifier::BOLD);
    if bg != crate::render::Color::Primary {
        style = style.bg(crate::view::to_color(bg));
    }
    Line::from(Span::styled(bar_str, style))
}

/// Render a bottom gap divider as a styled Ratatui [`Line`] with theme support.
pub fn bottom_gap_divider_line(
    hidden_gap: usize,
    is_current: bool,
    is_selected: bool,
    content_width: usize,
    theme: &DiffuseTheme,
) -> Line<'static> {
    let txt = bottom_gap_divider_text(hidden_gap);
    let txt_w = display_width(&txt);
    let bar_str = if txt_w < content_width {
        let ruler_len = content_width - txt_w;
        format!("{txt}{}", "─".repeat(ruler_len))
    } else {
        slice_by_visual_width(&txt, 0, content_width)
    };

    let divider = theme.palette[crate::render::Cursor::from_banner_state(is_selected, is_current)]
        .hunk_divider;
    let (bg, fg) = (divider.bg, divider.fg);
    let mut style = Style::default()
        .fg(crate::view::to_color(fg))
        .add_modifier(Modifier::BOLD);
    if bg != crate::render::Color::Primary {
        style = style.bg(crate::view::to_color(bg));
    }
    Line::from(Span::styled(bar_str, style))
}
