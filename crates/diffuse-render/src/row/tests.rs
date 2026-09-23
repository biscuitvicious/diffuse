use super::*;
use crate::core::{merge::RunKind, wire::OutputRun};

#[test]
fn test_prefix_gutters() {
    let r_add = prefix_of(RowKind::Add, None, Some(42), None, &[]);
    assert_eq!(r_add, "  42 +");

    let r_del = prefix_of(RowKind::Del, None, None, Some(10), &[]);
    assert_eq!(r_del, "  10 -");

    let r_mod = prefix_of(RowKind::Mod, None, Some(55), Some(54), &[]);
    assert_eq!(r_mod, "  55 ~");
}

#[test]
fn test_addition_row_with_all_common_runs_preserves_plus_gutter() {
    let runs = vec![OutputRun {
        kind: RunKind::Common,
        color: None,
        sgr: None,
        text: "    syntax::{".into(),
    }];
    let r_add = prefix_of(RowKind::Add, None, Some(53), None, &runs);
    assert_eq!(
        r_add, "  53 +",
        "Leading add row must keep '+' in gutter even if runs are common"
    );
}

#[test]
fn test_render_addition_row_with_subword_styling() {
    let theme = DiffuseTheme::DEFAULT;
    let runs = vec![
        OutputRun {
            kind: RunKind::Common,
            color: Some("syntaxKeyword".into()),
            sgr: None,
            text: "pub fn ".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: Some("syntaxFunction".into()),
            sgr: None,
            text: "render".into(),
        },
    ];
    let prefix = "  12 + ";
    let rendered = render_row_segment(
        "pub fn render",
        None,
        &Spans::default(),
        RowKind::Add,
        &runs,
        0,
        13,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(rendered.contains("pub fn "));
    assert!(rendered.contains("render"));
    assert!(rendered.contains(prefix));
}

#[test]
fn test_grapheme_clusters_never_split() {
    let theme = DiffuseTheme::DEFAULT;
    // "e" + combining acute accent = ONE grapheme cluster; "x" the second.
    let text = "e\u{301}x";
    let runs = vec![OutputRun {
        kind: RunKind::Common,
        color: None,
        sgr: None,
        text: text.into(),
    }];
    let prefix = "   1  ";
    // seg [0,1) in grapheme units must keep the whole cluster, not split it.
    let seg = render_row_segment(
        text,
        None,
        &Spans::default(),
        RowKind::Ctx,
        &runs,
        0,
        1,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        seg.contains("e\u{301}"),
        "a grapheme cluster must survive slicing: {seg:?}"
    );
    assert!(!seg.contains('x'));

    // col_offset=1 skips one grapheme (the whole cluster), landing on "x".
    let off = render_row_segment(
        text,
        None,
        &Spans::default(),
        RowKind::Ctx,
        &runs,
        0,
        1,
        prefix,
        false,
        false,
        1,
        true,
        true,
        &theme,
    );
    assert!(
        off.contains('x'),
        "offset must land on the second grapheme: {off:?}"
    );
    assert!(!off.contains("e\u{301}"));
}

#[test]
fn test_syntax_on_toggle_disables_token_colors() {
    let theme = DiffuseTheme::DEFAULT;
    let runs = vec![OutputRun {
        kind: RunKind::Common,
        color: Some("syntaxKeyword".into()),
        sgr: Some(1),
        text: "fn".into(),
    }];
    let prefix = "   1  ";
    let on = render_row_segment(
        "fn",
        None,
        &Spans::default(),
        RowKind::Ctx,
        &runs,
        0,
        2,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        on.contains("\x1b[38;5;"),
        "syntax on must color the token: {on:?}"
    );
    let off = render_row_segment(
        "fn",
        None,
        &Spans::default(),
        RowKind::Ctx,
        &runs,
        0,
        2,
        prefix,
        false,
        false,
        0,
        true,
        false,
        &theme,
    );
    assert!(
        !off.contains("\x1b[38;5;"),
        "syntax off must leave the token plain: {off:?}"
    );
}

#[test]
fn test_col_offset_applies_to_token_runs() {
    let theme = DiffuseTheme::DEFAULT;
    let runs = vec![
        OutputRun {
            kind: RunKind::Common,
            color: Some("syntaxKeyword".into()),
            sgr: None,
            text: "pub fn ".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: Some("syntaxFunction".into()),
            sgr: None,
            text: "render".into(),
        },
    ];
    let prefix = "  12 + ";
    let rendered = render_row_segment(
        "pub fn render",
        None,
        &Spans::default(),
        RowKind::Add,
        &runs,
        0,
        13,
        prefix,
        false,
        false,
        4, // col_offset skips "pub "
        true,
        true,
        &theme,
    );
    assert!(
        !rendered.contains("pub"),
        "horizontal scroll must skip the first 4 chars, got: {rendered}"
    );
    assert!(rendered.contains("fn "));
    assert!(rendered.contains("render"));
}

#[test]
fn test_is_pure_deletion_mod_multibyte_unicode() {
    let text = "abc—def";
    let spans = Spans {
        del: vec![],
        add: vec![(0, 4)],
    };
    // Should not panic on multibyte character boundary
    let res = is_pure_deletion_mod(&spans, text);
    assert!(!res);
}

#[test]
fn test_render_raw_row_colors() {
    let add = render_raw_row(RowKind::Add, "  42 + ", "added_line", false, false, true);
    assert!(add.contains("  42 + "));
    assert!(add.contains("\x1b[38;5;120madded_line"));

    let del = render_raw_row(RowKind::Del, "  41 - ", "deleted_line", false, false, true);
    assert!(del.contains("  41 - "));
    assert!(del.contains("\x1b[38;5;203mdeleted_line"));

    let ctx = render_raw_row(RowKind::Ctx, "  40   ", "context_line", false, false, true);
    assert!(ctx.contains("  40   "));
    assert!(ctx.contains("context_line"));
}

#[test]
fn test_render_raw_row_trailing_whitespace() {
    let add_trailing = render_raw_row(RowKind::Add, "  42 + ", "let x = 1;   ", false, false, true);
    assert!(add_trailing.contains("  42 + "));
    assert!(add_trailing.contains("\x1b[38;5;120mlet x = 1;"));
    // add_fill_bg is Rgb(26,84,44) and add_gutter_num is Fixed(120)
    assert!(
        add_trailing.contains("\x1b[48;2;30;96;50m\x1b[38;5;120m   "),
        "added trailing spaces must have fill bg + gutter-num fg: {add_trailing:?}"
    );
    assert!(
        !add_trailing.contains("\x1b[9m"),
        "added trailing spaces must not be crossed out"
    );

    // When highlight_trailing is false (intermediate wrapped segment), do not
    // highlight
    let add_wrapped_intermediate = render_raw_row(
        RowKind::Add,
        "  42 + ",
        "let x = 1;   ",
        false,
        false,
        false,
    );
    assert!(
        !add_wrapped_intermediate.contains("\x1b[48;2;30;96;50m"),
        "intermediate wrapped segments must not highlight spaces at wrap boundary"
    );

    let del_trailing = render_raw_row(RowKind::Del, "  41 - ", "foo();  ", false, false, true);
    assert!(del_trailing.contains("  41 - "));
    assert!(del_trailing.contains("\x1b[38;5;203mfoo();"));
    // del_cursor_bg is Rgb(68,24,26), del_gutter_num is Fixed(203)
    assert!(
        del_trailing.contains("\x1b[48;2;68;24;26m\x1b[9m\x1b[38;5;203m  \x1b[29m"),
        "deleted trailing spaces must have subline highlight: {del_trailing:?}"
    );
}

#[test]
fn test_render_moved_row_with_indentation_subwords() {
    let theme = DiffuseTheme::DEFAULT;
    let runs = vec![
        OutputRun {
            kind: RunKind::Common,
            color: Some("text".into()),
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: Some("text".into()),
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Common,
            color: Some("syntaxFunction".into()),
            sgr: None,
            text: "foo()".into(),
        },
    ];
    let prefix = "  10≈+ ";
    let spans = Spans {
        del: vec![(0, 4)],
        add: vec![(0, 8)],
    };
    let rendered = render_row_segment(
        "        foo()",
        Some("    foo()"),
        &spans,
        RowKind::Add,
        &runs,
        0,
        usize::MAX,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    // Gutter prefix
    assert!(rendered.contains("  10≈+ "));
    // Subline addition fill on the 4 added indentation spaces
    assert!(
        rendered.contains("\x1b[48;2;30;96;50m"),
        "added indentation spaces on moved row must receive green subline fill"
    );
}

#[test]
fn test_render_mod_row_with_indentation_subwords() {
    let theme = DiffuseTheme::DEFAULT;
    let runs = vec![
        OutputRun {
            kind: RunKind::Common,
            color: Some("text".into()),
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: Some("text".into()),
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Common,
            color: Some("syntaxFunction".into()),
            sgr: None,
            text: "bar()".into(),
        },
    ];
    let prefix = "  20 ~ ";
    let spans = Spans {
        del: vec![(0, 4)],
        add: vec![(0, 8)],
    };
    let rendered = render_row_segment(
        "        bar()",
        Some("    bar()"),
        &spans,
        RowKind::Mod,
        &runs,
        0,
        usize::MAX,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(rendered.contains("  20 ~ "));
    assert!(
        rendered.contains("\x1b[48;2;30;96;50m"),
        "added indentation spaces on mod row must receive green subline fill"
    );
}

#[test]
fn test_render_moved_source_del_row() {
    let theme = DiffuseTheme::DEFAULT;
    let s_del = theme.row_style(RowKind::Del);
    let runs = vec![OutputRun {
        kind: RunKind::Del,
        color: None,
        sgr: None,
        text: "    foo()".into(),
    }];
    let prefix = "  15≈- ";
    let rendered = render_row_segment(
        "    foo()",
        None,
        &Spans::default(),
        RowKind::Del,
        &runs,
        0,
        usize::MAX,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(rendered.contains("  15≈- "));
    assert!(
        rendered.contains(&s_del.fg.fg_code()),
        "deleted source of move must have deletion foreground color"
    );
}

#[test]
fn test_write_row_segment_direct_buffer() {
    let theme = DiffuseTheme::DEFAULT;
    let runs = vec![OutputRun {
        kind: RunKind::Common,
        color: Some("syntaxKeyword".into()),
        sgr: None,
        text: "let x = 1;".into(),
    }];
    let mut buf = String::new();
    write_row_segment(
        &mut buf,
        "let x = 1;",
        None,
        &Spans::default(),
        RowKind::Add,
        &runs,
        0,
        usize::MAX,
        "+ ",
        false,
        false,
        0,
        true,
        true,
        &theme,
    )
    .unwrap();
    assert_eq!(
        buf,
        render_row_segment(
            "let x = 1;",
            None,
            &Spans::default(),
            RowKind::Add,
            &runs,
            0,
            usize::MAX,
            "+ ",
            false,
            false,
            0,
            true,
            true,
            &theme,
        )
    );
}

#[test]
fn test_expand_tabs_and_runs() {
    assert_eq!(expand_tabs("hello world", 4), "hello world");
    assert_eq!(expand_tabs("\thello", 4), "    hello");
    assert_eq!(expand_tabs("a\tb", 4), "a   b");
    assert_eq!(expand_tabs("abcd\te", 4), "abcd    e");
    assert_eq!(expand_tabs("\t\t", 4), "        ");

    let runs = vec![
        OutputRun {
            kind: RunKind::Common,
            color: None,
            sgr: None,
            text: "\t".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: None,
            sgr: None,
            text: "x\ty".into(),
        },
    ];
    let exp = expand_runs_tabs(&runs, 4);
    assert_eq!(exp[0].text, "    ");
    assert_eq!(exp[1].text, "x   y");
}

#[test]
fn test_sliced_row_segment_expands_tabs_without_overflow() {
    let theme = DiffuseTheme::DEFAULT;
    let runs = vec![OutputRun {
        kind: RunKind::Common,
        color: None,
        sgr: None,
        text: "\t\tlet x = 1;".into(),
    }];
    // Segment [0, 8) covers the 8 expanded tab spaces
    let seg1 = render_row_segment(
        "\t\tlet x = 1;",
        None,
        &Spans::default(),
        RowKind::Ctx,
        &runs,
        0,
        8,
        "      ",
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(!seg1.contains('\t'));
    assert!(seg1.contains("        "));

    // Full row preserves original tabs (golden contract invariant)
    let full = render_row_segment(
        "\t\tlet x = 1;",
        None,
        &Spans::default(),
        RowKind::Ctx,
        &runs,
        0,
        usize::MAX,
        "      ",
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(full.contains('\t'));
}

#[test]
fn test_wide_emoji_never_exceeds_segment_window() {
    use crate::core::parse::RowKind as RK;
    let theme = DiffuseTheme::DEFAULT;
    let prefix = "   1  ";
    // "ab✅cd": 5 graphemes, 6 visual columns. Window [0,4) must hold
    // "ab✅" (4 columns), never the 5-column "ab✅c".
    let del = render_row_segment(
        "ab✅cd",
        None,
        &Spans::default(),
        RK::Del,
        &[],
        0,
        4,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        !del.contains('c'),
        "del segment must not overflow its visual window: {del:?}"
    );

    let runs = vec![OutputRun {
        kind: RunKind::Common,
        color: None,
        sgr: None,
        text: "ab✅cd".into(),
    }];
    let run_seg = render_row_segment(
        "ab✅cd",
        None,
        &Spans::default(),
        RK::Ctx,
        &runs,
        0,
        4,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        !run_seg.contains('c'),
        "run segment must not overflow its visual window: {run_seg:?}"
    );

    let empty = render_row_segment(
        "ab✅cd",
        None,
        &Spans::default(),
        RK::Ctx,
        &[],
        0,
        4,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        !empty.contains('c'),
        "runless segment must not overflow its visual window: {empty:?}"
    );

    // The remainder window still shows the tail content that fits.
    let tail = render_row_segment(
        "ab✅cd",
        None,
        &Spans::default(),
        RK::Del,
        &[],
        4,
        6,
        prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        tail.contains('c') && tail.contains('d'),
        "tail window must keep fitting content: {tail:?}"
    );
}
