//! Theme parsing: TOML loading, inheritance resolution, and diagnostics.
//!
//! Themes inherit from the bundled registry (`"dark"` or `"light"`) via the
//! `inherits` key. Entries in the registry declare no base themselves, so
//! inheritance chains are shallow and acyclic by construction at this stage;
//! deeper chains (and their cycle detection) arrive with declarative
//! user themes.

use thiserror::Error;

use super::{
    cascade,
    model::{Mode, ThemeModel},
};
use crate::theme::palette::scoped::ThemePalette as ScopedPalette;

/// Bundled canonical theme assets (phase 4), embedded at compile time.
const DARK_THEME_TOML: &str = include_str!("../../../themes/dark.toml");
const LIGHT_THEME_TOML: &str = include_str!("../../../themes/light.toml");

/// Registry of bundled builtin themes: the canonical embedded TOML assets.
///
/// A parse failure here is a compile-time-embedded asset regression and
/// panics by design (fail-fast invariant).
pub fn parse_builtin(name: &str) -> Option<ParsedTheme> {
    let source = match name {
        "default" | "dark" => DARK_THEME_TOML,
        "light" => LIGHT_THEME_TOML,
        _ => return None,
    };
    Some(parse_theme_str(source).expect("bundled builtin theme parses against the schema"))
}

/// Compose the `inherits` chain over the registry: the embedded asset for
/// every builtin name.
fn builtin(name: &str) -> Option<ThemeModel> {
    match name {
        "default" | "dark" => Some(builtin_model(DARK_THEME_TOML)),
        "light" => Some(builtin_model(LIGHT_THEME_TOML)),
        _ => None,
    }
}

/// Parse an embedded builtin asset, failing fast on schema regressions.
fn builtin_model(source: &'static str) -> ThemeModel {
    toml::from_str(source).expect("bundled builtin theme parses against the schema")
}

/// Errors encountered while parsing and cascading a declarative theme.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ThemeParseError {
    /// The document is not valid TOML or violates the theme schema.
    #[error("invalid theme: {0}")]
    Toml(#[from] toml::de::Error),

    /// A `$variable` reference (or bare swatch name) does not match any
    /// primitive in the effective `[palette]` table.
    #[error("unresolved palette variable '${name}' in '{path}'")]
    UnresolvedVariable {
        /// The referenced variable name, `$` prefix removed.
        name: String,
        /// Dotted path of the offending key (`{section}.{field}`).
        path: String,
    },

    /// A `syntax` map key is not part of the token color vocabulary.
    #[error("unknown syntax token key '{key}' in '{path}'")]
    UnknownToken {
        /// The unknown token key.
        key: String,
        /// Dotted path of the offending `syntax` table.
        path: String,
    },

    /// `inherits` names a theme outside the bundled registry.
    #[error("unknown inherited theme '{name}' (expected one of: dark, light)")]
    UnknownInherit {
        /// The unresolved base theme name.
        name: String,
    },
}

/// A parsed declarative theme, ready for scoped rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTheme {
    /// The fully resolved scoped palette.
    pub palette: ScopedPalette,
    /// Theme display name: the `name` key, else the inherited base name,
    /// else `"default"`.
    pub name: String,
    /// Effective application mode, selecting primitive and syntax baselines.
    pub mode: Mode,
}

/// Parse a declarative theme document into a scoped palette.
///
/// The document is validated against the schema, its `inherits` chain is
/// composed (self theme first, bases after), and every field is cascaded
/// through the cursor and diff-kind axes into a flat [`ScopedPalette`].
///
/// # Errors
///
/// [`ThemeParseError`] reports TOML/schema violations, unresolved palette
/// variables, unknown syntax token keys, and unknown inherited themes.
pub fn parse_theme_str(source: &str) -> Result<ParsedTheme, ThemeParseError> {
    let root: ThemeModel = toml::from_str(source)?;

    // Compose the inheritance chain: self first, then each declared base.
    // Registry entries declare no base of their own, so the loop terminates
    // and cycles are impossible.
    let mut models = Vec::new();
    let mut current = root;
    loop {
        let inherits = current.inherits.clone();
        models.push(current);
        match inherits.as_deref() {
            None => break,
            Some(name) => match builtin(name) {
                Some(base) => current = base,
                None => {
                    return Err(ThemeParseError::UnknownInherit {
                        name: name.to_string(),
                    });
                }
            },
        }
    }

    // Mode: the self theme's declaration, else the first declared in the
    // chain, else the dark baseline.
    let mode = models
        .iter()
        .find_map(|model| model.mode)
        .unwrap_or(Mode::Dark);

    // Name: self, else the nearest declared base name, else "default".
    let name = models
        .iter()
        .find_map(|model| model.name.clone())
        .unwrap_or_else(|| "default".to_string());

    let palette = cascade::compile(&models, mode)?;
    Ok(ParsedTheme {
        palette,
        name,
        mode,
    })
}

#[cfg(test)]
mod equivalence {
    //! Phase 4 acceptance: bundled assets reproduce the legacy flat
    //! palettes field-for-field through the scoped model.

    use diffuse_core::parse::RowKind;

    use crate::theme::{cursor::Cursor, diff::DiffKind, palette::ThemePalette as LegacyPalette};

    use super::{
        super::{cascade::TOKEN_KEYS, model::Mode},
        parse_theme_str,
    };

    /// Assert the inherited asset reproduces every legacy value: row
    /// surfaces and gutters, syntax tokens per row kind, banner states by
    /// cursor tier, and UI chrome.
    fn assert_asset_equivalence(mode: Mode) {
        let theme = parse_theme_str(match mode {
            Mode::Dark => "inherits = \"dark\"",
            Mode::Light => "inherits = \"light\"",
        })
        .unwrap();
        assert_eq!(theme.name, "default");
        assert_eq!(theme.mode, mode);
        let legacy = match mode {
            Mode::Dark => LegacyPalette::default_palette(),
            Mode::Light => LegacyPalette::light_palette(),
        };

        // Row surfaces & gutters: every tier cascades to the rest baseline
        // canonical values. Mov sides resolve the shared `mov` channel.
        for cursor in Cursor::cascade() {
            for (kind, _row_kind, bg, fill, fg, num, icon) in [
                (
                    DiffKind::Ctx,
                    RowKind::Ctx,
                    legacy.ctx_tint_bg,
                    None,
                    legacy.ctx_fg,
                    legacy.ctx_gutter_out_hunk,
                    legacy.ctx_gutter_out_hunk,
                ),
                (
                    DiffKind::Add,
                    RowKind::Add,
                    legacy.add_tint_bg,
                    Some(legacy.add_fill_bg),
                    legacy.add_fg,
                    legacy.add_gutter_num,
                    legacy.add_gutter_icon,
                ),
                (
                    DiffKind::Del,
                    RowKind::Del,
                    legacy.del_tint_bg,
                    Some(legacy.del_fill_bg),
                    legacy.del_fg,
                    legacy.del_gutter_num,
                    legacy.del_gutter_icon,
                ),
                (
                    DiffKind::Mod,
                    RowKind::Mod,
                    legacy.mod_tint_bg,
                    Some(legacy.mod_fill_bg),
                    legacy.mod_fg,
                    legacy.mod_gutter_num,
                    legacy.mod_gutter_icon,
                ),
                (
                    DiffKind::Mov,
                    RowKind::Mov,
                    legacy.mov_tint_bg,
                    None,
                    legacy.mov_fg,
                    legacy.mov_gutter_num,
                    legacy.mov_gutter_icon,
                ),
            ] {
                let palette = theme.palette[cursor][kind];
                // Context gutters switch in-hunk: Hunk and Row tiers
                // resolve the in-hunk color, rest fall to out-of-hunk.
                let (num, icon) = match (kind, cursor) {
                    (DiffKind::Ctx, Cursor::Hunk) | (DiffKind::Ctx, Cursor::Row) => {
                        (legacy.ctx_gutter_in_hunk, legacy.ctx_gutter_in_hunk)
                    }
                    (DiffKind::Ctx, _) => (legacy.ctx_gutter_out_hunk, legacy.ctx_gutter_out_hunk),
                    _ => (num, icon),
                };
                assert_eq!(
                    palette.surface.bg, bg,
                    "{mode:?} {kind:?} surface.bg at {cursor:?}"
                );
                assert_eq!(
                    palette.surface.fg, fg,
                    "{mode:?} {kind:?} surface.fg at {cursor:?}"
                );
                assert_eq!(
                    palette.surface.fill, fill,
                    "{mode:?} {kind:?} surface.fill at {cursor:?}"
                );
                assert_eq!(
                    palette.gutter.num.color, num,
                    "{mode:?} {kind:?} gutter.num at {cursor:?}"
                );
                assert_eq!(
                    palette.gutter.icon.color, icon,
                    "{mode:?} {kind:?} gutter.icon at {cursor:?}"
                );
            }
        }

        // In-hunk context rows switch gutter colors.
        let in_hunk = theme.palette[Cursor::Hunk][DiffKind::Ctx].gutter;
        assert_eq!(in_hunk.num.color, legacy.ctx_gutter_in_hunk);
        assert_eq!(in_hunk.icon.color, legacy.ctx_gutter_in_hunk);

        // The del gutter number italic is palette data (fused at render
        // time by `StyledColor::fg_code`), not renderer logic.
        let del_num = theme.palette[Cursor::Rest][DiffKind::Del].gutter.num;
        assert!(del_num.italic, "{mode:?} del gutter num carries italic");
        assert_eq!(del_num.color, legacy.del_gutter_num);

        // Syntax tokens per row kind: Mod/Mov rows resolve the ctx arm,
        // matching legacy `token_color` semantics.
        for (key, token) in TOKEN_KEYS {
            for (kind, row_kind) in [
                (DiffKind::Ctx, RowKind::Ctx),
                (DiffKind::Add, RowKind::Add),
                (DiffKind::Del, RowKind::Del),
                (DiffKind::Mod, RowKind::Mod),
                (DiffKind::Mov, RowKind::Mov),
            ] {
                assert_eq!(
                    theme.palette[Cursor::Rest][kind].syntax.token_color(token),
                    legacy.token_color(token, row_kind),
                    "{mode:?} syntax '{key}' on {kind:?}"
                );
            }
        }

        // Banner states: REST at Rest, CURRENT at File, SELECTED at Row.
        for (cursor, bg, fg) in [
            (
                Cursor::Rest,
                legacy.file_header_bg_rest,
                legacy.file_header_fg_rest,
            ),
            (
                Cursor::File,
                legacy.file_header_bg_current,
                legacy.file_header_fg_current,
            ),
            (
                Cursor::Row,
                legacy.file_header_bg_selected,
                legacy.file_header_fg_selected,
            ),
        ] {
            let header = theme.palette[cursor].file_header;
            assert_eq!(header.bg, bg, "{mode:?} file_header.bg at {cursor:?}");
            assert_eq!(header.fg, fg, "{mode:?} file_header.fg at {cursor:?}");
            assert_eq!(header.fill, None);
        }
        for (cursor, bg, fg) in [
            (
                Cursor::Rest,
                legacy.hunk_divider_bg_rest,
                legacy.hunk_divider_fg,
            ),
            (
                Cursor::File,
                legacy.hunk_divider_bg_current,
                legacy.hunk_divider_fg,
            ),
            (
                Cursor::Row,
                legacy.hunk_divider_bg_selected,
                legacy.hunk_divider_fg_selected,
            ),
        ] {
            let divider = theme.palette[cursor].hunk_divider;
            assert_eq!(divider.bg, bg, "{mode:?} hunk_divider.bg at {cursor:?}");
            assert_eq!(divider.fg, fg, "{mode:?} hunk_divider.fg at {cursor:?}");
            assert_eq!(divider.fill, None);
        }

        // UI chrome, verified field-for-field.
        macro_rules! ui_field_eq {
            ($($field:ident),+ $(,)?) => {
                $(
                    assert_eq!(
                        theme.palette.ui.$field,
                        legacy.$field,
                        "{mode:?} ui.{}",
                        stringify!($field)
                    );
                )+
            };
        }
        ui_field_eq!(
            bg_base,
            warning,
            note,
            accent,
            status_bar_bg,
            status_bar_fg,
            status_bar_dim,
            status_bar_accent,
            ribbon_divider,
            ribbon_text,
            header_bar_bg,
            header_bar_fg,
            header_repo,
            header_branch,
            header_dim,
            header_staged,
            header_prefix,
            scrollbar_thumb,
            scrollbar_track,
            modal_border,
            modal_title,
            modal_text,
            modal_dim,
            modal_selected_bg,
            modal_selected_fg,
            count_dels,
            count_adds,
            count_mods,
            count_movs,
            count_notes
        );
        assert_eq!(theme.palette.ui.bg_modal, legacy.modal_bg);
    }

    #[test]
    fn test_dark_asset_equivalence() {
        assert_asset_equivalence(Mode::Dark);
    }

    #[test]
    fn test_light_asset_equivalence() {
        assert_asset_equivalence(Mode::Light);
    }

    #[test]
    fn test_assets_reference_mode_pairs() {
        let dark = parse_theme_str("inherits = \"dark\"").unwrap();
        let light = parse_theme_str("inherits = \"light\"").unwrap();
        assert_eq!(dark.mode, Mode::Dark);
        assert_eq!(light.mode, Mode::Light);
        // The pairing declarations survive schema parsing (pair consumption
        // arrives with TUI mode switching).
        assert_eq!(
            super::builtin("dark").unwrap().light_pair.as_deref(),
            Some("light")
        );
        assert_eq!(
            super::builtin("light").unwrap().dark_pair.as_deref(),
            Some("dark")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{
        Color, cursor::Cursor, diff::DiffKind, primitive::PrimitivePalette, syntax::SyntaxPalette,
    };

    #[test]
    fn test_parse_builtin_by_registry_name() {
        let dark = parse_builtin("dark").unwrap();
        assert_eq!(dark.name, "default");
        assert_eq!(dark.mode, Mode::Dark);
        assert_eq!(
            parse_builtin("default").map(|t| t.palette),
            Some(dark.palette)
        );
        assert_eq!(parse_builtin("light").unwrap().mode, Mode::Light);
        assert!(parse_builtin("noir").is_none());
        assert!(parse_builtin("").is_none());
    }

    #[test]
    fn test_builtin_registry() {
        assert_eq!(builtin("dark").unwrap().mode, Some(Mode::Dark));
        assert_eq!(builtin("dark").unwrap().name.as_deref(), Some("default"));
        assert_eq!(builtin("default").unwrap().name.as_deref(), Some("default"));
        assert_eq!(builtin("light").unwrap().mode, Some(Mode::Light));
        assert!(builtin("noir").is_none());
        assert!(builtin("").is_none());
    }

    #[test]
    fn test_parse_plain_theme_with_overrides() {
        let theme = parse_theme_str(
            r#"
            name = "test"
            [rest.ctx]
            bg = 100
            fg = 101
            [ui]
            accent = "$red"
            "#,
        )
        .unwrap();
        assert_eq!(theme.name, "test");
        assert_eq!(theme.mode, Mode::Dark);
        assert_eq!(
            theme.palette[Cursor::Rest][DiffKind::Ctx].surface.bg,
            Color::Fixed(100)
        );
        assert_eq!(
            theme.palette[Cursor::Rest][DiffKind::Ctx].surface.fg,
            Color::Fixed(101)
        );
        assert_eq!(theme.palette.ui.accent, Color::Fixed(203));
    }

    #[test]
    fn test_parse_unnamed_theme_defaults_name_and_mode() {
        let theme = parse_theme_str("[rest.ctx]\nbg = 5\n").unwrap();
        assert_eq!(theme.name, "default");
        assert_eq!(theme.mode, Mode::Dark);
    }

    #[test]
    fn test_inherits_dark_overlay() {
        let theme = parse_theme_str(
            r#"
            inherits = "dark"
            [ui]
            count_adds = "$green"
            "#,
        )
        .unwrap();
        assert_eq!(theme.name, "default", "name inherits from the asset");
        assert_eq!(theme.mode, Mode::Dark);
        assert_eq!(
            theme.palette.ui.count_adds,
            Color::Fixed(120),
            "dark primitives resolve $green over the inherited asset"
        );
    }

    #[test]
    fn test_inherits_light_changes_baseline() {
        let theme = parse_theme_str(
            r#"
            inherits = "light"
            [ui]
            accent = "$green"
            "#,
        )
        .unwrap();
        assert_eq!(theme.name, "default");
        assert_eq!(theme.mode, Mode::Light);
        assert_eq!(
            theme.palette.ui.accent,
            Color::Fixed(28),
            "light primitives resolve $green differently from dark"
        );
        assert_eq!(
            theme.palette[Cursor::Rest][DiffKind::Ctx].surface.bg,
            Color::Rgb(250, 250, 250),
            "light canonical ctx tint from the bundled asset"
        );
        assert_eq!(
            theme.palette[Cursor::Rest][DiffKind::Ctx].syntax.keyword,
            SyntaxPalette::light().keyword,
            "light syntax baseline flows through the asset"
        );
    }

    #[test]
    fn test_invalid_toml_source() {
        let error = parse_theme_str("not [ toml[").unwrap_err();
        assert!(error.to_string().starts_with("invalid theme:"));
        assert!(matches!(error, ThemeParseError::Toml(_)));
    }

    #[test]
    fn test_schema_violation_rejects_document() {
        let error = parse_theme_str("[rest.ctx]\nbcc = 5\n").unwrap_err();
        assert!(
            matches!(error, ThemeParseError::Toml(_)),
            "unknown fields surface as TOML errors: {error}"
        );
    }

    #[test]
    fn test_unresolved_variable_reports_path() {
        let error = parse_theme_str("[row.add]\nbg = \"$nolight\"\n").unwrap_err();
        assert_eq!(
            error,
            ThemeParseError::UnresolvedVariable {
                name: "nolight".to_string(),
                path: "row.add.bg".to_string(),
            }
        );
    }

    #[test]
    fn test_unknown_inherit() {
        let error = parse_theme_str("inherits = \"noir\"\n").unwrap_err();
        assert_eq!(
            error,
            ThemeParseError::UnknownInherit {
                name: "noir".to_string()
            }
        );
        assert_eq!(
            error.to_string(),
            "unknown inherited theme 'noir' (expected one of: dark, light)"
        );
    }

    #[test]
    fn test_unresolved_variable_in_syntax_map() {
        let error = parse_theme_str("[rest.ctx.syntax]\nkeyword = \"$nope\"\n").unwrap_err();
        assert_eq!(
            error,
            ThemeParseError::UnresolvedVariable {
                name: "nope".to_string(),
                path: "rest.ctx.syntax.keyword".to_string(),
            }
        );
    }

    #[test]
    fn test_inherited_primitives_overlay() {
        // `[palette]` overrides apply before inheritance resolution, so a
        // child redefines a swatch for its own references.
        let theme = parse_theme_str(
            r#"
            [palette]
            green = 9
            [rest.ctx]
            bg = "$green"
            "#,
        )
        .unwrap();
        assert_eq!(
            theme.palette[Cursor::Rest][DiffKind::Ctx].surface.bg,
            Color::Fixed(9)
        );
        // The primitive table itself is the overlay, verifiable through $refs:
        assert_eq!(PrimitivePalette::dark().green, Color::Fixed(120));
    }

    #[test]
    fn test_parsed_theme_is_clone() {
        let theme = parse_theme_str("[ui]\naccent = 1\n").unwrap();
        let clone = theme.clone();
        assert_eq!(clone, theme);
    }
}
