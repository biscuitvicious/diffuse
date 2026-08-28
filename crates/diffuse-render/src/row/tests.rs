use super::*;
use crate::core::{merge::RunKind, wire::OutputRun};

#[test]
fn test_prefix_gutters() {
    let r_add = prefix_of(RowKind::Add, Some(42), None, false, &[]);
    assert_eq!(r_add, "  42 +");

    let r_del = prefix_of(RowKind::Del, None, Some(10), false, &[]);
    assert_eq!(r_del, "  10 -");

    let r_mod = prefix_of(RowKind::Mod, Some(55), Some(54), false, &[]);
    assert_eq!(r_mod, "  55 ~");
}

#[test]
fn test_render_addition_row_with_subword_styling() {
    let theme = DiffuseTheme::MAGIC;
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
        false,
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
    assert!(rendered.starts_with(prefix));
}

#[test]
fn test_grapheme_clusters_never_split() {
    let theme = DiffuseTheme::MAGIC;
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
        false,
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
        false,
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
    let theme = DiffuseTheme::MAGIC;
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
        false,
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
        false,
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
    let theme = DiffuseTheme::MAGIC;
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
        false,
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
    assert!(
        add_trailing.contains("\x1b[48;5;28m\x1b[38;5;151m   \x1b[49m"),
        "added trailing spaces must reset background to default immediately after spaces"
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
        !add_wrapped_intermediate.contains("\x1b[48;5;28m"),
        "intermediate wrapped segments must not highlight spaces at wrap boundary"
    );

    let del_trailing = render_raw_row(RowKind::Del, "  41 - ", "foo();  ", false, false, true);
    assert!(del_trailing.contains("  41 - "));
    assert!(del_trailing.contains("\x1b[38;5;203mfoo();"));
    assert!(
        del_trailing.contains("\x1b[48;5;88m\x1b[9m\x1b[38;5;167m  \x1b[29m\x1b[49m"),
        "deleted trailing spaces must have subline highlight and reset background to default"
    );
}
