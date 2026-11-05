use std::{env, sync::Mutex};

use diffuse_syntax::color::TokenColor;

use super::{
    baseline::{DARK, LIGHT},
    *,
};

/// Serializes the environment-touching tests below: process env is global,
/// so parallel auto-detect tests would otherwise observe each other's
/// `DIFFUSE_THEME`/`COLORFGBG` values.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_default_theme_token_colors() {
    let theme = DiffuseTheme::dark();
    let kw_add = theme.render_token(TokenColor::Keyword, "fn", "add", None, None);
    assert_eq!(kw_add, "\x1b[38;5;147mfn\x1b[39m");

    let kw_del = theme.render_token(TokenColor::Keyword, "fn", "del", None, None);
    assert_eq!(kw_del, "\x1b[38;5;183mfn\x1b[39m");

    let kw_ctx = theme.render_token(TokenColor::Keyword, "fn", "ctx", None, None);
    assert_eq!(kw_ctx, "\x1b[38;5;147mfn\x1b[39m");

    let ty_add = theme.render_token(TokenColor::Type, "Color", "add", None, None);
    assert_eq!(ty_add, "\x1b[38;5;220mColor\x1b[39m");

    let ty_del = theme.render_token(TokenColor::Type, "Color", "del", None, None);
    assert_eq!(ty_del, "\x1b[38;5;215mColor\x1b[39m");

    let ty_ctx = theme.render_token(TokenColor::Type, "Color", "ctx", None, None);
    assert_eq!(ty_ctx, "\x1b[38;5;220mColor\x1b[39m");

    let num_add = theme.render_token(TokenColor::Number, "42", "add", None, None);
    assert_eq!(num_add, "\x1b[38;5;221m42\x1b[39m");

    let num_del = theme.render_token(TokenColor::Number, "42", "del", None, None);
    assert_eq!(num_del, "\x1b[38;5;222m42\x1b[39m");
}

#[test]
fn test_sgr_modifiers_application_and_reset() {
    let theme = DiffuseTheme::dark();
    let bold_tok = theme.render_token(TokenColor::Bold, "heading", "ctx", None, Some(1));
    assert!(bold_tok.starts_with("\x1b[1m"));
    assert!(bold_tok.ends_with("\x1b[22m"));

    let strike_tok = theme.render_token(TokenColor::Strike, "removed", "ctx", None, Some(9));
    assert_eq!(strike_tok, "\x1b[2;9mremoved\x1b[29m\x1b[22m");
}

#[test]
fn test_strikethrough_diff_token_dim_strike_no_clashing_foreground() {
    let theme = DiffuseTheme::dark();
    for (row_kind, tok_kind) in [
        ("ctx", "ctx"),
        ("add", "add"),
        ("del", "del"),
        ("mod", "ctx"),
    ] {
        let tok = theme.render_token(
            TokenColor::Strike,
            "deprecated",
            row_kind,
            Some(tok_kind),
            Some(9),
        );
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
    let dark = DiffuseTheme::dark();
    let palette = dark.palette;
    let rest = &palette[Cursor::Rest];
    let row = &palette[Cursor::Row];
    // Row surfaces & foregrounds (rest tier) and cursor backgrounds (row
    // tier), frozen from the legacy flat values.
    assert_eq!(rest[DiffKind::Add].surface.fg, Color::Fixed(120));
    assert_eq!(rest[DiffKind::Del].surface.fg, Color::Fixed(203));
    assert_eq!(rest[DiffKind::Add].surface.bg, Color::Rgb(28, 50, 30));
    assert_eq!(rest[DiffKind::Del].surface.bg, Color::Rgb(38, 12, 14));
    assert_eq!(
        rest[DiffKind::Add].surface.fill,
        Some(Color::Rgb(30, 96, 50))
    );
    assert_eq!(
        row[DiffKind::Add].surface.bg,
        Color::Rgb(46, 74, 48),
        "add cursor background"
    );
    assert_eq!(
        row[DiffKind::Add].surface.fill,
        Some(Color::Rgb(37, 120, 65))
    );
    assert_eq!(
        rest[DiffKind::Mod].surface.fill,
        Some(Color::Rgb(30, 96, 50))
    );
    assert_eq!(
        row[DiffKind::Mod].surface.fill,
        Some(Color::Rgb(37, 120, 65))
    );
    assert_eq!(rest[DiffKind::Mod].surface.bg, Color::Rgb(38, 38, 38));
    assert_eq!(row[DiffKind::Mod].surface.bg, Color::Rgb(66, 66, 66));
    // Context canvas: cursor and base foreground.
    assert_eq!(row[DiffKind::Ctx].surface.bg, Color::Fixed(237));
    assert_eq!(rest[DiffKind::Ctx].surface.fg, Color::Primary);
    // UI: base text on the dark canvas is the terminal default.
    assert_eq!(palette.ui.text, Color::Primary);
    assert_eq!(dark.name, "default");
}
#[test]
fn test_row_style_methods() {
    let theme = DiffuseTheme::dark();
    let palette = theme.palette;
    let rest = &palette[Cursor::Rest];
    let row = &palette[Cursor::Row];

    let add_style = theme.row_style(diffuse_core::parse::RowKind::Add);
    assert_eq!(add_style.tint_bg, rest[DiffKind::Add].surface.bg);
    assert_eq!(add_style.cursor_bg, row[DiffKind::Add].surface.bg);
    assert_eq!(add_style.fill_bg, rest[DiffKind::Add].surface.fill);
    assert_eq!(add_style.fill_cursor_bg, row[DiffKind::Add].surface.fill);
    assert_eq!(add_style.fg, rest[DiffKind::Add].surface.fg);

    let del_style = theme.row_style(diffuse_core::parse::RowKind::Del);
    assert_eq!(del_style.fill_style(), None);
    assert_eq!(del_style.fill_cursor_style(), None);
}
#[test]
fn test_row_style_palette_parity() {
    // row_style reads through the theme palette (rest tier for
    // tint/underlying style, row tier for cursor backgrounds); output must
    // match the frozen legacy baseline values for every kind, in both modes.
    for theme in [DiffuseTheme::dark(), DiffuseTheme::light()] {
        let legacy = match theme.mode {
            Mode::Dark => &DARK,
            Mode::Light => &LIGHT,
        };
        let add = theme.row_style(diffuse_core::parse::RowKind::Add);
        assert_eq!(add.tint_bg, legacy.add_tint_bg);
        assert_eq!(add.cursor_bg, legacy.add_cursor_bg);
        assert_eq!(add.fill_bg, Some(legacy.add_fill_bg));
        assert_eq!(add.fill_cursor_bg, Some(legacy.add_fill_cursor_bg));
        assert_eq!(add.fg, legacy.add_fg, "{:?}", theme.name);

        let del = theme.row_style(diffuse_core::parse::RowKind::Del);
        assert_eq!(del.tint_bg, legacy.del_tint_bg);
        assert_eq!(del.cursor_bg, legacy.del_cursor_bg);
        assert_eq!(del.fill_bg, None, "del has no fill (dead legacy field)");
        assert_eq!(del.fill_cursor_bg, None);
        assert_eq!(del.fg, legacy.del_fg);

        let r#mod = theme.row_style(diffuse_core::parse::RowKind::Mod);
        assert_eq!(r#mod.tint_bg, legacy.mod_tint_bg);
        assert_eq!(r#mod.cursor_bg, legacy.mod_cursor_bg);
        assert_eq!(r#mod.fill_bg, Some(legacy.mod_fill_bg));
        assert_eq!(r#mod.fill_cursor_bg, Some(legacy.mod_fill_cursor_bg));
        assert_eq!(r#mod.fg, legacy.mod_fg);

        let mov = theme.row_style(diffuse_core::parse::RowKind::Mov);
        assert_eq!(mov.tint_bg, legacy.mov_tint_bg);
        assert_eq!(mov.cursor_bg, legacy.mov_cursor_bg);
        assert_eq!(mov.fill_bg, None);
        assert_eq!(mov.fill_cursor_bg, None);
        assert_eq!(mov.fg, legacy.mov_fg);
    }
}
#[test]
fn test_theme_paired_mode_toggle() {
    // Phase 6: the `t`-key mode toggle resolves the opposite-mode pair
    // declared by the theme's asset metadata, not hardcoded constants.
    assert_eq!(
        DiffuseTheme::dark().paired(),
        Some(DiffuseTheme::light()),
        "dark pairs to light"
    );
    assert_eq!(
        DiffuseTheme::light().paired(),
        Some(DiffuseTheme::dark()),
        "light pairs to dark"
    );
    // Round trip: toggling twice returns the original theme.
    assert_eq!(
        DiffuseTheme::dark().paired().and_then(|t| t.paired()),
        Some(DiffuseTheme::dark())
    );
}

#[test]
fn test_theme_from_name_and_env() {
    let _guard = ENV_MUTEX.lock().unwrap();
    assert_eq!(
        DiffuseTheme::from_name("default"),
        Some(DiffuseTheme::dark())
    );
    assert_eq!(DiffuseTheme::from_name("DARK"), Some(DiffuseTheme::dark()));
    assert_eq!(DiffuseTheme::from_name("black"), Some(DiffuseTheme::dark()));
    assert_eq!(DiffuseTheme::from_name("nonexistent"), None);
    assert_eq!(DiffuseTheme::from_name(""), None);
    assert_eq!(DiffuseTheme::from_name("unknown-theme"), None);

    unsafe {
        env::set_var("DIFFUSE_THEME", "default");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::dark());
        env::remove_var("DIFFUSE_THEME");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::dark());
        env::set_var("DIFFUSE_THEME", "invalid");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::dark());
        env::remove_var("DIFFUSE_THEME");
    }
}

#[test]
fn test_stacked_sgr_emission_order_and_reset() {
    let theme = DiffuseTheme::dark();
    let mut out = String::new();
    theme.write_token_stacked(TokenColor::Bold, "Title", "ctx", None, &[1, 4], &mut out);
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
    let theme = DiffuseTheme::dark();
    let tok = theme.render_token(TokenColor::TableHeader, "Header", "ctx", None, Some(14));
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
fn test_code_span_resolves_to_dedicated_code_palette_key() {
    let theme = DiffuseTheme::dark();
    let code_ctx = theme.render_token(TokenColor::Code, "code", "ctx", None, None);
    assert_eq!(code_ctx, "\x1b[38;5;81mcode\x1b[39m");
    // Row-aware overrides resolve to the code palette keys.
    let ctx = theme.palette[Cursor::Rest][DiffKind::Ctx].syntax;
    let add = theme.palette[Cursor::Rest][DiffKind::Add].syntax;
    let del = theme.palette[Cursor::Rest][DiffKind::Del].syntax;
    assert_eq!(ctx.token_color(TokenColor::Code), Some(ctx.code));
    assert_eq!(add.token_color(TokenColor::Code), Some(add.code));
    assert_eq!(del.token_color(TokenColor::Code), Some(del.code));
    // Code is distinct from string so spans are visually separable.
    assert_ne!(ctx.code, ctx.string);
    assert_ne!(add.code, add.string);
    // Deletions keep the red string tint for consistency.
    assert_eq!(del.code, del.string);
}
#[test]
fn test_table_border_and_header_aliases_resolve_to_palette_keys() {
    let theme = DiffuseTheme::dark();
    let ctx = theme.palette[Cursor::Rest][DiffKind::Ctx].syntax;
    let add = theme.palette[Cursor::Rest][DiffKind::Add].syntax;
    let del = theme.palette[Cursor::Rest][DiffKind::Del].syntax;
    assert_eq!(
        ctx.token_color(TokenColor::TableBorder),
        Some(ctx.table_border)
    );
    assert_eq!(
        add.token_color(TokenColor::TableBorder),
        Some(add.table_border)
    );
    assert_eq!(
        del.token_color(TokenColor::TableBorder),
        Some(del.table_border)
    );

    assert_eq!(
        ctx.token_color(TokenColor::TableHeader),
        Some(ctx.table_header)
    );
    assert_eq!(
        add.token_color(TokenColor::TableHeader),
        Some(add.table_header)
    );
    assert_eq!(
        del.token_color(TokenColor::TableHeader),
        Some(del.table_header)
    );
}
#[test]
fn test_link_border_aliases_resolve_to_palette_key() {
    let theme = DiffuseTheme::dark();
    let ctx = theme.palette[Cursor::Rest][DiffKind::Ctx].syntax;
    let add = theme.palette[Cursor::Rest][DiffKind::Add].syntax;
    let del = theme.palette[Cursor::Rest][DiffKind::Del].syntax;
    // Border tokens share the theme's default border color (legacy
    // `DEFAULT_BORDER_COLOR_DARK`).
    assert_eq!(ctx.table_border, Color::Fixed(243));
    assert_eq!(ctx.link_border, Color::Fixed(243));
    assert_eq!(add.table_border, Color::Fixed(243));
    assert_eq!(del.table_border, Color::Fixed(243));
    assert_eq!(
        ctx.token_color(TokenColor::LinkBorder),
        Some(ctx.link_border)
    );
    assert_eq!(
        add.token_color(TokenColor::LinkBorder),
        Some(add.link_border)
    );
    assert_eq!(
        del.token_color(TokenColor::LinkBorder),
        Some(del.link_border)
    );
}
#[test]
fn test_quote_and_fence_border_aliases_resolve_to_palette_keys() {
    let dark = DiffuseTheme::dark();
    let light = DiffuseTheme::light();
    // All border keys share the per-theme default border color (legacy
    // `DEFAULT_BORDER_COLOR_{DARK,LIGHT}`).
    for syntax in [
        dark.palette[Cursor::Rest][DiffKind::Ctx].syntax,
        light.palette[Cursor::Rest][DiffKind::Ctx].syntax,
    ] {
        assert_eq!(syntax.quote_border, syntax.table_border);
        assert_eq!(syntax.fence_border, syntax.table_border);
        assert_eq!(syntax.link_border, syntax.table_border);
    }
    assert_eq!(
        dark.palette[Cursor::Rest][DiffKind::Ctx]
            .syntax
            .table_border,
        Color::Fixed(243)
    );
    assert_eq!(
        light.palette[Cursor::Rest][DiffKind::Ctx]
            .syntax
            .table_border,
        Color::Rgb(101, 109, 118)
    );
    let ctx = dark.palette[Cursor::Rest][DiffKind::Ctx].syntax;
    let add = dark.palette[Cursor::Rest][DiffKind::Add].syntax;
    let del = dark.palette[Cursor::Rest][DiffKind::Del].syntax;
    assert_eq!(
        ctx.token_color(TokenColor::QuoteBorder),
        Some(ctx.quote_border)
    );
    assert_eq!(
        add.token_color(TokenColor::QuoteBorder),
        Some(add.quote_border)
    );
    assert_eq!(
        del.token_color(TokenColor::QuoteBorder),
        Some(del.quote_border)
    );

    assert_eq!(
        ctx.token_color(TokenColor::FenceBorder),
        Some(ctx.fence_border)
    );
    assert_eq!(
        add.token_color(TokenColor::FenceBorder),
        Some(add.fence_border)
    );
    assert_eq!(
        del.token_color(TokenColor::FenceBorder),
        Some(del.fence_border)
    );
}
#[test]
fn test_palette_semantic_ui_fields() {
    let p = DiffuseTheme::dark().palette.ui;
    assert_eq!(p.status_bar_fg, Color::Fixed(252));
    assert_eq!(p.status_bar_dim, Color::Fixed(244));
    assert_eq!(p.status_bar_accent, Color::Fixed(220));
    assert_eq!(p.header_repo, Color::Fixed(117));
    assert_eq!(p.header_branch, Color::Fixed(252));
    assert_eq!(p.header_staged, Color::Fixed(120));
    assert_eq!(p.header_prefix, Color::Fixed(152));
    assert_eq!(p.ribbon_divider, Color::Fixed(244));
    assert_eq!(p.ribbon_text, Color::Fixed(244));
    assert_eq!(p.bg_modal, Color::Fixed(236));
    assert_eq!(p.modal_border, Color::Fixed(240));
    assert_eq!(p.modal_title, Color::Fixed(231));
    assert_eq!(p.count_dels, Color::Fixed(203));
    assert_eq!(p.count_adds, Color::Fixed(120));
    assert_eq!(p.count_mods, Color::Fixed(214));
    assert_eq!(p.count_movs, Color::Fixed(147));
    assert_eq!(p.count_notes, Color::Fixed(220));
}
#[test]
fn test_delim_palette_keys() {
    for theme in [DiffuseTheme::dark(), DiffuseTheme::light()] {
        let ctx = theme.palette[Cursor::Rest][DiffKind::Ctx].syntax;
        let add = theme.palette[Cursor::Rest][DiffKind::Add].syntax;
        let del = theme.palette[Cursor::Rest][DiffKind::Del].syntax;
        assert_eq!(ctx.token_color(TokenColor::Delimiter), Some(ctx.delim));
        assert_eq!(add.token_color(TokenColor::Delimiter), Some(add.delim));
        assert_eq!(del.token_color(TokenColor::Delimiter), Some(del.delim));
    }
}
#[test]
fn test_channel_syntax_palettes_resolve_and_override_independently() {
    let theme = DiffuseTheme::dark();
    let ctx = theme.palette[Cursor::Rest][DiffKind::Ctx].syntax;
    let add = theme.palette[Cursor::Rest][DiffKind::Add].syntax;
    let del = theme.palette[Cursor::Rest][DiffKind::Del].syntax;

    // In the default theme, the add and del channels map to predefined
    // per-kind keyword colors.
    assert_eq!(
        add.token_color(TokenColor::Keyword),
        Some(Color::Fixed(147))
    );
    assert_eq!(
        del.token_color(TokenColor::Keyword),
        Some(Color::Fixed(183))
    );
    // Unstyled default tokens on the dark theme return None (use the
    // terminal default text).
    assert_eq!(ctx.token_color(TokenColor::Default), None);
    assert_eq!(add.token_color(TokenColor::Default), None);
    assert_eq!(del.token_color(TokenColor::Default), None);

    // The syntax palettes are `Copy`, so an overridden clone resolves
    // standalone without touching the shared palette.
    let mut add_overlay = add;
    add_overlay.keyword = Color::Rgb(10, 20, 30);
    assert_eq!(
        add_overlay.token_color(TokenColor::Keyword),
        Some(Color::Rgb(10, 20, 30))
    );
    assert_eq!(
        theme.palette[Cursor::Rest][DiffKind::Add]
            .syntax
            .token_color(TokenColor::Keyword),
        Some(Color::Fixed(147)),
        "the shared palette is unaffected by the local overlay"
    );
}
#[test]
fn test_light_theme_palette_and_resolution() {
    let light = DiffuseTheme::light();
    let dark = DiffuseTheme::dark();
    let lp = light.palette;
    let dp = dark.palette;

    // Verify key fields are distinct from the dark palette.
    assert_ne!(lp.ui.bg_base, dp.ui.bg_base);
    assert_ne!(
        lp[Cursor::Rest][DiffKind::Ctx].surface.fg,
        dp[Cursor::Rest][DiffKind::Ctx].surface.fg
    );
    assert_ne!(
        lp[Cursor::Rest][DiffKind::Add].surface.bg,
        dp[Cursor::Rest][DiffKind::Add].surface.bg
    );
    assert_ne!(
        lp[Cursor::Rest][DiffKind::Add].surface.fg,
        dp[Cursor::Rest][DiffKind::Add].surface.fg
    );
    assert_ne!(
        lp[Cursor::Rest][DiffKind::Del].surface.bg,
        dp[Cursor::Rest][DiffKind::Del].surface.bg
    );
    assert_ne!(
        lp[Cursor::Rest][DiffKind::Del].surface.fg,
        dp[Cursor::Rest][DiffKind::Del].surface.fg
    );
    assert_ne!(
        lp[Cursor::Rest][DiffKind::Mod].surface.bg,
        dp[Cursor::Rest][DiffKind::Mod].surface.bg
    );
    assert_ne!(
        lp[Cursor::Rest][DiffKind::Mov].surface.bg,
        dp[Cursor::Rest][DiffKind::Mov].surface.bg
    );

    assert_eq!(light.name, "light");

    assert_eq!(DiffuseTheme::from_name("light"), Some(light));
    assert_eq!(DiffuseTheme::from_name("LIGHT"), Some(light));
    assert_eq!(DiffuseTheme::from_name("white"), Some(light));

    // Verify row styles under light theme.
    let add_style = light.row_style(diffuse_core::parse::RowKind::Add);
    assert_eq!(
        add_style.tint_bg,
        lp[Cursor::Rest][DiffKind::Add].surface.bg
    );
    assert_eq!(add_style.fg, lp[Cursor::Rest][DiffKind::Add].surface.fg);
    assert_eq!(
        add_style.fill_bg,
        lp[Cursor::Rest][DiffKind::Add].surface.fill
    );

    let del_style = light.row_style(diffuse_core::parse::RowKind::Del);
    assert_eq!(
        del_style.tint_bg,
        lp[Cursor::Rest][DiffKind::Del].surface.bg
    );
    assert_eq!(del_style.fg, lp[Cursor::Rest][DiffKind::Del].surface.fg);
}
#[test]
fn test_syntax_text_token_colors() {
    use diffuse_core::parse::RowKind;

    let syntax = |theme: &DiffuseTheme, rk: RowKind| {
        theme.palette[Cursor::Rest][DiffKind::from_row(rk)].syntax
    };

    // Dark theme should NOT emit foreground escape sequences for unclassified
    // default tokens, preserving byte-for-byte golden fidelity.
    let dark = DiffuseTheme::dark();
    assert_eq!(
        syntax(&dark, RowKind::Ctx).token_color(TokenColor::Default),
        None
    );
    assert_eq!(
        syntax(&dark, RowKind::Add).token_color(TokenColor::Default),
        None
    );
    assert_eq!(
        syntax(&dark, RowKind::Del).token_color(TokenColor::Default),
        None
    );
    assert_eq!(
        syntax(&dark, RowKind::Mod).token_color(TokenColor::Default),
        None
    );
    assert_eq!(
        dark.render_token(TokenColor::Default, "yansi", "ctx", None, None),
        "yansi"
    );

    // Light theme MUST resolve default text tokens to high-contrast colors
    // (e.g. dark charcoal #24292f) so identifiers like "yansi" in `use
    // yansi::Color;` are clearly legible on light backgrounds.
    let light = DiffuseTheme::light();
    assert_eq!(
        syntax(&light, RowKind::Ctx).token_color(TokenColor::Default),
        Some(syntax(&light, RowKind::Ctx).text)
    );
    assert_eq!(
        syntax(&light, RowKind::Add).token_color(TokenColor::Default),
        Some(syntax(&light, RowKind::Add).text)
    );
    assert_eq!(
        syntax(&light, RowKind::Del).token_color(TokenColor::Default),
        Some(syntax(&light, RowKind::Del).text)
    );
    // Mod rows inherit the ctx channel, matching legacy semantics.
    assert_eq!(
        syntax(&light, RowKind::Mod).token_color(TokenColor::Default),
        Some(syntax(&light, RowKind::Ctx).text)
    );
    let yansi_rendered = light.render_token(TokenColor::Default, "yansi", "ctx", None, None);
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
    let t = DiffuseTheme::light();
    let bg_base = t.palette.ui.bg_base;
    let ctx = t.palette[Cursor::Rest][DiffKind::Ctx].syntax;
    let add = t.palette[Cursor::Rest][DiffKind::Add].syntax;
    let del = t.palette[Cursor::Rest][DiffKind::Del].syntax;
    let add_tint = t.palette[Cursor::Rest][DiffKind::Add].surface.bg;
    let del_tint = t.palette[Cursor::Rest][DiffKind::Del].surface.bg;

    // WCAG AA requirement for normal text is >= 4.5:1
    macro_rules! ratio {
        ($expr:expr, $name:literal) => {
            assert!(
                contrast_ratio($expr, bg_base) >= 4.5,
                concat!($name, " must have >= 4.5 contrast")
            );
        };
    }
    assert!(
        contrast_ratio(t.palette[Cursor::Rest][DiffKind::Ctx].surface.fg, bg_base) >= 4.5,
        "ctx fg must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(t.palette[Cursor::Rest][DiffKind::Add].surface.fg, bg_base) >= 4.5,
        "add fg must have >= 4.5 contrast"
    );
    assert!(
        contrast_ratio(t.palette[Cursor::Rest][DiffKind::Del].surface.fg, bg_base) >= 4.5,
        "del fg must have >= 4.5 contrast"
    );
    ratio!(ctx.keyword, "keyword");
    ratio!(ctx.function, "function");
    ratio!(ctx.string, "string");
    ratio!(ctx.code, "code");
    ratio!(ctx.table_border, "table_border");
    ratio!(ctx.table_header, "table_header");
    ratio!(ctx.number, "number");
    ratio!(ctx.typ, "typ");
    assert!(
        contrast_ratio(t.palette.ui.modal_text, t.palette.ui.bg_modal) >= 4.5,
        "modal_text must have >= 4.5 contrast against bg_modal"
    );
    assert!(
        contrast_ratio(t.palette.ui.status_bar_fg, t.palette.ui.status_bar_bg) >= 4.5,
        "status_bar_fg must have >= 4.5 contrast against status_bar_bg"
    );
    ratio!(t.palette.ui.count_adds, "count_adds");
    ratio!(t.palette.ui.count_dels, "count_dels");
    ratio!(t.palette.ui.text, "text");
    assert!(
        contrast_ratio(add.text, add_tint) >= 4.5,
        "add text must have >= 4.5 contrast against the add tint"
    );
    assert!(
        contrast_ratio(del.text, del_tint) >= 4.5,
        "del text must have >= 4.5 contrast against the del tint"
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
    assert!(auto_theme == DiffuseTheme::dark() || auto_theme == DiffuseTheme::light());
}

#[test]
fn test_theme_char_literal_rendering() {
    let dark = DiffuseTheme::dark();
    let chr_dark_add = dark.render_token(TokenColor::CharLiteral, "'x'", "add", None, None);
    let chr_dark_del = dark.render_token(TokenColor::CharLiteral, "'x'", "del", None, None);
    let chr_dark_ctx = dark.render_token(TokenColor::CharLiteral, "'x'", "ctx", None, None);
    assert_eq!(chr_dark_add, "\x1b[38;5;158m'x'\x1b[39m");
    assert_eq!(chr_dark_del, "\x1b[38;5;210m'x'\x1b[39m");
    assert_eq!(chr_dark_ctx, "\x1b[38;5;158m'x'\x1b[39m");

    let light = DiffuseTheme::light();
    let chr_light_add = light.render_token(TokenColor::CharLiteral, "'x'", "add", None, None);
    let chr_light_del = light.render_token(TokenColor::CharLiteral, "'x'", "del", None, None);
    let chr_light_ctx = light.render_token(TokenColor::CharLiteral, "'x'", "ctx", None, None);
    assert_eq!(chr_light_add, "\x1b[38;2;16;128;67m'x'\x1b[39m");
    assert_eq!(chr_light_del, "\x1b[38;2;190;45;55m'x'\x1b[39m");
    assert_eq!(chr_light_ctx, "\x1b[38;2;16;128;67m'x'\x1b[39m");
}

#[test]
fn test_theme_auto_interactive_matches_env_tiers() {
    let _guard = ENV_MUTEX.lock().unwrap();
    // Interactive auto honors explicit names without querying.
    unsafe {
        env::set_var("DIFFUSE_THEME", "light");
        assert_eq!(
            DiffuseTheme::auto_detect_interactive(),
            DiffuseTheme::light()
        );
        assert_eq!(DiffuseTheme::from_env_interactive(), DiffuseTheme::light());
        env::set_var("DIFFUSE_THEME", "dark");
        assert_eq!(DiffuseTheme::from_env_interactive(), DiffuseTheme::dark());
        // Explicit auto, invalid values, and unset all fall through to the
        // query-backed pipeline (a non-TTY test session resolves default).
        env::set_var("DIFFUSE_THEME", "auto");
        let auto_theme = DiffuseTheme::from_env_interactive();
        assert!(auto_theme == DiffuseTheme::dark() || auto_theme == DiffuseTheme::light());
        env::set_var("DIFFUSE_THEME", "invalid");
        let invalid_theme = DiffuseTheme::from_env_interactive();
        assert!(invalid_theme == DiffuseTheme::dark() || invalid_theme == DiffuseTheme::light());
        env::remove_var("DIFFUSE_THEME");
        let unset_theme = DiffuseTheme::from_env_interactive();
        assert!(unset_theme == DiffuseTheme::dark() || unset_theme == DiffuseTheme::light());
    }
}

#[test]
fn test_theme_from_name_auto_stays_query_free() {
    let _guard = ENV_MUTEX.lock().unwrap();
    // `auto` by name resolves through the environment-only pipeline, so
    // one-shot callers of `from_name` can never trigger a terminal query.
    unsafe {
        env::remove_var("DIFFUSE_THEME");
        env::remove_var("COLORFGBG");
        env::remove_var("TERM_BACKGROUND");
    }
    let named = DiffuseTheme::from_name("auto");
    assert!(named.is_some());
    let safe = DiffuseTheme::auto_detect();
    assert_eq!(named, Some(safe));
    // `DIFFUSE_THEME=auto` selects the same query-free pipeline (tier 1
    // falls through to the lower environment tiers, never to a query).
    unsafe {
        env::set_var("DIFFUSE_THEME", "auto");
        assert_eq!(DiffuseTheme::from_env(), DiffuseTheme::auto_detect());
        env::remove_var("DIFFUSE_THEME");
    }
}

#[test]
fn test_palette_resolves_by_theme_mode() {
    // Each mode compiles its own palette, cached as a static:
    // repeated construction resolves the same compiled instance.
    let dark = DiffuseTheme::dark();
    let light = DiffuseTheme::light();
    assert_eq!(dark.palette, DiffuseTheme::dark().palette, "dark is cached");
    assert_eq!(
        light.palette,
        DiffuseTheme::light().palette,
        "light is cached"
    );
    assert_eq!(dark.palette, dark.palette);

    // Banner states resolve per cursor tier from the frozen legacy
    // baseline values.
    assert_eq!(
        dark.palette[Cursor::Rest].file_header.bg,
        DARK.file_header_bg_rest
    );
    assert_eq!(
        dark.palette[Cursor::File].file_header.bg,
        DARK.file_header_bg_current
    );
    assert_eq!(
        dark.palette[Cursor::Row].file_header.bg,
        DARK.file_header_bg_selected
    );
}
#[test]
fn test_token_dispatch_matches_legacy() {
    // write_token dispatches through the theme palette now; output must
    // stay byte-identical for every row kind and token.
    let theme = DiffuseTheme::dark();
    for (row_kind, tok_kind) in [
        ("ctx", None),
        ("ctx", Some("ctx")),
        ("add", None),
        ("add", Some("add")),
        ("del", None),
        ("del", Some("del")),
        ("mod", None),
        ("", None),
        ("unknown", None),
    ] {
        for token in [
            TokenColor::Keyword,
            TokenColor::Comment,
            TokenColor::Url,
            TokenColor::Bold,
            TokenColor::Default,
        ] {
            assert_eq!(
                theme.render_token(token, "x", row_kind, tok_kind, None),
                {
                    let rk = if row_kind == "add" || tok_kind == Some("add") {
                        diffuse_core::parse::RowKind::Add
                    } else if row_kind == "del" || tok_kind == Some("del") {
                        diffuse_core::parse::RowKind::Del
                    } else if row_kind == "mod" {
                        diffuse_core::parse::RowKind::Mod
                    } else {
                        diffuse_core::parse::RowKind::Ctx
                    };
                    let mut out = String::new();
                    if let Some(c) = theme.palette[Cursor::Rest][DiffKind::from_row(rk)]
                        .syntax
                        .token_color(token)
                    {
                        out.push_str(&c.fg_code());
                        out.push('x');
                        out.push_str(crate::ansi::FG_RESET);
                    } else {
                        out.push('x');
                    }
                    out
                },
                "token {token:?} on row_kind {row_kind:?}/{tok_kind:?}"
            );
        }
    }
}
