use super::*;
use crate::core::{
    counts::FileCounts,
    wire::{OutputFile, OutputHunk},
};

fn dummy_file() -> OutputFile {
    OutputFile {
        path: "src/main.rs".to_string(),
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
            adds: 5,
            dels: 2,
            mods: 1,
            movs: 0,
        },
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        bytes: 0,
        old_bytes: 0,
        new_bytes: 0,
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
        mov_side: None,
        new_line: None,
        old_line: None,
        text: String::new(),
        del_text: None,
        spans: Default::default(),
        runs: Vec::new(),
        wholesale: false,
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

#[test]
fn test_ratatui_diff_grid_lines() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
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
        counts: Default::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        bytes: 0,
        old_bytes: 0,
        new_bytes: 0,
    };

    let f_line = file_header_line(&file, false, false, false, false, 80);
    assert!(f_line.to_string().contains("src/main.rs"));
    assert!(f_line.to_string().contains("▼"));

    let hunk = OutputHunk {
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 6,
        rows: vec![],
    };
    let h_line = hunk_divider_line(0, &hunk, 10, true, 80);
    assert!(h_line.to_string().contains("H1"));
    assert!(h_line.to_string().contains("10 hidden"));

    let b_line = bottom_gap_divider_line(5, false, 80);
    assert!(b_line.to_string().contains("5 lines hidden"));
}

#[test]
fn test_render_file_header_segment_narrow_width() {
    let file = OutputFile {
        path: "crates/diffuse-tui/src/view/modals/mod.rs".to_string(),
        old_path: Some("crates/diffuse-tui/src/view/modals.rs".to_string()),
        old_mode: None,
        new_mode: None,
        status: "renamed".to_string(),
        new_line_count: 10,
        old_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        hunks: vec![],
        raw_hunks: vec![],
        counts: Default::default(),
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        bytes: 0,
        old_bytes: 0,
        new_bytes: 0,
    };

    let rendered_seg0 = render_file_header_segment(&file, false, false, false, false, 0, 50);
    let vw0 = crate::view::header::visible_width(&rendered_seg0);
    assert_eq!(
        vw0, 50,
        "file header segment 0 visual width must be exactly 50 on 50-col viewport"
    );
    assert!(
        rendered_seg0.contains("crates/diffuse-tui/src/view/")
            && rendered_seg0.contains("modals.rs"),
        "rendered banner segment 0 must preserve the un-truncated directory prefix and from path"
    );

    let rendered_seg1 = render_file_header_segment(&file, false, false, false, false, 50, 50);
    let vw1 = crate::view::header::visible_width(&rendered_seg1);
    assert_eq!(
        vw1, 50,
        "file header segment 1 visual width must be exactly 50 on 50-col viewport"
    );
    assert!(
        rendered_seg1.contains("mod.rs"),
        "rendered banner segment 1 must contain the wrapped rename target"
    );
}

#[test]
fn test_shorten_path_for_width() {
    assert_eq!(
        shorten_path_for_width(
            "crates/diffuse-core/src/algo/mod.rs -> crates/diffuse-core/src/algo.rs",
            75
        ),
        "crates/diffuse-core/src/algo/mod.rs -> crates/diffuse-core/src/algo.rs"
    );
    assert_eq!(
        shorten_path_for_width(
            "crates/diffuse-core/src/algo/mod.rs -> crates/diffuse-core/src/algo.rs",
            50
        ),
        "…/src/algo/mod.rs -> …/src/algo.rs"
    );
    assert_eq!(
        shorten_path_for_width(
            "crates/diffuse-core/src/algo/mod.rs -> crates/diffuse-core/src/algo.rs",
            30
        ),
        "…/algo/mod.rs -> …/algo.rs"
    );
    assert_eq!(
        shorten_path_for_width("crates/diffuse-tui/src/view/diff_grid.rs", 25),
        "…/src/view/diff_grid.rs"
    );
}

#[test]
fn test_file_header_wrapping_preserves_common_prefix_and_underline() {
    use crate::render::DiffuseTheme;

    let file = OutputFile {
        path: "crates/diffuse-core/src/algo.rs".to_string(),
        old_path: Some("crates/diffuse-core/src/algo/mod.rs".to_string()),
        old_mode: None,
        new_mode: None,
        status: "renamed".to_string(),
        counts: FileCounts::default(),
        hunks: vec![],
        raw_hunks: vec![],
        old_line_count: 10,
        new_line_count: 10,
        new_content: vec![],
        old_content: vec![],
        fences: vec![],
        notes: vec![],
        content_runs: vec![],
        rows: vec![],
        is_binary: false,
        bytes: 100,
        old_bytes: 100,
        new_bytes: 100,
    };

    let theme = DiffuseTheme::from_env();
    let common_prefix = Some("crates/");

    // Total plain text:
    // "──── ▶ diffuse-core/src/algo/mod.rs -> algo.rs (100B) -0 +0 ~0
    // [collapsed] "
    let full_text = file_header_text_with_prefix(&file, true, common_prefix);
    assert!(
        full_text.contains("diffuse-core/src/algo/mod.rs -> algo.rs"),
        "full text must keep the full un-mangled rename path without initial ellipsis: {full_text}"
    );

    // Render segment 0 at width 45
    let seg0 = render_file_header_segment_themed(
        &file,
        true,
        false,
        false,
        false,
        0,
        45,
        &theme,
        common_prefix,
    );
    assert_eq!(
        crate::view::header::visible_width(&seg0),
        45,
        "segment 0 must be 45 visual columns"
    );
    // Underlined common prefix on segment 0 with desaturated purple 139m:
    assert!(
        seg0.contains("\x1b[4m\x1b[38;5;139mdiffuse-core/src/\x1b[24m"),
        "segment 0 must preserve underlined common prefix without initial ellipsis or shortening: {seg0:?}"
    );

    // Render segment 1 at width 45
    let seg1 = render_file_header_segment_themed(
        &file,
        true,
        false,
        false,
        false,
        45,
        45,
        &theme,
        common_prefix,
    );
    assert_eq!(
        crate::view::header::visible_width(&seg1),
        45,
        "segment 1 must be 45 visual columns"
    );
    assert!(
        seg1.contains("(100B)"),
        "segment 1 must contain the wrapped suffix: {seg1:?}"
    );
}
