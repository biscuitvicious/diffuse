//! Terminal Theme & ANSI Stylizer for the diffuse renderer.
//!
//! Provides the 256-color / RGB palette, row styles, background
//! tints, cursor backgrounds, and SGR modifiers.

use std::{env, fmt::Write, sync::OnceLock};

use diffuse_syntax::color::TokenColor;

pub mod palette;
pub use palette::*;

pub mod detect;
pub use detect::*;

pub mod primitive;
pub use primitive::PrimitivePalette;

pub mod cursor;
pub use cursor::Cursor;

pub mod diff;
pub use diff::{DiffKind, DiffPalette};

pub mod styled_color;
pub use styled_color::{GutterColors, StyledColor};

pub mod surface;
pub use surface::SurfaceColors;

pub mod syntax;
pub use syntax::SyntaxPalette;

pub mod toml;
pub use toml::{ParsedTheme, ThemeParseError, parse_builtin, parse_theme_str};

pub mod loader;
pub use loader::{ThemeLoadError, load_theme};

/// Effective terminal mode of a theme (its `mode` key or the first declared
/// in its `inherits` chain).
pub use toml::model::Mode;

pub use crate::ansi::{
    Color, ColorExt, FG_RESET, Style, decode_sgr, encode_sgr, sgr_off, sgr_on_str, strip_ansi,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowStyle {
    pub icon: &'static str,
    pub tint_bg: Color,
    pub cursor_bg: Color,
    pub fill_bg: Option<Color>,
    pub fill_cursor_bg: Option<Color>,
    pub fg: Color,
}

impl RowStyle {
    #[inline]
    #[must_use]
    pub const fn tint_style(&self) -> Style {
        Style::new().bg(self.tint_bg)
    }

    #[inline]
    #[must_use]
    pub const fn cursor_style(&self) -> Style {
        Style::new().bg(self.cursor_bg)
    }

    #[inline]
    #[must_use]
    pub const fn fill_style(&self) -> Option<Style> {
        match self.fill_bg {
            Some(bg) => Some(Style::new().bg(bg)),
            None => None,
        }
    }

    #[inline]
    #[must_use]
    pub const fn fill_cursor_style(&self) -> Option<Style> {
        match self.fill_cursor_bg {
            Some(bg) => Some(Style::new().bg(bg)),
            None => None,
        }
    }

    #[inline]
    #[must_use]
    pub const fn fg_style(&self) -> Style {
        Style::new().fg(self.fg)
    }

    #[inline]
    #[must_use]
    pub fn tint_bg(&self) -> String {
        format!("{}", self.tint_style().prefix())
    }

    #[inline]
    #[must_use]
    pub fn cursor_bg(&self) -> String {
        format!("{}", self.cursor_style().prefix())
    }

    #[inline]
    #[must_use]
    pub fn fill_bg(&self) -> String {
        self.fill_style()
            .map_or_else(String::new, |s| format!("{}", s.prefix()))
    }

    #[inline]
    #[must_use]
    pub fn fill_cursor_bg(&self) -> String {
        self.fill_cursor_style()
            .map_or_else(String::new, |s| format!("{}", s.prefix()))
    }
}

/// A resolved theme: its display name, effective terminal mode, and the
/// compiled palette.
///
/// The palette is a `&'static` reference into a process-wide cache: the
/// bundled builtins initialize once on first access, and (with the phase 7
/// loader) user themes leak their compiled palette into the static cache on
/// load. Copy semantics keep theme handoff allocation-free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffuseTheme {
    /// Theme display name (the asset `name` key; `"default"` for the
    /// bundled dark theme).
    pub name: &'static str,
    /// Effective terminal mode, selecting primitive and syntax baselines.
    pub mode: Mode,
    /// Compiled cursor-tier palette, indexed `palette[cursor][kind]`.
    pub palette: &'static ThemePalette,
}

/// Compiled palettes, initialized on first access from the bundled
/// TOML assets (`themes/{dark,light}.toml`).
static DARK_PALETTE: OnceLock<ThemePalette> = OnceLock::new();
static LIGHT_PALETTE: OnceLock<ThemePalette> = OnceLock::new();

impl DiffuseTheme {
    /// The bundled dark theme (asset `themes/dark.toml`), compiled on first
    /// access and cached as a process-wide static.
    #[must_use]
    pub fn dark() -> Self {
        let parsed = DARK_PALETTE.get_or_init(|| {
            toml::parse_builtin("dark")
                .expect("bundled dark theme parses against the schema")
                .palette
        });
        Self {
            name: "default",
            mode: Mode::Dark,
            palette: parsed,
        }
    }

    /// The bundled light theme (asset `themes/light.toml`), compiled on
    /// first access and cached as a process-wide static.
    #[must_use]
    pub fn light() -> Self {
        let parsed = LIGHT_PALETTE.get_or_init(|| {
            toml::parse_builtin("light")
                .expect("bundled light theme parses against the schema")
                .palette
        });
        Self {
            name: "light",
            mode: Mode::Light,
            palette: parsed,
        }
    }

    /// Get the row background and icon style for the specified diff row kind
    /// under this theme.
    ///
    /// Resolves through the theme palette: tint/fill/foreground at the rest
    /// tier, the cursor and fill-cursor backgrounds at the row tier.
    #[must_use]
    pub fn row_style(&self, kind: diffuse_core::parse::RowKind) -> RowStyle {
        use diffuse_core::parse::RowKind;
        let palette = self.palette;
        let rest = &palette[Cursor::Rest];
        let row = &palette[Cursor::Row];
        match kind {
            RowKind::Add => RowStyle {
                icon: "+",
                tint_bg: rest[DiffKind::Add].surface.bg,
                cursor_bg: row[DiffKind::Add].surface.bg,
                fill_bg: rest[DiffKind::Add].surface.fill,
                fill_cursor_bg: row[DiffKind::Add].surface.fill,
                fg: rest[DiffKind::Add].surface.fg,
            },
            RowKind::Del => RowStyle {
                icon: "-",
                tint_bg: rest[DiffKind::Del].surface.bg,
                cursor_bg: row[DiffKind::Del].surface.bg,
                // Del carries no fill in the row styler (the legacy flat
                // `del_fill_bg` is dead, and the asset clears the row-tier
                // fill to match).
                fill_bg: None,
                fill_cursor_bg: None,
                fg: rest[DiffKind::Del].surface.fg,
            },
            RowKind::Mod => RowStyle {
                icon: "~",
                tint_bg: rest[DiffKind::Mod].surface.bg,
                cursor_bg: row[DiffKind::Mod].surface.bg,
                fill_bg: rest[DiffKind::Mod].surface.fill,
                fill_cursor_bg: row[DiffKind::Mod].surface.fill,
                fg: rest[DiffKind::Mod].surface.fg,
            },
            _ => RowStyle {
                icon: "≈",
                tint_bg: rest[DiffKind::Mov].surface.bg,
                cursor_bg: row[DiffKind::Mov].surface.bg,
                fill_bg: None,
                fill_cursor_bg: None,
                fg: rest[DiffKind::Mov].surface.fg,
            },
        }
    }

    /// Resolve a token color through the theme palette at the canonical
    /// render cursor tier ([`Cursor::Rest`]), dispatching the row kind to
    /// its diff-kind syntax channel. Mod/Mov rows inherit the ctx channel,
    /// matching the legacy flat `token_color` semantics (frozen by the
    /// phase 4 equivalence suite).
    #[inline]
    #[must_use]
    pub fn token_color(
        &self,
        color: TokenColor,
        row_kind: diffuse_core::parse::RowKind,
    ) -> Option<Color> {
        self.palette.token_color(color, row_kind)
    }

    /// Render a syntax/markdown token directly into an output buffer.
    pub fn write_token<W: Write>(
        &self,
        color: TokenColor,
        text: &str,
        row_kind: &str,
        tok_kind: Option<&str>,
        sgr: Option<u32>,
        out: &mut W,
    ) {
        use diffuse_core::parse::RowKind;
        let rk = if row_kind == "add" || tok_kind == Some("add") {
            RowKind::Add
        } else if row_kind == "del" || tok_kind == Some("del") {
            RowKind::Del
        } else if row_kind == "mod" {
            RowKind::Mod
        } else {
            RowKind::Ctx
        };

        let ansi_color = self.token_color(color, rk);

        if let Some(s) = sgr {
            if let Some(on) = sgr_on_str(s) {
                let _ = out.write_str(on);
            } else {
                let _ = write!(out, "\x1b[{s}m");
            }
        }
        if let Some(c) = ansi_color {
            let _ = c.write_fg(out);
            let _ = out.write_str(text);
            let _ = out.write_str(FG_RESET);
        } else {
            let _ = out.write_str(text);
        }
        if let Some(s) = sgr
            && let Some(off) = sgr_off(s)
        {
            let _ = out.write_str(off);
        }
    }

    /// Render a token with stacked SGR modifiers as one combined sequence
    /// (`\x1b[1;4m`), with per-modifier resets in reverse order. Styles
    /// like heading titles combine bold and underline, which a single SGR
    /// slot cannot express.
    pub fn write_token_stacked<W: Write>(
        &self,
        color: TokenColor,
        text: &str,
        row_kind: &str,
        tok_kind: Option<&str>,
        sgrs: &[u32],
        out: &mut W,
    ) {
        use diffuse_core::parse::RowKind;
        let rk = if row_kind == "add" || tok_kind == Some("add") {
            RowKind::Add
        } else if row_kind == "del" || tok_kind == Some("del") {
            RowKind::Del
        } else if row_kind == "mod" {
            RowKind::Mod
        } else {
            RowKind::Ctx
        };

        let ansi_color = self.token_color(color, rk);

        if !sgrs.is_empty() {
            let _ = out.write_str("\x1b[");
            for (i, s) in sgrs.iter().enumerate() {
                if i > 0 {
                    let _ = out.write_str(";");
                }
                let _ = write!(out, "{s}");
            }
            let _ = out.write_str("m");
        }
        if let Some(c) = ansi_color {
            let _ = c.write_fg(out);
            let _ = out.write_str(text);
            let _ = out.write_str(FG_RESET);
        } else {
            let _ = out.write_str(text);
        }
        for s in sgrs.iter().rev() {
            if let Some(off) = sgr_off(*s) {
                let _ = out.write_str(off);
            }
        }
    }

    /// Render a syntax/markdown token to styled ANSI text.
    #[must_use]
    pub fn render_token(
        &self,
        color: TokenColor,
        text: &str,
        row_kind: &str,
        tok_kind: Option<&str>,
        sgr: Option<u32>,
    ) -> String {
        let mut out = String::with_capacity(text.len() + 32);
        self.write_token(color, text, row_kind, tok_kind, sgr, &mut out);
        out
    }

    /// Automatically detect the theme based on the terminal's background mode.
    /// Environment indicators only — never queries the terminal, so this is
    /// safe in one-shot (pager/pipe) modes. Falls back to
    /// [`Self::dark`] when detection is unavailable.
    #[must_use]
    pub fn auto_detect() -> Self {
        match detect_terminal_mode() {
            Some(TerminalMode::Light) => Self::light(),
            Some(TerminalMode::Dark) | None => Self::dark(),
        }
    }

    /// Automatically detect the theme, querying the terminal background via
    /// OSC 11 when the environment tiers are silent. Call ONLY from the
    /// interactive TUI (see
    /// [`detect_terminal_mode_interactive`](crate::theme::detect_terminal_mode_interactive)):
    /// one-shot modes must use [`auto_detect`](Self::auto_detect), or a
    /// late query response leaks into the shell as typed input.
    #[must_use]
    pub fn auto_detect_interactive() -> Self {
        match detect_terminal_mode_interactive() {
            Some(TerminalMode::Light) => Self::light(),
            Some(TerminalMode::Dark) | None => Self::dark(),
        }
    }

    /// Resolve a theme by name (`"default"`, `"dark"`, `"black"`, `"light"`,
    /// `"white"`, `"auto"`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "default" | "dark" | "black" => Some(Self::dark()),
            "light" | "white" => Some(Self::light()),
            "auto" => Some(Self::auto_detect()),
            _ => None,
        }
    }

    /// The opposite-mode theme declared by this theme's pairing metadata
    /// (`light_pair` for dark themes, `dark_pair` for light themes), resolved
    /// by name. `None` when the asset declares no pair or the pair does not
    /// resolve to a known theme.
    #[must_use]
    pub fn paired(&self) -> Option<Self> {
        let parsed = toml::parse_builtin(self.name)?;
        let pair = match parsed.mode {
            toml::model::Mode::Dark => parsed.light_pair,
            toml::model::Mode::Light => parsed.dark_pair,
        }?;
        // A pair naming the theme itself would loop; treat it as unpaired.
        if pair.eq_ignore_ascii_case(self.name) {
            return None;
        }
        Self::from_name(&pair)
    }

    /// Resolve the active theme from the `DIFFUSE_THEME` environment variable,
    /// falling back to automated detection if unset or `auto`, and defaulting
    /// to `default` if invalid. Never queries the terminal; safe in any mode.
    #[must_use]
    pub fn from_env() -> Self {
        if let Ok(val) = env::var("DIFFUSE_THEME") {
            Self::from_name(&val).unwrap_or_else(Self::auto_detect)
        } else {
            Self::auto_detect()
        }
    }

    /// [`from_env`](Self::from_env) for interactive sessions: `auto`
    /// (explicit, unset, or invalid) falls back to the OSC 11 terminal
    /// query. Call ONLY from the interactive TUI.
    #[must_use]
    pub fn from_env_interactive() -> Self {
        if let Ok(val) = env::var("DIFFUSE_THEME") {
            if val.trim().eq_ignore_ascii_case("auto") {
                return Self::auto_detect_interactive();
            }
            if let Some(named) = Self::from_name(&val) {
                return named;
            }
            Self::auto_detect_interactive()
        } else {
            Self::auto_detect_interactive()
        }
    }
}

pub fn default_theme() -> DiffuseTheme {
    DiffuseTheme::dark()
}

#[cfg(test)]
mod baseline;
#[cfg(test)]
mod tests;
