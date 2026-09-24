//! Diff grid row, hunk divider, and file header line formatting.

use super::header::truncate_pad;
use crate::{
    core::wire::{OutputFile, OutputHunk},
    render::{Badge, ColorExt, DiffuseTheme, slice_by_visual_width, width::display_width},
};

fn rename_prefix_visual_span(
    file: &OutputFile,
    badge: Badge,
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
        // Prefix starts after the indicator, badge and dot (`════ ▼ F3/17 · `
        // or `════ F3/17 · ` for binaries) and includes trailing '/'
        let badge_shift = display_width(format!("{} · ", badge.file_text()));
        let p_start = if file.is_binary {
            5 + badge_shift
        } else {
            7 + badge_shift
        };
        let p_len = display_width(prefix) + 1;
        Some((p_start, p_start + p_len))
    } else {
        None
    }
}

/// Char column of the collapse/expand triangle in [`file_header_text`].
pub const FILE_HEADER_INDICATOR_COL: usize = 5;

/// Indicator prefix (with file position badge) and collapse tag of a file
/// header banner row: `════ ▼ F3/17 · ` or `════ F3/17 · ` for binaries.
fn file_header_indicator(file: &OutputFile, badge: Badge, is_collapsed: bool) -> (String, String) {
    let badge = format!("{} · ", badge.file_text());
    if file.is_binary {
        (format!("════ {badge}"), String::new())
    } else {
        let indicator = if is_collapsed { "▶" } else { "▼" };
        let tag = if is_collapsed { " [collapsed]" } else { "" };
        (format!("════ {indicator} {badge}"), tag.to_string())
    }
}

/// Assembled plain parts of a file header banner row, in render order.
/// Both the raw text builder and the styled segment renderer consume these,
/// so slicing columns and style ranges cannot drift apart.
struct FileHeaderParts {
    indicator: String,
    path: String,
    counts: String,
    status: String,
    collapse: String,
}

fn file_header_parts(
    file: &OutputFile,
    badge: Badge,
    is_collapsed: bool,
    common_prefix: Option<&str>,
) -> FileHeaderParts {
    use super::header::format_counts_plain;

    let (indicator_prefix, collapse_tag) = file_header_indicator(file, badge, is_collapsed);
    let path_display = file.display_path_with_prefix(common_prefix);
    let status_str = file.display_status();
    let status_tag = if status_str.is_empty() || status_str == "modified" {
        String::new()
    } else {
        format!(" ({status_str})")
    };
    FileHeaderParts {
        indicator: indicator_prefix,
        path: path_display,
        counts: format_counts_plain(&file.counts),
        status: status_tag,
        collapse: collapse_tag,
    }
}

/// Build the raw text of a file header banner row with optional common prefix
/// stripping. Change counts sit between the filename and the status parens,
/// rendered with the shared [`format_counts_plain`] (zero counts omitted).
pub fn file_header_text_with_prefix(
    file: &OutputFile,
    badge: Badge,
    is_collapsed: bool,
    common_prefix: Option<&str>,
) -> String {
    let parts = file_header_parts(file, badge, is_collapsed, common_prefix);
    if parts.counts.is_empty() {
        format!(
            "{}{}{}{} ",
            parts.indicator, parts.path, parts.status, parts.collapse
        )
    } else {
        format!(
            "{}{} {}{}{} ",
            parts.indicator, parts.path, parts.counts, parts.status, parts.collapse
        )
    }
}

/// Build the raw text of a file header banner row.
pub fn file_header_text(file: &OutputFile, badge: Badge, is_collapsed: bool) -> String {
    file_header_text_with_prefix(file, badge, is_collapsed, None)
}

/// A styled column range over plain banner text: emitted with `open`
/// before the overlapped slice and `close` after it.
struct BannerStyleRun {
    start: usize,
    end: usize,
    open: String,
    close: String,
}

/// Render a visual slice of plain banner text, applying the styled column
/// ranges overlapping the window. Returns the styled piece and its visible
/// width (measured before styling, so ANSI never pollutes layout math).
fn render_banner_piece(
    plain: &str,
    runs: &[BannerStyleRun],
    seg_start: usize,
    width: usize,
) -> (String, usize) {
    use unicode_segmentation::UnicodeSegmentation;

    use crate::render::width::grapheme_width;

    let mut out = String::new();
    let mut visible = 0usize;
    let mut col = 0usize;
    let mut open: Option<usize> = None;
    let mut open_emitted = false;
    for g in plain.graphemes(true) {
        let gs = col;
        col += grapheme_width(g);
        // Latest-starting match wins, so nested ranges (rename underline
        // inside the white path) resolve innermost.
        let active = runs.iter().rposition(|r| gs >= r.start && gs < r.end);
        if active != open {
            if open_emitted {
                if let Some(idx) = open {
                    out.push_str(&runs[idx].close);
                }
                open_emitted = false;
            }
            open = active;
        }
        if gs < seg_start || gs >= seg_start + width {
            continue;
        }
        if let Some(idx) = open
            && !open_emitted
        {
            out.push_str(&runs[idx].open);
            open_emitted = true;
        }
        out.push_str(g);
        visible += grapheme_width(g);
    }
    if open_emitted && let Some(idx) = open {
        out.push_str(&runs[idx].close);
    }
    (out, visible)
}

/// Render a specific visual segment of a file header banner row with theme
/// support.
#[allow(clippy::too_many_arguments)]
pub fn render_file_header_segment_themed(
    file: &OutputFile,
    badge: Badge,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    seg_start: usize,
    content_width: usize,
    theme: &DiffuseTheme,
    common_prefix: Option<&str>,
) -> String {
    use super::header::count_cells;

    let prefix_text = file_header_text_with_prefix(file, badge, is_collapsed, common_prefix);
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

    // Styled ranges share the status bar's count cells (same segments, same
    // colors); the filename and status parens render in the per-state path
    // white (selected brightest, rest most muted).
    // Columns walk the shared parts in order, so text and ranges cannot
    // drift apart.
    let parts = file_header_parts(file, badge, is_collapsed, common_prefix);
    let path_color = if is_selected {
        theme.palette.file_header_path_selected
    } else if in_band || is_current {
        theme.palette.file_header_path_current
    } else {
        theme.palette.file_header_path_rest
    };
    let white_open = format!("\x1b[22m{}", path_color.fg_code());
    let white_close = format!("\x1b[39m\x1b[1m{fg_code}");
    let mut runs: Vec<BannerStyleRun> = Vec::new();
    let mut col = display_width(&parts.indicator);
    let path_w = display_width(&parts.path);
    runs.push(BannerStyleRun {
        start: col,
        end: col + path_w,
        open: white_open.clone(),
        close: white_close.clone(),
    });
    col += path_w;
    if !parts.counts.is_empty() {
        col += 1;
        for cell in count_cells(&file.counts) {
            let text = format!("{}{}", cell.marker, cell.value);
            let w = display_width(&text);
            runs.push(BannerStyleRun {
                start: col,
                end: col + w,
                open: format!("\x1b[1m\x1b[38;5;{}m", cell.color),
                close: format!("\x1b[39m{fg_code}"),
            });
            col += w + 1;
        }
        col -= 1;
    }
    if !parts.status.is_empty() {
        let status_w = display_width(&parts.status);
        runs.push(BannerStyleRun {
            start: col,
            end: col + status_w,
            open: white_open,
            close: white_close,
        });
    }
    if let Some((p_start, p_end)) = rename_prefix_visual_span(file, badge, common_prefix) {
        runs.push(BannerStyleRun {
            start: p_start,
            end: p_end,
            open: "\x1b[4m\x1b[38;5;139m".to_string(),
            close: format!("\x1b[24m{fg_code}"),
        });
    }
    runs.sort_by_key(|r| r.start);

    let (styled_piece, piece_w) =
        render_banner_piece(&prefix_text, &runs, seg_start, content_width);

    let bar_str = if piece_w < content_width {
        let ruler_len = content_width - piece_w;
        format!("{styled_piece}{}", "═".repeat(ruler_len))
    } else {
        styled_piece
    };

    format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m")
}

/// Render a specific visual segment of a file header banner row.
#[allow(clippy::too_many_arguments)]
pub fn render_file_header_segment(
    file: &OutputFile,
    badge: Badge,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    render_file_header_segment_themed(
        file,
        badge,
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
    badge: Badge,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
) -> String {
    render_file_header_segment(
        file,
        badge,
        is_collapsed,
        is_selected,
        is_current,
        in_band,
        0,
        content_width,
    )
}

/// Build the raw text of a hunk header divider with format
/// `H2/3 · 6 lines: −1 +4 ~1` plus the hidden-gap tail.
/// Change counts reuse the shared [`format_counts_plain`] (zero counts
/// omitted). The caller is responsible for filling any remaining width with
/// `─` chars.
pub fn hunk_divider_text(badge: Badge, hunk: &OutputHunk, hidden_gap: usize) -> String {
    use super::header::format_counts_plain;

    let counts = hunk.counts();
    let (dels, adds, mods, movs) = (counts.dels, counts.adds, counts.mods, counts.movs);
    let total = dels + adds + mods + movs;
    let unit = if total == 1 { "line" } else { "lines" };
    let summary = format!("{} {unit}", total);
    let counts_str = format_counts_plain(&counts);
    let summary = if counts_str.is_empty() {
        summary
    } else {
        format!("{summary}: {counts_str}")
    };

    let gap_part = if hidden_gap > 0 {
        let gap_unit = if hidden_gap == 1 { "line" } else { "lines" };
        let arrows = if badge.idx == 0 {
            "▼"
        } else {
            "⬍ · ▲ · ▼"
        };
        format!(" · {hidden_gap} more {gap_unit}: {arrows}")
    } else {
        String::new()
    };
    format!("──── {} · {summary}{gap_part} ────", badge.hunk_text())
}

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
pub fn render_hunk_divider_segment_themed(
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
    let (bg, fg) = theme.palette.hunk_divider_colors(is_selected, is_current);
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
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    render_hunk_divider_segment_themed(
        badge,
        hunk,
        hidden_gap,
        is_current,
        false,
        seg_start,
        content_width,
        &DiffuseTheme::from_env(),
    )
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
) -> String {
    render_hunk_divider_segment(badge, hunk, hidden_gap, is_current, 0, content_width)
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
    let (bg, fg) = theme.palette.hunk_divider_colors(false, is_current);
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
#[allow(clippy::too_many_arguments)]
pub fn file_header_line_themed(
    file: &OutputFile,
    badge: Badge,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
    theme: &DiffuseTheme,
) -> Line<'static> {
    let bar = file_header_text(file, badge, is_collapsed);
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
    badge: Badge,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
) -> Line<'static> {
    file_header_line_themed(
        file,
        badge,
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
    let (bg, fg) = theme.palette.hunk_divider_colors(false, is_current);
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
    badge: Badge,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> Line<'static> {
    hunk_divider_line_themed(
        badge,
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
    let (bg, fg) = theme.palette.hunk_divider_colors(false, is_current);
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
