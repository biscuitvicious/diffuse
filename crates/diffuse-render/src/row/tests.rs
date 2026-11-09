use super::*;
use crate::{
    ansi::ColorExt,
    core::{
        merge::RunKind,
        parse::{RowGroup, RowKind, Spans},
        wire::{MovSide, OutputRun},
    },
    span::StyledSpan,
    theme::{Cursor, DiffKind, DiffuseTheme},
};
use diffuse_syntax::color::TokenColor;

/// Emit gutter prefix spans with the exact CLI spelling, mirroring what
/// `write_row_segment` does for first-segment prefixes.
#[allow(clippy::too_many_arguments)]
fn emitted_prefix(
    kind: RowKind,
    mov_side: Option<MovSide>,
    group: Option<RowGroup>,
    new_line: Option<u32>,
    old_line: Option<u32>,
    runs: &[OutputRun],
    in_hunk: bool,
    theme: &DiffuseTheme,
) -> String {
    let spans = styled_prefix_spans(
        kind, mov_side, group, new_line, old_line, runs, in_hunk, None, theme,
    );
    let mut out = String::new();
    let is_mov_src = mov_side == Some(MovSide::Src);
    let mid_italic_off = kind == RowKind::Del || is_mov_src;
    write_gutter_prefix(&mut out, &spans, mid_italic_off, true).unwrap();
    out
}

#[test]
fn test_prefix_gutters() {
    let r_add = prefix_of(RowKind::Add, None, None, Some(42), None, &[]);
    assert_eq!(r_add, "  42 +");

    let r_del = prefix_of(RowKind::Del, None, None, None, Some(10), &[]);
    assert_eq!(r_del, "  10 -");

    // Non-group 1-to-1 mod row styles with right-aligned ~ ("  55 ~")
    let r_mod = prefix_of(RowKind::Mod, None, None, Some(55), Some(54), &[]);
    assert_eq!(r_mod, "  55 ~");
}

#[test]
fn test_prefix_group_gutters() {
    // Group anchor row has single left-aligned ~ (" 141~ ")
    let r_anchor = prefix_of(
        RowKind::Mod,
        None,
        Some(RowGroup::Anchor),
        Some(141),
        Some(140),
        &[],
    );
    assert_eq!(r_anchor, " 141~ ");

    // Group aux add row has ~+
    let r_aux_add = prefix_of(
        RowKind::Add,
        None,
        Some(RowGroup::Aux { anchor: 141 }),
        Some(140),
        None,
        &[],
    );
    assert_eq!(r_aux_add, " 140~+");

    // Group aux del row has ~-
    let r_aux_del = prefix_of(
        RowKind::Del,
        None,
        Some(RowGroup::Aux { anchor: 141 }),
        None,
        Some(140),
        &[],
    );
    assert_eq!(r_aux_del, " 140~-");
}

#[test]
fn test_styled_prefix_group_and_non_group() {
    let theme = DiffuseTheme::dark();

    // Non-group mod row has right-aligned ~ ("  55 ~")
    let s_mod = emitted_prefix(
        RowKind::Mod,
        None,
        None,
        Some(55),
        Some(54),
        &[],
        true,
        &theme,
    );
    assert_eq!(crate::ansi::strip_ansi(&s_mod), "  55 ~");
    assert!(
        s_mod.contains(
            &theme.palette[Cursor::Rest][DiffKind::Mod]
                .gutter
                .icon
                .color
                .fg_code()
        )
    );

    // Group anchor row has left-aligned ~ (" 141~ ")
    let s_anchor = emitted_prefix(
        RowKind::Mod,
        None,
        Some(RowGroup::Anchor),
        Some(141),
        Some(140),
        &[],
        true,
        &theme,
    );
    assert_eq!(crate::ansi::strip_ansi(&s_anchor), " 141~ ");
    assert!(
        s_anchor.contains(
            &theme.palette[Cursor::Rest][DiffKind::Mod]
                .gutter
                .icon
                .color
                .fg_code()
        )
    );

    // Group aux add row has ~+ (" 140~+")
    let s_aux_add = emitted_prefix(
        RowKind::Add,
        None,
        Some(RowGroup::Aux { anchor: 141 }),
        Some(140),
        None,
        &[],
        true,
        &theme,
    );
    assert_eq!(crate::ansi::strip_ansi(&s_aux_add), " 140~+");
    assert!(
        s_aux_add.contains(
            &theme.palette[Cursor::Rest][DiffKind::Mod]
                .gutter
                .icon
                .color
                .fg_code()
        )
    );
    assert!(
        s_aux_add.contains(
            &theme.palette[Cursor::Rest][DiffKind::Add]
                .gutter
                .icon
                .color
                .fg_code()
        )
    );
    // Aux add numbers use standard add number color (green)
    assert!(
        s_aux_add.starts_with(
            &theme.palette[Cursor::Rest][DiffKind::Add]
                .gutter
                .num
                .color
                .fg_code()
        ),
        "aux add number must match standard add number color: {s_aux_add:?}"
    );

    // Group aux del row has ~- (" 140~-")
    let s_aux_del = emitted_prefix(
        RowKind::Del,
        None,
        Some(RowGroup::Aux { anchor: 141 }),
        None,
        Some(140),
        &[],
        true,
        &theme,
    );
    assert_eq!(crate::ansi::strip_ansi(&s_aux_del), " 140~-");
    assert!(
        s_aux_del.contains(
            &theme.palette[Cursor::Rest][DiffKind::Mod]
                .gutter
                .icon
                .color
                .fg_code()
        )
    );
    assert!(
        s_aux_del.contains(
            &theme.palette[Cursor::Rest][DiffKind::Del]
                .gutter
                .icon
                .color
                .fg_code()
        )
    );
    // Aux del numbers use standard del number color (red) in italic style
    assert!(
        s_aux_del.starts_with(
            &theme.palette[Cursor::Rest][DiffKind::Del]
                .gutter
                .num
                .color
                .italic_fg_code()
        ),
        "aux del number must use standard del number color in italic style: {s_aux_del:?}"
    );
    assert!(
        s_aux_del.contains("\x1b[23m"),
        "aux del must turn off italic after line number: {s_aux_del:?}"
    );
}

#[test]
fn test_addition_row_with_all_common_runs_preserves_plus_gutter() {
    let runs = vec![OutputRun {
        kind: RunKind::Ctx,
        color: TokenColor::Default,
        sgr: None,
        text: "    syntax::{".into(),
    }];
    let r_add = prefix_of(RowKind::Add, None, None, Some(53), None, &runs);
    assert_eq!(
        r_add, "  53 +",
        "Leading add row must keep '+' in gutter even if runs are common"
    );
}

#[test]
fn test_render_addition_row_with_subword_styling() {
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Keyword,
            sgr: None,
            text: "pub fn ".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Function,
            sgr: None,
            text: "render".into(),
        },
    ];
    let prefix = "  12 + ";
    let rendered = render_row_segment(
        "pub fn render",
        false,
        RowKind::Add,
        None,
        &runs,
        0,
        13,
        &[StyledSpan::plain(prefix)],
        false,
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
    let theme = DiffuseTheme::dark();
    // "e" + combining acute accent = ONE grapheme cluster; "x" the second.
    let text = "e\u{301}x";
    let runs = vec![OutputRun {
        kind: RunKind::Ctx,
        color: TokenColor::Default,
        sgr: None,
        text: text.into(),
    }];
    let prefix = "   1  ";
    // seg [0,1) in grapheme units must keep the whole cluster, not split it.
    let seg = render_row_segment(
        text,
        false,
        RowKind::Ctx,
        None,
        &runs,
        0,
        1,
        &[StyledSpan::plain(prefix)],
        false,
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
        false,
        RowKind::Ctx,
        None,
        &runs,
        0,
        1,
        &[StyledSpan::plain(prefix)],
        false,
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
    let theme = DiffuseTheme::dark();
    let runs = vec![OutputRun {
        kind: RunKind::Ctx,
        color: TokenColor::Keyword,
        sgr: Some(1),
        text: "fn".into(),
    }];
    let prefix = "   1  ";
    let on = render_row_segment(
        "fn",
        false,
        RowKind::Ctx,
        None,
        &runs,
        0,
        2,
        &[StyledSpan::plain(prefix)],
        false,
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
        false,
        RowKind::Ctx,
        None,
        &runs,
        0,
        2,
        &[StyledSpan::plain(prefix)],
        false,
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
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Keyword,
            sgr: None,
            text: "pub fn ".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Function,
            sgr: None,
            text: "render".into(),
        },
    ];
    let prefix = "  12 + ";
    let rendered = render_row_segment(
        "pub fn render",
        false,
        RowKind::Add,
        None,
        &runs,
        0,
        13,
        &[StyledSpan::plain(prefix)],
        false,
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
    let theme = DiffuseTheme::dark();
    let add = render_raw_row(
        RowKind::Add,
        &[StyledSpan::plain("  42 + ")],
        "added_line",
        false,
        false,
        true,
        &theme,
    );
    assert!(add.contains("  42 + "));
    assert!(add.contains("\x1b[38;5;120madded_line"));

    let del = render_raw_row(
        RowKind::Del,
        &[StyledSpan::plain("  41 - ")],
        "deleted_line",
        false,
        false,
        true,
        &theme,
    );
    assert!(del.contains("  41 - "));
    assert!(del.contains("\x1b[38;5;203mdeleted_line"));

    let ctx = render_raw_row(
        RowKind::Ctx,
        &[StyledSpan::plain("  40   ")],
        "context_line",
        false,
        false,
        true,
        &theme,
    );
    assert!(ctx.contains("  40   "));
    assert!(ctx.contains("context_line"));
}

#[test]
fn test_render_raw_row_trailing_whitespace() {
    let theme = DiffuseTheme::dark();
    let add_trailing = render_raw_row(
        RowKind::Add,
        &[StyledSpan::plain("  42 + ")],
        "let x = 1;   ",
        false,
        false,
        true,
        &theme,
    );
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
        &[StyledSpan::plain("  42 + ")],
        "let x = 1;   ",
        false,
        false,
        false,
        &theme,
    );
    assert!(
        !add_wrapped_intermediate.contains("\x1b[48;2;30;96;50m"),
        "intermediate wrapped segments must not highlight spaces at wrap boundary"
    );

    let del_trailing = render_raw_row(
        RowKind::Del,
        &[StyledSpan::plain("  41 - ")],
        "foo();  ",
        false,
        false,
        true,
        &theme,
    );
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
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Default,
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Default,
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Function,
            sgr: None,
            text: "foo()".into(),
        },
    ];
    let prefix = "  10≈+ ";
    let rendered = render_row_segment(
        "        foo()",
        true,
        RowKind::Add,
        None,
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(prefix)],
        false,
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
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Default,
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Default,
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Function,
            sgr: None,
            text: "bar()".into(),
        },
    ];
    let prefix = "  20 ~ ";
    let rendered = render_row_segment(
        "        bar()",
        false,
        RowKind::Mod,
        None,
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(prefix)],
        false,
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
    let theme = DiffuseTheme::dark();
    let s_del = theme.row_style(RowKind::Del);
    let runs = vec![OutputRun {
        kind: RunKind::Del,
        color: TokenColor::Default,
        sgr: None,
        text: "    foo()".into(),
    }];
    let prefix = "  15≈- ";
    let rendered = render_row_segment(
        "    foo()",
        false,
        RowKind::Del,
        None,
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(prefix)],
        false,
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
fn test_mov_src_renders_like_del_row_without_strikethrough() {
    let theme = DiffuseTheme::dark();
    let s_del = theme.row_style(RowKind::Del);
    let runs = vec![OutputRun {
        kind: RunKind::Del,
        color: TokenColor::Default,
        sgr: None,
        text: "    foo()".into(),
    }];
    let prefix = "  15≈- ";
    let rendered = render_row_segment(
        "    foo()",
        false,
        RowKind::Mov,
        Some(MovSide::Src),
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(prefix)],
        false,
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
        "mov src must have deletion foreground color"
    );
    assert!(
        rendered.contains(&s_del.tint_bg()),
        "mov src must have deletion tint background"
    );
    assert!(
        !rendered.contains("\x1b[9m"),
        "mov src must NOT have strikethrough styling"
    );
}

#[test]
fn test_mov_src_with_del_and_ctx_runs_renders_strikethrough_del() {
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Del,
            color: TokenColor::Default,
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Function,
            sgr: None,
            text: "foo()".into(),
        },
    ];
    let prefix = "  15≈- ";
    let rendered = render_row_segment(
        "    foo()",
        false,
        RowKind::Mov,
        Some(MovSide::Src),
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(prefix)],
        false,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(rendered.contains("  15≈- "));
    assert!(
        rendered.contains("\x1b[9m"),
        "mov src with Del run must have strikethrough styling on deleted run"
    );
    assert!(
        rendered.contains("\x1b[29m"),
        "mov src must turn off strikethrough after deleted run"
    );
}

#[test]
fn test_mov_tgt_filters_out_del_runs_no_strikethrough() {
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Del,
            color: TokenColor::Default,
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Function,
            sgr: None,
            text: "foo()".into(),
        },
    ];
    let prefix = "  20≈+ ";
    let rendered = render_row_segment(
        "foo()",
        false,
        RowKind::Mov,
        Some(MovSide::Tgt),
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(prefix)],
        false,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(rendered.contains("  20≈+ "));
    assert!(
        !rendered.contains("\x1b[9m"),
        "mov tgt must NOT have strikethrough styling (del run should be filtered out)"
    );
    assert!(
        !rendered.contains("    foo()"),
        "mov tgt must not contain deleted spaces"
    );
    assert!(
        rendered.contains("foo()"),
        "mov tgt must contain target text"
    );
}

#[test]
fn test_mov_src_filters_out_add_runs() {
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Default,
            sgr: None,
            text: "    ".into(),
        },
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Function,
            sgr: None,
            text: "foo()".into(),
        },
    ];
    let prefix = "  15≈- ";
    let rendered = render_row_segment(
        "foo()",
        false,
        RowKind::Mov,
        Some(MovSide::Src),
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(prefix)],
        false,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(rendered.contains("  15≈- "));
    assert!(
        !rendered.contains("    foo()"),
        "mov src must NOT contain added text"
    );
    assert!(
        rendered.contains("foo()"),
        "mov src must contain source text"
    );
}

#[test]
fn test_mod_row_del_span_uses_del_fill_background() {
    let theme = DiffuseTheme::dark();
    let s_del = theme.row_style(RowKind::Del);
    let runs = vec![
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Default,
            sgr: None,
            text: "foo ".into(),
        },
        OutputRun {
            kind: RunKind::Del,
            color: TokenColor::Default,
            sgr: None,
            text: "bar".into(),
        },
    ];
    let rendered = render_row_segment(
        "foo bar",
        false,
        RowKind::Mod,
        None,
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain("  15~- ")],
        false,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    let fill = s_del.fill_bg();
    assert!(
        !fill.is_empty(),
        "del row style must carry a fill background"
    );
    assert!(
        rendered.contains(&format!("{fill}\x1b[9m")),
        "struck del span on mod row must sit on the del fill background"
    );
}

#[test]
fn test_write_row_segment_direct_buffer() {
    let theme = DiffuseTheme::dark();
    let runs = vec![OutputRun {
        kind: RunKind::Ctx,
        color: TokenColor::Keyword,
        sgr: None,
        text: "let x = 1;".into(),
    }];
    let mut buf = String::new();
    write_row_segment(
        &mut buf,
        "let x = 1;",
        false,
        RowKind::Add,
        None,
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain("+ ")],
        false,
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
            false,
            RowKind::Add,
            None,
            &runs,
            0,
            usize::MAX,
            &[StyledSpan::plain("+ ")],
            false,
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
            kind: RunKind::Ctx,
            color: TokenColor::Default,
            sgr: None,
            text: "\t".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Default,
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
    let theme = DiffuseTheme::dark();
    let runs = vec![OutputRun {
        kind: RunKind::Ctx,
        color: TokenColor::Default,
        sgr: None,
        text: "\t\tlet x = 1;".into(),
    }];
    // Segment [0, 8) covers the 8 expanded tab spaces
    let seg1 = render_row_segment(
        "\t\tlet x = 1;",
        false,
        RowKind::Ctx,
        None,
        &runs,
        0,
        8,
        &[StyledSpan::plain("      ")],
        false,
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
        false,
        RowKind::Ctx,
        None,
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain("      ")],
        false,
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
    let theme = DiffuseTheme::dark();
    let prefix = "   1  ";
    // "ab✅cd": 5 graphemes, 6 visual columns. Window [0,4) must hold
    // "ab✅" (4 columns), never the 5-column "ab✅c".
    let del = render_row_segment(
        "ab✅cd",
        false,
        RK::Del,
        None,
        &[],
        0,
        4,
        &[StyledSpan::plain(prefix)],
        false,
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
        kind: RunKind::Ctx,
        color: TokenColor::Default,
        sgr: None,
        text: "ab✅cd".into(),
    }];
    let run_seg = render_row_segment(
        "ab✅cd",
        false,
        RK::Ctx,
        None,
        &runs,
        0,
        4,
        &[StyledSpan::plain(prefix)],
        false,
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
        false,
        RK::Ctx,
        None,
        &[],
        0,
        4,
        &[StyledSpan::plain(prefix)],
        false,
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
        false,
        RK::Del,
        None,
        &[],
        4,
        6,
        &[StyledSpan::plain(prefix)],
        false,
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

#[test]
fn test_render_row_segment_markdown_strikethrough_inherits_row_color_with_dim_strike() {
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Delimiter,
            sgr: None,
            text: "~~".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Strike,
            sgr: Some(9),
            text: "deprecated".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Delimiter,
            sgr: None,
            text: "~~".into(),
        },
    ];
    let rendered = render_row_segment(
        "~~deprecated~~",
        false,
        RowKind::Add,
        None,
        &runs,
        0,
        20,
        &[StyledSpan::plain("  42 + ")],
        false,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        rendered.contains("\x1b[2;9mdeprecated\x1b[29m\x1b[22m"),
        "rendered row must contain dim+strike wrapped deprecated: {rendered:?}"
    );
    assert!(
        !rendered.contains("\x1b[38;5;168m"),
        "must not contain pink foreground: {rendered:?}"
    );
}

#[test]
fn test_render_moved_src_run_skipped() {
    let theme = DiffuseTheme::dark();
    let runs = vec![
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Default,
            sgr: None,
            text: "    assert_eq!(".into(),
        },
        OutputRun {
            kind: RunKind::MovedSrc,
            color: TokenColor::Default,
            sgr: None,
            text: "super::resolve_theme".into(),
        },
        OutputRun {
            kind: RunKind::Del,
            color: TokenColor::Default,
            sgr: None,
            text: ", ".into(),
        },
    ];
    let rendered = render_row_segment(
        "    assert_eq!(",
        false,
        RowKind::Mod,
        None,
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(" 135~ ")],
        false,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        !rendered.contains("super::resolve_theme"),
        "moved-out source text must not render: {rendered:?}"
    );
    assert!(
        rendered.contains("assert_eq!"),
        "common head must render: {rendered:?}"
    );
    // Genuinely deleted text keeps its strike.
    assert!(
        rendered.contains("\x1b[9m"),
        "struck deletion must keep strike styling: {rendered:?}"
    );
}

#[test]
fn test_render_mov_tgt_with_add_row_bg() {
    let theme = DiffuseTheme::dark();
    let s_add = theme.row_style(RowKind::Add);
    let runs = vec![OutputRun {
        kind: RunKind::MovedTgt,
        color: TokenColor::Function,
        sgr: None,
        text: "resolve_theme".into(),
    }];
    let rendered_mov = render_row_segment(
        "resolve_theme",
        false,
        RowKind::Mov,
        Some(MovSide::Tgt),
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(" 135~+ ")],
        false,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    // MovTgt row background must use the addition tint background like
    // RowKind::Add.
    assert!(
        rendered_mov.contains(&s_add.tint_bg()),
        "MovTgt must render with addition tint background: {rendered_mov:?}"
    );
    // RunKind::MovedTgt tokens remain context-colored, not novelty-colored.
    let ctx_color = theme.palette[Cursor::Rest][DiffKind::Ctx]
        .syntax
        .token_color(TokenColor::Function)
        .unwrap();
    assert!(
        rendered_mov.contains(&ctx_color.fg_code()),
        "MovedTgt tokens must keep context token color: {rendered_mov:?}"
    );

    // Selected MovTgt row background must use add_cursor_bg.
    let rendered_sel = render_row_segment(
        "resolve_theme",
        false,
        RowKind::Mov,
        Some(MovSide::Tgt),
        &runs,
        0,
        usize::MAX,
        &[StyledSpan::plain(" 135~+ ")],
        false,
        true,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert!(
        rendered_sel.contains(&s_add.cursor_bg()),
        "Selected MovTgt must render with addition cursor background: {rendered_sel:?}"
    );
}

#[test]
fn test_display_text_skips_moved_source_runs() {
    // Geometry must measure what renders: moved-out source text is
    // skipped by the renderer, so measuring it would wrap the row onto
    // blank corpse lines.
    let runs = vec![
        OutputRun {
            kind: RunKind::Ctx,
            color: TokenColor::Default,
            sgr: None,
            text: "    assert_eq!(".into(),
        },
        OutputRun {
            kind: RunKind::MovedSrc,
            color: TokenColor::Default,
            sgr: None,
            text: "super::resolve_theme".into(),
        },
        OutputRun {
            kind: RunKind::Del,
            color: TokenColor::Default,
            sgr: None,
            text: " ".into(),
        },
    ];
    let disp = display_text("    assert_eq!(", false, RowKind::Mod, &runs, 0, true);
    assert_eq!(disp, "    assert_eq!( ");
}

fn span_run(kind: RunKind, color: TokenColor, sgr: Option<u32>, text: &str) -> OutputRun {
    OutputRun {
        kind,
        color,
        sgr,
        text: text.to_string().into(),
    }
}

fn span_text(spans: &[crate::span::StyledSpan]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

#[test]
fn test_styled_prefix_spans_match_ansi_prefix_text() {
    let theme = DiffuseTheme::dark();
    let bg = Some(theme.palette[Cursor::Row][DiffKind::Ctx].surface.bg);
    let cases = [
        (RowKind::Ctx, None, None, Some(7), Some(7)),
        (RowKind::Add, None, None, Some(42), None),
        (RowKind::Del, None, None, None, Some(10)),
        (RowKind::Mod, None, None, Some(55), Some(54)),
        (
            RowKind::Mod,
            None,
            Some(RowGroup::Anchor),
            Some(141),
            Some(140),
        ),
        (
            RowKind::Add,
            None,
            Some(RowGroup::Aux { anchor: 141 }),
            Some(140),
            None,
        ),
        (
            RowKind::Del,
            None,
            Some(RowGroup::Aux { anchor: 141 }),
            None,
            Some(140),
        ),
        (RowKind::Mov, Some(MovSide::Src), None, None, Some(9)),
        (RowKind::Mov, Some(MovSide::Tgt), None, Some(11), None),
    ];
    for (kind, mov_side, group, new_line, old_line) in cases {
        let ansi = emitted_prefix(kind, mov_side, group, new_line, old_line, &[], true, &theme);
        let spans = styled_prefix_spans(
            kind,
            mov_side,
            group,
            new_line,
            old_line,
            &[],
            true,
            bg,
            &theme,
        );
        assert_eq!(
            span_text(&spans),
            crate::ansi::strip_ansi(&ansi),
            "prefix spans must carry the ANSI prefix text for {kind:?}"
        );
        assert!(
            spans.iter().all(|span| span.bg == bg),
            "every prefix span must ride on the line background for {kind:?}"
        );
    }
}

#[test]
fn test_styled_prefix_spans_carry_gutter_colors() {
    let theme = DiffuseTheme::dark();
    let gutter = &theme.palette[Cursor::Rest][DiffKind::Mod].gutter;
    let spans = styled_prefix_spans(
        RowKind::Mod,
        None,
        None,
        Some(55),
        Some(54),
        &[],
        true,
        None,
        &theme,
    );
    let num = spans
        .iter()
        .find(|span| span.text.contains("55"))
        .expect("number span");
    assert_eq!(num.fg, Some(gutter.num.color));
    let icon = spans
        .iter()
        .find(|span| span.text == "~")
        .expect("icon span");
    assert_eq!(icon.fg, Some(gutter.icon.color));
}

#[test]
fn test_mov_row_prefix_line_number_colors() {
    let theme = DiffuseTheme::dark();
    let add_gutter = &theme.palette[Cursor::Rest][DiffKind::Add].gutter;
    let del_gutter = &theme.palette[Cursor::Rest][DiffKind::Del].gutter;
    let mov_gutter = &theme.palette[Cursor::Rest][DiffKind::Mov].gutter;

    // MovSrc: line number carries Del rendering (red + italic), icons stay mov.icon
    // and del.icon
    let src_spans = styled_prefix_spans(
        RowKind::Mov,
        Some(MovSide::Src),
        None,
        None,
        Some(42),
        &[],
        true,
        None,
        &theme,
    );
    assert_eq!(src_spans[0].fg, Some(del_gutter.num.color));
    assert_eq!(src_spans[0].italic, del_gutter.num.italic);
    assert_eq!(src_spans[1].fg, Some(mov_gutter.icon.color));
    assert_eq!(src_spans[1].text, "≈");
    assert_eq!(src_spans[2].fg, Some(del_gutter.icon.color));
    assert_eq!(src_spans[2].text, "-");

    // MovTgt: line number carries Add rendering (green), icons stay mov.icon and
    // add.icon
    let tgt_spans = styled_prefix_spans(
        RowKind::Mov,
        Some(MovSide::Tgt),
        None,
        Some(99),
        None,
        &[],
        true,
        None,
        &theme,
    );
    assert_eq!(tgt_spans[0].fg, Some(add_gutter.num.color));
    assert_eq!(tgt_spans[0].italic, add_gutter.num.italic);
    assert_eq!(tgt_spans[1].fg, Some(mov_gutter.icon.color));
    assert_eq!(tgt_spans[1].text, "≈");
    assert_eq!(tgt_spans[2].fg, Some(add_gutter.icon.color));
    assert_eq!(tgt_spans[2].text, "+");
}

#[test]
fn test_continuation_prefix_spans_match_ansi_continuation() {
    let theme = DiffuseTheme::dark();
    let bg = Some(theme.palette[Cursor::Rest][DiffKind::Mod].surface.bg);
    let spans = continuation_prefix_spans(bg);
    assert_eq!(span_text(&spans), "      ");
    assert!(spans.iter().all(|span| span.bg == bg));
}

#[test]
fn test_row_segment_spans_match_ansi_text_across_matrix() {
    let theme = DiffuseTheme::dark();
    let runs = vec![
        span_run(RunKind::Ctx, TokenColor::Keyword, None, "pub fn "),
        span_run(RunKind::Del, TokenColor::Default, None, "old_"),
        span_run(RunKind::Add, TokenColor::Function, Some(1), "new"),
    ];
    let text = "pub fn old_new";
    for kind in [RowKind::Ctx, RowKind::Add, RowKind::Del, RowKind::Mod] {
        for (selected, band) in [(false, false), (true, false), (false, true)] {
            for syntax_on in [false, true] {
                for (seg_start, seg_end) in [(0, usize::MAX), (0, 6), (4, 10)] {
                    let prefix = styled_prefix_spans(
                        kind,
                        None,
                        None,
                        Some(3),
                        Some(2),
                        &runs,
                        true,
                        None,
                        &theme,
                    );
                    let ansi = render_row_segment(
                        text, false, kind, None, &runs, seg_start, seg_end, &prefix, false,
                        selected, band, 0, true, syntax_on, &theme,
                    );
                    let line = row_segment_spans(
                        text, false, kind, None, &runs, seg_start, seg_end, prefix, selected, band,
                        0, true, syntax_on, &theme,
                    );
                    assert_eq!(
                        span_text(&line.spans),
                        crate::ansi::strip_ansi(&ansi),
                        "span text must match ANSI text for {kind:?} sel={selected} band={band} syn={syntax_on} seg={seg_start}..{seg_end}"
                    );
                }
            }
        }
    }
}

#[test]
fn test_row_segment_spans_carry_del_strike_and_fill() {
    let theme = DiffuseTheme::dark();
    let s_del = theme.row_style(RowKind::Del);
    let s_mod = theme.row_style(RowKind::Mod);
    let runs = vec![
        span_run(RunKind::Ctx, TokenColor::Default, None, "foo "),
        span_run(RunKind::Del, TokenColor::Default, None, "bar"),
    ];
    let prefix = styled_prefix_spans(
        RowKind::Mod,
        None,
        None,
        Some(15),
        Some(14),
        &runs,
        true,
        None,
        &theme,
    );
    let line = row_segment_spans(
        "foo bar",
        false,
        RowKind::Mod,
        None,
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
    let struck = line
        .spans
        .iter()
        .find(|span| span.text == "bar")
        .expect("del span");
    assert!(struck.strikethrough, "del run span must be struck");
    assert_eq!(
        struck.bg, s_del.fill_bg,
        "del run span must sit on the del fill"
    );
    assert_eq!(
        struck.fg,
        Some(s_del.fg),
        "del run span must use the del foreground"
    );
    assert_eq!(
        line.line_bg,
        Some(s_mod.tint_bg),
        "unselected mod line background must be the mod tint"
    );
}

#[test]
fn test_row_segment_spans_carry_word_fill_and_cursor() {
    let theme = DiffuseTheme::dark();
    let s_mod = theme.row_style(RowKind::Mod);
    let s_add = theme.row_style(RowKind::Add);
    let runs = vec![
        span_run(RunKind::Ctx, TokenColor::Keyword, None, "let "),
        span_run(RunKind::Add, TokenColor::Function, None, "x"),
    ];
    let prefix = styled_prefix_spans(
        RowKind::Add,
        None,
        None,
        Some(8),
        None,
        &runs,
        true,
        None,
        &theme,
    );
    let line = row_segment_spans(
        "let x",
        false,
        RowKind::Add,
        None,
        &runs,
        0,
        usize::MAX,
        prefix,
        true,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert_eq!(
        line.line_bg,
        Some(s_add.cursor_bg),
        "selected add line background must be the add cursor background"
    );
    let added = line
        .spans
        .iter()
        .find(|span| span.text == "x")
        .expect("add span");
    assert_eq!(
        added.bg, s_mod.fill_cursor_bg,
        "selected add run must sit on the fill-cursor background"
    );
}

#[test]
fn test_raw_row_spans_match_ansi_text_and_trailing_fill() {
    let theme = DiffuseTheme::dark();
    let prefix = styled_prefix_spans(
        RowKind::Add,
        None,
        None,
        Some(8),
        None,
        &[],
        true,
        None,
        &theme,
    );
    let ansi = render_raw_row(
        RowKind::Add,
        &prefix,
        "let x = 1;  ",
        false,
        false,
        true,
        &theme,
    );
    let line = raw_row_spans(
        RowKind::Add,
        prefix,
        "let x = 1;  ",
        false,
        false,
        true,
        &theme,
    );
    assert_eq!(
        span_text(&line.spans),
        crate::ansi::strip_ansi(&ansi),
        "raw span text must match ANSI raw text"
    );
    let trailing = line.spans.last().expect("trailing span");
    assert_eq!(
        trailing.text, "  ",
        "trailing whitespace must be its own span"
    );
    assert_eq!(
        trailing.bg,
        theme.palette[Cursor::Rest][DiffKind::Add].surface.fill,
        "trailing whitespace must sit on the add fill"
    );
}

#[test]
fn test_aux_add_row_use_merged_display_text() {
    let runs = vec![
        OutputRun {
            kind: RunKind::Del,
            color: TokenColor::Default,
            sgr: None,
            text: "old_part".into(),
        },
        OutputRun {
            kind: RunKind::Add,
            color: TokenColor::Default,
            sgr: None,
            text: "new_part".into(),
        },
    ];
    // With is_aux_row = true, display_text merges the del and add runs.
    let disp_aux = display_text("new_part", true, RowKind::Add, &runs, 0, true);
    assert_eq!(disp_aux, "old_partnew_part");

    // With is_aux_row = false, display_text does not merge; returns text.
    let disp_plain = display_text("new_part", false, RowKind::Add, &runs, 0, true);
    assert_eq!(disp_plain, "new_part");
}
