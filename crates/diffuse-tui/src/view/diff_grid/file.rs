use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::super::header::truncate_pad;
use crate::{
    core::wire::OutputFile,
    render::{Badge, ColorExt, DiffuseTheme, width::display_width},
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
    use super::super::header::format_counts_plain;

    let (indicator_prefix, collapse_tag) = file_header_indicator(file, badge, is_collapsed);
    let path_display = file.display_path_with_prefix(common_prefix);
    let status_str = file.display_status();
    let status_tag =
        if status_str.is_empty() || status_str == "modified" || status_str == "unmodified" {
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
        let gw = grapheme_width(g);
        let gs = col;
        col += gw;
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
        // A grapheme belongs to this segment when it starts inside it and
        // fully fits, OR when it straddles the segment's left edge (i.e. it
        // did not fit at the tail of the previous segment and wraps here).
        let in_segment = (gs >= seg_start && gs + gw <= seg_start + width)
            || (gs < seg_start && gs + gw > seg_start);
        if !in_segment {
            continue;
        }
        if let Some(idx) = open
            && !open_emitted
        {
            out.push_str(&runs[idx].open);
            open_emitted = true;
        }
        out.push_str(g);
        visible += gw;
    }
    if open_emitted && let Some(idx) = open {
        out.push_str(&runs[idx].close);
    }
    (out, visible)
}

/// Render a specific visual segment of a file header banner row with theme
/// support.
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
    theme: &DiffuseTheme,
    common_prefix: Option<&str>,
) -> String {
    use super::super::header::count_cells;

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
        for cell in count_cells(&file.counts, &theme.palette) {
            let text = format!("{}{}", cell.marker, cell.value);
            let w = display_width(&text);
            runs.push(BannerStyleRun {
                start: col,
                end: col + w,
                open: format!("\x1b[1m{}", cell.color.fg_code()),
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
        let fills_with_ruler = seg_start + content_width >= total_prefix_w;
        let pad = if fills_with_ruler {
            "═".repeat(ruler_len)
        } else {
            " ".repeat(ruler_len)
        };
        format!("{styled_piece}{pad}")
    } else {
        styled_piece
    };

    format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m")
}

/// Render a file header banner row with collapse status indicator and counts.
#[allow(clippy::too_many_arguments)]
pub fn render_file_header(
    file: &OutputFile,
    badge: Badge,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
    theme: &DiffuseTheme,
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
        theme,
        None,
    )
}

/// Render a file header banner row as a styled Ratatui [`Line`] with theme
/// support.
#[allow(clippy::too_many_arguments)]
pub fn file_header_line(
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
