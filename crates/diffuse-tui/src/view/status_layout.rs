//! Shared status-bar layout model: one ordered segment list with overflow
//! wrapping, consumed by both renderers — the ANSI string path and the
//! ratatui span path — so their decisions can never drift apart.

use crate::render::Color;

use super::header::StatusBarContext;
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

fn push_segment(
    segs: &mut Vec<StatusSegment>,
    text: String,
    color: Color,
    bold: bool,
    p: &crate::render::ThemePalette,
) {
    if !segs.is_empty() {
        segs.push(StatusSegment {
            text: " \u{00b7} ".to_string(),
            color: p.status_bar_dim,
            bold: false,
        });
        segs.push(StatusSegment { text, color, bold });
    } else {
        segs.push(StatusSegment {
            text: format!(" {text}"),
            color,
            bold,
        });
    }
}

/// Build the ordered segment list: the status message first, then the
/// persistent empty-state notice (zero files), then the transient
/// indicators.
fn segments_for(ctx: &StatusBarContext<'_>) -> Vec<StatusSegment> {
    let p = &ctx.theme.palette;
    let mut segs = Vec::new();
    if let Some((msg, is_ok)) = ctx.status_message {
        let color = if is_ok { p.header_staged } else { p.count_dels };
        push_segment(&mut segs, msg.to_string(), color, true, p);
    }
    // Persistent empty state (no timeout): with no files the bar always
    // carries the notice instead of the main area, so the reserved slot
    // stays meaningful and the viewport keeps its geometry.
    if ctx.total_files == 0 {
        push_segment(
            &mut segs,
            "No diff to display.".to_string(),
            p.status_bar_dim,
            false,
            p,
        );
    }
    if ctx.is_streaming {
        let dot = if ctx.streaming_dot.is_empty() {
            "●"
        } else {
            ctx.streaming_dot
        };
        push_segment(&mut segs, format!("{dot} streaming..."), p.accent, true, p);
    }
    if ctx.raw_diff {
        push_segment(
            &mut segs,
            "raw diff".to_string(),
            p.status_bar_accent,
            true,
            p,
        );
    }
    if ctx.col_offset > 0 {
        push_segment(
            &mut segs,
            format!("col {}+", ctx.col_offset),
            p.status_bar_dim,
            false,
            p,
        );
    }
    if !ctx.num_buffer.is_empty() {
        push_segment(
            &mut segs,
            format!("go {}", ctx.num_buffer),
            p.warning,
            true,
            p,
        );
    }
    // Global directory prefix trails last, truncated leftmost-first so the
    // deepest part stays visible.
    if let Some(prefix) = ctx.common_prefix
        && !prefix.is_empty()
    {
        let used: usize = segs.iter().map(StatusSegment::width).sum();
        let sep = if segs.is_empty() { 1 } else { 3 };
        let budget = ctx.width.saturating_sub(used + sep);
        if budget > 0 {
            let text = super::header::truncate_status_path(prefix, budget)
                .unwrap_or_else(|| prefix.to_string());
            push_segment(&mut segs, text, p.status_bar_dim, false, p);
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

/// Resolve the status-bar layout: segments when non-empty, then wrapping on
/// overflow.
pub fn status_layout(ctx: &StatusBarContext<'_>) -> StatusLayout {
    let width = ctx.width;
    let seg_width = |segs: &[StatusSegment]| segs.iter().map(StatusSegment::width).sum::<usize>();

    let segs = segments_for(ctx);
    if segs.is_empty() {
        return StatusLayout {
            segments: Vec::new(),
            wraps: false,
        };
    }
    if seg_width(&segs) <= width {
        return StatusLayout {
            segments: segs,
            wraps: false,
        };
    }

    // Overflow: wrap the segments.
    StatusLayout {
        segments: segs,
        wraps: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{counts::FileCounts, wire::OutputFile};

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
                status_message: None,
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
    fn test_status_layout_omits_hunk_data_and_is_empty() {
        let (mut c, file) = ctx(80, "short.rs");
        c.file = Some(&file);
        c.cur_hunk = 0;
        c.total_hunks = 3;
        c.hunk_counts = Some(FileCounts {
            adds: 2,
            dels: 1,
            ..Default::default()
        });
        let layout = status_layout(&c);
        assert!(
            layout.segments.is_empty(),
            "status bar must omit hunk data and be empty"
        );
    }

    #[test]
    fn test_layout_single_segment_when_fits() {
        let (mut c, file) = ctx(80, "short.rs");
        c.file = Some(&file);
        c.is_streaming = true;
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "● streaming...");
    }

    #[test]
    fn test_layout_multiple_segments_joined() {
        let (mut c, file) = ctx(80, "crates/diffuse-core/src/lib.rs");
        c.file = Some(&file);
        c.raw_diff = true;
        c.col_offset = 12;
        c.num_buffer = "42";
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "raw diff · col 12+ · go 42");
    }

    #[test]
    fn test_layout_wraps_when_overflows() {
        let (mut c, file) = ctx(10, "crates/diffuse-core/src/lib.rs");
        c.file = Some(&file);
        c.raw_diff = true;
        c.col_offset = 12;
        let layout = status_layout(&c);
        assert!(layout.wraps, "must wrap: {}", plain(&layout));
        assert_eq!(plain(&layout), "raw diff · col 12+");
    }

    #[test]
    fn test_layout_status_message_leads_segments() {
        let (mut c, _file) = ctx(80, "short.rs");
        c.status_message = Some(("staged hunk", true));
        c.num_buffer = "42";
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "staged hunk · go 42");
    }

    #[test]
    fn test_layout_status_message_alone() {
        let (mut c, _file) = ctx(80, "short.rs");
        c.status_message = Some(("no staged changes", false));
        let layout = status_layout(&c);
        assert!(!layout.segments.is_empty());
        assert_eq!(plain(&layout), "no staged changes");
    }

    #[test]
    fn test_layout_status_message_colors_by_severity() {
        let (mut c, _file) = ctx(80, "short.rs");
        c.status_message = Some(("ok note", true));
        let ok = status_layout(&c);
        assert_eq!(
            ok.segments[0].color,
            crate::render::DiffuseTheme::DEFAULT.palette.header_staged
        );
        assert!(ok.segments[0].bold);
        c.status_message = Some(("bad note", false));
        let err = status_layout(&c);
        assert_eq!(
            err.segments[0].color,
            crate::render::DiffuseTheme::DEFAULT.palette.count_dels
        );
        assert!(err.segments[0].bold);
    }

    #[test]
    fn test_layout_prefix_alone_when_no_segments() {
        let (mut c, _file) = ctx(80, "short.rs");
        c.common_prefix = Some("crates/diffuse-core/src/");
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "crates/diffuse-core/src/");
    }

    #[test]
    fn test_layout_prefix_trails_segments_with_dot_separator() {
        let (mut c, _file) = ctx(80, "short.rs");
        c.raw_diff = true;
        c.common_prefix = Some("crates/");
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "raw diff · crates/");
    }

    #[test]
    fn test_layout_prefix_truncates_left_keeping_deepest() {
        let (mut c, _file) = ctx(20, "short.rs");
        c.common_prefix = Some("crates/diffuse-core/src/moves/");
        let layout = status_layout(&c);
        let text = plain(&layout);
        assert!(
            text.ends_with("src/moves/"),
            "deepest path part must stay visible: {text}"
        );
        assert!(text.starts_with("…"), "cut must be marked: {text}");
        assert!(!layout.wraps, "truncated prefix must fit one line: {text}");
    }
}

#[cfg(test)]
mod empty_state_tests {
    use super::*;

    fn empty_ctx(width: usize) -> StatusBarContext<'static> {
        StatusBarContext {
            cur_fi: 0,
            total_files: 0,
            cur_hunk: 0,
            total_hunks: 0,
            raw_diff: false,
            col_offset: 0,
            num_buffer: "",
            common_prefix: None,
            width,
            is_streaming: false,
            streaming_dot: "●",
            status_message: None,
            file: None,
            hunk_counts: None,
            theme: &crate::render::DiffuseTheme::DEFAULT,
        }
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
    fn test_layout_empty_state_when_no_files() {
        let c = empty_ctx(80);
        let layout = status_layout(&c);
        assert_eq!(plain(&layout), "No diff to display.");
    }

    #[test]
    fn test_layout_empty_state_trails_status_message() {
        let mut c = empty_ctx(80);
        c.status_message = Some(("staged hunk", true));
        let layout = status_layout(&c);
        assert_eq!(plain(&layout), "staged hunk · No diff to display.");
    }

    #[test]
    fn test_layout_no_empty_state_with_files() {
        let mut c = empty_ctx(80);
        c.total_files = 3;
        let layout = status_layout(&c);
        assert!(
            layout.segments.is_empty(),
            "files present must not show the empty state: {}",
            plain(&layout)
        );
    }
}
