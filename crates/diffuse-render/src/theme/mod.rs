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
    pub tint: u8,
    pub cursor: u8,
    pub fill: Option<u8>,
    pub fill_cursor: Option<u8>,
    pub tint_bg_str: &'static str,
    pub cursor_bg_str: &'static str,
    pub fill_bg_str: &'static str,
    pub fill_cursor_bg_str: &'static str,
    pub fg: &'static str,
}

impl RowStyle {
    /// `\x1b[48;5;{tint}m` — the row's resting background.
    #[inline]
    pub fn tint_bg(&self) -> &'static str {
        self.tint_bg_str
    }

    /// `\x1b[48;5;{cursor}m` — the row's selected (lifted) background.
    #[inline]
    pub fn cursor_bg(&self) -> &'static str {
        self.cursor_bg_str
    }

    /// `\x1b[48;5;{fill}m` — the mod rows' word-fill background.
    #[inline]
    pub fn fill_bg(&self) -> &'static str {
        self.fill_bg_str
    }

    /// `\x1b[48;5;{fill_cursor}m` — the selected word-fill background.
    #[inline]
    pub fn fill_cursor_bg(&self) -> &'static str {
        self.fill_cursor_bg_str
    }
}

pub const S_ADD: RowStyle = RowStyle {
    icon: "+",
    tint: 22,
    cursor: 28,
    fill: Some(28),
    fill_cursor: Some(34),
    tint_bg_str: "\x1b[48;5;22m",
    cursor_bg_str: "\x1b[48;5;28m",
    fill_bg_str: "\x1b[48;5;28m",
    fill_cursor_bg_str: "\x1b[48;5;34m",
    fg: "toolDiffAdded",
};

pub const S_DEL: RowStyle = RowStyle {
    icon: "-",
    tint: 52,
    cursor: 88,
    fill: None,
    fill_cursor: None,
    tint_bg_str: "\x1b[48;5;52m",
    cursor_bg_str: "\x1b[48;5;88m",
    fill_bg_str: "\x1b[48;5;52m",
    fill_cursor_bg_str: "\x1b[48;5;88m",
    fg: "toolDiffRemoved",
};

pub const S_MOD: RowStyle = RowStyle {
    icon: "~",
    tint: 236,
    cursor: 237,
    fill: Some(28),
    fill_cursor: Some(34),
    tint_bg_str: "\x1b[48;5;236m",
    cursor_bg_str: "\x1b[48;5;237m",
    fill_bg_str: "\x1b[48;5;28m",
    fill_cursor_bg_str: "\x1b[48;5;34m",
    fg: "toolDiffContext",
};

pub const S_MOV: RowStyle = RowStyle {
    icon: "≈",
    tint: 236,
    cursor: 237,
    fill: None,
    fill_cursor: None,
    tint_bg_str: "\x1b[48;5;236m",
    cursor_bg_str: "\x1b[48;5;237m",
    fill_bg_str: "\x1b[48;5;236m",
    fill_cursor_bg_str: "\x1b[48;5;237m",
    fg: "muted",
};

/// A token color classifier and ANSI stylizer.
#[derive(Clone, Debug)]
pub struct DiffuseTheme {
    pub name: &'static str,
}

impl DiffuseTheme {
    pub const MAGIC: Self = Self { name: "magic" };
    pub const LEGACY: Self = Self { name: "legacy" };

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
        let is_add = row_kind == "add" || tok_kind == Some("add");
        let is_del = row_kind == "del" || tok_kind == Some("del");

        let ansi_color: Option<&'static str> = if self.name == "legacy" {
            let mut c = color;
            if c == "mdBold" {
                c = "syntaxType";
            } else if c == "mdItalic" || c == "mdUrl" || c == "mdStrike" {
                c = "muted";
            } else if c == "mdLink" {
                c = "text";
            } else if c == "mdDelim" {
                c = "accent";
            }

            if is_add && (c == "syntaxComment" || c == "muted") {
                Some("\x1b[38;5;250m")
            } else if c == "syntaxType" {
                Some("\x1b[38;5;173m")
            } else {
                Self::fallback_named_color(c)
            }
        } else if is_add {
            match color {
                "mdBold" => Some("\x1b[38;5;172m"),
                "syntaxType" | "syntaxNumber" => Some("\x1b[38;5;221m"),
                "syntaxKeyword" => Some("\x1b[38;5;147m"),
                "syntaxFunction" => Some("\x1b[38;5;123m"),
                "syntaxOperator" | "mdLink" => Some("\x1b[38;5;117m"),
                "syntaxComment" => Some("\x1b[38;5;150m"),
                "mdItalic" | "muted" => Some("\x1b[38;5;229m"),
                "syntaxString" => Some("\x1b[38;5;121m"),
                "mdUrl" | "mdStrike" => Some("\x1b[38;5;250m"),
                "accent" | "mdDelim" => Some("\x1b[38;5;51m"),
                _ => Self::fallback_named_color(color),
            }
        } else if is_del {
            match color {
                "mdBold" | "syntaxType" | "syntaxNumber" => Some("\x1b[38;5;222m"),
                "syntaxKeyword" => Some("\x1b[38;5;183m"),
                "syntaxFunction" => Some("\x1b[38;5;159m"),
                "syntaxOperator" | "mdLink" => Some("\x1b[38;5;153m"),
                "mdItalic" | "syntaxComment" | "muted" => Some("\x1b[38;5;224m"),
                "syntaxString" => Some("\x1b[38;5;122m"),
                "mdUrl" | "mdStrike" => Some("\x1b[38;5;248m"),
                "accent" | "mdDelim" => Some("\x1b[38;5;51m"),
                _ => Self::fallback_named_color(color),
            }
        } else {
            match color {
                "mdBold" => Some("\x1b[38;5;172m"),
                "syntaxType" => Some("\x1b[38;5;173m"),
                "syntaxKeyword" => Some("\x1b[38;5;141m"),
                "syntaxFunction" => Some("\x1b[38;5;117m"),
                "syntaxNumber" => Some("\x1b[38;5;179m"),
                "syntaxOperator" => Some("\x1b[38;5;110m"),
                "mdItalic" => Some("\x1b[38;5;187m"),
                "syntaxString" => Some("\x1b[38;5;114m"),
                "mdLink" => Some("\x1b[38;5;75m"),
                "mdUrl" | "mdStrike" | "syntaxComment" | "muted" => Some("\x1b[38;5;243m"),
                "mdRule" => Some("\x1b[38;5;220m"),
                "accent" | "mdDelim" => Some("\x1b[38;5;51m"),
                _ => Self::fallback_named_color(color),
            }
        };

        if let Some(s) = sgr {
            use std::fmt::Write;
            let _ = write!(out, "\x1b[{s}m");
        }
        if let Some(c) = ansi_color {
            out.push_str(c);
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

    pub fn fallback_named_color(name: &str) -> Option<&'static str> {
        match name {
            "toolDiffAdded" => Some("\x1b[38;5;150m"),
            "toolDiffRemoved" => Some("\x1b[38;5;167m"),
            "toolDiffContext" | "muted" => Some("\x1b[38;5;244m"),
            "warning" => Some("\x1b[38;5;220m"),
            "border" => Some("\x1b[38;5;240m"),
            "accent" => Some("\x1b[38;5;214m"),
            _ => None,
        }
    }
}

pub fn default_theme() -> DiffuseTheme {
    DiffuseTheme::MAGIC
}

#[cfg(test)]
mod tests;
