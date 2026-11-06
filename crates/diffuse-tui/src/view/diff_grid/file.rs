use ratatui::text::Line;

use crate::{
    core::wire::OutputFile,
    render::{
        Badge, Cursor, DiffuseTheme, FileHeaderSpec, banner_spans, file_header_banner,
        file_header_text as canonical_file_header_text,
        span::{StyledSpan, slice_spans, spans_width},
        width::display_width,
    },
    view::span::line_from_spans,
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

/// Render a specific visual segment of a file banner row as a styled
/// Ratatui [`Line`]: role-styled banner text sliced at `seg_start`, padded
/// with the ruler (or spaces mid-banner) to `content_width`. The direct
/// grid path: no ANSI round-trip.
#[allow(clippy::too_many_arguments)]
pub fn file_header_segment_line(
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
) -> Line<'static> {
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
    let ruler = || {
        StyledSpan::styled(
            "═".repeat(content_width),
            Some(banner.surface.fg),
            Some(banner.surface.bg),
        )
        .with_bold(true)
    };
    let total_w = display_width(&banner.text);
    if seg_start >= total_w {
        return line_from_spans(vec![ruler()]);
    }
    let rename = rename_prefix_visual_span(file, badge, common_prefix);
    let mut sliced = slice_spans(&banner_spans(&banner, rename), seg_start, content_width);
    let piece_w = spans_width(&sliced);
    if piece_w < content_width {
        let ruler_len = content_width - piece_w;
        let fills_with_ruler = seg_start + content_width >= total_w;
        let pad_char = if fills_with_ruler { "═" } else { " " };
        sliced.push(
            StyledSpan::styled(
                pad_char.repeat(ruler_len),
                Some(banner.surface.fg),
                Some(banner.surface.bg),
            )
            .with_bold(true),
        );
    }
    line_from_spans(sliced)
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
    file_header_segment_line(
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
