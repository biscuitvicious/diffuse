use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::super::header::truncate_pad;
use crate::{
    core::wire::OutputFile,
    render::{
        Badge, ColorExt, Cursor, DiffuseTheme, FileHeaderBanner, FileHeaderSpec, HeaderRole,
        file_header_banner, file_header_text as canonical_file_header_text, width::display_width,
    },
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
        // Prefix starts after the indicator, badge and dot (`════ ▼ File 3/17 · `
        // or `════ File 3/17 · ` for binaries) and includes trailing '/'
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

/// Char column of the collapse/expand triangle in the file banner.
pub const FILE_HEADER_INDICATOR_COL: usize = 5;

/// A styled column range over plain banner text: emitted with `open`
/// before the overlapped slice and `close` after it.
struct BannerStyleRun {
    start: usize,
    end: usize,
    open: String,
    close: String,
}

/// Build the styled column ranges of a canonical banner, one per non-base
/// run, so the renderer only chooses escape syntax.
fn banner_runs(banner: &FileHeaderBanner) -> Vec<BannerStyleRun> {
    let base_fg = banner.surface.fg.fg_code();
    let path_fg = banner.surface.fg_or_path().fg_code();
    let mut runs = Vec::with_capacity(banner.spans.len());
    let mut col = 0usize;
    for span in &banner.spans {
        let width = display_width(&span.text);
        let escapes = match span.role {
            HeaderRole::Base => None,
            HeaderRole::Path => Some((
                format!("\x1b[22m{path_fg}"),
                format!("\x1b[39m\x1b[1m{base_fg}"),
            )),
            HeaderRole::Count(color) => Some((
                format!("\x1b[1m{}", color.fg_code()),
                format!("\x1b[39m{base_fg}"),
            )),
        };
        if let Some((open, close)) = escapes {
            runs.push(BannerStyleRun {
                start: col,
                end: col + width,
                open,
                close,
            });
        }
        col += width;
    }
    runs
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

/// Canonical plain text of a file banner row with the shared directory prefix
/// stripped.
pub fn file_header_text_with_prefix(
    file: &OutputFile,
    badge: Badge,
    is_collapsed: bool,
    common_prefix: Option<&str>,
) -> String {
    canonical_file_header_text(
        file,
        &FileHeaderSpec {
            badge: Some(badge),
            is_collapsed,
            common_prefix,
        },
    )
}

/// Canonical plain text of a file banner row.
pub fn file_header_text(file: &OutputFile, badge: Badge, is_collapsed: bool) -> String {
    file_header_text_with_prefix(file, badge, is_collapsed, None)
}

/// Render a specific visual segment of a file banner row with theme support.
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
    // Selected banners lift to the row tier; current / in-band banners sit
    // at the file tier; everything else rests.
    let tier = Cursor::from_banner_state(is_selected, is_current || in_band);
    let banner = file_header_banner(
        file,
        &FileHeaderSpec {
            badge: Some(badge),
            is_collapsed,
            common_prefix,
        },
        theme,
        tier,
    );
    let bg_code = banner.surface.bg.bg_code();
    let fg_code = banner.surface.fg.fg_code();

    let total_prefix_w = display_width(&banner.text);
    if seg_start >= total_prefix_w {
        let bar_str = "═".repeat(content_width);
        return format!("{bg_code}\x1b[1m{fg_code}{bar_str}\x1b[0m");
    }

    let mut runs = banner_runs(&banner);
    if let Some((start, end)) = rename_prefix_visual_span(file, badge, common_prefix) {
        runs.push(BannerStyleRun {
            start,
            end,
            open: "\x1b[4m\x1b[38;5;139m".to_string(),
            close: format!("\x1b[24m{fg_code}"),
        });
    }
    runs.sort_by_key(|r| r.start);

    let (styled_piece, piece_w) =
        render_banner_piece(&banner.text, &runs, seg_start, content_width);

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

/// Render a file banner row as a styled Ratatui [`Line`] with theme support.
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
    let banner = file_header_banner(
        file,
        &FileHeaderSpec {
            badge: Some(badge),
            is_collapsed,
            common_prefix: None,
        },
        theme,
        Cursor::from_banner_state(is_selected, is_current || in_band),
    );
    let bar_str = truncate_pad(&banner.text, content_width);
    let style = Style::default()
        .bg(crate::view::to_color(banner.surface.bg))
        .fg(crate::view::to_color(banner.surface.fg))
        .add_modifier(Modifier::BOLD);
    Line::from(Span::styled(bar_str, style))
}
