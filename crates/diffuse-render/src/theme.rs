//! Terminal Theme & ANSI Stylizer for the diffuse renderer.
//!
//! Provides the 256-color / RGB palette, row styles, background
//! tints, cursor backgrounds, and SGR modifiers.

pub mod palette;
pub use palette::*;

pub mod detect;
pub use detect::*;

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

impl DiffuseTheme {
    pub const DEFAULT: Self = Self {
        name: "default",
        palette: ThemePalette::default_palette(),
    };

    pub const LIGHT: Self = Self {
        name: "light",
        palette: ThemePalette::light_palette(),
    };

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

    /// Render a syntax/markdown token directly into an output buffer.
    pub fn write_token<W: std::fmt::Write>(
        &self,
        color: &str,
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

        let ansi_color = self.palette.syntax_token_color(color, rk);

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
    pub fn write_token_stacked<W: std::fmt::Write>(
        &self,
        color: &str,
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

        let ansi_color = self.palette.syntax_token_color(color, rk);

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
        color: &str,
        text: &str,
        row_kind: &str,
        tok_kind: Option<&str>,
        sgr: Option<u32>,
    ) -> String {
        let mut out = String::with_capacity(text.len() + 32);
        self.write_token(color, text, row_kind, tok_kind, sgr, &mut out);
        out
    }

    #[must_use]
    pub fn fallback_color(&self, name: &str) -> Option<Color> {
        self.palette.named_color(name)
    }

    /// Automatically detect the theme based on the terminal's background mode.
    /// Falls back to `DiffuseTheme::DEFAULT` when detection is unavailable.
    #[must_use]
    pub fn auto_detect() -> Self {
        match detect_terminal_mode() {
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
    /// to `default` if invalid.
    #[must_use]
    pub fn from_env() -> Self {
        if let Ok(val) = std::env::var("DIFFUSE_THEME") {
            Self::from_name(&val).unwrap_or_else(Self::auto_detect)
        } else {
            Self::auto_detect()
        }
    }
}

pub fn default_theme() -> DiffuseTheme {
    DiffuseTheme::DEFAULT
}

#[cfg(test)]
mod tests;
