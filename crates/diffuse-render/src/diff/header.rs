//! File banner and header bar formatting.
//!
//! A file banner is defined once, here, as plain text plus an ordered list of
//! styled runs. Renderers only serialize those runs: the interactive TUI as
//! Ratatui spans, the pager and batch printer as ANSI. Content, ordering, and
//! colors therefore cannot drift between modes.

use std::fmt::{self, Write};

use yansi::Color;

use crate::{
    Badge,
    ansi::{ColorExt, SGR_RESET, strip_ansi},
    core::wire::OutputFile,
    counts::{count_cells, format_counts_plain},
    theme::{Cursor, DiffuseTheme, SurfaceColors, UiPalette},
    width::display_width,
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

    // Indicator: ruler, collapse caret (regular files only), position badge.
    let mut indicator = String::from("════ ");
    if !file.is_binary {
        indicator.push_str(if spec.is_collapsed { "▶ " } else { "▼ " });
    }
    if let Some(badge) = spec.badge {
        indicator.push_str(&badge.file_text());
        indicator.push_str(" · ");
    }
    text.push_str(&indicator);
    spans.push(base(&indicator));

    let path = file.display_path_with_prefix(spec.common_prefix);
    spans.push(HeaderSpan {
        text: path.clone(),
        role: HeaderRole::Path,
    });
    text.push_str(&path);

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
    let path_fg = surface.fg_or_path().fg_code();

    let mut bar = String::with_capacity(banner.text.len() + 96);
    write!(bar, "{bg_code}\x1b[1m{base_fg}")?;
    for span in &banner.spans {
        match span.role {
            HeaderRole::Base => bar.push_str(&span.text),
            HeaderRole::Path => write!(
                bar,
                "\x1b[22m{path_fg}{}\x1b[39m\x1b[1m{base_fg}",
                span.text
            )?,
            HeaderRole::Count(color) => {
                write!(
                    bar,
                    "\x1b[1m{}{}\x1b[39m{base_fg}",
                    color.fg_code(),
                    span.text
                )?;
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
