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

/// A token color classifier and ANSI stylizer backed by a [`ThemePalette`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffuseTheme {
    pub name: &'static str,
    pub palette: ThemePalette,
}

/// Compiled scoped palettes, initialized on first access from the bundled
/// TOML assets (`themes/{dark,light}.toml`).
static SCOPED_DARK: OnceLock<palette::scoped::ThemePalette> = OnceLock::new();
static SCOPED_LIGHT: OnceLock<palette::scoped::ThemePalette> = OnceLock::new();

impl DiffuseTheme {
    pub const DEFAULT: Self = Self {
        name: "default",
        palette: ThemePalette::default_palette(),
    };

    pub const LIGHT: Self = Self {
        name: "light",
        palette: ThemePalette::light_palette(),
    };

    /// The compiled scoped palette for this theme, cached as a process-wide
    /// static and compiled from the bundled TOML asset on first access.
    ///
    /// Selection is keyed on the theme mode (`name == "light"`); the legacy
    /// flat [`ThemePalette`](palette::ThemePalette) remains authoritative for
    /// fields with no scoped slot until the consumer migrations complete.
    #[must_use]
    pub fn scoped_palette(&self) -> &'static palette::scoped::ThemePalette {
        if self.name == "light" {
            SCOPED_LIGHT.get_or_init(|| {
                toml::parse_builtin("light")
                    .expect("bundled light theme parses")
                    .palette
            })
        } else {
            SCOPED_DARK.get_or_init(|| {
                toml::parse_builtin("dark")
                    .expect("bundled dark theme parses")
                    .palette
            })
        }
    }

    /// Get the row background and icon style for the specified diff row kind
    /// under this theme.
    #[must_use]
    pub fn row_style(&self, kind: diffuse_core::parse::RowKind) -> RowStyle {
        use diffuse_core::parse::RowKind;
        match kind {
            RowKind::Add => RowStyle {
                icon: "+",
                tint_bg: self.palette.add_tint_bg,
                cursor_bg: self.palette.add_cursor_bg,
                fill_bg: Some(self.palette.add_fill_bg),
                fill_cursor_bg: Some(self.palette.add_fill_cursor_bg),
                fg: self.palette.add_fg,
            },
            RowKind::Del => RowStyle {
                icon: "-",
                tint_bg: self.palette.del_tint_bg,
                cursor_bg: self.palette.del_cursor_bg,
                fill_bg: None,
                fill_cursor_bg: None,
                fg: self.palette.del_fg,
            },
            RowKind::Mod => RowStyle {
                icon: "~",
                tint_bg: self.palette.mod_tint_bg,
                cursor_bg: self.palette.mod_cursor_bg,
                fill_bg: Some(self.palette.mod_fill_bg),
                fill_cursor_bg: Some(self.palette.mod_fill_cursor_bg),
                fg: self.palette.mod_fg,
            },
            _ => RowStyle {
                icon: "≈",
                tint_bg: self.palette.mov_tint_bg,
                cursor_bg: self.palette.mov_cursor_bg,
                fill_bg: None,
                fill_cursor_bg: None,
                fg: self.palette.mov_fg,
            },
        }
    }

    /// Resolve a token color through the scoped palette at the canonical
    /// render cursor tier ([`Cursor::Rest`]), dispatching the row kind to
    /// its diff-kind syntax channel. Mod/Mov rows inherit the ctx channel,
    /// matching the legacy flat `token_color` semantics (frozen by the
    /// phase 4 equivalence suite).
    fn scoped_token_color(
        &self,
        color: TokenColor,
        row_kind: diffuse_core::parse::RowKind,
    ) -> Option<Color> {
        let kind = DiffKind::from_row(row_kind);
        self.scoped_palette()[Cursor::Rest][kind]
            .syntax
            .token_color(color)
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

        let ansi_color = self.scoped_token_color(color, rk);

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

        let ansi_color = self.scoped_token_color(color, rk);

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
    /// `DiffuseTheme::DEFAULT` when detection is unavailable.
    #[must_use]
    pub fn auto_detect() -> Self {
        match detect_terminal_mode() {
            Some(TerminalMode::Light) => Self::LIGHT,
            Some(TerminalMode::Dark) | None => Self::DEFAULT,
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
            Some(TerminalMode::Light) => Self::LIGHT,
            Some(TerminalMode::Dark) | None => Self::DEFAULT,
        }
    }

    /// Resolve a theme by name (`"default"`, `"dark"`, `"black"`, `"light"`,
    /// `"white"`, `"auto"`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "default" | "dark" | "black" => Some(Self::DEFAULT),
            "light" | "white" => Some(Self::LIGHT),
            "auto" => Some(Self::auto_detect()),
            _ => None,
        }
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
    DiffuseTheme::DEFAULT
}

#[cfg(test)]
mod tests;
