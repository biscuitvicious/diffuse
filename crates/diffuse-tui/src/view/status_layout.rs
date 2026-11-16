//! Shared status-bar layout model: one ordered segment list with overflow
//! wrapping, consumed by both renderers — the ANSI string path and the
//! ratatui span path — so their decisions can never drift apart.

use std::borrow::Cow;

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
    p: &crate::render::UiPalette,
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

/// Build the ordered segment list from most persistent to most ephemeral:
/// common prefix -> raw diff -> col -> go -> error/status message ->
/// persistent empty-state notice (zero files) -> streaming.
fn segments_for(ctx: &StatusBarContext<'_>) -> Vec<StatusSegment> {
    let p = &ctx.theme.palette.ui;

    // Collect trailing segments following the common prefix
    let mut trailing: Vec<(Cow<'_, str>, Color, bool)> = Vec::new();

    if ctx.raw_diff {
        trailing.push((Cow::Borrowed("raw diff"), p.status_bar_accent, true));
    } else if !ctx.side_by_side && ctx.expanded_mods_count > 0 {
        let label = if ctx.total_mods > 0 && ctx.expanded_mods_count == ctx.total_mods {
            Cow::Borrowed("expanded mods")
        } else if ctx.expanded_mods_count == 1 {
            Cow::Borrowed("1 expanded mod")
        } else {
            Cow::Owned(format!("{} expanded mods", ctx.expanded_mods_count))
        };
        trailing.push((label, p.status_bar_accent, true));
    }
    if ctx.col_offset > 0 {
        trailing.push((
            Cow::Owned(format!("col {}+", ctx.col_offset)),
            p.status_bar_dim,
            false,
        ));
    }
    if !ctx.num_buffer.is_empty() {
        trailing.push((
            Cow::Owned(format!("go {}", ctx.num_buffer)),
            p.warning,
            true,
        ));
    }
    if let Some((msg, is_ok)) = ctx.status_message {
        let color = if is_ok { p.header_staged } else { p.count_dels };
        trailing.push((Cow::Borrowed(msg), color, true));
    }
    // Persistent empty state (no timeout): with no files the bar carries the
    // notice instead of the main area, so the reserved slot stays meaningful
    // and the viewport keeps its geometry. While the diff is still streaming
    // the absence of files says nothing — the first file may be milliseconds
    // away — so the notice waits for the stream to finish.
    if ctx.total_files == 0 && !ctx.is_streaming {
        trailing.push((
            Cow::Borrowed("No diff to display."),
            p.status_bar_dim,
            false,
        ));
    }
    if ctx.is_streaming {
        let dot = if ctx.streaming_dot.is_empty() {
            "●"
        } else {
            ctx.streaming_dot
        };
        trailing.push((Cow::Owned(format!("{dot} streaming...")), p.accent, true));
    }

    let mut segs = Vec::new();

    // 1. Global directory prefix leads first, truncated leftmost-first so the
    // deepest part stays visible. It is budgeted against trailing segments so
    // that active view modes and ephemeral messages are not crowded out.
    if let Some(prefix) = ctx.common_prefix
        && !prefix.is_empty()
    {
        let trailing_text_width: usize = trailing.iter().map(|(t, _, _)| display_width(t)).sum();
        let trailing_separators = if trailing.is_empty() {
            0
        } else {
            (trailing.len() - 1) * 3 + 3
        };
        let trailing_width = trailing_text_width + trailing_separators;
        let budget = ctx.width.saturating_sub(1 + trailing_width);
        if budget > 0 {
            let text = super::header::truncate_status_path(prefix, budget)
                .unwrap_or_else(|| prefix.to_string());
            push_segment(&mut segs, text, p.status_bar_dim, false, p);
        }
    }

    // 2. Trailing segments in persistent-to-ephemeral order
    for (text, color, bold) in trailing {
        push_segment(&mut segs, text.into_owned(), color, bold, p);
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
            status: diffuse_core::FileStatus::Modified,
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
                side_by_side: false,
                expanded_mods_count: 0,
                total_mods: 0,
                col_offset: 0,
                num_buffer: "",
                common_prefix: None,
                width,
                is_streaming: false,
                streaming_dot: "●",
                status_message: None,
                file: None,
                hunk_counts: None,
                theme: crate::render::DiffuseTheme::dark(),
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
    fn test_layout_go_leads_status_message() {
        let (mut c, _file) = ctx(80, "short.rs");
        c.status_message = Some(("staged hunk", true));
        c.num_buffer = "42";
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "go 42 · staged hunk");
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
            crate::render::DiffuseTheme::dark().palette.ui.header_staged
        );
        assert!(ok.segments[0].bold);
        c.status_message = Some(("bad note", false));
        let err = status_layout(&c);
        assert_eq!(
            err.segments[0].color,
            crate::render::DiffuseTheme::dark().palette.ui.count_dels
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
    fn test_layout_prefix_leads_raw_diff_with_dot_separator() {
        let (mut c, _file) = ctx(80, "short.rs");
        c.raw_diff = true;
        c.common_prefix = Some("crates/");
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(plain(&layout), "crates/ · raw diff");
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

    #[test]
    fn test_layout_prefix_budgeted_against_trailing_segments() {
        let (mut c, _file) = ctx(25, "short.rs");
        c.common_prefix = Some("crates/diffuse-core/src/moves/");
        c.raw_diff = true;
        let layout = status_layout(&c);
        let text = plain(&layout);
        assert!(
            text.ends_with(" · raw diff"),
            "trailing segment must be preserved: {text}"
        );
        assert!(
            text.starts_with("…"),
            "prefix must be truncated on left: {text}"
        );
        assert!(!layout.wraps, "budgeted prefix must not cause wrap: {text}");
    }

    #[test]
    fn test_layout_full_order_persistent_to_ephemeral() {
        let (mut c, _file) = ctx(120, "short.rs");
        c.common_prefix = Some("crates/");
        c.raw_diff = true;
        c.col_offset = 8;
        c.num_buffer = "10";
        c.status_message = Some(("note saved", true));
        let layout = status_layout(&c);
        assert!(!layout.wraps);
        assert_eq!(
            plain(&layout),
            "crates/ · raw diff · col 8+ · go 10 · note saved"
        );
    }

    #[test]
    fn test_status_bar_expanded_mods_badges() {
        let (mut c, _file) = ctx(120, "short.rs");
        c.total_mods = 3;
        c.expanded_mods_count = 0;
        let layout = status_layout(&c);
        assert!(!plain(&layout).contains("expanded mod"));

        c.expanded_mods_count = 1;
        let layout = status_layout(&c);
        assert_eq!(plain(&layout), "1 expanded mod");

        c.expanded_mods_count = 2;
        let layout = status_layout(&c);
        assert_eq!(plain(&layout), "2 expanded mods");

        c.expanded_mods_count = 3;
        let layout = status_layout(&c);
        assert_eq!(plain(&layout), "expanded mods");

        // When in raw mode, expanded mods badge is suppressed in favor of raw diff
        c.raw_diff = true;
        let layout = status_layout(&c);
        assert_eq!(plain(&layout), "raw diff");

        // When in split mode, expanded mods badge is suppressed and split badge is
        // omitted
        c.raw_diff = false;
        c.side_by_side = true;
        let layout = status_layout(&c);
        assert_eq!(plain(&layout), "");
        assert!(!plain(&layout).contains("expanded"));
        assert!(!plain(&layout).contains("split"));
    }

    #[test]
    fn test_status_bar_split_badge_omitted() {
        let (mut c, _f) = ctx(100, "");
        c.side_by_side = true;
        let layout = status_layout(&c);
        assert!(!plain(&layout).contains("split"));
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
            side_by_side: false,
            expanded_mods_count: 0,
            total_mods: 0,
            col_offset: 0,
            num_buffer: "",
            common_prefix: None,
            width,
            is_streaming: false,
            streaming_dot: "●",
            status_message: None,
            file: None,
            hunk_counts: None,
            theme: crate::render::DiffuseTheme::dark(),
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
    fn test_layout_empty_state_waits_for_the_stream() {
        let mut c = empty_ctx(80);
        c.is_streaming = true;
        let layout = status_layout(&c);
        let text = plain(&layout);
        assert!(
            !text.contains("No diff to display."),
            "empty state must not preempt the stream: {text}"
        );
        assert!(
            text.contains("streaming..."),
            "the stream indicator stands in for the files: {text}"
        );
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
