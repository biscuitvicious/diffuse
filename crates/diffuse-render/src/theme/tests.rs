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
    assert_eq!(palette.ctx_fg, Color::Primary);
    assert_eq!(palette.syntax_text, Color::Primary);

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

#[test]
fn test_palette_semantic_ui_fields() {
    let p = ThemePalette::default_palette();
    assert_eq!(p.status_bar_fg, Color::Fixed(252));
    assert_eq!(p.status_bar_dim, Color::Fixed(244));
    assert_eq!(p.status_bar_accent, Color::Fixed(220));
    assert_eq!(p.header_repo, Color::Fixed(117));
    assert_eq!(p.header_branch, Color::Fixed(252));
    assert_eq!(p.header_staged, Color::Fixed(120));
    assert_eq!(p.header_prefix, Color::Fixed(152));
    assert_eq!(p.ribbon_divider, Color::Fixed(244));
    assert_eq!(p.ribbon_text, Color::Fixed(244));
    assert_eq!(p.modal_bg, Color::Fixed(236));
    assert_eq!(p.modal_border, Color::Fixed(240));
    assert_eq!(p.modal_title, Color::Fixed(231));
    assert_eq!(p.count_dels, Color::Fixed(203));
    assert_eq!(p.count_adds, Color::Fixed(120));
    assert_eq!(p.count_mods, Color::Fixed(214));
    assert_eq!(p.count_movs, Color::Fixed(147));
    assert_eq!(p.count_notes, Color::Fixed(220));
}

#[test]
fn test_syntax_token_color_delegates_to_palette() {
    use diffuse_core::parse::RowKind;
    let mut p = ThemePalette::default_palette();

    // In default palette, RowKind::Add and Del map to predefined colors
    assert_eq!(
        p.syntax_token_color("syntaxKeyword", RowKind::Add),
        Some(Color::Fixed(147))
    );
    assert_eq!(
        p.syntax_token_color("syntaxKeyword", RowKind::Del),
        Some(Color::Fixed(183))
    );

    // Overriding the palette fields changes the resolved syntax color
    p.add_syntax_keyword = Color::Rgb(10, 20, 30);
    p.del_syntax_keyword = Color::Rgb(40, 50, 60);

    assert_eq!(
        p.syntax_token_color("syntaxKeyword", RowKind::Add),
        Some(Color::Rgb(10, 20, 30))
    );
    assert_eq!(
        p.syntax_token_color("syntaxKeyword", RowKind::Del),
        Some(Color::Rgb(40, 50, 60))
    );
}

#[test]
fn test_light_theme_palette_and_resolution() {
    let p = ThemePalette::light_palette();
    let d = ThemePalette::default_palette();

    // Verify key fields are distinct from dark palette
    assert_ne!(p.bg_base, d.bg_base);
    assert_ne!(p.ctx_fg, d.ctx_fg);
    assert_ne!(p.add_tint_bg, d.add_tint_bg);
    assert_ne!(p.add_fg, d.add_fg);
    assert_ne!(p.del_tint_bg, d.del_tint_bg);
    assert_ne!(p.del_fg, d.del_fg);
    assert_ne!(p.mod_tint_bg, d.mod_tint_bg);
    assert_ne!(p.mod_fg, d.mod_fg);
    assert_ne!(p.mov_tint_bg, d.mov_tint_bg);
    assert_ne!(p.mov_fg, d.mov_fg);

    let light_theme = DiffuseTheme::LIGHT;
    assert_eq!(light_theme.name, "light");
    assert_eq!(light_theme.palette, p);

    assert_eq!(DiffuseTheme::from_name("light"), Some(DiffuseTheme::LIGHT));
    assert_eq!(DiffuseTheme::from_name("LIGHT"), Some(DiffuseTheme::LIGHT));
    assert_eq!(DiffuseTheme::from_name("white"), Some(DiffuseTheme::LIGHT));

    // Verify row styles under light theme
    let add_style = light_theme.row_style(diffuse_core::parse::RowKind::Add);
    assert_eq!(add_style.tint_bg, p.add_tint_bg);
    assert_eq!(add_style.fg, p.add_fg);
    assert_eq!(add_style.fill_bg, Some(p.add_fill_bg));

    let del_style = light_theme.row_style(diffuse_core::parse::RowKind::Del);
    assert_eq!(del_style.tint_bg, p.del_tint_bg);
    assert_eq!(del_style.fg, p.del_fg);
}

#[test]
fn test_syntax_text_token_colors() {
    use diffuse_core::parse::RowKind;

    // Dark theme should NOT emit foreground escape sequences for unclassified
    // "text" tokens, preserving byte-for-byte golden fidelity.
    let dark = DiffuseTheme::DEFAULT;
    assert_eq!(dark.palette.syntax_token_color("text", RowKind::Ctx), None);
    assert_eq!(dark.palette.syntax_token_color("text", RowKind::Add), None);
    assert_eq!(dark.palette.syntax_token_color("text", RowKind::Del), None);
    assert_eq!(dark.palette.syntax_token_color("text", RowKind::Mod), None);
    assert_eq!(
        dark.render_token("text", "yansi", "ctx", None, None),
        "yansi"
    );

    // Light theme MUST resolve "text" tokens to high-contrast colors (e.g. dark
    // charcoal #24292f) so identifiers like "yansi" in `use yansi::Color;`
    // are clearly legible on light backgrounds.
    let light = DiffuseTheme::LIGHT;
    assert_eq!(
        light.palette.syntax_token_color("text", RowKind::Ctx),
        Some(light.palette.syntax_text)
    );
    assert_eq!(
        light.palette.syntax_token_color("text", RowKind::Add),
        Some(light.palette.add_syntax_text)
    );
    assert_eq!(
        light.palette.syntax_token_color("text", RowKind::Del),
        Some(light.palette.del_syntax_text)
    );
    assert_eq!(
        light.palette.syntax_token_color("text", RowKind::Mod),
        Some(light.palette.syntax_text)
    );
    let yansi_rendered = light.render_token("text", "yansi", "ctx", None, None);
    assert!(
        yansi_rendered.contains("\x1b[38;2;36;41;47m"),
        "yansi in light theme must have dark charcoal fg: {yansi_rendered:?}"
    );
}

fn srgb_to_linear(val: u8) -> f64 {
    let c = val as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn color_luminance(c: Color) -> f64 {
    match c {
        Color::Rgb(r, g, b) => {
            0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
        }
        _ => 0.5,
    }
}

fn contrast_ratio(c1: Color, c2: Color) -> f64 {
    let l1 = color_luminance(c1);
    let l2 = color_luminance(c2);
    let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (lighter + 0.05) / (darker + 0.05)
}

#[test]
fn test_light_palette_contrast_ratios() {
    let p = ThemePalette::light_palette();

    // WCAG AA requirement for normal text is >= 4.5:1
    assert!(
        contrast_ratio(p.ctx_fg, p.bg_base) >= 4.5,
        "ctx_fg must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.add_fg, p.bg_base) >= 4.5,
        "add_fg must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.del_fg, p.bg_base) >= 4.5,
        "del_fg must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.syntax_keyword, p.bg_base) >= 4.5,
        "syntax_keyword must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.syntax_function, p.bg_base) >= 4.5,
        "syntax_function must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.syntax_string, p.bg_base) >= 4.5,
        "syntax_string must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.syntax_number, p.bg_base) >= 4.5,
        "syntax_number must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.syntax_type, p.bg_base) >= 4.5,
        "syntax_type must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.modal_text, p.modal_bg) >= 4.5,
        "modal_text must have >= 4.5 contrast against modal_bg"
    );
    assert!(
        contrast_ratio(p.status_bar_fg, p.status_bar_bg) >= 4.5,
        "status_bar_fg must have >= 4.5 contrast against status_bar_bg"
    );
    assert!(
        contrast_ratio(p.count_adds, p.bg_base) >= 4.5,
        "count_adds must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.count_dels, p.bg_base) >= 4.5,
        "count_dels must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.syntax_text, p.bg_base) >= 4.5,
        "syntax_text must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(p.add_syntax_text, p.add_tint_bg) >= 4.5,
        "add_syntax_text must have >= 4.5 contrast against add_tint_bg"
    );
    assert!(
        contrast_ratio(p.del_syntax_text, p.del_tint_bg) >= 4.5,
        "del_syntax_text must have >= 4.5 contrast against del_tint_bg"
    );
}

#[test]
fn test_colorfgbg_parsing() {
    assert_eq!(parse_colorfgbg("15;0"), Some(TerminalMode::Dark));
    assert_eq!(parse_colorfgbg("0;15"), Some(TerminalMode::Light));
    assert_eq!(parse_colorfgbg("15;default;0"), Some(TerminalMode::Dark));
    assert_eq!(parse_colorfgbg("0;default;15"), Some(TerminalMode::Light));
    assert_eq!(parse_colorfgbg("15;7"), Some(TerminalMode::Light));
    assert_eq!(parse_colorfgbg("15;8"), Some(TerminalMode::Dark));
    assert_eq!(parse_colorfgbg("15;232"), Some(TerminalMode::Dark));
    assert_eq!(parse_colorfgbg("0;255"), Some(TerminalMode::Light));
    assert_eq!(parse_colorfgbg("invalid"), None);
    assert_eq!(parse_colorfgbg(""), None);
    assert_eq!(parse_colorfgbg("15;"), None);
}

#[test]
fn test_term_background_parsing() {
    assert_eq!(parse_term_background("light"), Some(TerminalMode::Light));
    assert_eq!(parse_term_background("LIGHT"), Some(TerminalMode::Light));
    assert_eq!(parse_term_background("white"), Some(TerminalMode::Light));
    assert_eq!(parse_term_background("dark"), Some(TerminalMode::Dark));
    assert_eq!(parse_term_background("DARK"), Some(TerminalMode::Dark));
    assert_eq!(parse_term_background("black"), Some(TerminalMode::Dark));
    assert_eq!(parse_term_background("invalid"), None);
}

#[test]
fn test_osc11_response_parsing() {
    // 4-digit hex
    assert_eq!(
        parse_osc11_response("\x1b]11;rgb:ffff/ffff/ffff\x1b\\"),
        Some((65535, 65535, 65535))
    );
    assert_eq!(
        parse_osc11_response("\x1b]11;rgb:0000/0000/0000\x07"),
        Some((0, 0, 0))
    );
    assert_eq!(
        parse_osc11_response("\x1b]11;rgb:1234/5678/9abc\x1b\\"),
        Some((0x1234, 0x5678, 0x9abc))
    );

    // 2-digit hex
    assert_eq!(
        parse_osc11_response("\x1b]11;rgb:ff/ee/dd\x1b\\"),
        Some((0xffff, 0xeeee, 0xdddd))
    );

    // 1-digit hex
    assert_eq!(
        parse_osc11_response("\x1b]11;rgb:f/e/d\x1b\\"),
        Some((0xffff, 0xeeee, 0xdddd))
    );

    // Malformed cases
    assert_eq!(parse_osc11_response(""), None);
    assert_eq!(parse_osc11_response("invalid"), None);
    assert_eq!(parse_osc11_response("\x1b]11;rgb:1234/5678\x1b\\"), None);
    assert_eq!(
        parse_osc11_response("\x1b]11;rgb:1234/5678/9abc/def0\x1b\\"),
        None
    );
    assert_eq!(
        parse_osc11_response("\x1b]11;rgb:gggg/0000/0000\x1b\\"),
        None
    );
}

#[test]
fn test_luminance_and_classification() {
    assert_eq!(classify_luminance(0, 0, 0), TerminalMode::Dark);
    assert_eq!(classify_luminance(65535, 65535, 65535), TerminalMode::Light);
    // Dark gray
    assert_eq!(classify_luminance(8000, 8000, 8000), TerminalMode::Dark);
    // Pure white
    assert!(relative_luminance(65535, 65535, 65535) > 0.99);
    // Pure black
    assert!(relative_luminance(0, 0, 0) < 0.01);
}

#[test]
fn test_theme_auto_resolution() {
    assert!(DiffuseTheme::from_name("auto").is_some());
    let auto_theme = DiffuseTheme::auto_detect();
    assert!(auto_theme == DiffuseTheme::DEFAULT || auto_theme == DiffuseTheme::LIGHT);
}
