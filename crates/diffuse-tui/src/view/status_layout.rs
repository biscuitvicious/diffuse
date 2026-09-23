//! Shared status-bar layout model: one ordered segment list and one overflow
//! cascade (full badges → abbreviated badges → path truncation) consumed by
//! both renderers — the ANSI string path and the ratatui span path — so
//! their decisions can never drift apart.

use crate::core::counts::FileCounts;

use super::header::{
    StatusBarContext, count_color, file_status_tag, stripped_file_path, truncate_status_path,
};
use crate::render::width::display_width;

/// One logical status-bar segment. Text is plain (no ANSI); `color` and
/// `bold` carry the styling both renderers apply.
pub struct StatusSegment {
    pub text: String,
    pub color: u8,
    pub bold: bool,
}

impl StatusSegment {
    /// Visible width of the segment text.
    pub fn width(&self) -> usize {
        display_width(&self.text)
    }
}

/// Badge texts for one variant of the overflow cascade.
#[derive(Clone)]
struct Badges {
    files: String,
    hunk: String,
}

fn badges_for(ctx: &StatusBarContext<'_>, abbrev: bool) -> Badges {
    let (file_tag, hunk_tag) = if abbrev {
        ("F", "H")
    } else {
        ("file ", "hunk ")
    };
    let files = if ctx.total_files == 0 {
        format!("{file_tag}0/0")
    } else {
        format!("{file_tag}{}/{}", ctx.cur_fi + 1, ctx.total_files)
    };
    let hunk = if ctx.total_files == 0 || ctx.total_hunks == 0 {
        format!("{hunk_tag}0/0")
    } else {
        format!("{hunk_tag}{}/{}", ctx.cur_hunk + 1, ctx.total_hunks)
    };
    Badges { files, hunk }
}

fn counts_segments(counts: &FileCounts, first_space: bool) -> Vec<StatusSegment> {
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
                color: count_color(seg.kind),
                bold: true,
            }
        })
        .collect()
}

/// Build the ordered segment list for one cascade variant.
fn segments_for(
    ctx: &StatusBarContext<'_>,
    badges: &Badges,
    path: Option<&str>,
) -> Vec<StatusSegment> {
    let mut segs = Vec::new();
    segs.push(StatusSegment {
        text: format!(" {}", badges.files),
        color: 220,
        bold: true,
    });
    if ctx.is_streaming {
        let dot = if ctx.streaming_dot.is_empty() {
            "●"
        } else {
            ctx.streaming_dot
        };
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: 240,
            bold: false,
        });
        segs.push(StatusSegment {
            text: format!("{dot} streaming..."),
            color: 117,
            bold: true,
        });
    }
    if let Some(f) = ctx.file
        && !f.counts.is_empty()
    {
        segs.extend(counts_segments(&f.counts, true));
    }
    segs.push(StatusSegment {
        text: " \u{00b7} ".to_string(),
        color: 240,
        bold: false,
    });
    segs.push(StatusSegment {
        text: badges.hunk.clone(),
        color: 213,
        bold: true,
    });
    if let Some(c) = ctx.hunk_counts
        && !c.is_empty()
    {
        segs.extend(counts_segments(&c, true));
    }
    if ctx.raw_diff {
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: 240,
            bold: false,
        });
        segs.push(StatusSegment {
            text: "raw diff".to_string(),
            color: 220,
            bold: true,
        });
    }
    if ctx.col_offset > 0 {
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: 240,
            bold: false,
        });
        segs.push(StatusSegment {
            text: format!("col {}+", ctx.col_offset),
            color: 244,
            bold: false,
        });
    }
    if !ctx.num_buffer.is_empty() {
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: 240,
            bold: false,
        });
        segs.push(StatusSegment {
            text: format!("go {}", ctx.num_buffer),
            color: 221,
            bold: true,
        });
    }
    if let (Some(f), Some(path)) = (ctx.file, path) {
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: 240,
            bold: false,
        });
        segs.push(StatusSegment {
            text: path.to_string(),
            color: 231,
            bold: true,
        });
        let status = file_status_tag(f);
        if !status.is_empty() {
            segs.push(StatusSegment {
                text: status,
                color: 244,
                bold: false,
            });
        }
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
/// `F`/`H` badges, then path truncation, then wrapping.
pub fn status_layout(ctx: &StatusBarContext<'_>) -> StatusLayout {
    let path_full: Option<String> = ctx.file.map(|f| stripped_file_path(f, ctx.common_prefix));
    let width = ctx.width;
    let seg_width = |segs: &[StatusSegment]| segs.iter().map(StatusSegment::width).sum::<usize>();

    let badges_full = badges_for(ctx, false);
    let segs = segments_for(ctx, &badges_full, path_full.as_deref());
    if seg_width(&segs) <= width {
        return StatusLayout {
            segments: segs,
            wraps: false,
        };
    }

    let badges_short = badges_for(ctx, true);
    let segs = segments_for(ctx, &badges_short, path_full.as_deref());
    if seg_width(&segs) <= width {
        return StatusLayout {
            segments: segs,
            wraps: false,
        };
    }

    if let Some(path) = path_full.as_deref() {
        let path_w = display_width(path);
        let budget = width.saturating_sub(seg_width(&segs).saturating_sub(path_w));
        if let Some(short) = truncate_status_path(path, budget) {
            let segs = segments_for(ctx, &badges_short, Some(&short));
            return StatusLayout {
                segments: segs,
                wraps: false,
            };
        }
    }

    // Nothing fits: wrap the full-badges variant.
    let segs = segments_for(ctx, &badges_full, path_full.as_deref());
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
        assert_eq!(plain(&layout), "file 1/25 +3 · hunk 1/1 · short.rs");
    }

    #[test]
    fn test_layout_cascade_abbreviates_then_truncates() {
        let (mut c, file) = ctx(45, "crates/diffuse-core/src/lib.rs");
        c.file = Some(&file);
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        // Width 45: the abbreviated badges still cannot carry the full path,
        // so the cascade truncates it tail-kept in the same step.
        assert_eq!(
            plain(&layout),
            "F1/25 +3 · H1/1 · …s/diffuse-core/src/lib.rs"
        );

        // Tighter still: the truncation recedes further, never wrapping.
        c.width = 36;
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert!(
            plain(&layout).starts_with("F1/25 +3 · H1/1 · …"),
            "{}",
            plain(&layout)
        );
    }

    #[test]
    fn test_layout_wraps_when_nothing_fits() {
        let (mut c, file) = ctx(15, "crates/diffuse-core/src/lib.rs");
        c.file = Some(&file);
        let layout = status_layout(&c);
        assert!(layout.wraps, "must wrap: {}", plain(&layout));
    }
}
