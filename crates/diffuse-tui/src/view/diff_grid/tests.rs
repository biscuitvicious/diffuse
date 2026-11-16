use super::*;
use crate::{
    core::{
        counts::FileCounts,
        wire::{OutputFile, OutputHunk},
    },
    render::{Badge, DiffuseTheme, width::display_width},
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
    use ratatui::style::Color;

    let f = dummy_file();
    let badge = Badge {
        idx: 0,
        total: Some(1),
    };
    let theme = DiffuseTheme::dark();

    let unselected = file_header_line(&f, badge, false, false, false, false, 80, &theme);
    let style = unselected.spans[0].style;
    assert_eq!(
        style.bg,
        Some(Color::Indexed(233)),
        "unselected file header must have default file_header_bg_rest (233) background stripe"
    );
    assert_eq!(
        style.fg,
        Some(Color::Indexed(214)),
        "unselected file header must have default file_header_fg_rest (214)"
    );

    let current_file = file_header_line(&f, badge, false, false, true, false, 80, &theme);
    let style = current_file.spans[0].style;
    assert_eq!(
        style.bg,
        Some(Color::Indexed(235)),
        "current file header must have default file_header_bg_current (235)"
    );
    assert_eq!(
        style.fg,
        Some(Color::Indexed(220)),
        "current file header must have default file_header_fg_current (220)"
    );

    let selected = file_header_line(&f, badge, false, true, true, false, 80, &theme);
    let style = selected.spans[0].style;
    assert_eq!(
        style.bg,
        Some(Color::Indexed(238)),
        "selected file header must have default file_header_bg_selected (238)"
    );
    assert_eq!(
        style.fg,
        Some(Color::Indexed(221)),
        "selected file header must have default file_header_fg_selected (221)"
    );
}

#[test]
fn test_binary_file_header_has_no_expand_caret() {
    let mut f = dummy_file();
    f.is_binary = true;
    f.path = "test.png".to_string();
    let rendered = file_header_line(
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
    )
    .to_string();
    assert!(
        !rendered.contains('▼'),
        "binary file must not show expanded caret ▼"
    );
    assert!(
        !rendered.contains('▶'),
        "binary file must not show collapsed caret ▶"
    );
    assert!(
        rendered.contains("════ test.png 1/1 −2 +5 ~1"),
        "binary file header must start with flat rule without caret: {rendered:?}"
    );
    assert!(
        rendered.contains(" (binary)"),
        "binary file header must carry the status tag: {rendered:?}"
    );
}

#[test]
fn test_hunk_divider_is_orange() {
    use ratatui::style::Color;

    let h = dummy_hunk();
    let regular = hunk_divider_line(
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
    assert_eq!(
        regular.spans[0].style.fg,
        Some(Color::Indexed(213)),
        "regular hunk divider must use yellow-orange 213 foreground"
    );

    let current = hunk_divider_line(
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
    assert_eq!(
        current.spans[0].style.fg,
        Some(Color::Indexed(213)),
        "current hunk divider must have brighter yellow-orange foreground 213"
    );
    assert_eq!(
        current.spans[0].style.bg,
        Some(Color::Indexed(235)),
        "current hunk divider must have default hunk_divider_bg_current (235)"
    );

    let theme = DiffuseTheme::dark();
    let selected = hunk_divider_segment_line(
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
    assert_eq!(
        selected.spans[0].style.fg,
        Some(Color::Rgb(255, 155, 255)),
        "selected hunk divider must have brighter purple foreground"
    );
    assert_eq!(
        selected.spans[0].style.bg,
        Some(Color::Indexed(238)),
        "selected hunk divider must have hunk_divider_bg_selected 238"
    );
}

#[test]
fn test_hunk_divider_horizontal_rule_runs_to_end_of_line() {
    let h = dummy_hunk();
    let rendered = hunk_divider_line(
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
    let plain = rendered.to_string();
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
    let beyond = hunk_divider_segment_line(
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
    let plain_beyond = beyond.to_string();
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

    // Bottom gap segment beyond text width fills with horizontal rule dashes
    let bottom_beyond =
        bottom_gap_divider_segment_line(10, true, false, 200, 20, &DiffuseTheme::dark());
    let plain_bottom_beyond = bottom_beyond.to_string();
    assert_eq!(plain_bottom_beyond, "─".repeat(20));

    // Bottom gap Ratatui Line renderer
    let b_line = bottom_gap_divider_line(10, false, false, 80, &DiffuseTheme::dark());
    assert_eq!(b_line.to_string().chars().count(), 80);
    assert!(b_line.to_string().ends_with("─────"));

    let b_line_narrow = bottom_gap_divider_line(10, true, false, 10, &DiffuseTheme::dark());
    assert_eq!(b_line_narrow.to_string().chars().count(), 10);

    let theme = DiffuseTheme::dark();
    let selected_bottom = bottom_gap_divider_segment_line(10, true, true, 0, 80, &theme);
    let divider = theme.palette[crate::render::Cursor::Row].hunk_divider;
    let (sel_bg, sel_fg) = (divider.bg, divider.fg);
    assert_eq!(
        selected_bottom.spans[0].style.bg,
        Some(crate::view::to_color(sel_bg)),
        "selected bottom gap divider must contain selected background color"
    );
    assert_eq!(
        selected_bottom.spans[0].style.fg,
        Some(crate::view::to_color(sel_fg)),
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
    let contiguous = hunk_divider_line(
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
    assert!(contiguous.to_string().contains("2/2 ────"));
    assert!(!contiguous.to_string().contains("0 lines"));
    assert!(!contiguous.to_string().contains("hidden"));

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
    let theme = DiffuseTheme::dark();
    let with_rows = hunk_divider_line(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        0,
        false,
        80,
        &theme,
    );
    assert!(with_rows.to_string().contains("2/2 −1 +4 ~1 ────"));
    assert!(!with_rows.to_string().contains("lines:"));

    let find_span_color =
        |line: &ratatui::text::Line, text: &str| -> Option<ratatui::style::Color> {
            line.spans
                .iter()
                .find(|s| s.content == text)
                .and_then(|s| s.style.fg)
        };
    let ui = &theme.palette.ui;
    assert_eq!(
        find_span_color(&with_rows, "−1"),
        Some(crate::view::to_color(ui.count_dels))
    );
    assert_eq!(
        find_span_color(&with_rows, "+4"),
        Some(crate::view::to_color(ui.count_adds))
    );
    assert_eq!(
        find_span_color(&with_rows, "~1"),
        Some(crate::view::to_color(ui.count_mods))
    );

    // Normal 8 lines hidden gap
    let expanded = hunk_divider_line(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        8,
        false,
        80,
        &theme,
    );
    assert!(
        expanded
            .to_string()
            .contains("2/2 −1 +4 ~1 · 8 more lines ────")
    );
    assert!(!expanded.to_string().contains("lines:"));

    // Zero-change hunk (e.g. synthetic note hunk) -> count badge is omitted
    let zero_hunk = OutputHunk {
        old_start: 10,
        old_count: 5,
        new_start: 10,
        new_count: 5,
        rows: vec![mk(RowKind::Ctx)],
    };
    let zero_div = hunk_divider_line(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &zero_hunk,
        0,
        false,
        80,
        &theme,
    );
    assert!(zero_div.to_string().contains("1/1 ────"));
    assert!(!zero_div.to_string().contains("0 lines"));

    // Zero-change hunk with hidden gap -> shows gap without count badge
    let zero_gap_div = hunk_divider_line(
        Badge {
            idx: 0,
            total: Some(1),
        },
        &zero_hunk,
        30,
        false,
        80,
        &theme,
    );
    assert!(
        zero_gap_div
            .to_string()
            .contains("1/1 · 30 more lines ────")
    );
    assert!(!zero_gap_div.to_string().contains("0 lines"));

    // With movs (both source and target count towards modified lines)
    h.rows.push(mk(RowKind::Mov));
    let with_movs = hunk_divider_line(
        Badge {
            idx: 1,
            total: Some(2),
        },
        &h,
        0,
        false,
        80,
        &theme,
    );
    assert!(with_movs.to_string().contains("2/2 −1 +4 ~1 ≈1 ────"));
    assert!(!with_movs.to_string().contains("lines:"));
    assert_eq!(
        find_span_color(&with_movs, "≈1"),
        Some(crate::view::to_color(ui.count_movs))
    );

    // Single line hidden
    let single = hunk_divider_line(
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
    assert!(single.to_string().contains("· 1 more line ────"));

    // Top gap divider (50 lines hidden at top of file, no icons)
    let top_gap = hunk_divider_line(
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
    assert!(top_gap.to_string().contains("· 50 more lines ────"));
    assert!(!top_gap.to_string().contains("▲"));

    // Bottom gap divider — 4-dash prefix and post
    let bottom = bottom_gap_divider_text(80);
    assert_eq!(bottom, "──── 80 more lines ────");
    assert!(!bottom.contains('▲'));
    assert!(!bottom.contains('▼'));
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
    let seg0 = hunk_divider_segment_line(
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
    assert!(seg0.to_string().contains("──── 2/2 +50 ·"));

    // Segment 1 contains the wrapped continuation
    let seg1 = hunk_divider_segment_line(
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
    assert!(seg1.width() > 0);
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
    assert!(h_line.to_string().contains("1/1"));
    assert!(h_line.to_string().contains("10 more lines"));

    let b_line = bottom_gap_divider_line(5, false, false, 80, &DiffuseTheme::dark());
    assert!(b_line.to_string().contains("5 more lines"));
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

    let rendered_seg0 = file_header_segment_line(
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
    let vw0 = rendered_seg0.width();
    assert_eq!(
        vw0, 55,
        "file header segment 0 visual width must be exactly 55 on 55-col viewport"
    );
    assert!(
        rendered_seg0
            .to_string()
            .contains("crates/diffuse-tui/src/view/")
            && rendered_seg0.to_string().contains("modals.rs"),
        "rendered banner segment 0 must preserve the un-truncated directory prefix and from path"
    );

    let rendered_seg1 = file_header_segment_line(
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
    let vw1 = rendered_seg1.width();
    assert_eq!(
        vw1, 55,
        "file header segment 1 visual width must be exactly 55 on 55-col viewport"
    );
    assert!(
        rendered_seg1.to_string().contains("mod.rs"),
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
    let seg0 = file_header_segment_line(
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
    assert_eq!(seg0.width(), 45, "segment 0 must be 45 visual columns");
    // Underlined common prefix on segment 0 with desaturated purple 139:
    let underlined = seg0
        .spans
        .iter()
        .find(|s| s.content.contains("diffuse-core/src/"))
        .expect("underlined common prefix span");
    assert!(
        underlined
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::UNDERLINED),
        "segment 0 must preserve underlined common prefix without initial ellipsis or shortening: {seg0:?}"
    );
    assert_eq!(
        underlined.style.fg,
        Some(ratatui::style::Color::Indexed(139)),
        "underlined common prefix keeps desaturated purple 139"
    );

    // Render segment 1 at width 45
    let seg1 = file_header_segment_line(
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
    assert_eq!(seg1.width(), 45, "segment 1 must be 45 visual columns");
    assert!(
        seg1.to_string().contains("(renamed, 100B)"),
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
        banner.contains("════ ▼ src/main.rs 1/1 −2 +5 ~1"),
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
    // Leftover holds text content: the wrap stays.
    let txt = "──── 1/1 · 3 more lines ────";
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
    let seg = file_header_segment_line(
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
    let has_colored = |text: &str, fg: ratatui::style::Color| {
        seg.spans
            .iter()
            .any(|s| s.content.contains(text) && s.style.fg == Some(fg))
    };
    assert!(
        has_colored("−2", ratatui::style::Color::Indexed(203)),
        "deletion count must be red: {seg:?}"
    );
    assert!(
        has_colored("+5", ratatui::style::Color::Indexed(120)),
        "addition count must be green: {seg:?}"
    );
    assert!(
        has_colored("~1", ratatui::style::Color::Indexed(214)),
        "mod count must be orange: {seg:?}"
    );
    // Filename renders in status-bar white.
    assert!(
        has_colored("src/main.rs", ratatui::style::Color::Indexed(251)),
        "rest-state filename must be muted: {seg:?}"
    );
    // Cursor states brighten the path: selected brightest, current middle.
    let seg_cur = file_header_segment_line(
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
        seg_cur
            .spans
            .iter()
            .any(|s| s.content.contains("src/main.rs")
                && s.style.fg == Some(ratatui::style::Color::Indexed(254))),
        "current-state filename must keep mid white: {seg_cur:?}"
    );
    let seg_sel = file_header_segment_line(
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
        seg_sel
            .spans
            .iter()
            .any(|s| s.content.contains("src/main.rs")
                && s.style.fg == Some(ratatui::style::Color::Indexed(255))),
        "selected-state filename must be brightest: {seg_sel:?}"
    );
    // Counts follow the path, before the status parens.
    let plain = seg.to_string();
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
        banner.contains("════ ▼ src/main.rs 1/1"),
        "no double space when counts omitted: {banner:?}"
    );
}
