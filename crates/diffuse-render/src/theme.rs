//! Terminal Theme & ANSI Stylizer for the diffuse renderer.
//!
//! Provides the exact 256-color palette (`THEME_MAGIC`), row styles, background
//! tints, cursor backgrounds, and SGR modifiers matching the canonical
//! reviewer.

pub mod palette;
pub use palette::*;

pub use crate::ansi::{FG_RESET, decode_sgr, encode_sgr, sgr_off, strip_ansi};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowStyle {
    pub icon: &'static str,
    pub tint_bg: crate::ansi::AnsiColor,
    pub cursor_bg: crate::ansi::AnsiColor,
    pub fill_bg: Option<crate::ansi::AnsiColor>,
    pub fill_cursor_bg: Option<crate::ansi::AnsiColor>,
    pub fg: crate::ansi::AnsiColor,
}

impl RowStyle {
    #[inline]
    #[must_use]
    pub fn tint_bg(&self) -> String {
        self.tint_bg.bg_code()
    }

    #[inline]
    #[must_use]
    pub fn cursor_bg(&self) -> String {
        self.cursor_bg.bg_code()
    }

    #[inline]
    #[must_use]
    pub fn fill_bg(&self) -> String {
        self.fill_bg.map_or_else(String::new, |c| c.bg_code())
    }

    #[inline]
    #[must_use]
    pub fn fill_cursor_bg(&self) -> String {
        self.fill_cursor_bg
            .map_or_else(String::new, |c| c.bg_code())
    }
}

pub const S_ADD: RowStyle = RowStyle {
    icon: "+",
    tint_bg: crate::ansi::AnsiColor::Index(22),
    cursor_bg: crate::ansi::AnsiColor::Index(28),
    fill_bg: Some(crate::ansi::AnsiColor::Index(28)),
    fill_cursor_bg: Some(crate::ansi::AnsiColor::Index(34)),
    fg: crate::ansi::AnsiColor::Index(150),
};

pub const S_DEL: RowStyle = RowStyle {
    icon: "-",
    tint_bg: crate::ansi::AnsiColor::Index(52),
    cursor_bg: crate::ansi::AnsiColor::Index(88),
    fill_bg: None,
    fill_cursor_bg: None,
    fg: crate::ansi::AnsiColor::Index(167),
};

pub const S_MOD: RowStyle = RowStyle {
    icon: "~",
    tint_bg: crate::ansi::AnsiColor::Index(236),
    cursor_bg: crate::ansi::AnsiColor::Index(237),
    fill_bg: Some(crate::ansi::AnsiColor::Index(28)),
    fill_cursor_bg: Some(crate::ansi::AnsiColor::Index(34)),
    fg: crate::ansi::AnsiColor::Index(244),
};

pub const S_MOV: RowStyle = RowStyle {
    icon: "≈",
    tint_bg: crate::ansi::AnsiColor::Index(236),
    cursor_bg: crate::ansi::AnsiColor::Index(237),
    fill_bg: None,
    fill_cursor_bg: None,
    fg: crate::ansi::AnsiColor::Index(244),
};

/// A token color classifier and ANSI stylizer backed by a [`ThemePalette`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffuseTheme {
    pub name: &'static str,
    pub palette: ThemePalette,
}

impl DiffuseTheme {
    pub const MAGIC: Self = Self {
        name: "magic",
        palette: ThemePalette::default_magic(),
    };
    pub const DEEP_BLACK: Self = Self {
        name: "deep_black",
        palette: ThemePalette::deep_black(),
    };
    pub const LEGACY: Self = Self {
        name: "legacy",
        palette: ThemePalette::legacy(),
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
    pub fn write_token(
        &self,
        color: &str,
        text: &str,
        row_kind: &str,
        tok_kind: Option<&str>,
        sgr: Option<u32>,
        out: &mut String,
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

        let ansi_color = self
            .palette
            .syntax_token_color(color, rk, self.name == "legacy");

        if let Some(s) = sgr {
            use std::fmt::Write;
            let _ = write!(out, "\x1b[{s}m");
        }
        if let Some(c) = ansi_color {
            c.write_fg(out).ok();
            out.push_str(text);
            out.push_str(FG_RESET);
        } else {
            out.push_str(text);
        }
        if let Some(s) = sgr
            && let Some(off) = sgr_off(s)
        {
            out.push_str(off);
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
    pub fn fallback_color(&self, name: &str) -> Option<crate::ansi::AnsiColor> {
        self.palette.named_color(name)
    }

    pub fn fallback_named_color(name: &str) -> Option<&'static str> {
        match name {
            "toolDiffAdded" => Some("\x1b[38;5;150m"),
            "toolDiffRemoved" => Some("\x1b[38;5;167m"),
            "toolDiffContext" | "muted" => Some("\x1b[38;5;244m"),
            "warning" => Some("\x1b[38;5;220m"),
            "border" => Some("\x1b[38;5;240m"),
            _ => None,
        }
    }

    /// Resolve a theme by name (`"magic"`, `"deep_black"`, `"legacy"`).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "magic" => Some(Self::MAGIC),
            "deep_black" | "deep-black" | "dark" | "black" => Some(Self::DEEP_BLACK),
            "legacy" => Some(Self::LEGACY),
            _ => None,
        }
    }

    /// Resolve the active theme from the `DIFFUSE_THEME` environment variable,
    /// falling back to the default `magic` theme if unset or invalid.
    #[must_use]
    pub fn from_env() -> Self {
        if let Ok(val) = std::env::var("DIFFUSE_THEME")
            && let Some(theme) = Self::from_name(&val)
        {
            theme
        } else {
            Self::MAGIC
        }
    }
}

pub fn default_theme() -> DiffuseTheme {
    DiffuseTheme::MAGIC
}

#[cfg(test)]
mod tests;
