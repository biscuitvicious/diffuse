//! Declarative theme cascade compiler.
//!
//! Merges a chain of [`ThemeModel`]s (self first, inherited bases after)
//! into a flat, cursor-tiered [`ThemePalette`].
//!
//! Resolution walks the cursor axis innermost-first (Row → Hunk → File
//! → Rest). For each diff kind the explicit kind channel is consulted along
//! the cursor walk first, then the `ctx` channel. Unresolved fields fall
//! back to the mode baseline: primitive swatches for surfaces, the mode's
//! [`SyntaxPalette`] for token colors.

use std::collections::BTreeMap;

use crate::syntax::color::TokenColor;

use super::{
    model::{ColorValue, DiffModel, Mode, StyledValue, SurfaceModel, ThemeModel},
    parse::ThemeParseError,
};
use crate::theme::{
    Color,
    cursor::Cursor,
    diff::{DiffKind, DiffPalette},
    palette::{CursorPalette, ThemePalette, UiPalette},
    primitive::PrimitivePalette,
    styled_color::{GutterColors, StyledColor},
    surface::SurfaceColors,
    syntax::SyntaxPalette,
};

/// Canonical `(key, token)` table for `syntax` maps, covering every
/// [`SyntaxPalette`] field and [`TokenColor`] variant.
pub(in crate::theme) const TOKEN_KEYS: [(&str, TokenColor); 23] = [
    ("text", TokenColor::Default),
    ("keyword", TokenColor::Keyword),
    ("typ", TokenColor::Type),
    ("function", TokenColor::Function),
    ("number", TokenColor::Number),
    ("literal", TokenColor::Literal),
    ("string", TokenColor::String),
    ("char_literal", TokenColor::CharLiteral),
    ("comment", TokenColor::Comment),
    ("operator", TokenColor::Operator),
    ("bold", TokenColor::Bold),
    ("italic", TokenColor::Italic),
    ("strike", TokenColor::Strike),
    ("link", TokenColor::Link),
    ("url", TokenColor::Url),
    ("delim", TokenColor::Delimiter),
    ("rule", TokenColor::Rule),
    ("code", TokenColor::Code),
    ("table_border", TokenColor::TableBorder),
    ("table_header", TokenColor::TableHeader),
    ("link_border", TokenColor::LinkBorder),
    ("quote_border", TokenColor::QuoteBorder),
    ("fence_border", TokenColor::FenceBorder),
];

/// All diff kinds, ordered by their [`DiffPalette`] array index.
const DIFF_KINDS: [DiffKind; DiffKind::COUNT] = [
    DiffKind::Ctx,
    DiffKind::Add,
    DiffKind::Del,
    DiffKind::Mod,
    DiffKind::Mov,
];

/// Per-tier surface slot of a cursor-tier palette.
#[derive(Clone, Copy)]
enum Surface {
    FileHeader,
    HunkDivider,
    /// Visual-selection band for diff rows.
    Band,
}

impl Surface {
    /// TOML key for the surface.
    #[inline]
    fn key(self) -> &'static str {
        match self {
            Surface::FileHeader => "file_header",
            Surface::HunkDivider => "hunk_divider",
            Surface::Band => "band",
        }
    }
}

/// TOML key for a cursor tier.
#[inline]
fn cursor_key(cursor: Cursor) -> &'static str {
    match cursor {
        Cursor::Rest => "rest",
        Cursor::File => "file",
        Cursor::Hunk => "hunk",
        Cursor::Row => "row",
    }
}

/// Apply an overlay of optional color fields onto a matching target struct,
/// resolving each value against the primitive swatch table first.
macro_rules! overlay_color_fields {
    ($source:expr, $target:expr, $primitives:expr, $section:literal; $($field:ident),+ $(,)?) => {
        $(
            if let Some(value) = $source.$field.as_ref() {
                let resolved = resolve_color(
                    value,
                    $primitives,
                    concat!($section, ".", stringify!($field)),
                )?;
                $target.$field = resolved;
            }
        )+
    };
}

/// Resolve a color specification against the primitive swatch table.
///
/// `$name`-forced and bare references resolve through the primitive table;
/// unknown names reject the theme with the referencing path attached.
fn resolve_color(
    value: &ColorValue,
    primitives: &PrimitivePalette,
    path: &str,
) -> Result<Color, ThemeParseError> {
    match value {
        ColorValue::Primary => Ok(Color::Primary),
        ColorValue::Named(name) => {
            let variable = name.strip_prefix('$').unwrap_or(name);
            primitives
                .get(variable)
                .ok_or_else(|| ThemeParseError::UnresolvedVariable {
                    name: variable.to_string(),
                    path: path.to_string(),
                })
        }
        ColorValue::Rgb { r, g, b } => Ok(Color::Rgb(*r, *g, *b)),
        ColorValue::Indexed(index) => Ok(Color::Fixed(*index)),
        ColorValue::Clear => Err(ThemeParseError::InvalidColor {
            value: "none".to_string(),
            path: path.to_string(),
        }),
    }
}

/// Resolve a styled color specification (color plus attribute modifiers).
fn resolve_styled(
    value: &StyledValue,
    primitives: &PrimitivePalette,
    path: &str,
) -> Result<StyledColor, ThemeParseError> {
    match value {
        StyledValue::Color(color) => Ok(StyledColor::new(resolve_color(color, primitives, path)?)),
        StyledValue::Styled {
            color,
            bold,
            italic,
            dim,
            underline,
        } => Ok(StyledColor::new(resolve_color(color, primitives, path)?)
            .with_bold(bold.unwrap_or(false))
            .with_italic(italic.unwrap_or(false))
            .with_dim(dim.unwrap_or(false))
            .with_underline(underline.unwrap_or(false))),
    }
}

/// Channel tiers for a diff kind, in resolution priority: the explicit kind
/// channel, then the `ctx` channel. `None` entries are skipped.
#[inline]
fn diff_tiers(kind: DiffKind) -> [Option<DiffKind>; 2] {
    match kind {
        DiffKind::Ctx => [Some(DiffKind::Ctx), None],
        DiffKind::Add => [Some(DiffKind::Add), Some(DiffKind::Ctx)],
        DiffKind::Del => [Some(DiffKind::Del), Some(DiffKind::Ctx)],
        DiffKind::Mod => [Some(DiffKind::Mod), Some(DiffKind::Ctx)],
        DiffKind::Mov => [Some(DiffKind::Mov), Some(DiffKind::Ctx)],
    }
}

/// Cascade compiler over an inherit chain: `models[0]` overrides all bases.
struct Compiler<'a> {
    models: &'a [ThemeModel],
    primitives: PrimitivePalette,
    mode: Mode,
}

impl<'a> Compiler<'a> {
    /// The first diff model defining `kind` at `cursor`, searching the chain
    /// self-first.
    fn cursor_slot(&self, cursor: Cursor, kind: DiffKind) -> Option<&'a DiffModel> {
        self.models
            .iter()
            .find_map(|model| model.cursor(cursor).and_then(|s| s.slot(kind)))
    }

    /// The first surface model defining `surface` at `cursor`, searching the
    /// chain self-first.
    fn surface_channel(&self, cursor: Cursor, surface: Surface) -> Option<&'a SurfaceModel> {
        self.models.iter().find_map(|model| {
            model.cursor(cursor).and_then(|s| match surface {
                Surface::FileHeader => s.file_header.as_ref(),
                Surface::HunkDivider => s.hunk_divider.as_ref(),
                Surface::Band => s.band.as_ref(),
            })
        })
    }

    /// Resolution-ordered diff channel list for `(cursor, kind)`: for each
    /// channel tier (kind channel, `ctx`), the cursor walk innermost-first
    /// with the inherit chain self-first.
    fn diff_channels(&self, cursor: Cursor, kind: DiffKind) -> Vec<&'a DiffModel> {
        let mut channels = Vec::new();
        for tier in diff_tiers(kind).into_iter().flatten() {
            let mut current = Some(cursor);
            while let Some(cursor) = current {
                current = cursor.outer();
                if let Some(diff) = self.cursor_slot(cursor, tier) {
                    channels.push(diff);
                }
            }
        }
        channels
    }

    /// Compile one diff kind palette: explicit channels then mode-baseline
    /// fallbacks.
    fn build_diff(&self, cursor: Cursor, kind: DiffKind) -> Result<DiffPalette, ThemeParseError> {
        let channels = self.diff_channels(cursor, kind);
        let section = format!("{}.{}", cursor_key(cursor), kind.name());

        let bg = channels.iter().find_map(|c| c.bg.as_ref());
        let fill = channels.iter().find_map(|c| c.fill.as_ref());
        let fg = channels.iter().find_map(|c| c.fg.as_ref());
        let num = channels.iter().find_map(|c| c.num.as_ref());
        let icon = channels.iter().find_map(|c| c.icon.as_ref());
        let syntax_maps: Vec<Option<&BTreeMap<String, ColorValue>>> =
            channels.iter().map(|c| c.syntax.as_ref()).collect();

        let surface = SurfaceColors {
            bg: match bg {
                Some(value) => resolve_color(value, &self.primitives, &format!("{section}.bg"))?,
                None => self.primitives.base_bg,
            },
            fill: match fill {
                // `fill = "none"` explicitly clears an inherited fill.
                Some(ColorValue::Clear) => None,
                Some(value) => Some(resolve_color(
                    value,
                    &self.primitives,
                    &format!("{section}.fill"),
                )?),
                None => None,
            },
            fg: match fg {
                Some(value) => resolve_color(value, &self.primitives, &format!("{section}.fg"))?,
                None => self.primitives.base_fg,
            },
            path: None, // diff rows carry no banner path data
        };
        let gutter = GutterColors {
            num: match num {
                Some(value) => resolve_styled(value, &self.primitives, &format!("{section}.num"))?,
                None => StyledColor::new(self.primitives.base_fg),
            },
            icon: match icon {
                Some(value) => resolve_styled(value, &self.primitives, &format!("{section}.icon"))?,
                None => StyledColor::new(self.primitives.base_fg),
            },
        };
        let syntax = self.build_syntax(&syntax_maps, &section)?;

        Ok(DiffPalette::new(surface, gutter, syntax))
    }

    /// Build the syntax token palette from the resolved `syntax` maps.
    ///
    /// Unknown keys reject the theme with the referencing section attached;
    /// each token resolves independently along the channel list, falling
    /// back to the mode baseline.
    fn build_syntax(
        &self,
        maps: &[Option<&BTreeMap<String, ColorValue>>],
        section: &str,
    ) -> Result<SyntaxPalette, ThemeParseError> {
        let unknown = maps.iter().filter_map(|map| *map).find_map(|map| {
            map.keys()
                .find(|key| !TOKEN_KEYS.iter().any(|(known, _)| known == *key))
                .cloned()
        });
        if let Some(key) = unknown {
            return Err(ThemeParseError::UnknownToken {
                key,
                path: format!("{section}.syntax"),
            });
        }

        let mut syntax = match self.mode {
            Mode::Dark => SyntaxPalette::dark(),
            Mode::Light => SyntaxPalette::light(),
        };
        for (key, token) in TOKEN_KEYS {
            let resolved = maps
                .iter()
                .filter_map(|map| *map)
                .find_map(|map| map.get(key));
            if let Some(value) = resolved {
                let color =
                    resolve_color(value, &self.primitives, &format!("{section}.syntax.{key}"))?;
                Self::apply_token(&mut syntax, token, color);
            }
        }
        Ok(syntax)
    }

    /// Assign a resolved token color to its palette field.
    fn apply_token(syntax: &mut SyntaxPalette, token: TokenColor, color: Color) {
        match token {
            TokenColor::Default => syntax.text = color,
            TokenColor::Keyword => syntax.keyword = color,
            TokenColor::Type => syntax.typ = color,
            TokenColor::Function => syntax.function = color,
            TokenColor::Number => syntax.number = color,
            TokenColor::Literal => syntax.literal = color,
            TokenColor::String => syntax.string = color,
            TokenColor::CharLiteral => syntax.char_literal = color,
            TokenColor::Comment => syntax.comment = color,
            TokenColor::Operator => syntax.operator = color,
            TokenColor::Bold => syntax.bold = color,
            TokenColor::Italic => syntax.italic = color,
            TokenColor::Strike => syntax.strike = color,
            TokenColor::Link => syntax.link = color,
            TokenColor::Url => syntax.url = color,
            TokenColor::Delimiter => syntax.delim = color,
            TokenColor::Rule => syntax.rule = color,
            TokenColor::Code => syntax.code = color,
            TokenColor::TableBorder => syntax.table_border = color,
            TokenColor::TableHeader => syntax.table_header = color,
            TokenColor::LinkBorder => syntax.link_border = color,
            TokenColor::QuoteBorder => syntax.quote_border = color,
            TokenColor::FenceBorder => syntax.fence_border = color,
        }
    }

    /// Compile one banner surface.
    fn build_surface(
        &self,
        cursor: Cursor,
        surface: Surface,
    ) -> Result<SurfaceColors, ThemeParseError> {
        let mut channels = Vec::new();
        let mut current = Some(cursor);
        while let Some(cursor) = current {
            current = cursor.outer();
            if let Some(model) = self.surface_channel(cursor, surface) {
                channels.push(model);
            }
        }

        let section = format!("{}.{}", cursor_key(cursor), surface.key());
        let bg = channels.iter().find_map(|c| c.bg.as_ref());
        let fill = channels.iter().find_map(|c| c.fill.as_ref());
        let fg = channels.iter().find_map(|c| c.fg.as_ref());
        let path = channels.iter().find_map(|c| c.path.as_ref());

        let fg = match fg {
            Some(value) => resolve_color(value, &self.primitives, &format!("{section}.fg"))?,
            None => self.primitives.base_fg,
        };
        // Unset banner paths share the foreground color (`fg_or_path`).
        let path = match path {
            Some(value) => resolve_color(value, &self.primitives, &format!("{section}.path"))?,
            None => fg,
        };

        Ok(SurfaceColors {
            bg: match bg {
                Some(value) => resolve_color(value, &self.primitives, &format!("{section}.bg"))?,
                None => self.primitives.base_bg,
            },
            fill: match fill {
                // `fill = "none"` explicitly clears an inherited fill.
                Some(ColorValue::Clear) => None,
                Some(value) => Some(resolve_color(
                    value,
                    &self.primitives,
                    &format!("{section}.fill"),
                )?),
                None => None,
            },
            fg,
            path: Some(path),
        })
    }

    /// Compile one cursor-tier palette: both banners, the band surface, and
    /// every diff kind.
    fn build_cursor(&self, cursor: Cursor) -> Result<CursorPalette, ThemeParseError> {
        let file_header = self.build_surface(cursor, Surface::FileHeader)?;
        let hunk_divider = self.build_surface(cursor, Surface::HunkDivider)?;
        let band = self.build_surface(cursor, Surface::Band)?;
        let mut diffs = [DiffPalette::default(); DiffKind::COUNT];
        for (index, kind) in DIFF_KINDS.iter().enumerate() {
            diffs[index] = self.build_diff(cursor, *kind)?;
        }
        Ok(CursorPalette::new(file_header, hunk_divider, band, diffs))
    }

    /// Compile UI chrome: inherit-chain overlays over the default palette,
    /// with bases applied first and the self theme last (winning).
    fn build_ui(&self) -> Result<UiPalette, ThemeParseError> {
        let mut ui = UiPalette::default();
        for model in self.models.iter().rev() {
            if let Some(source) = model.ui.as_ref() {
                overlay_color_fields!(source, ui, &self.primitives, "ui";
                    bg_base, text, bg_modal, warning, note, accent,
                    status_bar_bg, status_bar_fg, status_bar_dim, status_bar_accent,
                    ribbon_divider, ribbon_text,
                    header_bar_bg, header_bar_fg, header_repo, header_branch,
                    header_dim, header_staged, header_prefix,
                    scrollbar_thumb, scrollbar_track,
                    modal_border, modal_title, modal_text, modal_dim,
                    modal_selected_bg, modal_selected_fg,
                    count_dels, count_adds, count_mods, count_movs, count_notes);
            }
        }
        Ok(ui)
    }
}

/// Primitive swatch table for the effective mode: the mode baseline overlaid
/// by the inherit chain, bases first so the self theme wins.
fn primitives_for(models: &[ThemeModel], mode: Mode) -> Result<PrimitivePalette, ThemeParseError> {
    let mut primitives = match mode {
        Mode::Dark => PrimitivePalette::dark(),
        Mode::Light => PrimitivePalette::light(),
    };
    for model in models.iter().rev() {
        if let Some(source) = model.palette.as_ref() {
            overlay_color_fields!(source, primitives, &primitives, "palette";
                base_bg, base_fg, red, green, yellow, orange, blue, purple,
                cyan, gray, dark_red, dark_green);
        }
    }
    Ok(primitives)
}

/// Compile an inherit chain into a cursor-tiered palette. `models[0]` is the
/// self theme; subsequent entries are inherited bases in decreasing priority.
pub(super) fn compile(models: &[ThemeModel], mode: Mode) -> Result<ThemePalette, ThemeParseError> {
    let compiler = Compiler {
        models,
        primitives: primitives_for(models, mode)?,
        mode,
    };
    let mut tiers = [CursorPalette::default(); Cursor::COUNT];
    for cursor in [Cursor::Rest, Cursor::File, Cursor::Hunk, Cursor::Row] {
        tiers[cursor as usize] = compiler.build_cursor(cursor)?;
    }
    let ui = compiler.build_ui()?;
    Ok(ThemePalette::new(tiers, ui))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    fn compile_model(source: &str) -> Result<ThemePalette, ThemeParseError> {
        let model: ThemeModel = toml::from_str(source).expect("model parses");
        compile(&[model], Mode::Dark)
    }

    fn compile_chain(models: &[ThemeModel]) -> Result<ThemePalette, ThemeParseError> {
        let mode = models
            .iter()
            .find_map(|model| model.mode)
            .unwrap_or(Mode::Dark);
        compile(models, mode)
    }

    #[test]
    fn test_resolve_color_forms() {
        let primitives = PrimitivePalette::dark();
        let primary = ColorValue::Primary;
        assert_eq!(
            resolve_color(&primary, &primitives, "x").unwrap(),
            Color::Primary
        );
        let named = ColorValue::Named("$red".to_string());
        assert_eq!(
            resolve_color(&named, &primitives, "p.red").unwrap(),
            Color::Fixed(203)
        );
        let bare = ColorValue::Named("green".to_string());
        assert_eq!(
            resolve_color(&bare, &primitives, "p.green").unwrap(),
            Color::Fixed(120)
        );
        let unknown = ColorValue::Named("$nolight".to_string());
        assert_eq!(
            resolve_color(&unknown, &primitives, "rest.add.bg").unwrap_err(),
            ThemeParseError::UnresolvedVariable {
                name: "nolight".to_string(),
                path: "rest.add.bg".to_string(),
            }
        );
        let rgb = ColorValue::Rgb { r: 1, g: 2, b: 3 };
        assert_eq!(
            resolve_color(&rgb, &primitives, "x").unwrap(),
            Color::Rgb(1, 2, 3)
        );
        let indexed = ColorValue::Indexed(9);
        assert_eq!(
            resolve_color(&indexed, &primitives, "x").unwrap(),
            Color::Fixed(9)
        );
    }

    #[test]
    fn test_resolve_styled_forms() {
        let primitives = PrimitivePalette::dark();
        let plain = StyledValue::Color(ColorValue::Named("red".to_string()));
        assert_eq!(
            resolve_styled(&plain, &primitives, "p").unwrap(),
            StyledColor::new(Color::Fixed(203))
        );
        #[derive(Deserialize)]
        struct Wrap {
            s: StyledValue,
        }
        let full: Wrap = toml::from_str(
            "s = { color = \"$red\", bold = true, italic = true, dim = true, underline = true }",
        )
        .unwrap();
        assert_eq!(
            resolve_styled(&full.s, &primitives, "p").unwrap(),
            StyledColor::new(Color::Fixed(203))
                .with_bold(true)
                .with_italic(true)
                .with_dim(true)
                .with_underline(true)
        );
        let partial: Wrap = toml::from_str("s = { color = 88, bold = true }").unwrap();
        assert_eq!(
            resolve_styled(&partial.s, &primitives, "p").unwrap(),
            StyledColor::new(Color::Fixed(88)).with_bold(true)
        );
    }

    #[test]
    fn test_diff_tiers_all_kinds() {
        assert_eq!(diff_tiers(DiffKind::Ctx), [Some(DiffKind::Ctx), None]);
        assert_eq!(
            diff_tiers(DiffKind::Add),
            [Some(DiffKind::Add), Some(DiffKind::Ctx)]
        );
        assert_eq!(
            diff_tiers(DiffKind::Del),
            [Some(DiffKind::Del), Some(DiffKind::Ctx)]
        );
        assert_eq!(
            diff_tiers(DiffKind::Mod),
            [Some(DiffKind::Mod), Some(DiffKind::Ctx)]
        );
        assert_eq!(
            diff_tiers(DiffKind::Mov),
            [Some(DiffKind::Mov), Some(DiffKind::Ctx)]
        );
    }

    #[test]
    fn test_variable_substitution_into_ui() {
        let palette = compile_model(
            r##"
            [palette]
            green = "#00ff00"
            [ui]
            count_adds = "$green"
            accent = "red"
            "##,
        )
        .unwrap();
        assert_eq!(
            palette.ui.count_adds,
            Color::Rgb(0, 255, 0),
            "$variable resolves against the [palette] overlay"
        );
        assert_eq!(palette.ui.accent, Color::Fixed(203), "bare primitive name");
    }

    #[test]
    fn test_default_baseline_is_mode_primitives() {
        let palette = compile_model("").unwrap();
        let ctx = palette[Cursor::Rest][DiffKind::Ctx];
        assert_eq!(ctx.surface.bg, Color::Fixed(233), "dark base_bg baseline");
        assert_eq!(ctx.surface.fg, Color::Primary, "dark base_fg baseline");
        assert_eq!(ctx.surface.fill, None);
        assert_eq!(ctx.gutter.num, StyledColor::default());
        assert_eq!(ctx.gutter.icon, StyledColor::default());
        assert_eq!(ctx.syntax, SyntaxPalette::dark());
        assert_eq!(palette[Cursor::Row].file_header.bg, Color::Fixed(233));
        assert_eq!(palette[Cursor::Row].hunk_divider.bg, Color::Fixed(233));
        assert_eq!(
            palette[Cursor::Row][DiffKind::Add].syntax,
            SyntaxPalette::dark()
        );
    }

    #[test]
    fn test_light_mode_baseline() {
        let model: ThemeModel = toml::from_str("mode = \"light\"").unwrap();
        let palette = compile_chain(&[model]).unwrap();
        assert_eq!(
            palette[Cursor::Rest][DiffKind::Ctx].surface.bg,
            Color::Fixed(255)
        );
        assert_eq!(
            palette[Cursor::Rest][DiffKind::Ctx].syntax,
            SyntaxPalette::light()
        );
    }

    #[test]
    fn test_partial_cascade_across_tier_and_ctx_channels() {
        let palette = compile_model(
            r#"
            [rest.ctx]
            bg = 100
            fg = 101
            syntax = { keyword = 111, string = 112 }
            [rest.add]
            bg = 200
            [row.add]
            fill = 210
            "#,
        )
        .unwrap();

        let add = palette[Cursor::Row][DiffKind::Add];
        assert_eq!(add.surface.fill, Some(Color::Fixed(210)), "innermost fill");
        assert_eq!(
            add.surface.bg,
            Color::Fixed(200),
            "row.add falls to rest.add"
        );
        assert_eq!(
            add.surface.fg,
            Color::Fixed(101),
            "row.add fg falls through add channels to the ctx channel"
        );
        assert_eq!(
            add.syntax.keyword,
            Color::Fixed(111),
            "add tokens inherit from ctx along the cursor walk"
        );
        assert_eq!(add.syntax.string, Color::Fixed(112));
        assert_eq!(
            add.syntax.typ,
            SyntaxPalette::dark().typ,
            "unmentioned tokens stay at the mode baseline"
        );

        let ctx = palette[Cursor::Hunk][DiffKind::Ctx];
        assert_eq!(
            ctx.surface.bg,
            Color::Fixed(100),
            "ctx defined at none cascades up"
        );
        assert_eq!(ctx.syntax.keyword, Color::Fixed(111));
    }

    #[test]
    fn test_mov_alias_channel() {
        let palette = compile_model(
            r#"
            [rest.mov]
            bg = 30
            fg = 32
            num = 40
            "#,
        )
        .unwrap();

        let mov = palette[Cursor::Rest][DiffKind::Mov];
        assert_eq!(
            mov.surface.bg,
            Color::Fixed(30),
            "mov resolves its explicit channel"
        );
        assert_eq!(mov.surface.fg, Color::Fixed(32));
        assert_eq!(mov.gutter.num.color, Color::Fixed(40), "mov explicit slot");
        assert_eq!(
            mov.gutter.icon,
            StyledColor::default(),
            "icon not in any channel"
        );

        let add = palette[Cursor::Rest][DiffKind::Add];
        assert_eq!(
            add.surface.bg,
            Color::Fixed(233),
            "non-relocation kinds never touch the mov channel"
        );
    }

    #[test]
    fn test_banner_cascade_is_tier_local() {
        let palette = compile_model(
            r#"
            [rest.file_header]
            bg = 240
            fg = 241
            fill = 242
            "#,
        )
        .unwrap();
        let header = palette[Cursor::Rest].file_header;
        assert_eq!(header.bg, Color::Fixed(240));
        assert_eq!(header.fg, Color::Fixed(241));
        assert_eq!(header.fill, Some(Color::Fixed(242)));

        // Row mode divert: an inner-tier banner applies only there; the
        // outer tier retains its own chain resolution.
        let palette = compile_model("[row.file_header]\nbg = 1\n").unwrap();
        assert_eq!(palette[Cursor::Row].file_header.bg, Color::Fixed(1));
        assert_eq!(palette[Cursor::Rest].file_header.bg, Color::Fixed(233));
    }

    #[test]
    fn test_unknown_syntax_token_rejects() {
        let error = compile_model("[rest.ctx.syntax]\nkwds = 5\n").unwrap_err();
        assert_eq!(
            error,
            ThemeParseError::UnknownToken {
                key: "kwds".to_string(),
                path: "rest.ctx.syntax".to_string(),
            }
        );
        let error = compile_model("[row.add.syntax]\nbogus = 5\n").unwrap_err();
        assert_eq!(
            error.to_string(),
            "unknown syntax token key 'bogus' in 'row.add.syntax'"
        );
    }

    #[test]
    fn test_syntax_overlay_on_every_token_channel() {
        let palette = compile_model(
            r#"
            [rest.ctx.syntax]
            text = 10
            keyword = 11
            typ = 12
            function = 13
            number = 14
            literal = 15
            string = 16
            char_literal = 17
            comment = 18
            operator = 19
            bold = 20
            italic = 21
            strike = 22
            link = 23
            url = 24
            delim = 25
            rule = 26
            code = 27
            table_border = 28
            table_header = 29
            link_border = 30
            quote_border = 31
            fence_border = 32
            "#,
        )
        .unwrap();
        let syntax = palette[Cursor::Rest][DiffKind::Ctx].syntax;
        assert_eq!(syntax.text, Color::Fixed(10));
        assert_eq!(syntax.keyword, Color::Fixed(11));
        assert_eq!(syntax.typ, Color::Fixed(12));
        assert_eq!(syntax.function, Color::Fixed(13));
        assert_eq!(syntax.number, Color::Fixed(14));
        assert_eq!(syntax.literal, Color::Fixed(15));
        assert_eq!(syntax.string, Color::Fixed(16));
        assert_eq!(syntax.char_literal, Color::Fixed(17));
        assert_eq!(syntax.comment, Color::Fixed(18));
        assert_eq!(syntax.operator, Color::Fixed(19));
        assert_eq!(syntax.bold, Color::Fixed(20));
        assert_eq!(syntax.italic, Color::Fixed(21));
        assert_eq!(syntax.strike, Color::Fixed(22));
        assert_eq!(syntax.link, Color::Fixed(23));
        assert_eq!(syntax.url, Color::Fixed(24));
        assert_eq!(syntax.delim, Color::Fixed(25));
        assert_eq!(syntax.rule, Color::Fixed(26));
        assert_eq!(syntax.code, Color::Fixed(27));
        assert_eq!(syntax.table_border, Color::Fixed(28));
        assert_eq!(syntax.table_header, Color::Fixed(29));
        assert_eq!(syntax.link_border, Color::Fixed(30));
        assert_eq!(syntax.quote_border, Color::Fixed(31));
        assert_eq!(syntax.fence_border, Color::Fixed(32));
    }

    #[test]
    fn test_inherit_chain_overlay_priority() {
        let base: ThemeModel = toml::from_str(
            r#"
            name = "dark"
            mode = "dark"
            [ui]
            accent = 10
            [rest.ctx]
            bg = 11
            "#,
        )
        .unwrap();
        let child: ThemeModel = toml::from_str(
            r#"
            name = "child"
            inherits = "dark"
            [ui]
            accent = 20
            "#,
        )
        .unwrap();

        // The registry is applied by the parser; compile the chain directly
        // (self first) to verify overlay priority.
        let palette = compile_chain(&[child.clone(), base.clone()]).unwrap();
        assert_eq!(palette.ui.accent, Color::Fixed(20), "self wins over base");
        assert_eq!(
            palette[Cursor::Rest][DiffKind::Ctx].surface.bg,
            Color::Fixed(11),
            "base values flow through when not overridden"
        );

        // Base chain: base first, then child.
        let palette = compile_chain(&[base, child]).unwrap();
        assert_eq!(palette.ui.accent, Color::Fixed(10), "later models win");
    }

    #[test]
    fn test_primitives_for_overlays_bases_first() {
        let base: ThemeModel = toml::from_str("[palette]\ngreen = 100\n").unwrap();
        let child: ThemeModel = toml::from_str("[palette]\ngreen = 200\nblue = 50\n").unwrap();
        let primitives = primitives_for(&[child, base], Mode::Dark).unwrap();
        assert_eq!(primitives.green, Color::Fixed(200), "self (first) wins");
        assert_eq!(
            primitives.get("blue"),
            Some(Color::Fixed(50)),
            "self-only keys land on the baseline"
        );
        assert_eq!(
            primitives.get("red"),
            Some(Color::Fixed(203)),
            "unmentioned baseline"
        );
        let empty = primitives_for(&[], Mode::Light).unwrap();
        assert_eq!(empty, PrimitivePalette::light());
    }
}
