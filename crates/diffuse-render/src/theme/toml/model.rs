//! Intermediate TOML deserialization model for theme files.
//!
//! This layer validates structure and value syntax (`#RRGGBB` hex, ANSI-256
//! indices, `$variable` references) without resolving semantics; the cascade
//! engine compiles validated models into a flat [`ThemePalette`].
//!
//! Unknown fields are rejected with precise TOML parse errors, so typos in
//! theme files surface at load time rather than silently dropping slots.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

use super::super::scope::Scope;

/// Theme application mode, selecting the primitive swatch baseline.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Dark,
    Light,
}

/// Primitive `[palette]` swatch overrides. Field names match the
/// [`PrimitivePalette`](super::super::primitive::PrimitivePalette) swatch
/// vocabulary used by `$variable` references.
#[derive(Clone, Default, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PaletteModel {
    pub base_bg: Option<ColorValue>,
    pub base_fg: Option<ColorValue>,
    pub red: Option<ColorValue>,
    pub green: Option<ColorValue>,
    pub yellow: Option<ColorValue>,
    pub orange: Option<ColorValue>,
    pub blue: Option<ColorValue>,
    pub purple: Option<ColorValue>,
    pub cyan: Option<ColorValue>,
    pub gray: Option<ColorValue>,
    pub dark_red: Option<ColorValue>,
    pub dark_green: Option<ColorValue>,
}

/// A color specification in any accepted form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorValue {
    /// A `$variable` reference (forced) or a named primitive swatch.
    Named(String),
    /// `#RRGGBB` hexadecimal literal.
    Rgb { r: u8, g: u8, b: u8 },
    /// ANSI 256-color terminal index.
    Indexed(u8),
}

impl<'de> Deserialize<'de> for ColorValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ColorVisitor;

        impl serde::de::Visitor<'_> for ColorVisitor {
            type Value = ColorValue;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a color (\"#RRGGBB\", an ANSI 256 index, or a variable name)")
            }

            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<ColorValue, E> {
                if let Some(hex) = s.strip_prefix('#') {
                    let [r, g, b] = parse_hex_triplet(hex).map_err(|_| {
                        E::invalid_value(serde::de::Unexpected::Str(s), &"\"#RRGGBB\"")
                    })?;
                    Ok(ColorValue::Rgb { r, g, b })
                } else {
                    Ok(ColorValue::Named(s.to_string()))
                }
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<ColorValue, E> {
                let index = u8::try_from(v).map_err(|_| {
                    E::invalid_value(
                        serde::de::Unexpected::Unsigned(v),
                        &"an ANSI 256 index in 0..=255",
                    )
                })?;
                Ok(ColorValue::Indexed(index))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<ColorValue, E> {
                self.visit_u64(v.try_into().map_err(|_| {
                    E::invalid_value(
                        serde::de::Unexpected::Signed(v),
                        &"an ANSI 256 index in 0..=255",
                    )
                })?)
            }
        }

        d.deserialize_any(ColorVisitor)
    }
}

/// Parse six hexadecimal digits into an RGB triplet.
fn parse_hex_triplet(hex: &str) -> Result<[u8; 3], ()> {
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(());
    }
    let pair = |i: usize| -> Result<u8, ()> {
        let hi = (hex.as_bytes()[i] as char).to_digit(16).ok_or(())? as u8;
        let lo = (hex.as_bytes()[i + 1] as char).to_digit(16).ok_or(())? as u8;
        Ok(hi * 16 + lo)
    };
    Ok([pair(0)?, pair(2)?, pair(4)?])
}

/// A color with optional text attribute modifiers: either a bare color
/// specification or an inline table (`color = "..."`, plus `bold`, `italic`,
/// `dim`, `underline` flags).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum StyledValue {
    Color(ColorValue),
    Styled {
        color: ColorValue,
        bold: Option<bool>,
        italic: Option<bool>,
        dim: Option<bool>,
        underline: Option<bool>,
    },
}

/// Surface slot overrides (banner backgrounds or row surfaces).
#[derive(Clone, Default, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceModel {
    pub bg: Option<ColorValue>,
    pub fill: Option<ColorValue>,
    pub fg: Option<ColorValue>,
}

/// Per-diff-kind override slots: surface (`bg`/`fill`/`fg`), gutter
/// (`num`/`icon`), and syntax token colors (`syntax` map keyed by token name).
#[derive(Clone, Default, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffModel {
    pub bg: Option<ColorValue>,
    pub fill: Option<ColorValue>,
    pub fg: Option<ColorValue>,
    pub num: Option<StyledValue>,
    pub icon: Option<StyledValue>,
    pub syntax: Option<BTreeMap<String, ColorValue>>,
}

/// Slots a scope provides for diff styling. `Mov` is the alias channel
/// shared by relocation sides unless explicitly overridden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindSlot {
    Ctx,
    Add,
    Del,
    Mod,
    Mov,
    MovSrc,
    MovTgt,
}

impl KindSlot {
    /// Number of slots, including the `mov` alias channel.
    pub const COUNT: usize = 7;
}

/// Per-scope override tables.
#[derive(Clone, Default, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeModel {
    pub file_header: Option<SurfaceModel>,
    pub hunk_divider: Option<SurfaceModel>,
    pub ctx: Option<DiffModel>,
    pub add: Option<DiffModel>,
    pub del: Option<DiffModel>,
    #[serde(rename = "mod")]
    pub r#mod: Option<DiffModel>,
    pub mov: Option<DiffModel>,
    pub mov_src: Option<DiffModel>,
    pub mov_tgt: Option<DiffModel>,
}

impl ScopeModel {
    /// The diff slot for a kind channel.
    #[inline]
    #[must_use]
    pub fn slot(&self, slot: KindSlot) -> Option<&DiffModel> {
        match slot {
            KindSlot::Ctx => &self.ctx,
            KindSlot::Add => &self.add,
            KindSlot::Del => &self.del,
            KindSlot::Mod => &self.r#mod,
            KindSlot::Mov => &self.mov,
            KindSlot::MovSrc => &self.mov_src,
            KindSlot::MovTgt => &self.mov_tgt,
        }
        .as_ref()
    }
}

/// UI chrome overrides keyed by
/// [`UiPalette`](super::super::palette::UiPalette) field names (validated by
/// the deny-unknown-fields struct definition).
#[derive(Clone, Default, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiModel {
    pub bg_base: Option<ColorValue>,
    pub bg_modal: Option<ColorValue>,
    pub warning: Option<ColorValue>,
    pub note: Option<ColorValue>,
    pub accent: Option<ColorValue>,
    pub status_bar_bg: Option<ColorValue>,
    pub status_bar_fg: Option<ColorValue>,
    pub status_bar_dim: Option<ColorValue>,
    pub status_bar_accent: Option<ColorValue>,
    pub ribbon_divider: Option<ColorValue>,
    pub ribbon_text: Option<ColorValue>,
    pub header_bar_bg: Option<ColorValue>,
    pub header_bar_fg: Option<ColorValue>,
    pub header_repo: Option<ColorValue>,
    pub header_branch: Option<ColorValue>,
    pub header_dim: Option<ColorValue>,
    pub header_staged: Option<ColorValue>,
    pub header_prefix: Option<ColorValue>,
    pub scrollbar_thumb: Option<ColorValue>,
    pub scrollbar_track: Option<ColorValue>,
    pub modal_border: Option<ColorValue>,
    pub modal_title: Option<ColorValue>,
    pub modal_text: Option<ColorValue>,
    pub modal_dim: Option<ColorValue>,
    pub modal_selected_bg: Option<ColorValue>,
    pub modal_selected_fg: Option<ColorValue>,
    pub count_dels: Option<ColorValue>,
    pub count_adds: Option<ColorValue>,
    pub count_mods: Option<ColorValue>,
    pub count_movs: Option<ColorValue>,
    pub count_notes: Option<ColorValue>,
}

/// Complete intermediate theme model, one per TOML document.
#[derive(Clone, Default, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeModel {
    pub name: Option<String>,
    pub mode: Option<Mode>,
    pub inherits: Option<String>,
    pub palette: Option<PaletteModel>,
    pub ui: Option<UiModel>,
    pub none: Option<ScopeModel>,
    pub file: Option<ScopeModel>,
    pub hunk: Option<ScopeModel>,
    pub row: Option<ScopeModel>,
}

impl ThemeModel {
    /// The scope table for a containment scope.
    #[inline]
    #[must_use]
    pub fn scope(&self, scope: Scope) -> Option<&ScopeModel> {
        match scope {
            Scope::None => &self.none,
            Scope::File => &self.file,
            Scope::Hunk => &self.hunk,
            Scope::Row => &self.row,
        }
        .as_ref()
    }

    /// The theme's declared mode, or the provided default when absent.
    #[inline]
    #[must_use]
    pub fn effective_mode(&self, default: Mode) -> Mode {
        self.mode.unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_value_forms() {
        assert_eq!(parse_hex_triplet("1a2b3c").unwrap(), [0x1a, 0x2b, 0x3c]);
        assert!(parse_hex_triplet("1a2b3").is_err());
        assert!(parse_hex_triplet("1a2b3g").is_err());
        assert!(parse_hex_triplet("1a2b3c9").is_err());
        assert_eq!(ColorValue::Indexed(233), ColorValue::Indexed(233));
    }

    #[test]
    fn test_deserialize_color_value_via_toml() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wrap {
            v: ColorValue,
        }
        let ok = toml::from_str::<Wrap>("v = 233").unwrap();
        assert_eq!(ok.v, ColorValue::Indexed(233));
        let ok = toml::from_str::<Wrap>("v = \"#0f1104\"").unwrap();
        assert_eq!(ok.v, ColorValue::Rgb { r: 15, g: 17, b: 4 });
        let ok = toml::from_str::<Wrap>("v = \"$green\"").unwrap();
        assert_eq!(ok.v, ColorValue::Named("$green".to_string()));
        let ok = toml::from_str::<Wrap>("v = \"green\"").unwrap();
        assert_eq!(ok.v, ColorValue::Named("green".to_string()));

        // Out-of-range indices and malformed hex fail at parse time.
        assert!(toml::from_str::<Wrap>("v = 256").is_err());
        assert!(toml::from_str::<Wrap>("v = -3").is_err());
        assert!(toml::from_str::<Wrap>("v = \"#xyz111\"").is_err());
        assert!(toml::from_str::<Wrap>("v = \"#11122\"").is_err());
    }

    #[test]
    fn test_deserialize_styled_value_and_struct_tables() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wrap {
            num: StyledValue,
            icon: StyledValue,
            #[serde(rename = "mod")]
            r#mod: Option<DiffModel>,
        }
        let w = toml::from_str::<Wrap>("num = 233\nicon = { color = \"$red\", bold = true }\n")
            .unwrap();
        assert_eq!(w.num, StyledValue::Color(ColorValue::Indexed(233)));
        assert_eq!(
            w.icon,
            StyledValue::Styled {
                color: ColorValue::Named("$red".to_string()),
                bold: Some(true),
                italic: None,
                dim: None,
                underline: None,
            }
        );
        let w = toml::from_str::<Wrap>("num = 1\nicon = 2\nmod = { bg = \"#000000\" }\n").unwrap();
        assert_eq!(
            w.r#mod.map(|m| m.bg),
            Some(Some(ColorValue::Rgb { r: 0, g: 0, b: 0 }))
        );

        // Unknown keys anywhere are rejected.
        assert!(toml::from_str::<Wrap>("num = 1\nicon = 2\nunknown = 3\n").is_err());
        assert!(toml::from_str::<Wrap>("num = 1\nicon = 2\n[noneMisc]\nbg = 3\n").is_err());
    }

    #[test]
    fn test_scope_model_slot_access() {
        let scope: ScopeModel = toml::from_str(
            "ctx = { bg = 10 }\nadd = { bg = 11 }\ndel = { bg = 12 }\nmod = { bg = 13 }\nmov = { bg = 14 }\nmov_src = { bg = 15 }\nmov_tgt = { bg = 16 }\nfile_header = { fill = 25 }\n",
        )
        .unwrap();
        let slot_bg = |slot: KindSlot| scope.slot(slot).and_then(|diff| diff.bg.clone());
        assert_eq!(slot_bg(KindSlot::Ctx), Some(ColorValue::Indexed(10)));
        assert_eq!(slot_bg(KindSlot::Add), Some(ColorValue::Indexed(11)));
        assert_eq!(slot_bg(KindSlot::Del), Some(ColorValue::Indexed(12)));
        assert_eq!(slot_bg(KindSlot::Mod), Some(ColorValue::Indexed(13)));
        assert_eq!(slot_bg(KindSlot::Mov), Some(ColorValue::Indexed(14)));
        assert_eq!(slot_bg(KindSlot::MovSrc), Some(ColorValue::Indexed(15)));
        assert_eq!(slot_bg(KindSlot::MovTgt), Some(ColorValue::Indexed(16)));
        assert_eq!(
            scope.file_header.as_ref().unwrap().fill,
            Some(ColorValue::Indexed(25))
        );
    }

    #[test]
    fn test_theme_model_scope_tables() {
        let theme: ThemeModel = toml::from_str(
            "[none]\n[file]\nctx = { bg = 21 }\n[hunk]\nctx = { bg = 22 }\n[row]\nctx = { bg = 23 }\n",
        )
        .unwrap();
        assert!(theme.scope(Scope::None).unwrap().ctx.is_none());
        assert_eq!(
            theme
                .scope(Scope::File)
                .unwrap()
                .slot(KindSlot::Ctx)
                .unwrap()
                .bg,
            Some(ColorValue::Indexed(21))
        );
        assert_eq!(
            theme
                .scope(Scope::Hunk)
                .unwrap()
                .slot(KindSlot::Ctx)
                .unwrap()
                .bg,
            Some(ColorValue::Indexed(22))
        );
        assert_eq!(
            theme
                .scope(Scope::Row)
                .unwrap()
                .slot(KindSlot::Ctx)
                .unwrap()
                .bg,
            Some(ColorValue::Indexed(23))
        );
        assert!(ThemeModel::default().scope(Scope::Row).is_none());
    }

    #[test]
    fn test_full_theme_model_parses() {
        let m = toml::from_str::<ThemeModel>(
            "\n[none]\n[none.add]\nbg = 233\n[none.add.syntax]\nkeyword = \"$red\"\n",
        )
        .unwrap();
        let add = m.scope(Scope::None).unwrap().slot(KindSlot::Add).unwrap();
        assert_eq!(add.bg, Some(ColorValue::Indexed(233)));
        assert_eq!(
            add.syntax.as_ref().unwrap()["keyword"],
            ColorValue::Named("$red".to_string())
        );
        assert_eq!(m.effective_mode(Mode::Dark), Mode::Dark);
        assert_eq!(m.effective_mode(Mode::Light), Mode::Light);
        let m: ThemeModel = toml::from_str("mode = \"light\"").unwrap();
        assert_eq!(m.effective_mode(Mode::Dark), Mode::Light);
    }
}
