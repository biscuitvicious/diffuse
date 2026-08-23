//! Diff grid row, hunk divider, and file header line formatting.

use super::header::truncate_pad;
use diffuse_core::wire::{OutputFile, OutputHunk};

/// Render a file header banner row with collapse status indicator and counts.
pub fn render_file_header(
    file: &OutputFile,
    is_collapsed: bool,
    is_selected: bool,
    is_current: bool,
    in_band: bool,
    content_width: usize,
) -> String {
    let f_counts = &file.counts;
    let indicator = if is_collapsed { "▸" } else { "▾" };
    let collapse_tag = if is_collapsed { " [collapsed]" } else { "" };
    let bar = format!(
        "─── {} {} ({}) +{} -{} ~{}{} ───",
        indicator,
        file.path,
        file.status,
        f_counts.adds,
        f_counts.dels,
        f_counts.mods,
        collapse_tag
    );
    let bar_str = truncate_pad(&bar, content_width);
    if is_selected {
        format!("\x1b[48;5;241m\x1b[1m\x1b[38;5;219m{bar_str}\x1b[0m")
    } else if in_band || is_current {
        format!("\x1b[48;5;239m\x1b[1m\x1b[38;5;219m{bar_str}\x1b[0m")
    } else {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;183m{bar_str}\x1b[0m")
    }
}

/// Render a hunk header divider with context envelope markers (`ctx N▴ M▾`).
pub fn render_hunk_divider(
    hi: usize,
    hunk: &OutputHunk,
    ctx: (usize, usize),
    is_current: bool,
    content_width: usize,
) -> String {
    let (above, below) = ctx;
    let (old_start, old_count) = if hunk.old_start == 0 && hunk.old_count == 0 {
        (0, 0)
    } else {
        (
            hunk.old_start.saturating_sub(above as u32).max(1),
            hunk.old_count + (above + below) as u32,
        )
    };
    let (new_start, new_count) = if hunk.new_start == 0 && hunk.new_count == 0 {
        (0, 0)
    } else {
        (
            hunk.new_start.saturating_sub(above as u32).max(1),
            hunk.new_count + (above + below) as u32,
        )
    };
    let span = format!("−{old_start},{old_count} +{new_start},{new_count}");
    let txt = format!(
        "───── H{} · {span} · ctx {}▴ {}▾ ─────",
        hi + 1,
        above,
        below
    );
    let txt_str = truncate_pad(&txt, content_width);
    if is_current {
        format!("\x1b[48;5;237m\x1b[1m\x1b[38;5;220m{txt_str}\x1b[0m")
    } else {
        format!("\x1b[1m\x1b[38;5;214m{txt_str}\x1b[0m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diffuse_core::{
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
        let regular = render_hunk_divider(0, &h, (3, 3), false, 80);
        assert!(
            regular.contains("38;5;214m"),
            "regular hunk divider must use orange 214 foreground"
        );

        let current = render_hunk_divider(0, &h, (3, 3), true, 80);
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
        let h = OutputHunk {
            old_start: 85,
            old_count: 3,
            new_start: 86,
            new_count: 4,
            rows: vec![],
        };
        // Zero context (e.g. raw diff)
        let raw = render_hunk_divider(1, &h, (0, 0), false, 80);
        assert!(raw.contains("H2 · −85,3 +86,4 · ctx 0▴ 0▾"));

        // Normal 6/6 context
        let expanded = render_hunk_divider(1, &h, (6, 6), false, 80);
        assert!(expanded.contains("H2 · −79,15 +80,16 · ctx 6▴ 6▾"));

        // Near top of file (above = 1, below = 5)
        let top_hunk = OutputHunk {
            old_start: 1,
            old_count: 3,
            new_start: 1,
            new_count: 4,
            rows: vec![],
        };
        let top_expanded = render_hunk_divider(0, &top_hunk, (1, 5), false, 80);
        assert!(top_expanded.contains("H1 · −1,9 +1,10 · ctx 1▴ 5▾"));

        // New file (old_start = 0, old_count = 0)
        let new_file_hunk = OutputHunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 20,
            rows: vec![],
        };
        let new_file = render_hunk_divider(0, &new_file_hunk, (0, 0), false, 80);
        assert!(new_file.contains("H1 · −0,0 +1,20 · ctx 0▴ 0▾"));
    }
}
