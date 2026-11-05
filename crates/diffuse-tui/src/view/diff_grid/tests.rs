use super::*;
use crate::{
    core::{
        counts::FileCounts,
        wire::{OutputFile, OutputHunk},
    },
    render::{Badge, ColorExt, DiffuseTheme, width::display_width},
};

fn dummy_file() -> OutputFile {
    OutputFile {
        path: "src/main.rs".to_string(),
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
            adds: 5,
            dels: 2,
            mods: 1,
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
    }
}

fn dummy_hunk() -> OutputHunk {
    OutputHunk {
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 5,
        rows: vec![],
    }
}

#[test]
fn test_file_header_has_background_stripe() {
    let f = dummy_file();
    let unselected = render_file_header(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        false,
        false,
        false,
        80,
        &DiffuseTheme::dark(),
    );
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

    let current_file = render_file_header(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        false,
        true,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(
        current_file.contains("\x1b[48;5;235m"),
        "current file header must have default file_header_bg_current (235)"
    );
    assert!(
        current_file.contains("\x1b[38;5;220m"),
        "current file header must have default file_header_fg_current (220)"
    );

    let selected = render_file_header(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        true,
        true,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(
        selected.contains("\x1b[48;5;238m"),
        "selected file header must have default file_header_bg_selected (238)"
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
    let rendered = render_file_header(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        false,
        false,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(
        !rendered.contains('▼'),
        "binary file must not show expanded caret ▼"
    );
    assert!(
        !rendered.contains('▶'),
        "binary file must not show collapsed caret ▶"
    );
    let plain = crate::render::strip_ansi(&rendered);
    assert!(
        plain.contains("════ F1/1 · test.png −2 +5 ~1"),
        "binary file header must start with flat rule without caret: {plain:?}"
    );
    assert!(
        rendered.contains("\x1b[22m\x1b[38;5;251m (binary)"),
        "status parens must be muted white: {rendered:?}"
    );
}

#[test]
fn test_hunk_divider_is_orange() {
    let h = dummy_hunk();
    let regular = render_hunk_divider(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        8,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(
        regular.contains("38;5;213m"),
        "regular hunk divider must use yellow-orange 213 foreground"
    );

    let current = render_hunk_divider(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        8,
        true,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(
        current.contains("38;5;213m"),
        "current hunk divider must have brighter yellow-orange foreground 213"
    );
    assert!(
        current.contains("48;5;235m"),
        "current hunk divider must have default hunk_divider_bg_current (235)"
    );

    let theme = DiffuseTheme::dark();
    let selected = render_hunk_divider_segment(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        8,
        false,
        true,
        0,
        80,
        &theme,
    );
    assert!(
        selected.contains("38;2;255;155;255m"),
        "selected hunk divider must have brighter purple foreground"
    );
    assert!(
        selected.contains("48;5;238m"),
        "selected hunk divider must have hunk_divider_bg_selected 238"
    );
}

#[test]
fn test_hunk_divider_horizontal_rule_runs_to_end_of_line() {
    let h = dummy_hunk();
    let rendered = render_hunk_divider(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        0,
        false,
        80,
        &DiffuseTheme::dark(),
    );
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
    let beyond = render_hunk_divider_segment(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        0,
        false,
        false,
        200,
        20,
        &DiffuseTheme::dark(),
    );
    let plain_beyond = crate::render::strip_ansi(&beyond);
    assert_eq!(plain_beyond, "─".repeat(20));

    // Ratatui Line renderer: text shorter than width fills with rule
    let h_line = hunk_divider_line(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        0,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert_eq!(h_line.to_string().chars().count(), 80);
    assert!(h_line.to_string().ends_with("─────"));

    // Ratatui Line renderer: text wider than width slices without trailing fill
    let h_line_narrow = hunk_divider_line(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        0,
        true,
        10,
        &DiffuseTheme::dark(),
    );
    assert_eq!(h_line_narrow.to_string().chars().count(), 10);

    // Also verify bottom gap divider rule runs to the end of line
    let bottom = render_bottom_gap_divider(10, false, 80, &DiffuseTheme::dark());
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
    let bottom_beyond =
        render_bottom_gap_divider_segment(10, true, false, 200, 20, &DiffuseTheme::dark());
    let plain_bottom_beyond = crate::render::strip_ansi(&bottom_beyond);
    assert_eq!(plain_bottom_beyond, "─".repeat(20));

    // Bottom gap Ratatui Line renderer
    let b_line = bottom_gap_divider_line(10, false, false, 80, &DiffuseTheme::dark());
    assert_eq!(b_line.to_string().chars().count(), 80);
    assert!(b_line.to_string().ends_with("─────"));

    let b_line_narrow = bottom_gap_divider_line(10, true, false, 10, &DiffuseTheme::dark());
    assert_eq!(b_line_narrow.to_string().chars().count(), 10);

    let theme = DiffuseTheme::dark();
    let selected_bottom = render_bottom_gap_divider_segment(10, true, true, 0, 80, &theme);
    let divider = theme.palette[crate::render::Cursor::Row].hunk_divider;
    let (sel_bg, sel_fg) = (divider.bg, divider.fg);
    assert!(
        selected_bottom.contains(&sel_bg.bg_code()),
        "selected bottom gap divider must contain selected background color"
    );
    assert!(
        selected_bottom.contains(&sel_fg.fg_code()),
        "selected bottom gap divider must contain selected foreground color"
    );

    let selected_line = bottom_gap_divider_line(10, true, true, 80, &theme);
    assert_eq!(selected_line.to_string().chars().count(), 80);
}

#[test]
fn test_hunk_divider_modified_lines_excludes_context() {
    use crate::core::{parse::RowKind, wire::OutputRow};
    let mk = |kind: RowKind| OutputRow {
        kind,
        mov_side: None,
        new_line: None,
        old_line: None,
        runs: Vec::new(),
        wholesale: false,
        group: None,
    };
    let mut h = OutputHunk {
        old_start: 85,
        old_count: 3,
        new_start: 86,
        new_count: 4,
        rows: vec![],
    };
    // Zero gap (e.g. contiguous diff)
    let contiguous = render_hunk_divider(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        0,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(contiguous.contains("H2/2 ────"));
    assert!(!contiguous.contains("0 lines"));
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
    let with_rows = render_hunk_divider(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        0,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(with_rows.contains("H2/2 · 6 lines: −1 +4 ~1 ────"));

    // Normal 8 lines hidden gap
    let expanded = render_hunk_divider(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        8,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(expanded.contains("H2/2 · 6 lines: −1 +4 ~1 · 8 more lines: ⬍ · ▲ · ▼ ────"));

    // Zero-change hunk (e.g. synthetic note hunk) -> count badge is omitted
    let zero_hunk = OutputHunk {
        old_start: 10,
        old_count: 5,
        new_start: 10,
        new_count: 5,
        rows: vec![mk(RowKind::Ctx)],
    };
    let zero_div = render_hunk_divider(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &zero_hunk,
        0,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(zero_div.contains("H1/1 ────"));
    assert!(!zero_div.contains("0 lines"));

    // Zero-change hunk with hidden gap -> shows gap without count badge
    let zero_gap_div = render_hunk_divider(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &zero_hunk,
        30,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(zero_gap_div.contains("H1/1 · 30 more lines: ▼ ────"));
    assert!(!zero_gap_div.contains("0 lines"));

    // With movs (both source and target count towards modified lines)
    h.rows.push(mk(RowKind::Mov));
    let with_movs = render_hunk_divider(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        0,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(with_movs.contains("H2/2 · 7 lines: −1 +4 ~1 ≈1 ────"));

    // Single line hidden
    let single = render_hunk_divider(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        1,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(single.contains("· 1 more line: ⬍ · ▲ · ▼ ────"));

    // Top gap divider (50 lines hidden at top of file, only down arrow)
    let top_gap = render_hunk_divider(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        50,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(top_gap.contains("· 50 more lines: ▼ ────"));
    assert!(!top_gap.contains("▲"));

    // Bottom gap divider (only up arrow) — 4-dash prefix and post
    let bottom = render_bottom_gap_divider(80, false, 80, &DiffuseTheme::dark());
    assert!(bottom.contains("──── 80 more lines: ▲ ────"));
    assert!(!bottom.contains("▼"));
}

#[test]
fn test_hunk_divider_uses_four_dash_prefix_and_postfix() {
    let h = dummy_hunk();

    // Hunk divider text must start with exactly "──── " and end with " ────".
    let txt = hunk_divider_text(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &h,
        0,
    );
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
        runs: Vec::new(),
        wholesale: false,
        group: None,
    };
    let h = OutputHunk {
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 8,
        rows: (0..50).map(|_| mk(RowKind::Add)).collect(),
    };
    let txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        400,
    );
    let txt_w = display_width(txt);
    assert!(
        txt_w > 30,
        "test setup: hunk divider text must be wider than 30, got {txt_w}"
    );

    // Segment 0 contains the first 30 visual columns
    let seg0 = render_hunk_divider_segment(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        400,
        false,
        false,
        0,
        30,
        &DiffuseTheme::dark(),
    );
    assert!(seg0.contains("──── H2/2 ·"));

    // Segment 1 contains the wrapped continuation
    let seg1 = render_hunk_divider_segment(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        400,
        false,
        false,
        30,
        30,
        &DiffuseTheme::dark(),
    );
    assert!(!seg1.is_empty());
}

#[test]
fn test_ratatui_diff_grid_lines() {
    let file = OutputFile {
        path: "src/main.rs".to_string(),
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

    let f_line = file_header_line(
        &file,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        false,
        false,
        false,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(f_line.to_string().contains("src/main.rs"));
    assert!(f_line.to_string().contains("▼"));

    let hunk = OutputHunk {
        old_start: 1,
        old_count: 5,
        new_start: 1,
        new_count: 6,
        rows: vec![],
    };
    let h_line = hunk_divider_line(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &hunk,
        10,
        true,
        80,
        &DiffuseTheme::dark(),
    );
    assert!(h_line.to_string().contains("H1"));
    assert!(h_line.to_string().contains("10 more lines:"));

    let b_line = bottom_gap_divider_line(5, false, false, 80, &DiffuseTheme::dark());
    assert!(b_line.to_string().contains("5 more lines:"));
}

#[test]
fn test_render_file_header_segment_narrow_width() {
    let file = OutputFile {
        path: "crates/diffuse-tui/src/view/modals/mod.rs".to_string(),
        old_path: Some("crates/diffuse-tui/src/view/modals.rs".to_string()),
        old_mode: None,
        new_mode: None,
        status: diffuse_core::FileStatus::Renamed,
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

    let rendered_seg0 = render_file_header_segment(
        &file,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        false,
        false,
        false,
        0,
        55,
        &DiffuseTheme::dark(),
        None,
    );
    let vw0 = crate::view::header::visible_width(&rendered_seg0);
    assert_eq!(
        vw0, 55,
        "file header segment 0 visual width must be exactly 55 on 55-col viewport"
    );
    assert!(
        rendered_seg0.contains("crates/diffuse-tui/src/view/")
            && rendered_seg0.contains("modals.rs"),
        "rendered banner segment 0 must preserve the un-truncated directory prefix and from path"
    );

    let rendered_seg1 = render_file_header_segment(
        &file,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        false,
        false,
        false,
        55,
        55,
        &DiffuseTheme::dark(),
        None,
    );
    let vw1 = crate::view::header::visible_width(&rendered_seg1);
    assert_eq!(
        vw1, 55,
        "file header segment 1 visual width must be exactly 55 on 55-col viewport"
    );
    assert!(
        rendered_seg1.contains("mod.rs"),
        "rendered banner segment 1 must contain the wrapped rename target"
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
        status: diffuse_core::FileStatus::Renamed,
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

    let theme = DiffuseTheme::dark();
    let common_prefix = Some("crates/");

    // Total plain text:
    // "──── ▶ diffuse-core/src/algo/mod.rs -> algo.rs (100B) -0 +0 ~0
    // [collapsed] "
    let full_text = file_header_text_with_prefix(
        &file,
        Badge {
            idx: 0,
            total: Some(1),
        },
        true,
        common_prefix,
    );
    assert!(
        full_text.contains("diffuse-core/src/algo/mod.rs -> algo.rs"),
        "full text must keep the full un-mangled rename path without initial ellipsis: {full_text}"
    );

    // Render segment 0 at width 45
    let seg0 = render_file_header_segment(
        &file,
        Badge {
            idx: 0,
            total: Some(1),
        },
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
    let seg1 = render_file_header_segment(
        &file,
        Badge {
            idx: 0,
            total: Some(1),
        },
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
        seg1.contains("(renamed, 100B)"),
        "segment 1 must contain the wrapped suffix: {seg1:?}"
    );
}

#[test]
fn test_file_header_text_identical_formatted_size() {
    let mut f = dummy_file();
    f.old_bytes = Some(15110);
    f.new_bytes = Some(15115);
    let banner = file_header_text(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
    );
    assert!(
        banner.contains("(15kB)"),
        "banner must contain '(15kB)', got: {banner}"
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
    f.status = diffuse_core::FileStatus::Modified;
    let banner = file_header_text(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
    );
    assert!(
        !banner.contains("(modified)"),
        "file header banner must omit '(modified)', got: {banner}"
    );
    assert!(
        banner.contains("════ ▼ F1/1 · src/main.rs −2 +5 ~1"),
        "file header banner should directly show path and counts without '(modified)', got: {banner}"
    );

    f.status = diffuse_core::FileStatus::Added;
    let banner_added = file_header_text(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
    );
    assert!(
        banner_added.contains("(added)"),
        "file header banner must display '(added)', got: {banner_added}"
    );

    f.status = diffuse_core::FileStatus::Deleted;
    let banner_del = file_header_text(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
    );
    assert!(
        banner_del.contains("(deleted)"),
        "file header banner must display '(deleted)', got: {banner_del}"
    );

    f.status = diffuse_core::FileStatus::Modified;
    f.is_binary = true;
    let banner_bin = file_header_text(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
    );
    assert!(
        banner_bin.contains("(binary)"),
        "file header banner must display '(binary)', got: {banner_bin}"
    );
}

#[test]
fn test_divider_seg_count_drops_bar_only_trailing_segment() {
    // 16 columns of text: at width 14–15 the leftover is only trailing
    // dashes, so no segment just for the bar.
    assert_eq!(divider_seg_count("──── H1 · x ────", 14), 1);
    assert_eq!(divider_seg_count("──── H1 · x ────", 15), 1);
    // Exact fit stays one segment.
    let txt = "──── H1 · x ────";
    assert_eq!(divider_seg_count(txt, display_width(txt)), 1);
}

#[test]
fn test_divider_seg_count_keeps_salient_trailing_segment() {
    // Leftover holds an arrow glyph: the wrap stays.
    let txt = "──── H1 · 3 more lines: ▼ ────";
    let w = display_width(txt) - 6;
    assert_eq!(divider_seg_count(txt, w), 2);
    // Narrow widths still wrap mid-content.
    assert_eq!(divider_seg_count(txt, 10), 3);
}

#[test]
fn test_divider_seg_count_never_below_one() {
    assert_eq!(divider_seg_count("────", 1), 1);
    assert_eq!(divider_seg_count("", 80), 1);
}

#[test]
fn test_file_header_counts_colored_like_status_bar() {
    use crate::render::DiffuseTheme;

    let mut f = dummy_file();
    f.counts.dels = 2;
    f.counts.adds = 5;
    f.counts.mods = 1;
    let theme = DiffuseTheme::dark();
    let seg = render_file_header_segment(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        false,
        false,
        false,
        0,
        80,
        &theme,
        None,
    );
    // Same red/green/orange count colors as the status bar counts.
    assert!(
        seg.contains("\x1b[38;5;203m−2"),
        "deletion count must be red: {seg:?}"
    );
    assert!(
        seg.contains("\x1b[38;5;120m+5"),
        "addition count must be green: {seg:?}"
    );
    assert!(
        seg.contains("\x1b[38;5;214m~1"),
        "mod count must be orange: {seg:?}"
    );
    // Filename renders in status-bar white.
    assert!(
        seg.contains("\x1b[22m\x1b[38;5;251msrc/main.rs"),
        "rest-state filename must be muted: {seg:?}"
    );
    // Cursor states brighten the path: selected brightest, current middle.
    let seg_cur = render_file_header_segment(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        false,
        true,
        false,
        0,
        80,
        &theme,
        None,
    );
    assert!(
        seg_cur.contains("\x1b[22m\x1b[38;5;254msrc/main.rs"),
        "current-state filename must keep mid white: {seg_cur:?}"
    );
    let seg_sel = render_file_header_segment(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
        true,
        false,
        false,
        0,
        80,
        &theme,
        None,
    );
    assert!(
        seg_sel.contains("\x1b[22m\x1b[38;5;255msrc/main.rs"),
        "selected-state filename must be brightest: {seg_sel:?}"
    );
    // Counts follow the path, before the status parens.
    let plain = crate::render::strip_ansi(&seg);
    let counts_at = plain.find('−').expect("counts present");
    let path_at = plain.find("src/main.rs").expect("path present");
    assert!(path_at < counts_at, "counts must follow path: {plain:?}");
}

#[test]
fn test_file_header_zero_counts_omitted() {
    let mut f = dummy_file();
    f.counts.dels = 0;
    f.counts.adds = 0;
    f.counts.mods = 0;
    let banner = file_header_text(
        &f,
        Badge {
            idx: 0,
            total: Some(1),
        },
        false,
    );
    assert!(
        !banner.contains("~0") && !banner.contains("−0") && !banner.contains("+0"),
        "zero counts must be omitted: {banner:?}"
    );
    assert!(
        banner.contains("════ ▼ F1/1 · src/main.rs"),
        "no double space when counts omitted: {banner:?}"
    );
}
