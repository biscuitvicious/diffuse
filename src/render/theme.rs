//! Terminal Theme & ANSI Stylizer for the diffuse renderer.
//!
//! Provides the exact 256-color palette (`THEME_MAGIC`), row styles, background
//! tints, cursor backgrounds, and SGR modifiers matching the canonical
//! reviewer.

pub const FG_RESET: &str = "\x1b[39m";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowStyle {
    pub icon: &'static str,
    pub tint: u8,
    pub cursor: u8,
    pub fill: Option<u8>,
    pub fill_cursor: Option<u8>,
    pub fg: &'static str,
}

impl RowStyle {
    /// `\x1b[48;5;{tint}m` — the row's resting background.
    pub fn tint_bg(&self) -> String {
        format!("\x1b[48;5;{}m", self.tint)
    }

    /// `\x1b[48;5;{cursor}m` — the row's selected (lifted) background.
    pub fn cursor_bg(&self) -> String {
        format!("\x1b[48;5;{}m", self.cursor)
    }

    /// `\x1b[48;5;{fill}m` — the mod rows' word-fill background.
    pub fn fill_bg(&self) -> String {
        format!("\x1b[48;5;{}m", self.fill.unwrap_or(self.tint))
    }

    /// `\x1b[48;5;{fill_cursor}m` — the selected word-fill background.
    pub fn fill_cursor_bg(&self) -> String {
        format!("\x1b[48;5;{}m", self.fill_cursor.unwrap_or(self.cursor))
    }
}

pub const S_ADD: RowStyle = RowStyle {
    icon: "+",
    tint: 22,
    cursor: 28,
    fill: Some(28),
    fill_cursor: Some(34),
    fg: "toolDiffAdded",
};

pub const S_DEL: RowStyle = RowStyle {
    icon: "-",
    tint: 52,
    cursor: 88,
    fill: None,
    fill_cursor: None,
    fg: "toolDiffRemoved",
};

pub const S_MOD: RowStyle = RowStyle {
    icon: "~",
    tint: 236,
    cursor: 237,
    fill: Some(28),
    fill_cursor: Some(34),
    fg: "toolDiffContext",
};

pub const S_MOV: RowStyle = RowStyle {
    icon: "≈",
    tint: 236,
    cursor: 237,
    fill: None,
    fill_cursor: None,
    fg: "muted",
};

/// Get the SGR reset code corresponding to an SGR modifier.
pub fn sgr_off(sgr: u32) -> Option<u32> {
    match sgr {
        1 => Some(22), // bold off
        3 => Some(23), // italic off
        4 => Some(24), // underline off
        9 => Some(29), // strikethrough off
        _ => None,
    }
}

/// A token color classifier and ANSI stylizer.
#[derive(Clone, Debug)]
pub struct DiffuseTheme {
    pub name: &'static str,
}

impl DiffuseTheme {
    pub const MAGIC: Self = Self { name: "magic" };
    pub const LEGACY: Self = Self { name: "legacy" };

    /// Render a syntax/markdown token to styled ANSI text.
    pub fn render_token(
        &self,
        color: &str,
        text: &str,
        row_kind: &str,
        tok_kind: Option<&str>,
        sgr: Option<u32>,
    ) -> String {
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
                "accent" | "mdDelim" => Some("\x1b[38;5;51m"),
                _ => Self::fallback_named_color(color),
            }
        };

        let mut out = String::new();
        if let Some(s) = sgr {
            out.push_str(&format!("\x1b[{s}m"));
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
            out.push_str(&format!("\x1b[{off}m"));
        }
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

/// Convert ANSI escape sequences `\x1b[...m` to readable golden markup `{...}`.
pub fn encode_sgr(s: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\x1b\[([0-9;]*)m").unwrap());
    re.replace_all(s, "{$1}").to_string()
}

/// Convert readable golden markup `{...}` back to ANSI escape sequences
/// `\x1b[...m`.
pub fn decode_sgr(s: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\{([0-9;]*)\}").unwrap());
    re.replace_all(s, "\x1b[${1}m").to_string()
}

pub fn default_theme() -> DiffuseTheme {
    DiffuseTheme::MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_theme_token_colors() {
        let theme = DiffuseTheme::MAGIC;
        let kw_add = theme.render_token("syntaxKeyword", "fn", "add", None, None);
        assert_eq!(kw_add, "\x1b[38;5;147mfn\x1b[39m");

        let kw_del = theme.render_token("syntaxKeyword", "fn", "del", None, None);
        assert_eq!(kw_del, "\x1b[38;5;183mfn\x1b[39m");

        let kw_ctx = theme.render_token("syntaxKeyword", "fn", "ctx", None, None);
        assert_eq!(kw_ctx, "\x1b[38;5;141mfn\x1b[39m");
    }

    #[test]
    fn test_sgr_modifiers_application_and_reset() {
        let theme = DiffuseTheme::MAGIC;
        let bold_tok = theme.render_token("mdBold", "heading", "ctx", None, Some(1));
        assert!(bold_tok.starts_with("\x1b[1m"));
        assert!(bold_tok.ends_with("\x1b[22m"));

        let strike_tok = theme.render_token("mdStrike", "removed", "ctx", None, Some(9));
        assert!(strike_tok.starts_with("\x1b[9m"));
        assert!(strike_tok.ends_with("\x1b[29m"));
    }

    #[test]
    fn test_encode_decode_sgr() {
        let raw = "\x1b[38;5;231mReview: mirage\x1b[39m";
        let encoded = encode_sgr(raw);
        assert_eq!(encoded, "{38;5;231}Review: mirage{39}");
        let decoded = decode_sgr(&encoded);
        assert_eq!(decoded, raw);
    }
}
