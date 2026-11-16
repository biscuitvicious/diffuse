//! File banner and header bar formatting.
//!
//! A file banner is defined once, here, as plain text plus an ordered list of
//! styled runs. Renderers only serialize those runs: the interactive TUI as
//! Ratatui spans, the pager and batch printer as ANSI. Content, ordering, and
//! colors therefore cannot drift between modes.

use std::fmt::{self, Write};

use unicode_segmentation::UnicodeSegmentation;
use yansi::Color;

use crate::{
    Badge,
    ansi::{ColorExt, FG_RESET, SGR_RESET, strip_ansi},
    core::{parse::FileStatus, wire::OutputFile},
    counts::{count_cells, format_counts_plain},
    span::{StyledSpan, slice_spans},
    theme::{Cursor, DiffuseTheme, SurfaceColors, UiPalette},
    width::{display_width, grapheme_width},
};

/// Which banner a caller draws: its position badge (absent while streaming),
/// its collapse state, and the shared directory prefix stripped from the path.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileHeaderSpec<'a> {
    pub badge: Option<Badge>,
    pub is_collapsed: bool,
    pub common_prefix: Option<&'a str>,
}

/// Styling role of one banner run, independent of any renderer's escape
/// syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderRole {
    /// Badge, separators, collapse tag, and trailing gap: the banner's base
    /// foreground, bold.
    Base,
    /// File path and status parens: the banner's path color, never bold.
    Path,
    /// One change-count cell: bold in its bucket color.
    Count(Color),
}

/// One ordered, styled run of a file banner. Concatenating every `text` in
/// order yields [`FileHeaderBanner::text`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderSpan {
    pub text: String,
    pub role: HeaderRole,
}

/// The canonical file banner: its plain text, its styled runs, and the
/// surface they paint on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHeaderBanner {
    pub surface: SurfaceColors,
    pub text: String,
    pub spans: Vec<HeaderSpan>,
}

/// Status suffix rules shared by every mode: routine states stay unadorned.
fn status_tag(file: &OutputFile) -> String {
    let status = file.display_status();
    if status.is_empty() || status == "modified" || status == "unmodified" {
        String::new()
    } else {
        format!(" ({status})")
    }
}

/// Walk the banner in render order, accumulating plain text and (when a
/// palette is supplied) the matching styled runs.
fn walk(
    file: &OutputFile,
    spec: &FileHeaderSpec<'_>,
    ui: Option<&UiPalette>,
) -> (String, Vec<HeaderSpan>) {
    let mut text = String::with_capacity(96);
    let mut spans: Vec<HeaderSpan> = Vec::with_capacity(12);
    let base = |text: &str| HeaderSpan {
        text: text.to_string(),
        role: HeaderRole::Base,
    };

    // Indicator: ruler and collapse caret (regular files only).
    let mut indicator = String::from("════ ");
    if !file.is_binary {
        indicator.push_str(if spec.is_collapsed { "▶ " } else { "▼ " });
    }
    text.push_str(&indicator);
    spans.push(base(&indicator));

    let path = file.display_path_with_prefix(spec.common_prefix);
    spans.push(HeaderSpan {
        text: path.clone(),
        role: HeaderRole::Path,
    });
    text.push_str(&path);

    if let Some(badge) = spec.badge {
        let badge_text = format!(" {}", badge.number_text());
        text.push_str(&badge_text);
        spans.push(base(&badge_text));
    }

    let counts_text = format_counts_plain(&file.counts);
    if !counts_text.is_empty() {
        text.push(' ');
        spans.push(base(" "));
        match ui {
            Some(ui) => {
                for (i, cell) in count_cells(&file.counts, ui).iter().enumerate() {
                    if i > 0 {
                        text.push(' ');
                        spans.push(base(" "));
                    }
                    let cell_text = format!("{}{}", cell.marker, cell.value);
                    text.push_str(&cell_text);
                    spans.push(HeaderSpan {
                        text: cell_text,
                        role: HeaderRole::Count(cell.color),
                    });
                }
            }
            None => text.push_str(&counts_text),
        }
    }

    let status = status_tag(file);
    if !status.is_empty() {
        spans.push(HeaderSpan {
            text: status.clone(),
            role: HeaderRole::Path,
        });
        text.push_str(&status);
    }

    if !file.notes.is_empty() {
        let notes = format!(
            " · {} note{}",
            file.notes.len(),
            if file.notes.len() == 1 { "" } else { "s" }
        );
        text.push_str(&notes);
        spans.push(base(&notes));
    }

    if spec.is_collapsed {
        text.push_str(" [collapsed]");
        spans.push(base(" [collapsed]"));
    }

    // The trailing gap separates the banner from the next row.
    text.push(' ');
    spans.push(base(" "));

    (text, spans)
}

/// Plain text of a file banner.
#[must_use]
pub fn file_header_text(file: &OutputFile, spec: &FileHeaderSpec<'_>) -> String {
    walk(file, spec, None).0
}

/// The canonical banner: text, styled runs, and the surface for `tier`.
#[must_use]
pub fn file_header_banner(
    file: &OutputFile,
    spec: &FileHeaderSpec<'_>,
    theme: &DiffuseTheme,
    tier: Cursor,
) -> FileHeaderBanner {
    let (text, mut spans) = walk(file, spec, Some(&theme.palette.ui));
    // A text-only walk leaves the count runs unexpanded; rebuild nothing here
    // because the palette path above always expands them.
    spans.shrink_to_fit();
    FileHeaderBanner {
        surface: theme.palette[tier].file_header,
        text,
        spans,
    }
}

/// Format a file's header bar into an output buffer. Mirrors the TUI banner
/// exactly: same indicator, badge, path, counts, and status parens.
pub fn write_file_header_bar<W: Write>(
    out: &mut W,
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
) -> fmt::Result {
    write_file_header_bar_with_width(out, file, badge, theme, None)
}

/// Format a file's header bar into an output buffer, extending the banner
/// across `width` with the same ruler fill the TUI uses.
pub fn write_file_header_bar_with_width<W: Write>(
    out: &mut W,
    file: &OutputFile,
    badge: Option<Badge>,
    theme: &DiffuseTheme,
    width: Option<usize>,
) -> fmt::Result {
    let spec = FileHeaderSpec {
        badge,
        is_collapsed: false,
        common_prefix: None,
    };
    let banner = file_header_banner(file, &spec, theme, Cursor::Rest);
    let surface = banner.surface;
    let bg_code = surface.bg.bg_code();
    let base_fg = surface.fg.fg_code();
    // Style decisions live in `banner_spans`; the role machine below only
    // preserves the exact open/close skeleton (bold base runs emit bare
    // text, path runs drop bold around their color, count runs keep the
    // redundant bold set before their color).
    let styled = banner_spans(&banner, None);

    let mut bar = String::with_capacity(banner.text.len() + 96);
    write!(bar, "{bg_code}\x1b[1m{base_fg}")?;
    for (span, styled_span) in banner.spans.iter().zip(&styled) {
        let fg_code = styled_span
            .fg
            .map(|color| color.fg_code())
            .unwrap_or_default();
        match span.role {
            HeaderRole::Base => bar.push_str(&styled_span.text),
            HeaderRole::Path => write!(
                bar,
                "\x1b[22m{fg_code}{}\x1b[39m\x1b[1m{base_fg}",
                styled_span.text
            )?,
            HeaderRole::Count(_) => {
                write!(bar, "\x1b[1m{fg_code}{}\x1b[39m{base_fg}", styled_span.text)?;
            }
        }
    }

    if let Some(width) = width
        && width > 0
    {
        let visible = display_width(strip_ansi(&bar));
        if visible < width {
            bar.push_str(&"═".repeat(width - visible));
        }
    }

    writeln!(bar, "{SGR_RESET}")?;
    out.write_str(&bar)
}

/// Return the two traditional raw unified-diff header lines: `--- {old_side}`
/// and `+++ {new_side}`.
///
/// Paths carry git's `a/`/`b/` prefixes with `/dev/null` on the missing
/// side of added, untracked, and deleted files; renames read the old path
/// on the minus side.
#[must_use]
pub fn raw_file_header_lines(file: &OutputFile) -> (String, String) {
    let old_side = match file.status {
        FileStatus::Added | FileStatus::Untracked => "/dev/null".to_string(),
        _ => format!("a/{}", file.old_path.as_deref().unwrap_or(&file.path)),
    };
    let new_side = match file.status {
        FileStatus::Deleted => "/dev/null".to_string(),
        _ => format!("b/{}", file.path),
    };
    (format!("--- {old_side}"), format!("+++ {new_side}"))
}

/// Format a file's raw-mode header: traditional unified-diff `---`/`+++`
/// lines, each riding whole-line in the file header foreground (file header
/// yellow). Paths carry git's `a/`/`b/` prefixes with `/dev/null` on the
/// missing side of added, untracked, and deleted files; renames read the
/// old path on the minus side. No badge, counts, or ruler fill: traditional
/// headers carry no metadata, and only the foreground state is touched, so
/// each line closes with a foreground reset.
pub fn write_raw_file_header<W: Write>(
    out: &mut W,
    file: &OutputFile,
    theme: &DiffuseTheme,
) -> fmt::Result {
    let header_fg = theme.palette[Cursor::Rest].file_header.fg;
    let (line1, line2) = raw_file_header_lines(file);
    writeln!(out, "{}{line1}{FG_RESET}", header_fg.fg_code())?;
    writeln!(out, "{}{line2}{FG_RESET}", header_fg.fg_code())
}

/// Project a canonical [`FileHeaderBanner`] onto ordered [`StyledSpan`]s:
/// the span-native twin of the ANSI banner serializer in
/// [`write_file_header_bar_with_width`]. Base runs render bold in the
/// surface foreground, path runs in the path color without bold, and count
/// runs bold in their bucket color; every span rides on the banner surface
/// background. `rename` is an optional visual-column range (the shared
/// directory prefix) overlaid with the desaturated-purple underline the
/// ANSI path emits, preserving each overlapped span's other flags exactly
/// as the ANSI open sequence (which never touches bold state) does.
#[must_use]
pub fn banner_spans(banner: &FileHeaderBanner, rename: Option<(usize, usize)>) -> Vec<StyledSpan> {
    let bg = Some(banner.surface.bg);
    let mut spans = Vec::with_capacity(banner.spans.len());
    for span in &banner.spans {
        let styled = match span.role {
            HeaderRole::Base => {
                StyledSpan::styled(span.text.clone(), Some(banner.surface.fg), bg).with_bold(true)
            }
            HeaderRole::Path => {
                StyledSpan::styled(span.text.clone(), Some(banner.surface.fg_or_path()), bg)
            }
            HeaderRole::Count(color) => {
                StyledSpan::styled(span.text.clone(), Some(color), bg).with_bold(true)
            }
        };
        spans.push(styled);
    }
    if let Some((start, end)) = rename {
        spans = underline_range(spans, start, end, Color::Fixed(139));
    }
    spans
}

/// Overlay a visual-column range with an underline in `color`, splitting
/// overlapped spans and preserving their other flags.
fn underline_range(
    spans: Vec<StyledSpan>,
    start: usize,
    end: usize,
    color: Color,
) -> Vec<StyledSpan> {
    let mut out = Vec::with_capacity(spans.len() + 2);
    let mut col = 0usize;
    for span in spans {
        let span_width: usize = span.text.graphemes(true).map(grapheme_width).sum();
        let span_start = col;
        col += span_width;
        if col <= start || span_start >= end || span.text.is_empty() {
            out.push(span);
            continue;
        }
        let low = start.max(span_start);
        let high = end.min(col);
        for (from, to, over) in [
            (span_start, low, false),
            (low, high, true),
            (high, col, false),
        ] {
            if to <= from {
                continue;
            }
            let mut pieces = slice_spans(std::slice::from_ref(&span), from - span_start, to - from);
            if over {
                for piece in &mut pieces {
                    piece.fg = Some(color);
                    piece.underline = true;
                }
            }
            out.extend(pieces);
        }
    }
    out
}
