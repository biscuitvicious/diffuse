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

/// Registry of bundled builtin themes.
///
/// Returns an empty model carrying only the theme name and mode; every
/// unresolved field cascades to the matching mode baseline. The canonical
/// TOML assets replace these stubs when declarative themes ship.
fn builtin(name: &str) -> Option<ThemeModel> {
    match name {
        "default" | "dark" => Some(ThemeModel {
            name: Some("dark".to_string()),
            mode: Some(Mode::Dark),
            ..ThemeModel::default()
        }),
        "light" => Some(ThemeModel {
            name: Some("light".to_string()),
            mode: Some(Mode::Light),
            ..ThemeModel::default()
        }),
        _ => None,
    }
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
/// through the scope and diff-kind axes into a flat [`ScopedPalette`].
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
mod tests {
    use super::*;
    use crate::theme::{
        Color, diff::DiffKind, primitive::PrimitivePalette, scope::Scope, syntax::SyntaxPalette,
    };

    #[test]
    fn test_builtin_registry() {
        assert_eq!(builtin("dark").unwrap().mode, Some(Mode::Dark));
        assert_eq!(builtin("default").unwrap().name.as_deref(), Some("dark"));
        assert_eq!(builtin("light").unwrap().mode, Some(Mode::Light));
        assert!(builtin("noir").is_none());
        assert!(builtin("").is_none());
    }

    #[test]
    fn test_parse_plain_theme_with_overrides() {
        let theme = parse_theme_str(
            r#"
            name = "test"
            [none.ctx]
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
            theme.palette[Scope::None][DiffKind::Ctx].surface.bg,
            Color::Fixed(100)
        );
        assert_eq!(
            theme.palette[Scope::None][DiffKind::Ctx].surface.fg,
            Color::Fixed(101)
        );
        assert_eq!(theme.palette.ui.accent, Color::Fixed(203));
    }

    #[test]
    fn test_parse_unnamed_theme_defaults_name_and_mode() {
        let theme = parse_theme_str("[none.ctx]\nbg = 5\n").unwrap();
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
        assert_eq!(theme.name, "dark", "name inherits from the base");
        assert_eq!(theme.mode, Mode::Dark);
        assert_eq!(
            theme.palette.ui.count_adds,
            Color::Fixed(120),
            "dark primitives resolve $green"
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
        assert_eq!(theme.name, "light");
        assert_eq!(theme.mode, Mode::Light);
        assert_eq!(
            theme.palette.ui.accent,
            Color::Fixed(28),
            "light primitives resolve $green differently from dark"
        );
        assert_eq!(
            theme.palette[Scope::None][DiffKind::Ctx].surface.bg,
            Color::Fixed(255),
            "light base_bg baseline"
        );
        assert_eq!(
            theme.palette[Scope::None][DiffKind::Ctx].syntax,
            SyntaxPalette::light(),
            "light syntax baseline"
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
        let error = parse_theme_str("[none.ctx]\nbcc = 5\n").unwrap_err();
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
        let error = parse_theme_str("[none.ctx.syntax]\nkeyword = \"$nope\"\n").unwrap_err();
        assert_eq!(
            error,
            ThemeParseError::UnresolvedVariable {
                name: "nope".to_string(),
                path: "none.ctx.syntax.keyword".to_string(),
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
            [none.ctx]
            bg = "$green"
            "#,
        )
        .unwrap();
        assert_eq!(
            theme.palette[Scope::None][DiffKind::Ctx].surface.bg,
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
