//! Diff grid row, hunk divider, and file header line formatting.

use super::header::truncate_pad;
use crate::{
    core::wire::{OutputFile, OutputHunk},
    render::{ColorExt, DiffuseTheme, slice_by_visual_width, width::display_width},
};

/// Shorten a path string to fit within `target_width` columns, preserving the
/// filename or rename target `old -> new` by abbreviating leading directory
/// components with `…/`.
pub fn shorten_path_for_width(path: &str, target_width: usize) -> String {
    if display_width(path) <= target_width {
        return path.to_string();
    }

    if let Some((old_part, new_part)) = path.split_once(" -> ") {
        let old_parts: Vec<&str> = old_part.split('/').collect();
        let new_parts: Vec<&str> = new_part.split('/').collect();
        let max_parts = old_parts.len().max(new_parts.len());
        for i in 1..max_parts {
            let s_old = if i < old_parts.len() {
                format!("…/{}", old_parts[i..].join("/"))
            } else {
                old_parts.last().unwrap_or(&old_part).to_string()
            };
            let s_new = if i < new_parts.len() {
                format!("…/{}", new_parts[i..].join("/"))
            } else {
                new_parts.last().unwrap_or(&new_part).to_string()
            };
            let candidate = format!("{s_old} -> {s_new}");
            if display_width(&candidate) <= target_width {
                return candidate;
            }
        }
        return format!(
            "…/{} -> …/{}",
            old_parts.last().unwrap_or(&old_part),
            new_parts.last().unwrap_or(&new_part)
        );
    }

    // General path shortening: shorten leading directory segments with "…/"
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 1 {
        for i in 1..parts.len() {
            let candidate = format!("…/{}", parts[i..].join("/"));
            if display_width(&candidate) <= target_width {
                return candidate;
            }
        }
    }

    path.to_string()
}

fn rename_prefix_visual_span(
    file: &OutputFile,
    common_prefix: Option<&str>,
) -> Option<(usize, usize)> {
    let old_path = file.old_path.as_deref()?;
    if old_path == file.path {
        return None;
    }

    let (old, new) = match common_prefix {
        Some(prefix) => {
            let stripped_old = old_path
                .strip_prefix(prefix)
                .map(|s| s.to_string())
                .unwrap_or_else(|| old_path.to_string());
            let stripped_new = file
                .path
                .strip_prefix(prefix)
                .map(|s| s.to_string())
                .unwrap_or_else(|| file.path.clone());
            (stripped_old, stripped_new)
        }
        None => (old_path.to_string(), file.path.clone()),
    };

    let old_parts: Vec<&str> = old.split('/').collect();
    let new_parts: Vec<&str> = new.split('/').collect();

    let mut common_len = 0;
    while common_len + 1 < old_parts.len()
        && common_len + 1 < new_parts.len()
        && old_parts[common_len] == new_parts[common_len]
    {
        common_len += 1;
    }

    if common_len > 0 {
        let prefix = old_parts[..common_len].join("/");
        // Prefix starts after "──── ▶ " (7 cols) or "──── " (5 cols for binary)
        // and includes trailing '/'
        let p_start = if file.is_binary { 5 } else { 7 };
        let p_len = display_width(prefix) + 1;
        Some((p_start, p_start + p_len))
    } else {
        None
    }
}

/// Char column of the collapse/expand triangle in [`file_header_text`].
pub const FILE_HEADER_INDICATOR_COL: usize = 5;

/// Build the raw text of a file header banner row with optional common prefix
/// stripping.
pub fn file_header_text_with_prefix(
    file: &OutputFile,
    is_collapsed: bool,
    common_prefix: Option<&str>,
) -> String {
    let f_counts = &file.counts;
    let (indicator_prefix, collapse_tag) = if file.is_binary {
        ("════ ".to_string(), "")
    } else {
        let indicator = if is_collapsed { "▶" } else { "▼" };
        let tag = if is_collapsed { " [collapsed]" } else { "" };
        (format!("════ {indicator} "), tag)
    };
    let path_display = file.display_path_with_prefix(common_prefix);
    let status_str = file.display_status();
    let status_tag = if status_str.is_empty() || status_str == "modified" {
        String::new()
    } else {
        format!(" ({status_str})")
    };
    format!(
        "{indicator_prefix}{path_display}{status_tag} -{} +{} ~{}{} ",
        f_counts.dels, f_counts.adds, f_counts.mods, collapse_tag
    )
}

/// Build the raw text of a file header banner row.
pub fn file_header_text(file: &OutputFile, is_collapsed: bool) -> String {
    file_header_text_with_prefix(file, is_collapsed, None)
}

/// Render a specific visual segment of a file header banner row with theme
/// support.
#[allow(clippy::too_many_arguments)]
pub fn render_file_header_segment_themed(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    seg_start: usize,
    content_width: usize,
    theme: &DiffuseTheme,
    common_prefix: Option<&str>,
) -> String {
    let f_counts = &file.counts;
    let (fixed_prefix, collapse_tag) = if file.is_binary {
        ("════ ".to_string(), "")
    } else {
        let indicator = if is_collapsed { "▶" } else { "▼" };
        let tag = if is_collapsed { " [collapsed]" } else { "" };
        (format!("════ {indicator} "), tag)
    };
    let path_display = file.display_path_with_prefix(common_prefix);
    let status_str = file.display_status();
    let status_tag = if status_str.is_empty() || status_str == "modified" {
        String::new()
    } else {
        format!(" ({status_str})")
    };

    let fixed_suffix = format!(
        "{status_tag} -{} +{} ~{}{} ",
        f_counts.dels, f_counts.adds, f_counts.mods, collapse_tag
    );

    let prefix_text = format!("{fixed_prefix}{path_display}{fixed_suffix}");
    let (bg, fg) = theme
        .palette
        .file_header_colors(is_selected, is_current, in_band);
    let bg_code = bg.bg_code();
    let fg_code = fg.fg_code();

    let total_prefix_w = display_width(&prefix_text);
    if seg_start >= total_prefix_w {
        let bar_str = "═".repeat(content_width);
        return format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m");
    }

    let piece = slice_by_visual_width(&prefix_text, seg_start, content_width);
    let piece_w = display_width(&piece);

    let styled_piece =
        if let Some((p_start, p_end)) = rename_prefix_visual_span(file, common_prefix) {
            let seg_end = seg_start + piece_w;
            let overlap_s = p_start.max(seg_start);
            let overlap_e = p_end.min(seg_end);
            if overlap_s < overlap_e {
                let rel_s = overlap_s - seg_start;
                let rel_e = overlap_e - seg_start;
                let before = slice_by_visual_width(&piece, 0, rel_s);
                let underlined = slice_by_visual_width(&piece, rel_s, rel_e - rel_s);
                let after = slice_by_visual_width(&piece, rel_e, piece_w.saturating_sub(rel_e));
                format!("{before}\x1b[4m\x1b[38;5;139m{underlined}\x1b[24m{fg_code}{after}")
            } else {
                piece
            }
        } else {
            piece
        };

    let bar_str = if piece_w < content_width {
        let ruler_len = content_width - piece_w;
        format!("{styled_piece}{}", "═".repeat(ruler_len))
    } else {
        styled_piece
    };

    format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m")
}

/// Render a specific visual segment of a file header banner row.
pub fn render_file_header_segment(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    render_file_header_segment_themed(
        file,
        is_collapsed,
        is_selected,
        is_current,
        in_band,
        seg_start,
        content_width,
        &DiffuseTheme::from_env(),
        None,
    )
}

/// Render a file header banner row with collapse status indicator and counts.
pub fn render_file_header(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
) -> String {
    render_file_header_segment(
        file,
        is_collapsed,
        is_selected,
        is_current,
        in_band,
        0,
        content_width,
    )
}

/// Build the raw text of a hunk header divider with format
/// `{rows} lines (-dels +adds ~mods)` plus the hidden-gap tail.
/// The caller is responsible for filling any remaining width with `─` chars.
pub fn hunk_divider_text(hi: usize, hunk: &OutputHunk, hidden_gap: usize) -> String {
    let counts = hunk.counts();
    let (dels, adds, mods, movs) = (counts.dels, counts.adds, counts.mods, counts.movs);
    let total = dels + adds + mods + movs;
    let unit = if total == 1 { "line" } else { "lines" };
    let mov_str = if movs > 0 {
        format!(" ≈{movs}")
    } else {
        String::new()
    };
    let summary = format!("{total} {unit} (-{dels} +{adds} ~{mods}{mov_str})");

    let gap_part = if hidden_gap > 0 {
        let gap_unit = if hidden_gap == 1 { "line" } else { "lines" };
        let arrows = if hi == 0 { "▼" } else { "⬍ · ▲ · ▼" };
        format!(" · {hidden_gap} more {gap_unit}: {arrows}")
    } else {
        String::new()
    };
    format!("──── H{} · {summary}{gap_part} ────", hi + 1)
}

/// Render a specific visual segment of a hunk header divider with theme
/// support.
pub fn render_hunk_divider_segment_themed(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    seg_start: usize,
    content_width: usize,
    theme: &DiffuseTheme,
) -> String {
    let txt = hunk_divider_text(hi, hunk, hidden_gap);
    let (bg, fg) = theme.palette.hunk_divider_colors(is_current);
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

/// Render a specific visual segment of a hunk header divider.
pub fn render_hunk_divider_segment(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    render_hunk_divider_segment_themed(
        hi,
        hunk,
        hidden_gap,
        is_current,
        seg_start,
        content_width,
        &DiffuseTheme::from_env(),
    )
}

/// Render a hunk header divider with format `x-y → a-b`, hidden lines gap,
/// and context expansion buttons (`⬍ · ▲ · ▼` for interior hunks, or `▼` for H1
/// at top of file).
pub fn render_hunk_divider(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> String {
    render_hunk_divider_segment(hi, hunk, hidden_gap, is_current, 0, content_width)
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

/// Render a specific visual segment of a bottom gap divider.
pub fn render_bottom_gap_divider_segment(
    hidden_gap: usize,
    is_current: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    let txt = bottom_gap_divider_text(hidden_gap);
    let theme = DiffuseTheme::from_env();
    let (bg, fg) = theme.palette.hunk_divider_colors(is_current);
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
) -> String {
    render_bottom_gap_divider_segment(hidden_gap, is_current, 0, content_width)
}

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// Render a file header banner row as a styled Ratatui [`Line`] with theme
/// support.
pub fn file_header_line_themed(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
    theme: &DiffuseTheme,
) -> Line<'static> {
    let bar = file_header_text(file, is_collapsed);
    let bar_str = truncate_pad(&bar, content_width);
    let (bg, fg) = theme
        .palette
        .file_header_colors(is_selected, is_current, in_band);
    let style = Style::default()
        .bg(crate::view::to_color(bg))
        .fg(crate::view::to_color(fg))
        .add_modifier(Modifier::BOLD);
    Line::from(Span::styled(bar_str, style))
}

/// Render a file header banner row as a styled Ratatui [`Line`].
pub fn file_header_line(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
) -> Line<'static> {
    file_header_line_themed(
        file,
        is_collapsed,
        is_selected,
        is_current,
        in_band,
        content_width,
        &DiffuseTheme::from_env(),
    )
}

/// Render a hunk header divider as a styled Ratatui [`Line`] with theme
/// support.
pub fn hunk_divider_line_themed(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
    theme: &DiffuseTheme,
) -> Line<'static> {
    let txt = hunk_divider_text(hi, hunk, hidden_gap);
    let txt_w = display_width(&txt);
    let bar_str = if txt_w < content_width {
        let ruler_len = content_width - txt_w;
        format!("{txt}{}", "─".repeat(ruler_len))
    } else {
        slice_by_visual_width(&txt, 0, content_width)
    };
    let (bg, fg) = theme.palette.hunk_divider_colors(is_current);
    let mut style = Style::default()
        .fg(crate::view::to_color(fg))
        .add_modifier(Modifier::BOLD);
    if bg != crate::render::Color::Primary {
        style = style.bg(crate::view::to_color(bg));
    }
    Line::from(Span::styled(bar_str, style))
}

/// Render a hunk header divider as a styled Ratatui [`Line`].
pub fn hunk_divider_line(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> Line<'static> {
    hunk_divider_line_themed(
        hi,
        hunk,
        hidden_gap,
        is_current,
        content_width,
        &DiffuseTheme::from_env(),
    )
}

/// Render a bottom gap divider as a styled Ratatui [`Line`] with theme support.
pub fn bottom_gap_divider_line_themed(
    hidden_gap: usize,
    is_current: bool,
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
    let (bg, fg) = theme.palette.hunk_divider_colors(is_current);
    let mut style = Style::default()
        .fg(crate::view::to_color(fg))
        .add_modifier(Modifier::BOLD);
    if bg != crate::render::Color::Primary {
        style = style.bg(crate::view::to_color(bg));
    }
    Line::from(Span::styled(bar_str, style))
}

/// Render a bottom gap divider as a styled Ratatui [`Line`].
pub fn bottom_gap_divider_line(
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> Line<'static> {
    bottom_gap_divider_line_themed(
        hidden_gap,
        is_current,
        content_width,
        &DiffuseTheme::from_env(),
    )
}

#[cfg(test)]
mod tests;
