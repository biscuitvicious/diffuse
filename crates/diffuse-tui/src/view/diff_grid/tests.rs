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
        unselected.contains("\x1b[48;5;233m"),
        "unselected file header must have default file_header_bg_rest (233) background stripe"
    );
    assert!(
        unselected.contains("\x1b[38;5;214m"),
        "unselected file header must have default file_header_fg_rest (214)"
    );
    assert!(
        unselected.ends_with("\x1b[0m"),
        "unselected file header must reset style at the end"
    );

    let current_file = render_file_header(&f, false, false, true, false, 80);
    assert!(
        current_file.contains("\x1b[48;5;235m"),
        "current file header must have default file_header_bg_current (235)"
    );
    assert!(
        current_file.contains("\x1b[38;5;220m"),
        "current file header must have default file_header_fg_current (220)"
    );

    let selected = render_file_header(&f, false, true, true, false, 80);
    assert!(
        selected.contains("\x1b[48;5;237m"),
        "selected file header must have default file_header_bg_selected (237)"
    );
    assert!(
        selected.contains("\x1b[38;5;221m"),
        "selected file header must have default file_header_fg_selected (221)"
    );
}

#[test]
fn test_binary_file_header_has_no_expand_caret() {
    let mut f = dummy_file();
    f.is_binary = true;
    f.path = "test.png".to_string();
    let rendered = render_file_header(&f, false, false, false, false, 80);
    assert!(
        !rendered.contains('▼'),
        "binary file must not show expanded caret ▼"
    );
    assert!(
        !rendered.contains('▶'),
        "binary file must not show collapsed caret ▶"
    );
    assert!(
        rendered.contains("════ test.png"),
        "binary file header must start with flat rule without caret"
    );
}

#[test]
fn test_hunk_divider_is_orange() {
    let h = dummy_hunk();
    let regular = render_hunk_divider(0, &h, 8, false, 80);
    assert!(
        regular.contains("38;5;213m"),
        "regular hunk divider must use yellow-orange 213 foreground"
    );

    let current = render_hunk_divider(0, &h, 8, true, 80);
    assert!(
        current.contains("38;5;213m"),
        "current hunk divider must have brighter yellow-orange foreground 213"
    );
    assert!(
        current.contains("48;5;235m"),
        "current hunk divider must have default hunk_divider_bg_current (235)"
    );
}

#[test]
fn test_hunk_divider_horizontal_rule_runs_to_end_of_line() {
    let h = dummy_hunk();
    let rendered = render_hunk_divider(0, &h, 0, false, 80);
    let plain = crate::render::strip_ansi(&rendered);
    assert_eq!(
        plain.chars().count(),
        80,
        "rendered hunk divider must fill width"
    );
    assert!(
        plain.ends_with("─────"),
        "hunk divider horizontal rule must run to the end of the line, got: {plain:?}"
    );
    assert!(
        !plain.ends_with(' '),
        "hunk divider must not have trailing space padding"
    );

    // Segment beyond text width fills with horizontal rule dashes
    let beyond = render_hunk_divider_segment(0, &h, 0, false, 200, 20);
    let plain_beyond = crate::render::strip_ansi(&beyond);
    assert_eq!(plain_beyond, "─".repeat(20));

    // Ratatui Line renderer: text shorter than width fills with rule
    let h_line = hunk_divider_line(0, &h, 0, false, 80);
    assert_eq!(h_line.to_string().chars().count(), 80);
    assert!(h_line.to_string().ends_with("─────"));

    // Ratatui Line renderer: text wider than width slices without trailing fill
    let h_line_narrow = hunk_divider_line(0, &h, 0, true, 10);
    assert_eq!(h_line_narrow.to_string().chars().count(), 10);

    // Also verify bottom gap divider rule runs to the end of line
    let bottom = render_bottom_gap_divider(10, false, 80);
    let plain_bottom = crate::render::strip_ansi(&bottom);
    assert_eq!(
        plain_bottom.chars().count(),
        80,
        "bottom gap divider must fill width"
    );
    assert!(
        plain_bottom.ends_with("─────"),
        "bottom gap divider horizontal rule must run to the end of the line, got: {plain_bottom:?}"
    );

    // Bottom gap segment beyond text width fills with horizontal rule dashes
    let bottom_beyond = render_bottom_gap_divider_segment(10, true, 200, 20);
    let plain_bottom_beyond = crate::render::strip_ansi(&bottom_beyond);
    assert_eq!(plain_bottom_beyond, "─".repeat(20));

    // Bottom gap Ratatui Line renderer
    let b_line = bottom_gap_divider_line(10, false, 80);
    assert_eq!(b_line.to_string().chars().count(), 80);
    assert!(b_line.to_string().ends_with("─────"));

    let b_line_narrow = bottom_gap_divider_line(10, true, 10);
    assert_eq!(b_line_narrow.to_string().chars().count(), 10);
}

#[test]
fn test_hunk_divider_modified_lines_excludes_context() {
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
    assert!(contiguous.contains("H2 · 0 lines (-0 +0 ~0) ────"));
    assert!(!contiguous.contains("hidden"));

    // With content: 2 ctx, 1 del, 4 adds, 1 mod → 6 modified lines (2 context
    // lines excluded)
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
    assert!(with_rows.contains("H2 · 6 lines (-1 +4 ~1) ────"));

    // Normal 8 lines hidden gap
    let expanded = render_hunk_divider(1, &h, 8, false, 80);
    assert!(expanded.contains("H2 · 6 lines (-1 +4 ~1) · 8 more lines: ⬍ · ▲ · ▼ ────"));

    // With movs (both source and target count towards modified lines)
    h.rows.push(mk(RowKind::Mov));
    let with_movs = render_hunk_divider(1, &h, 0, false, 80);
    assert!(with_movs.contains("H2 · 7 lines (-1 +4 ~1 ≈1) ────"));

    // Single line hidden
    let single = render_hunk_divider(1, &h, 1, false, 80);
    assert!(single.contains("· 1 more line: ⬍ · ▲ · ▼ ────"));

    // Top gap divider (50 lines hidden at top of file, only down arrow)
    let top_gap = render_hunk_divider(0, &h, 50, false, 80);
    assert!(top_gap.contains("· 50 more lines: ▼ ────"));
    assert!(!top_gap.contains("▲"));

    // Bottom gap divider (only up arrow) — 4-dash prefix and post
    let bottom = render_bottom_gap_divider(80, false, 80);
    assert!(bottom.contains("──── 80 more lines: ▲ ────"));
    assert!(!bottom.contains("▼"));
}

#[test]
fn test_hunk_divider_uses_four_dash_prefix_and_postfix() {
    let h = dummy_hunk();

    // Hunk divider text must start with exactly "──── " and end with " ────".
    let txt = hunk_divider_text(0, &h, 0);
    assert!(
        txt.starts_with("──── "),
        "hunk divider must start with '──── ' (4 dashes), got: {txt:?}"
    );
    assert!(
        txt.ends_with(" ────"),
        "hunk divider must end with ' ────' (4 dashes), got: {txt:?}"
    );

    // Bottom gap divider text must also start with "──── " and end with "
    // ────".
    let btxt = bottom_gap_divider_text(5);
    assert!(
        btxt.starts_with("──── "),
        "bottom gap divider must start with '──── ' (4 dashes), got: {btxt:?}"
    );
    assert!(
        btxt.ends_with(" ────"),
        "bottom gap divider must end with ' ────' (4 dashes), got: {btxt:?}"
    );
}

#[test]
fn test_hunk_divider_wraps_across_segments_when_wide() {
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
    let h = OutputHunk {
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 8,
        rows: (0..50).map(|_| mk(RowKind::Add)).collect(),
    };
    let txt = hunk_divider_text(1, &h, 400);
    let txt_w = unicode_width::UnicodeWidthStr::width(txt.as_str());
    assert!(
        txt_w > 30,
        "test setup: hunk divider text must be wider than 30, got {txt_w}"
    );

    // Segment 0 contains the first 30 visual columns
    let seg0 = render_hunk_divider_segment(1, &h, 400, false, 0, 30);
    assert!(seg0.contains("──── H2 ·"));

    // Segment 1 contains the wrapped continuation
    let seg1 = render_hunk_divider_segment(1, &h, 400, false, 30, 30);
    assert!(!seg1.is_empty());
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
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
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
    assert!(h_line.to_string().contains("10 more lines:"));

    let b_line = bottom_gap_divider_line(5, false, 80);
    assert!(b_line.to_string().contains("5 more lines:"));
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
        old_bytes: None,
        new_bytes: None,
        raw_source: None,
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
        old_bytes: Some(100),
        new_bytes: Some(100),
        raw_source: None,
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

#[test]
fn test_file_header_text_identical_formatted_size() {
    let mut f = dummy_file();
    f.old_bytes = Some(15110);
    f.new_bytes = Some(15115);
    let banner = file_header_text(&f, false);
    assert!(
        banner.contains("(14.8kB)"),
        "banner must contain '(14.8kB)', got: {banner}"
    );
    assert!(
        !banner.contains('➞') && !banner.contains("->"),
        "banner must NOT contain an arrow when formatted sizes match, got: {banner}"
    );
}

#[test]
fn test_wrapped_glyph_hit_covers_exactly_three_screen_cells() {
    // Glyph at char index 4 with spaces on both sides.
    let txt = "abc ▲ de";
    let cw = 20;
    assert!(wrapped_glyph_hit(txt, "▲", 0, 3, cw));
    assert!(wrapped_glyph_hit(txt, "▲", 0, 4, cw));
    assert!(wrapped_glyph_hit(txt, "▲", 0, 5, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 0, 2, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 0, 6, cw));
    assert!(!wrapped_glyph_hit(txt, "▼", 0, 4, cw));
}

#[test]
fn test_wrapped_glyph_hit_wraps_across_segments() {
    // Glyph at unwrapped index 10 with cw=10 renders at segment 1, col 0:
    // the preceding space is the last cell of segment 0.
    let txt = "012345678 ▲ x";
    let cw = 10;
    assert!(wrapped_glyph_hit(txt, "▲", 1, 0, cw));
    assert!(wrapped_glyph_hit(txt, "▲", 1, 1, cw));
    assert!(wrapped_glyph_hit(txt, "▲", 0, 9, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 0, 8, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 1, 2, cw));

    // Glyph at unwrapped index 9 with cw=10 renders at segment 0, col 9:
    // the following space is the first cell of segment 1.
    let txt = "01234567 ▲ y";
    assert!(wrapped_glyph_hit(txt, "▲", 0, 8, cw));
    assert!(wrapped_glyph_hit(txt, "▲", 0, 9, cw));
    assert!(wrapped_glyph_hit(txt, "▲", 1, 0, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 0, 7, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 1, 1, cw));
}

#[test]
fn test_wrapped_glyph_hit_never_touches_scrollbar_column() {
    // Glyph at unwrapped index 5 with cw=6 renders at (0, 5); its +1
    // neighbor wraps to (1, 0).
    let txt = "abcd ▲e";
    let cw = 6;
    assert!(wrapped_glyph_hit(txt, "▲", 0, 4, cw));
    assert!(wrapped_glyph_hit(txt, "▲", 0, 5, cw));
    assert!(wrapped_glyph_hit(txt, "▲", 1, 0, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 0, 3, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 1, 1, cw));
    // The scrollbar column itself never hits, on any segment.
    assert!(!wrapped_glyph_hit(txt, "▲", 0, cw, cw));
    assert!(!wrapped_glyph_hit(txt, "▲", 1, cw, cw));
}

#[test]
fn test_file_header_modified_status_omitted_and_other_statuses_displayed() {
    let mut f = dummy_file();
    f.status = "modified".to_string();
    let banner = file_header_text(&f, false);
    assert!(
        !banner.contains("(modified)"),
        "file header banner must omit '(modified)', got: {banner}"
    );
    assert!(
        banner.contains("════ ▼ src/main.rs -2 +5 ~1 "),
        "file header banner should directly show path and counts without '(modified)', got: {banner}"
    );

    f.status = "added".to_string();
    let banner_added = file_header_text(&f, false);
    assert!(
        banner_added.contains("(added)"),
        "file header banner must display '(added)', got: {banner_added}"
    );

    f.status = "deleted".to_string();
    let banner_del = file_header_text(&f, false);
    assert!(
        banner_del.contains("(deleted)"),
        "file header banner must display '(deleted)', got: {banner_del}"
    );

    f.status = "modified".to_string();
    f.is_binary = true;
    let banner_bin = file_header_text(&f, false);
    assert!(
        banner_bin.contains("(binary)"),
        "file header banner must display '(binary)', got: {banner_bin}"
    );
}
