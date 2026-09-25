use super::*;

#[test]
fn test_default_theme_token_colors() {
    let theme = DiffuseTheme::DEFAULT;
    let kw_add = theme.render_token("syntaxKeyword", "fn", "add", None, None);
    assert_eq!(kw_add, "\x1b[38;5;147mfn\x1b[39m");

    let kw_del = theme.render_token("syntaxKeyword", "fn", "del", None, None);
    assert_eq!(kw_del, "\x1b[38;5;183mfn\x1b[39m");

    let kw_ctx = theme.render_token("syntaxKeyword", "fn", "ctx", None, None);
    assert_eq!(kw_ctx, "\x1b[38;5;147mfn\x1b[39m");

    let ty_add = theme.render_token("syntaxType", "Color", "add", None, None);
    assert_eq!(ty_add, "\x1b[38;5;220mColor\x1b[39m");

    let ty_del = theme.render_token("syntaxType", "Color", "del", None, None);
    assert_eq!(ty_del, "\x1b[38;5;215mColor\x1b[39m");

    let ty_ctx = theme.render_token("syntaxType", "Color", "ctx", None, None);
    assert_eq!(ty_ctx, "\x1b[38;5;220mColor\x1b[39m");

    let num_add = theme.render_token("syntaxNumber", "42", "add", None, None);
    assert_eq!(num_add, "\x1b[38;5;221m42\x1b[39m");

    let num_del = theme.render_token("syntaxNumber", "42", "del", None, None);
    assert_eq!(num_del, "\x1b[38;5;222m42\x1b[39m");
}

#[test]
fn test_sgr_modifiers_application_and_reset() {
    let theme = DiffuseTheme::DEFAULT;
    let bold_tok = theme.render_token("mdBold", "heading", "ctx", None, Some(1));
    assert!(bold_tok.starts_with("\x1b[1m"));
    assert!(bold_tok.ends_with("\x1b[22m"));

    let strike_tok = theme.render_token("mdStrike", "removed", "ctx", None, Some(9));
    assert_eq!(strike_tok, "\x1b[2;9mremoved\x1b[29m\x1b[22m");
}

#[test]
fn test_strikethrough_diff_token_dim_strike_no_clashing_foreground() {
    let theme = DiffuseTheme::DEFAULT;
    for (row_kind, tok_kind) in [
        ("ctx", "ctx"),
        ("add", "add"),
        ("del", "del"),
        ("mod", "ctx"),
    ] {
        let tok = theme.render_token("mdStrike", "deprecated", row_kind, Some(tok_kind), Some(9));
        assert!(
            tok.starts_with("\x1b[2;9m"),
            "must emit dim + strike stacked SGR for {row_kind}: {tok:?}"
        );
        assert!(
            tok.ends_with("\x1b[29m\x1b[22m"),
            "must reset strike then dim for {row_kind}: {tok:?}"
        );
        assert!(
            !tok.contains("\x1b[38;5;168m"),
            "must not contain pink foreground in {row_kind}: {tok:?}"
        );
        assert!(
            !tok.contains("\x1b[39m"),
            "must not emit foreground reset when inheriting row text color: {tok:?}"
        );
    }
}

#[test]
fn test_encode_decode_sgr() {
    let raw = "\x1b[38;5;231mReview: mirage\x1b[39m";
    let encoded = encode_sgr(raw);
    assert_eq!(encoded, "{38;5;231}Review: mirage{39}");
    let decoded = decode_sgr(&encoded);
    assert_eq!(decoded, raw);
}

#[test]
fn test_default_theme_palette() {
    let palette = ThemePalette::default_palette();
    assert_eq!(palette.bg_base, Color::Fixed(233));
    assert_eq!(palette.add_fg, Color::Fixed(120));
    assert_eq!(palette.del_fg, Color::Fixed(203));
    assert_eq!(palette.add_tint_bg, Color::Rgb(28, 50, 30));
    assert_eq!(palette.add_cursor_bg, Color::Rgb(46, 74, 48));
    assert_eq!(palette.del_tint_bg, Color::Rgb(38, 12, 14));
    assert_eq!(palette.add_fill_bg, Color::Rgb(30, 96, 50));
    assert_eq!(palette.add_fill_cursor_bg, Color::Rgb(37, 120, 65));
    assert_eq!(palette.mod_fill_bg, Color::Rgb(30, 96, 50));
    assert_eq!(palette.mod_fill_cursor_bg, Color::Rgb(37, 120, 65));
    assert_eq!(palette.mod_tint_bg, Color::Rgb(38, 38, 38));
    assert_eq!(palette.mod_cursor_bg, Color::Rgb(66, 66, 66));
    assert_eq!(palette.note_tint_bg, Color::Rgb(76, 64, 20));
    assert_eq!(palette.ctx_cursor_bg, Color::Fixed(237));

    let theme = DiffuseTheme::DEFAULT;
    assert_eq!(theme.name, "default");
}

#[test]
fn test_row_style_methods() {
    let theme = DiffuseTheme::DEFAULT;

    let add_style = theme.row_style(diffuse_core::parse::RowKind::Add);
    assert_eq!(add_style.tint_bg, theme.palette.add_tint_bg);
    assert_eq!(add_style.cursor_bg, theme.palette.add_cursor_bg);
    assert_eq!(add_style.fill_bg, Some(theme.palette.add_fill_bg));
    assert_eq!(
        add_style.fill_cursor_bg,
        Some(theme.palette.add_fill_cursor_bg)
    );
    assert_eq!(add_style.fg, theme.palette.add_fg);

    let del_style = theme.row_style(diffuse_core::parse::RowKind::Del);
    assert_eq!(del_style.fill_style(), None);
    assert_eq!(del_style.fill_cursor_style(), None);
}

#[test]
fn test_theme_from_name_and_env() {
    assert_eq!(
        DiffuseTheme::from_name("default"),
        Some(DiffuseTheme::DEFAULT)
    );
    assert_eq!(DiffuseTheme::from_name("DARK"), Some(DiffuseTheme::DEFAULT));
    assert_eq!(
        DiffuseTheme::from_name("black"),
        Some(DiffuseTheme::DEFAULT)
    );
    assert_eq!(DiffuseTheme::from_name("nonexistent"), None);
    assert_eq!(DiffuseTheme::from_name(""), None);
    assert_eq!(DiffuseTheme::from_name("unknown-theme"), None);

    unsafe {
        std::env::set_var("DIFFUSE_THEME", "default");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::DEFAULT);
        std::env::remove_var("DIFFUSE_THEME");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::DEFAULT);
        std::env::set_var("DIFFUSE_THEME", "invalid");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::DEFAULT);
        std::env::remove_var("DIFFUSE_THEME");
    }
}

#[test]
fn test_theme_palette_helpers() {
    let p = ThemePalette::default_palette();
    assert_eq!(
        p.file_header_colors(true, false, false),
        (p.file_header_bg_selected, p.file_header_fg_selected)
    );
    assert_eq!(
        p.file_header_colors(false, true, false),
        (p.file_header_bg_current, p.file_header_fg_current)
    );
    assert_eq!(
        p.file_header_colors(false, false, true),
        (p.file_header_bg_current, p.file_header_fg_current)
    );
    assert_eq!(
        p.file_header_colors(false, false, false),
        (p.file_header_bg_rest, p.file_header_fg_rest)
    );

    assert_eq!(
        p.hunk_divider_colors(true, false),
        (p.hunk_divider_bg_selected, p.hunk_divider_fg_selected)
    );
    assert_eq!(
        p.hunk_divider_colors(false, true),
        (p.hunk_divider_bg_current, p.hunk_divider_fg)
    );
    assert_eq!(
        p.hunk_divider_colors(false, false),
        (p.hunk_divider_bg_rest, p.hunk_divider_fg)
    );

    assert_eq!(p.named_color("warning"), Some(Color::Fixed(220)));
    assert_eq!(p.named_color("note"), Some(Color::Fixed(220)));
    assert_eq!(p.named_color("note_tint_bg"), Some(Color::Rgb(76, 64, 20)));
    assert_eq!(p.named_color("nonexistent"), None);
    assert_eq!(
        DiffuseTheme::fallback_named_color("note"),
        Some("\x1b[38;5;220m")
    );
    assert_eq!(
        DiffuseTheme::fallback_named_color("note_tint_bg"),
        Some("\x1b[48;2;76;64;20m")
    );
}

#[test]
fn test_stacked_sgr_emission_order_and_reset() {
    let theme = DiffuseTheme::DEFAULT;
    let mut out = String::new();
    theme.write_token_stacked("mdBold", "Title", "ctx", None, &[1, 4], &mut out);
    assert!(
        out.starts_with("\x1b[1;4m"),
        "combined on-sequence: {out:?}"
    );
    assert!(out.contains("Title"), "{out:?}");
    assert!(
        out.ends_with("\x1b[24m\x1b[22m"),
        "reverse-order offs: {out:?}"
    );
}

#[test]
fn test_table_header_token_bold_underline_and_reset() {
    let theme = DiffuseTheme::DEFAULT;
    let tok = theme.render_token("mdItalic", "Header", "ctx", None, Some(14));
    assert!(
        tok.starts_with("\x1b[1;4m"),
        "must start with bold+underline SGR: {tok:?}"
    );
    assert!(
        tok.contains("\x1b[38;5;229m"),
        "must use wheat yellow 229 fg: {tok:?}"
    );
    assert!(
        tok.ends_with("\x1b[24;22m"),
        "must end with underline+bold reset: {tok:?}"
    );
}

#[test]
fn test_muted_color_resolution() {
    let theme = DiffuseTheme::DEFAULT;
    for rk in ["ctx", "add", "del", "mod"] {
        let tok = theme.render_token("muted", "|", rk, None, None);
        assert_eq!(tok, "\x1b[38;5;243m|\x1b[39m");
    }
}
