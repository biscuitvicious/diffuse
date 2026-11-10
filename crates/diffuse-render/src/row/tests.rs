use super::*;
use crate::{
    ansi::ColorExt,
    core::{
        merge::RunKind,
        parse::{RowGroup, RowKind, Spans},
        wire::{ModGlyph, ModSide, MovSide, OutputRun},
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
        kind, mov_side, None, None, group, new_line, old_line, runs, in_hunk, None, theme,
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
    assert_eq!(r_add, "   42 +");

    let r_del = prefix_of(RowKind::Del, None, None, None, Some(10), &[]);
    assert_eq!(r_del, "   10 -");

    // Non-group 1-to-1 mod row styles with right-aligned ~ ("   55 ~")
    let r_mod = prefix_of(RowKind::Mod, None, None, Some(55), Some(54), &[]);
    assert_eq!(r_mod, "   55 ~");
}

#[test]
fn test_prefix_group_gutters() {
    // Group anchor row has single left-aligned ~ ("  141~ ")
    let r_anchor = prefix_of(
        RowKind::Mod,
        None,
        Some(RowGroup::Anchor),
        Some(141),
        Some(140),
        &[],
    );
    assert_eq!(r_anchor, "  141~ ");

    // Group aux add row has ~+
    let r_aux_add = prefix_of(
        RowKind::Add,
        None,
        Some(RowGroup::Aux { anchor: 141 }),
        Some(140),
        None,
        &[],
    );
    assert_eq!(r_aux_add, "  140~+");

    // Group aux del row has ~-
    let r_aux_del = prefix_of(
        RowKind::Del,
        None,
        Some(RowGroup::Aux { anchor: 141 }),
        None,
        Some(140),
        &[],
    );
    assert_eq!(r_aux_del, "  140~-");
}

#[test]
fn test_styled_prefix_group_and_non_group() {
    let theme = DiffuseTheme::dark();

    // Non-group mod row has right-aligned ~ ("   55 ~")
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
    assert_eq!(crate::ansi::strip_ansi(&s_mod), "   55 ~");
    assert!(
        s_mod.contains(
            &theme.palette[Cursor::Rest][DiffKind::Mod]
                .gutter
                .icon
                .color
                .fg_code()
        )
    );

    // Group anchor row has left-aligned ~ ("  141~ ")
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
    assert_eq!(crate::ansi::strip_ansi(&s_anchor), "  141~ ");
    assert!(
        s_anchor.contains(
            &theme.palette[Cursor::Rest][DiffKind::Mod]
                .gutter
                .icon
                .color
                .fg_code()
        )
    );

    // Group aux add row has ~+ ("  140~+")
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
    assert_eq!(crate::ansi::strip_ansi(&s_aux_add), "  140~+");
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
    assert_eq!(crate::ansi::strip_ansi(&s_aux_del), "  140~-");
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
        r_add, "   53 +",
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
            None,
            None,
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
        None,
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
    assert_eq!(span_text(&spans), "       ");
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
                        text, false, kind, None, None, &runs, seg_start, seg_end, prefix, selected,
                        band, 0, true, syntax_on, &theme,
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

#[test]
fn test_expanded_mod_prefix_spans_and_row_segment_spans() {
    use diffuse_core::ModSide;
    let theme = DiffuseTheme::dark();
    let mod_gutter = &theme.palette[Cursor::Rest][DiffKind::Mod].gutter;
    let del_gutter = &theme.palette[Cursor::Rest][DiffKind::Del].gutter;
    let add_gutter = &theme.palette[Cursor::Rest][DiffKind::Add].gutter;
    let s_del = theme.row_style(RowKind::Del);
    let s_add = theme.row_style(RowKind::Add);
    let s_mod = theme.row_style(RowKind::Mod);

    let runs = vec![
        span_run(RunKind::Ctx, TokenColor::Keyword, None, "let "),
        span_run(RunKind::Del, TokenColor::Default, None, "old"),
        span_run(RunKind::Add, TokenColor::Default, None, "new"),
        span_run(RunKind::Ctx, TokenColor::Default, None, " = 1;"),
    ];

    // 1. Del half: old_line, ┌ (mod orange) and - (del icon color)
    let del_prefix = styled_prefix_spans(
        RowKind::Mod,
        None,
        Some(ModSide::Del),
        None,
        None,
        Some(20),
        Some(10),
        &runs,
        true,
        None,
        &theme,
    );
    assert_eq!(del_prefix[0].fg, Some(del_gutter.num.color));
    assert!(del_prefix[0].text.contains("10"));
    assert_eq!(del_prefix[1].fg, Some(mod_gutter.icon.color));
    assert_eq!(del_prefix[1].text, "┌");
    assert_eq!(del_prefix[2].fg, Some(del_gutter.icon.color));
    assert_eq!(del_prefix[2].text, "-");

    // 2. Add half: new_line, └ (mod orange) and + (add icon color)
    let add_prefix = styled_prefix_spans(
        RowKind::Mod,
        None,
        Some(ModSide::Add),
        None,
        None,
        Some(20),
        Some(10),
        &runs,
        true,
        None,
        &theme,
    );
    assert_eq!(add_prefix[0].fg, Some(add_gutter.num.color));
    assert!(add_prefix[0].text.contains("20"));
    assert_eq!(add_prefix[1].fg, Some(mod_gutter.icon.color));
    assert_eq!(add_prefix[1].text, "└");
    assert_eq!(add_prefix[2].fg, Some(add_gutter.icon.color));
    assert_eq!(add_prefix[2].text, "+");

    // 3. Del row segment: filters out "new", styles "old" with del_bg &
    //    strikethrough
    let del_line = row_segment_spans(
        "let old = 1;",
        false,
        RowKind::Mod,
        None,
        Some(ModSide::Del),
        &runs,
        0,
        usize::MAX,
        del_prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert_eq!(del_line.line_bg, Some(s_del.tint_bg));
    let has_new_in_del = del_line.spans.iter().any(|s| s.text.contains("new"));
    assert!(!has_new_in_del, "del half must not contain add spans");
    let old_span = del_line
        .spans
        .iter()
        .find(|s| s.text == "old")
        .expect("old span");
    assert!(old_span.strikethrough);
    assert_eq!(old_span.bg, s_del.fill_bg);

    // 4. Add row segment: filters out "old", styles "new" with word_bg
    let add_line = row_segment_spans(
        "let new = 1;",
        false,
        RowKind::Mod,
        None,
        Some(ModSide::Add),
        &runs,
        0,
        usize::MAX,
        add_prefix,
        false,
        false,
        0,
        true,
        true,
        &theme,
    );
    assert_eq!(add_line.line_bg, Some(s_add.tint_bg));
    let has_old_in_add = add_line.spans.iter().any(|s| s.text.contains("old"));
    assert!(!has_old_in_add, "add half must not contain del spans");
    let new_span = add_line
        .spans
        .iter()
        .find(|s| s.text == "new")
        .expect("new span");
    assert!(!new_span.strikethrough);
    assert_eq!(new_span.bg, s_mod.fill_bg);
}

#[test]
fn test_mod_side_del_prefix_suppresses_italic_on_box_and_icon() {
    let theme = DiffuseTheme::dark();
    let runs = vec![
        span_run(RunKind::Del, TokenColor::Default, None, "old"),
        span_run(RunKind::Add, TokenColor::Default, None, "new"),
    ];
    let del_prefix = styled_prefix_spans(
        RowKind::Mod,
        None,
        Some(ModSide::Del),
        None,
        None,
        Some(20),
        Some(10),
        &runs,
        false,
        None,
        &theme,
    );
    let mut out = String::new();
    write_row_segment(
        &mut out,
        "let old = 1;",
        false,
        RowKind::Mod,
        None,
        Some(ModSide::Del),
        &runs,
        0,
        usize::MAX,
        &del_prefix,
        false,
        false,
        false,
        0,
        true,
        true,
        &theme,
    )
    .unwrap();

    // Line number 10 is styled with italic (fused SGR starting with 3;)
    assert!(
        out.contains("\x1b[3;") || out.contains("\x1b[3m"),
        "line number should have italic: {out:?}"
    );
    // After line number and before ┌, italic must be turned off with \x1b[23m
    let num_pos = out.find("10").expect("line number 10");
    let box_pos = out.find("┌").expect("box glyph ┌");
    let italic_off_pos = out.find("\x1b[23m").expect("italic off code");
    assert!(
        num_pos < italic_off_pos && italic_off_pos < box_pos,
        "italic must be turned off between line number and box glyph ┌: {out:?}"
    );
}

#[test]
fn test_continuation_prefix_with_mod_side() {
    let theme = DiffuseTheme::dark();
    let spans_del = continuation_prefix_spans_with_mod_side(Some(ModSide::Del), None, &theme, None);
    assert_eq!(spans_del.len(), 3);
    assert_eq!(spans_del[0].text, "     ");
    assert_eq!(spans_del[1].text, "│");
    assert_eq!(
        spans_del[1].fg,
        Some(theme.palette[Cursor::Rest][DiffKind::Mod].gutter.icon.color)
    );
    assert_eq!(spans_del[2].text, " ");

    let spans_add = continuation_prefix_spans_with_mod_side(Some(ModSide::Add), None, &theme, None);
    assert_eq!(spans_add.len(), 1);
    assert_eq!(spans_add[0].text, "       ");

    let spans_none = continuation_prefix_spans_with_mod_side(None, None, &theme, None);
    assert_eq!(spans_none.len(), 1);
    assert_eq!(spans_none[0].text, "       ");

    assert_eq!(
        continuation_prefix_plain_with_mod_side(Some(ModSide::Del), None),
        "     │ "
    );
    assert_eq!(
        continuation_prefix_plain_with_mod_side(Some(ModSide::Add), None),
        "       "
    );
    assert_eq!(
        continuation_prefix_plain_with_mod_side(None, None),
        "       "
    );

    // Test with explicit ModGlyph
    let spans_del_mid = continuation_prefix_spans_with_mod_side(
        Some(ModSide::Del),
        Some(ModGlyph::Mid),
        &theme,
        None,
    );
    assert_eq!(spans_del_mid.len(), 3);
    assert_eq!(spans_del_mid[1].text, "│");

    let spans_add_mid = continuation_prefix_spans_with_mod_side(
        Some(ModSide::Add),
        Some(ModGlyph::Mid),
        &theme,
        None,
    );
    assert_eq!(spans_add_mid.len(), 3);
    assert_eq!(spans_add_mid[1].text, "│");

    let spans_add_bottom = continuation_prefix_spans_with_mod_side(
        Some(ModSide::Add),
        Some(ModGlyph::Bottom),
        &theme,
        None,
    );
    assert_eq!(spans_add_bottom.len(), 1);
    assert_eq!(spans_add_bottom[0].text, "       ");

    assert_eq!(
        continuation_prefix_plain_with_mod_side(Some(ModSide::Add), Some(ModGlyph::Mid)),
        "     │ "
    );
    assert_eq!(
        continuation_prefix_plain_with_mod_side(Some(ModSide::Add), Some(ModGlyph::Bottom)),
        "       "
    );
}

#[test]
fn test_styled_prefix_spans_with_mod_glyph() {
    let theme = DiffuseTheme::dark();
    let mod_gutter = &theme.palette[Cursor::Rest][DiffKind::Mod].gutter;

    // Top Del
    let top_del = styled_prefix_spans(
        RowKind::Mod,
        None,
        Some(ModSide::Del),
        Some(ModGlyph::Top),
        None,
        Some(20),
        Some(10),
        &[],
        true,
        None,
        &theme,
    );
    assert_eq!(top_del[1].text, "┌");
    assert_eq!(top_del[1].fg, Some(mod_gutter.icon.color));
    assert_eq!(top_del[2].text, "-");

    // Mid Del
    let mid_del = styled_prefix_spans(
        RowKind::Mod,
        None,
        Some(ModSide::Del),
        Some(ModGlyph::Mid),
        None,
        Some(21),
        Some(11),
        &[],
        true,
        None,
        &theme,
    );
    assert_eq!(mid_del[1].text, "│");
    assert_eq!(mid_del[1].fg, Some(mod_gutter.icon.color));
    assert_eq!(mid_del[2].text, "-");

    // Mid Add
    let mid_add = styled_prefix_spans(
        RowKind::Mod,
        None,
        Some(ModSide::Add),
        Some(ModGlyph::Mid),
        None,
        Some(20),
        Some(10),
        &[],
        true,
        None,
        &theme,
    );
    assert_eq!(mid_add[1].text, "│");
    assert_eq!(mid_add[1].fg, Some(mod_gutter.icon.color));
    assert_eq!(mid_add[2].text, "+");

    // Bottom Add
    let bottom_add = styled_prefix_spans(
        RowKind::Mod,
        None,
        Some(ModSide::Add),
        Some(ModGlyph::Bottom),
        None,
        Some(21),
        Some(11),
        &[],
        true,
        None,
        &theme,
    );
    assert_eq!(bottom_add[1].text, "└");
    assert_eq!(bottom_add[1].fg, Some(mod_gutter.icon.color));
    assert_eq!(bottom_add[2].text, "+");
}

#[test]
fn test_styled_prefix_spans_with_custom_line_num_width() {
    let theme = DiffuseTheme::dark();

    // width = 0: line numbers hidden, only 2-character gutter
    let spans_w0 = styled_prefix_spans_with_width(
        RowKind::Add,
        None,
        None,
        None,
        None,
        Some(42),
        None,
        &[],
        true,
        None,
        &theme,
        0,
    );
    let total_w0: usize = spans_w0.iter().map(|s| s.text.len()).sum();
    assert_eq!(total_w0, 2);
    let full_w0: String = spans_w0.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(full_w0, " +");

    // width = 1: single digit
    let spans_w1 = styled_prefix_spans_with_width(
        RowKind::Add,
        None,
        None,
        None,
        None,
        Some(7),
        None,
        &[],
        true,
        None,
        &theme,
        1,
    );
    let full_w1: String = spans_w1.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(full_w1, "7 +");

    // width = 2: 2 digits
    let spans_w2 = styled_prefix_spans_with_width(
        RowKind::Del,
        None,
        None,
        None,
        None,
        None,
        Some(3),
        &[],
        true,
        None,
        &theme,
        2,
    );
    let full_w2: String = spans_w2.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(full_w2, " 3 -");

    // width = 0 continuation line
    let cont_w0 = continuation_prefix_spans_with_mod_side_and_width(
        Some(ModSide::Del),
        Some(ModGlyph::Top),
        &theme,
        None,
        0,
    );
    let full_cont_w0: String = cont_w0.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(full_cont_w0, "│ ");

    // width = 2 continuation line
    let cont_w2 = continuation_prefix_spans_with_mod_side_and_width(
        Some(ModSide::Del),
        Some(ModGlyph::Top),
        &theme,
        None,
        2,
    );
    let full_cont_w2: String = cont_w2.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(full_cont_w2, "  │ ");
}

#[test]
fn test_format_line_num_fit_and_truncation_with_ellipsis() {
    use super::gutter::{MAX_LINE_NUM_BUF_LEN, MAX_LINE_NUM_WIDTH, format_line_num};

    let mut buf = [b' '; MAX_LINE_NUM_BUF_LEN];

    // width = 0: always empty
    assert_eq!(format_line_num(None, 0, &mut buf), "");
    assert_eq!(format_line_num(Some(42), 0, &mut buf), "");

    // None produces spaces
    assert_eq!(format_line_num(None, 1, &mut buf), " ");
    assert_eq!(format_line_num(None, 3, &mut buf), "   ");
    assert_eq!(format_line_num(None, 5, &mut buf), "     ");

    // width = 1
    assert_eq!(format_line_num(Some(0), 1, &mut buf), "0");
    assert_eq!(format_line_num(Some(5), 1, &mut buf), "5");
    // 12 does not fit in 1: stripped until fits in 0 digits -> just ellipsis
    assert_eq!(format_line_num(Some(12), 1, &mut buf), "…");

    // width = 2
    assert_eq!(format_line_num(Some(0), 2, &mut buf), " 0");
    assert_eq!(format_line_num(Some(7), 2, &mut buf), " 7");
    assert_eq!(format_line_num(Some(42), 2, &mut buf), "42");
    // 100 does not fit in 2: 100 / 10 = 10 / 10 = 1 (fits in 2 - 1 = 1) -> "1…"
    assert_eq!(format_line_num(Some(100), 2, &mut buf), "1…");
    assert_eq!(format_line_num(Some(999), 2, &mut buf), "9…");

    // width = 3
    assert_eq!(format_line_num(Some(0), 3, &mut buf), "  0");
    assert_eq!(format_line_num(Some(42), 3, &mut buf), " 42");
    assert_eq!(format_line_num(Some(123), 3, &mut buf), "123");
    // 12345: repeatedly / 10 -> 1234 -> 123 -> 12 (fits in 3 - 1 = 2) -> "12…"
    assert_eq!(format_line_num(Some(12345), 3, &mut buf), "12…");
    assert_eq!(format_line_num(Some(9999), 3, &mut buf), "99…");

    // width = 4
    assert_eq!(format_line_num(Some(0), 4, &mut buf), "   0");
    assert_eq!(format_line_num(Some(1000), 4, &mut buf), "1000");
    assert_eq!(format_line_num(Some(10000), 4, &mut buf), "100…");
    assert_eq!(format_line_num(Some(123456), 4, &mut buf), "123…");

    // width = 5
    assert_eq!(format_line_num(Some(12345), 5, &mut buf), "12345");
    assert_eq!(format_line_num(Some(123456), 5, &mut buf), "1234…");

    // Constant verification & gutter_width
    assert_eq!(MAX_LINE_NUM_WIDTH, 16);
    assert_eq!(super::gutter::gutter_width(0), 2);
    assert_eq!(super::gutter::gutter_width(5), 7);
    assert_eq!(super::gutter::gutter_width(16), 18);
    assert_eq!(super::gutter::GUTTER_SPACES[0], "  ");
    assert_eq!(super::gutter::GUTTER_SPACES[5], "       ");
    assert_eq!(super::gutter::LINE_NUM_SPACES[0], "");
    assert_eq!(super::gutter::LINE_NUM_SPACES[5], "     ");
    assert_eq!(super::gutter::BOX_PREFIXES[0], "│ ");
    assert_eq!(super::gutter::BOX_PREFIXES[5], "     │ ");
}

#[test]
fn test_dual_gutter_width_and_prefix_formatting() {
    // 1. dual_gutter_width calculations
    assert_eq!(dual_gutter_width(0, 0), 2);
    assert_eq!(dual_gutter_width(0, 3), 5);
    assert_eq!(dual_gutter_width(0, 4), 6);
    assert_eq!(dual_gutter_width(3, 4), 10); // 3 + 1 + 4 + 2
    assert_eq!(dual_gutter_width(4, 4), 11); // 4 + 1 + 4 + 2
    assert_eq!(dual_gutter_width(5, 5), 13); // 5 + 1 + 5 + 2

    let theme = DiffuseTheme::dark();

    // 2. prefix_of_with_mod_side_and_widths with old_w = 3, new_w = 4
    let runs = [];
    let p_ctx = prefix_of_with_mod_side_and_widths(
        RowKind::Ctx,
        None,
        None,
        None,
        None,
        Some(10),
        Some(8),
        &runs,
        3,
        4,
    );
    assert_eq!(p_ctx, "  8   10  ");
    assert_eq!(p_ctx.len(), 10);

    let p_add = prefix_of_with_mod_side_and_widths(
        RowKind::Add,
        None,
        None,
        None,
        None,
        Some(10),
        None,
        &runs,
        3,
        4,
    );
    assert_eq!(p_add, "      10 +");
    assert_eq!(p_add.len(), 10);

    let p_del = prefix_of_with_mod_side_and_widths(
        RowKind::Del,
        None,
        None,
        None,
        None,
        None,
        Some(8),
        &runs,
        3,
        4,
    );
    assert_eq!(p_del, "  8      -");
    assert_eq!(p_del.len(), 10);

    let p_mod = prefix_of_with_mod_side_and_widths(
        RowKind::Mod,
        None,
        None,
        None,
        None,
        Some(10),
        Some(8),
        &runs,
        3,
        4,
    );
    assert_eq!(p_mod, "  8   10 ~");
    assert_eq!(p_mod.len(), 10);

    let p_mov_src = prefix_of_with_mod_side_and_widths(
        RowKind::Mov,
        Some(MovSide::Src),
        None,
        None,
        None,
        None,
        Some(8),
        &runs,
        3,
        4,
    );
    assert_eq!(p_mov_src, "  8     ≈-");
    assert_eq!(crate::width::display_width(&p_mov_src), 10);

    let p_mov_tgt = prefix_of_with_mod_side_and_widths(
        RowKind::Mov,
        Some(MovSide::Tgt),
        None,
        None,
        None,
        Some(10),
        None,
        &runs,
        3,
        4,
    );
    assert_eq!(p_mov_tgt, "      10≈+");
    assert_eq!(crate::width::display_width(&p_mov_tgt), 10);

    let p_mod_del = prefix_of_with_mod_side_and_widths(
        RowKind::Mod,
        None,
        Some(ModSide::Del),
        None,
        None,
        None,
        Some(8),
        &runs,
        3,
        4,
    );
    assert_eq!(p_mod_del, "  8     ┌-");
    assert_eq!(crate::width::display_width(&p_mod_del), 10);

    let p_mod_add = prefix_of_with_mod_side_and_widths(
        RowKind::Mod,
        None,
        Some(ModSide::Add),
        None,
        None,
        Some(10),
        None,
        &runs,
        3,
        4,
    );
    assert_eq!(p_mod_add, "      10└+");
    assert_eq!(crate::width::display_width(&p_mod_add), 10);

    // 3. styled_prefix_spans_with_widths width validation
    let span_len = |spans: &[StyledSpan]| {
        spans
            .iter()
            .map(|s| crate::width::display_width(&s.text))
            .sum::<usize>()
    };

    let s_ctx = styled_prefix_spans_with_widths(
        RowKind::Ctx,
        None,
        None,
        None,
        None,
        Some(10),
        Some(8),
        &runs,
        false,
        None,
        &theme,
        3,
        4,
    );
    assert_eq!(span_len(&s_ctx), 10);

    let s_add = styled_prefix_spans_with_widths(
        RowKind::Add,
        None,
        None,
        None,
        None,
        Some(10),
        None,
        &runs,
        false,
        None,
        &theme,
        3,
        4,
    );
    assert_eq!(span_len(&s_add), 10);

    let s_del = styled_prefix_spans_with_widths(
        RowKind::Del,
        None,
        None,
        None,
        None,
        None,
        Some(8),
        &runs,
        false,
        None,
        &theme,
        3,
        4,
    );
    assert_eq!(span_len(&s_del), 10);

    let s_mod = styled_prefix_spans_with_widths(
        RowKind::Mod,
        None,
        None,
        None,
        None,
        Some(10),
        Some(8),
        &runs,
        false,
        None,
        &theme,
        3,
        4,
    );
    assert_eq!(span_len(&s_mod), 10);

    let s_mov_src = styled_prefix_spans_with_widths(
        RowKind::Mov,
        Some(MovSide::Src),
        None,
        None,
        None,
        None,
        Some(8),
        &runs,
        false,
        None,
        &theme,
        3,
        4,
    );
    assert_eq!(span_len(&s_mov_src), 10);

    let s_mov_tgt = styled_prefix_spans_with_widths(
        RowKind::Mov,
        Some(MovSide::Tgt),
        None,
        None,
        None,
        Some(10),
        None,
        &runs,
        false,
        None,
        &theme,
        3,
        4,
    );
    assert_eq!(span_len(&s_mov_tgt), 10);

    let s_mod_del = styled_prefix_spans_with_widths(
        RowKind::Mod,
        None,
        Some(ModSide::Del),
        None,
        None,
        None,
        Some(8),
        &runs,
        false,
        None,
        &theme,
        3,
        4,
    );
    assert_eq!(s_mod_del[0].text, "  8");
    assert_eq!(s_mod_del[1].text, "     "); // 1 space + 4 blank_new
    assert_eq!(s_mod_del[2].text, "┌");
    assert_eq!(s_mod_del[3].text, "-");

    // 4. continuation_prefix_spans_with_mod_side_and_widths
    let cont_normal =
        continuation_prefix_spans_with_mod_side_and_widths(None, None, &theme, None, 3, 4);
    assert_eq!(span_len(&cont_normal), 10);

    let cont_box = continuation_prefix_spans_with_mod_side_and_widths(
        Some(ModSide::Del),
        None,
        &theme,
        None,
        3,
        4,
    );
    assert_eq!(cont_box[0].text, "        "); // 3 + 1 + 4 = 8 spaces
    assert_eq!(cont_box[1].text, "│");
    assert_eq!(cont_box[2].text, " ");

    let cont_plain_box =
        continuation_prefix_plain_with_mod_side_and_widths(Some(ModSide::Del), None, 3, 4);
    assert_eq!(cont_plain_box, "        │ ");
}
