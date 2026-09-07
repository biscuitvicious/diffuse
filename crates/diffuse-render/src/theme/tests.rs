use super::*;

#[test]
fn test_magic_theme_token_colors() {
    let theme = DiffuseTheme::MAGIC;
    let kw_add = theme.render_token("syntaxKeyword", "fn", "add", None, None);
    assert_eq!(kw_add, "\x1b[38;5;147mfn\x1b[39m");

    let kw_del = theme.render_token("syntaxKeyword", "fn", "del", None, None);
    assert_eq!(kw_del, "\x1b[38;5;183mfn\x1b[39m");

    let kw_ctx = theme.render_token("syntaxKeyword", "fn", "ctx", None, None);
    assert_eq!(kw_ctx, "\x1b[38;5;141mfn\x1b[39m");
}

#[test]
fn test_sgr_modifiers_application_and_reset() {
    let theme = DiffuseTheme::MAGIC;
    let bold_tok = theme.render_token("mdBold", "heading", "ctx", None, Some(1));
    assert!(bold_tok.starts_with("\x1b[1m"));
    assert!(bold_tok.ends_with("\x1b[22m"));

    let strike_tok = theme.render_token("mdStrike", "removed", "ctx", None, Some(9));
    assert!(strike_tok.starts_with("\x1b[9m"));
    assert!(strike_tok.ends_with("\x1b[29m"));
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
fn test_deep_black_theme_palette() {
    let palette = ThemePalette::deep_black();
    assert_eq!(palette.bg_base, Color::Fixed(233));
    assert_eq!(palette.add_fg, Color::Fixed(120));
    assert_eq!(palette.del_fg, Color::Fixed(203));
    assert_eq!(palette.add_tint_bg, Color::Rgb(29, 35, 31));
    assert_eq!(palette.del_tint_bg, Color::Rgb(48, 18, 20));

    let theme = DiffuseTheme::DEEP_BLACK;
    assert_eq!(theme.name, "deep_black");
}

#[test]
fn test_row_style_yansi_styles() {
    let s_add = S_ADD;
    assert_eq!(s_add.tint_bg, Color::Fixed(22));
    assert_eq!(s_add.cursor_bg, Color::Fixed(28));
    assert_eq!(s_add.fill_bg, Some(Color::Fixed(28)));
    assert_eq!(s_add.fill_cursor_bg, Some(Color::Fixed(34)));
    assert_eq!(s_add.fg, Color::Fixed(150));

    assert_eq!(s_add.tint_style(), Style::new().bg(Color::Fixed(22)));
    assert_eq!(s_add.cursor_style(), Style::new().bg(Color::Fixed(28)));
    assert_eq!(s_add.fill_style(), Some(Style::new().bg(Color::Fixed(28))));
    assert_eq!(
        s_add.fill_cursor_style(),
        Some(Style::new().bg(Color::Fixed(34)))
    );
    assert_eq!(s_add.fg_style(), Style::new().fg(Color::Fixed(150)));

    let s_del = S_DEL;
    assert_eq!(s_del.fill_style(), None);
    assert_eq!(s_del.fill_cursor_style(), None);
}

#[test]
fn test_theme_from_name_and_env() {
    assert_eq!(DiffuseTheme::from_name("magic"), Some(DiffuseTheme::MAGIC));
    assert_eq!(
        DiffuseTheme::from_name("deep_black"),
        Some(DiffuseTheme::DEEP_BLACK)
    );
    assert_eq!(
        DiffuseTheme::from_name("DARK"),
        Some(DiffuseTheme::DEEP_BLACK)
    );
    assert_eq!(
        DiffuseTheme::from_name("legacy"),
        Some(DiffuseTheme::LEGACY)
    );
    assert_eq!(DiffuseTheme::from_name("nonexistent"), None);

    unsafe {
        std::env::set_var("DIFFUSE_THEME", "deep_black");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::DEEP_BLACK);
        std::env::set_var("DIFFUSE_THEME", "magic");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::MAGIC);
        std::env::remove_var("DIFFUSE_THEME");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::MAGIC);
    }
}

#[test]
fn test_theme_palette_helpers() {
    let p = ThemePalette::default_magic();
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
        p.hunk_divider_colors(true),
        (p.hunk_divider_bg_current, p.hunk_divider_fg)
    );
    assert_eq!(
        p.hunk_divider_colors(false),
        (p.hunk_divider_bg_rest, p.hunk_divider_fg)
    );

    assert_eq!(p.named_color("warning"), Some(Color::Fixed(220)));
    assert_eq!(p.named_color("nonexistent"), None);
}
