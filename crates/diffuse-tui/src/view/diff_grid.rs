//! Diff grid row, hunk divider, and file header line formatting.

use super::header::truncate_pad;
use crate::{
    core::{
        parse::RowKind,
        wire::{OutputFile, OutputHunk},
    },
    render::slice_by_visual_width,
};

/// Char column of the collapse/expand triangle in [`file_header_text`].
pub const FILE_HEADER_INDICATOR_COL: usize = 12;

/// Build the raw text of a file header banner row.
pub fn file_header_text(file: &OutputFile, is_collapsed: bool) -> String {
    let f_counts = &file.counts;
    let indicator = if is_collapsed { "▶" } else { "▼" };
    let collapse_tag = if is_collapsed { " [collapsed]" } else { "" };
    format!(
        "──────────── {} {} ({}) -{} +{} ~{}{} ────────────",
        indicator,
        file.path,
        file.status,
        f_counts.dels,
        f_counts.adds,
        f_counts.mods,
        collapse_tag
    )
}

/// Render a specific visual segment of a file header banner row.
pub fn render_file_header_segment(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    let bar = file_header_text(file, is_collapsed);
    let piece = slice_by_visual_width(&bar, seg_start, content_width);
    let bar_str = truncate_pad(&piece, content_width);
    if is_selected {
        format!("\x1b[48;5;241m\x1b[1m\x1b[38;5;219m{bar_str}\x1b[0m")
    } else if in_band || is_current {
        format!("\x1b[48;5;239m\x1b[1m\x1b[38;5;219m{bar_str}\x1b[0m")
    } else {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;183m{bar_str}\x1b[0m")
    }
}

/// Render a file header banner row with collapse status indicator and counts.
pub fn render_file_header(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
) -> String {
    render_file_header_segment(
        file,
        is_collapsed,
        is_selected,
        is_current,
        in_band,
        0,
        content_width,
    )
}

/// Build the raw text of a hunk header divider with format
/// `{rows} lines (-dels +adds ~mods)` plus the hidden-gap tail.
pub fn hunk_divider_text(hi: usize, hunk: &OutputHunk, hidden_gap: usize) -> String {
    let mut dels: usize = 0;
    let mut adds: usize = 0;
    let mut mods: usize = 0;
    for r in &hunk.rows {
        match r.kind {
            RowKind::Del => dels += 1,
            RowKind::Add => adds += 1,
            RowKind::Mod => mods += 1,
            _ => {}
        }
    }
    let total = hunk.rows.len();
    let unit = if total == 1 { "line" } else { "lines" };
    let summary = format!("{total} {unit} (-{dels} +{adds} ~{mods})");

    let gap_part = if hidden_gap > 0 {
        let arrows = if hi == 0 {
            "▼▼"
        } else {
            "▲▼ ▲▲ ▼▼"
        };
        format!(" · {hidden_gap} hidden {arrows}")
    } else {
        String::new()
    };
    format!("───────── H{} · {summary}{gap_part} ─────────", hi + 1)
}

/// Render a specific visual segment of a hunk header divider.
pub fn render_hunk_divider_segment(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    let txt = hunk_divider_text(hi, hunk, hidden_gap);
    let piece = slice_by_visual_width(&txt, seg_start, content_width);
    let txt_str = truncate_pad(&piece, content_width);
    if is_current {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    } else {
        format!("\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    }
}

/// Render a hunk header divider with format `x-y → a-b`, hidden lines gap,
/// and context expansion buttons (`▲▼ ▲▲ ▼▼` for interior hunks, or `▼▼` for H1
/// at top of file).
pub fn render_hunk_divider(
    hi: usize,
    hunk: &OutputHunk,
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> String {
    render_hunk_divider_segment(hi, hunk, hidden_gap, is_current, 0, content_width)
}

/// Build the raw text of a bottom gap divider.
pub fn bottom_gap_divider_text(hidden_gap: usize) -> String {
    let unit = if hidden_gap == 1 { "line" } else { "lines" };
    format!("───────── {hidden_gap} {unit} hidden · ▲▲ ─────────")
}

/// Render a specific visual segment of a bottom gap divider.
pub fn render_bottom_gap_divider_segment(
    hidden_gap: usize,
    is_current: bool,
    seg_start: usize,
    content_width: usize,
) -> String {
    let txt = bottom_gap_divider_text(hidden_gap);
    let piece = slice_by_visual_width(&txt, seg_start, content_width);
    let txt_str = truncate_pad(&piece, content_width);
    if is_current {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    } else {
        format!("\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    }
}

/// Render a clean bottom divider for trailing hidden lines after the last hunk.
pub fn render_bottom_gap_divider(
    hidden_gap: usize,
    is_current: bool,
    content_width: usize,
) -> String {
    render_bottom_gap_divider_segment(hidden_gap, is_current, 0, content_width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        counts::FileCounts,
        wire::{OutputFile, OutputHunk},
    };

    fn dummy_file() -> OutputFile {
        OutputFile {
            path: "src/main.rs".to_string(),
            status: "modified".to_string(),
            new_line_count: 10,
            old_line_count: 10,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![],
            raw_hunks: vec![],
            counts: FileCounts {
                adds: 5,
                dels: 2,
                mods: 1,
                movs: 0,
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
        }
    }

    fn dummy_hunk() -> OutputHunk {
        OutputHunk {
            old_start: 1,
            old_count: 5,
            new_start: 1,
            new_count: 8,
            rows: vec![],
        }
    }

    #[test]
    fn test_file_header_has_background_stripe() {
        let f = dummy_file();
        let unselected = render_file_header(&f, false, false, false, false, 80);
        assert!(
            unselected.contains("\x1b[48;5;237m"),
            "unselected file header must have shade 237 background stripe"
        );
        assert!(
            unselected.contains("\x1b[38;5;183m"),
            "unselected file header must have subtle purple tint 183"
        );
        assert!(
            unselected.ends_with("\x1b[0m"),
            "unselected file header must reset style at the end"
        );

        let current_file = render_file_header(&f, false, false, true, false, 80);
        assert!(
            current_file.contains("\x1b[48;5;239m"),
            "current file header must have slightly brighter background stripe 239"
        );
        assert!(
            current_file.contains("\x1b[38;5;219m"),
            "current file header must have bright purple tint 219"
        );

        let selected = render_file_header(&f, false, true, true, false, 80);
        assert!(
            selected.contains("\x1b[48;5;241m"),
            "selected file header must have the brightest background stripe 241"
        );
        assert!(
            selected.contains("\x1b[38;5;219m"),
            "selected file header must have vivid purplish tint 219"
        );
    }

    #[test]
    fn test_hunk_divider_is_orange() {
        let h = dummy_hunk();
        let regular = render_hunk_divider(0, &h, 8, false, 80);
        assert!(
            regular.contains("38;5;220m"),
            "regular hunk divider must use golden-amber 220 foreground"
        );

        let current = render_hunk_divider(0, &h, 8, true, 80);
        assert!(
            current.contains("38;5;220m"),
            "current hunk divider must have brighter amber/orange foreground 220"
        );
        assert!(
            current.contains("48;5;237m"),
            "current hunk divider must have subtle background stripe"
        );
    }

    #[test]
    fn test_hunk_divider_span_includes_context() {
        use crate::core::{parse::RowKind, wire::OutputRow};
        let mk = |kind: RowKind| OutputRow {
            kind,
            new_line: None,
            old_line: None,
            text: String::new(),
            del_text: None,
            spans: Default::default(),
            runs: Vec::new(),
            wholesale: false,
            moved: false,
        };
        let mut h = OutputHunk {
            old_start: 85,
            old_count: 3,
            new_start: 86,
            new_count: 4,
            rows: vec![],
        };
        // Zero gap (e.g. contiguous diff)
        let contiguous = render_hunk_divider(1, &h, 0, false, 80);
        assert!(contiguous.contains("H2 · 0 lines (-0 +0 ~0) ─────"));
        assert!(!contiguous.contains("hidden"));

        // With content: 2 ctx, 1 del, 4 adds, 1 mod → 8 rows
        h.rows = vec![
            mk(RowKind::Ctx),
            mk(RowKind::Ctx),
            mk(RowKind::Del),
            mk(RowKind::Add),
            mk(RowKind::Add),
            mk(RowKind::Add),
            mk(RowKind::Add),
            mk(RowKind::Mod),
        ];
        let with_rows = render_hunk_divider(1, &h, 0, false, 80);
        assert!(with_rows.contains("H2 · 8 lines (-1 +4 ~1) ─────"));

        // Normal 8 lines hidden gap
        let expanded = render_hunk_divider(1, &h, 8, false, 80);
        assert!(expanded.contains("H2 · 8 lines (-1 +4 ~1) · 8 hidden ▲▼ ▲▲ ▼▼"));

        // Single line hidden
        let single = render_hunk_divider(1, &h, 1, false, 80);
        assert!(single.contains("· 1 hidden ▲▼ ▲▲ ▼▼"));

        // Top gap divider (50 lines hidden at top of file, only down arrow)
        let top_gap = render_hunk_divider(0, &h, 50, false, 80);
        assert!(top_gap.contains("· 50 hidden ▼▼"));
        assert!(!top_gap.contains("▲"));

        // Bottom gap divider (only up arrow)
        let bottom = render_bottom_gap_divider(80, false, 80);
        assert!(bottom.contains("───── 80 lines hidden · ▲▲ ─────"));
        assert!(!bottom.contains("▼"));
    }
}
