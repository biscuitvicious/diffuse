//! Shared status-bar layout model: one ordered segment list and one overflow
//! cascade (full badges → abbreviated badges → wrapping) consumed by
//! both renderers — the ANSI string path and the ratatui span path — so
//! their decisions can never drift apart.

use crate::{core::counts::FileCounts, render::Color};

use super::header::{StatusBarContext, count_color};
use crate::render::width::display_width;

/// One logical status-bar segment. Text is plain (no ANSI); `color` and
/// `bold` carry the styling both renderers apply.
pub struct StatusSegment {
    pub text: String,
    pub color: Color,
    pub bold: bool,
}

impl StatusSegment {
    /// Visible width of the segment text.
    pub fn width(&self) -> usize {
        display_width(&self.text)
    }
}

/// Badge text for one variant of the overflow cascade.
#[derive(Clone)]
struct Badges {
    hunk: String,
}

fn badges_for(ctx: &StatusBarContext<'_>, abbrev: bool) -> Badges {
    let hunk_tag = if abbrev { "H" } else { "hunk " };
    let hunk = if ctx.total_files == 0 || ctx.total_hunks == 0 {
        format!("{hunk_tag}0/0")
    } else {
        format!("{hunk_tag}{}/{}", ctx.cur_hunk + 1, ctx.total_hunks)
    };
    Badges { hunk }
}

fn counts_segments(
    counts: &FileCounts,
    first_space: bool,
    palette: &crate::render::ThemePalette,
) -> Vec<StatusSegment> {
    counts
        .segments()
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let sep = if i == 0 && first_space {
                " "
            } else if i == 0 {
                ""
            } else {
                " "
            };
            StatusSegment {
                text: format!("{sep}{}{}", seg.marker, seg.value),
                color: count_color(seg.kind, palette),
                bold: true,
            }
        })
        .collect()
}

/// Build the ordered segment list for one cascade variant.
fn segments_for(ctx: &StatusBarContext<'_>, badges: &Badges) -> Vec<StatusSegment> {
    let p = &ctx.theme.palette;
    let mut segs = Vec::new();
    segs.push(StatusSegment {
        text: format!(" {}", badges.hunk),
        color: p.hunk_divider_fg,
        bold: true,
    });
    if let Some(c) = ctx.hunk_counts
        && !c.is_empty()
    {
        segs.extend(counts_segments(&c, true, p));
    }
    if ctx.is_streaming {
        let dot = if ctx.streaming_dot.is_empty() {
            "●"
        } else {
            ctx.streaming_dot
        };
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: p.status_bar_dim,
            bold: false,
        });
        segs.push(StatusSegment {
            text: format!("{dot} streaming..."),
            color: p.accent,
            bold: true,
        });
    }
    if ctx.raw_diff {
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: p.status_bar_dim,
            bold: false,
        });
        segs.push(StatusSegment {
            text: "raw diff".to_string(),
            color: p.status_bar_accent,
            bold: true,
        });
    }
    if ctx.col_offset > 0 {
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: p.status_bar_dim,
            bold: false,
        });
        segs.push(StatusSegment {
            text: format!("col {}+", ctx.col_offset),
            color: p.status_bar_dim,
            bold: false,
        });
    }
    if !ctx.num_buffer.is_empty() {
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: p.status_bar_dim,
            bold: false,
        });
        segs.push(StatusSegment {
            text: format!("go {}", ctx.num_buffer),
            color: p.warning,
            bold: true,
        });
    }
    segs
}

/// The resolved status-bar layout: the segments to render and whether the
/// line wraps (the final cascade fallback).
pub struct StatusLayout {
    pub segments: Vec<StatusSegment>,
    pub wraps: bool,
}

/// Resolve the shared overflow cascade: full badges, then abbreviated
/// `F`/`H` badges, then wrapping.
pub fn status_layout(ctx: &StatusBarContext<'_>) -> StatusLayout {
    let width = ctx.width;
    let seg_width = |segs: &[StatusSegment]| segs.iter().map(StatusSegment::width).sum::<usize>();

    let badges_full = badges_for(ctx, false);
    let segs = segments_for(ctx, &badges_full);
    if seg_width(&segs) <= width {
        return StatusLayout {
            segments: segs,
            wraps: false,
        };
    }

    let badges_short = badges_for(ctx, true);
    let segs = segments_for(ctx, &badges_short);
    if seg_width(&segs) <= width {
        return StatusLayout {
            segments: segs,
            wraps: false,
        };
    }

    // Nothing fits: wrap the full-badges variant.
    let segs = segments_for(ctx, &badges_full);
    StatusLayout {
        segments: segs,
        wraps: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::wire::OutputFile;

    fn ctx(width: usize, path: &str) -> (StatusBarContext<'static>, OutputFile) {
        let file = OutputFile {
            path: path.to_string(),
            old_path: None,
            old_mode: None,
            new_mode: None,
            status: "modified".to_string(),
            new_line_count: 10,
            old_line_count: 10,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![],
            raw_hunks: vec![],
            counts: FileCounts {
                adds: 3,
                dels: 0,
                mods: 0,
                movs: 0,
                notes: 0,
                ..Default::default()
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
            is_binary: false,
            old_bytes: None,
            new_bytes: None,
            raw_source: None,
        };
        (
            StatusBarContext {
                cur_fi: 0,
                total_files: 25,
                cur_hunk: 0,
                total_hunks: 1,
                raw_diff: false,
                col_offset: 0,
                num_buffer: "",
                common_prefix: None,
                width,
                is_streaming: false,
                streaming_dot: "●",
                file: None,
                hunk_counts: None,
                theme: &crate::render::DiffuseTheme::DEFAULT,
            },
            file,
        )
    }

    fn plain(layout: &StatusLayout) -> String {
        layout
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>()
            .trim()
            .to_string()
    }

    #[test]
    fn test_layout_full_badges_when_fits() {
        let (mut c, file) = ctx(80, "short.rs");
        c.file = Some(&file);
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "hunk 1/1");
    }

    #[test]
    fn test_layout_cascade_abbreviates_on_overflow() {
        let (mut c, file) = ctx(6, "crates/diffuse-core/src/lib.rs");
        c.file = Some(&file);
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        // Width 6: " hunk 1/1" is 9 cols, so badge abbreviates to " H1/1" (5 cols)
        // which fits.
        assert_eq!(plain(&layout), "H1/1");
    }

    #[test]
    fn test_layout_wraps_when_nothing_fits() {
        let (mut c, file) = ctx(4, "crates/diffuse-core/src/lib.rs");
        c.file = Some(&file);
        let layout = status_layout(&c);
        assert!(layout.wraps, "must wrap: {}", plain(&layout));
        assert_eq!(plain(&layout), "hunk 1/1");
    }

    #[test]
    fn test_layout_shows_hunk_counts() {
        let (mut c, file) = ctx(80, "short.rs");
        c.file = Some(&file);
        c.hunk_counts = Some(FileCounts {
            adds: 2,
            dels: 1,
            ..Default::default()
        });
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "hunk 1/1 −1 +2");
    }
}
