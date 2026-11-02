//! Character literal and quote syntax profiles.

/// Style of Unicode escapes supported inside character literals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnicodeEscapeStyle {
    #[default]
    None,
    /// Braced Unicode scalar escape, e.g. `\u{1F980}` with 1 to 6 hex digits
    /// (Rust).
    Braced,
    /// Fixed 4-hex-digit escape, e.g. `\u0041` (Java, C++, Go).
    Fixed4,
    /// Fixed 4 or 8 hex digits, e.g. `\u0041` or `\U0001F980` (C++, Go).
    Fixed4Or8,
}

/// Character literal specification describing the constructs permitted inside
/// `'...'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharLiteralProfile {
    /// Whether arbitrary Unicode characters/scalars are permitted directly
    /// (e.g. `'🦀'`).
    pub unicode: bool,
    /// Maximum hex digits in `\xNN` escape (0 if unsupported, e.g. 2 for 8-bit
    /// hex).
    pub max_hex_digits: u8,
    /// If true, `\xNN` hex escape values must be <= 0x7F (Rust char literals
    /// restrict `\x` to 7-bit ASCII).
    pub hex_ascii_only: bool,
    /// Unicode escape format supported (`\u{...}`, `\uXXXX`, etc.).
    pub unicode_escape: UnicodeEscapeStyle,
    /// Whether octal escapes (`\0` or `\NNN`) are permitted.
    pub octal_escape: bool,
}

impl CharLiteralProfile {
    /// Rust char literal specification:
    /// - Unicode scalar values allowed (e.g. `'a'`, `'🦀'`)
    /// - Standard escapes: `\'`, `\"`, `\n`, `\r`, `\t`, `\\`, `\0`
    /// - Hex escape `\xNN` restricted to 7-bit ASCII (0x00..=0x7F)
    /// - Braced Unicode escape `\u{1..6 hex digits}` representing valid Unicode
    ///   scalar
    pub const RUST: Self = Self {
        unicode: true,
        max_hex_digits: 2,
        hex_ascii_only: true,
        unicode_escape: UnicodeEscapeStyle::Braced,
        octal_escape: false,
    };

    /// C / C++ char literal specification:
    /// - Supports hex escape `\xNN`
    /// - Fixed Unicode escape `\uXXXX` or `\UXXXXXXXX`
    /// - Octal escapes `\0`..`\777`
    pub const C_FAMILY: Self = Self {
        unicode: true,
        max_hex_digits: 2,
        hex_ascii_only: false,
        unicode_escape: UnicodeEscapeStyle::Fixed4Or8,
        octal_escape: true,
    };

    /// Go rune literal specification:
    /// - Unicode characters allowed
    /// - Hex escape `\xNN`
    /// - Fixed Unicode escape `\uXXXX` or `\UXXXXXXXX`
    /// - Octal escapes `\000`..`\377`
    pub const GO: Self = Self {
        unicode: true,
        max_hex_digits: 2,
        hex_ascii_only: false,
        unicode_escape: UnicodeEscapeStyle::Fixed4Or8,
        octal_escape: true,
    };

    /// Validate the inner character sequence (between the opening and closing
    /// single quotes). Returns true if the sequence represents a valid
    /// single character literal under this profile.
    pub fn validate_inner(&self, inner: &str) -> bool {
        if inner.is_empty() {
            return false;
        }
        if !inner.starts_with('\\') {
            let mut chars = inner.chars();
            let Some(c) = chars.next() else {
                return false;
            };
            if chars.next().is_some() {
                return false;
            }
            if c == '\'' || c == '\n' || c == '\r' {
                return false;
            }
            if !self.unicode && !c.is_ascii() {
                return false;
            }
            return true;
        }

        // Starts with '\'
        let rest = &inner[1..];
        if rest.is_empty() {
            return false;
        }

        let first = rest.chars().next().unwrap();
        match first {
            // Standard single-character escapes
            '\'' | '"' | '\\' | 'n' | 'r' | 't' => rest.len() == 1,
            'a' | 'b' | 'f' | 'v' => {
                // Supported in C, Go
                if self.octal_escape || self.unicode_escape == UnicodeEscapeStyle::Fixed4Or8 {
                    rest.len() == 1
                } else {
                    false
                }
            }
            '0' if rest.len() == 1 => true,
            '0'..='7' if self.octal_escape => {
                // Octal escape: 1 to 3 octal digits
                if rest.len() > 3 {
                    return false;
                }
                rest.chars().all(|c| ('0'..='7').contains(&c))
            }
            'x' if self.max_hex_digits > 0 => {
                let hex = &rest[1..];
                if hex.len() != self.max_hex_digits as usize {
                    return false;
                }
                if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return false;
                }
                if self.hex_ascii_only {
                    if let Ok(val) = u32::from_str_radix(hex, 16) {
                        if val > 0x7F {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            }
            'u' if self.unicode_escape == UnicodeEscapeStyle::Braced => {
                let u_rest = &rest[1..];
                if !u_rest.starts_with('{') || !u_rest.ends_with('}') {
                    return false;
                }
                let hex = &u_rest[1..u_rest.len() - 1];
                if hex.is_empty() || hex.len() > 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return false;
                }
                let Ok(val) = u32::from_str_radix(hex, 16) else {
                    return false;
                };
                char::from_u32(val).is_some()
            }
            'u' if matches!(
                self.unicode_escape,
                UnicodeEscapeStyle::Fixed4 | UnicodeEscapeStyle::Fixed4Or8
            ) =>
            {
                let hex = &rest[1..];
                if hex.len() != 4 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return false;
                }
                let Ok(val) = u32::from_str_radix(hex, 16) else {
                    return false;
                };
                char::from_u32(val).is_some()
            }
            'U' if self.unicode_escape == UnicodeEscapeStyle::Fixed4Or8 => {
                let hex = &rest[1..];
                if hex.len() != 8 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return false;
                }
                let Ok(val) = u32::from_str_radix(hex, 16) else {
                    return false;
                };
                char::from_u32(val).is_some()
            }
            _ => false,
        }
    }
}

/// Parsing mode for single quotes (`'...'`) in a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SingleQuoteKind {
    /// Multi-character string literal delimited by single quotes: `'...'`
    /// (Python, JS/TS, Shell, TOML, YAML).
    #[default]
    String,
    /// Single-character / rune literal only (Rust, C-family, Go).
    /// Single quotes only parse as valid tokens if their inner content conforms
    /// to the specified character literal rules.
    Char(CharLiteralProfile),
    /// Single quotes are not used for strings or character literals (e.g.
    /// JSON).
    None,
}
