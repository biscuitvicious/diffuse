//! Centralized ANSI escape sequences, color models, and terminal styling
//! primitives.
//!
//! Enforces that no raw `\x1b[...]` strings are hardcoded across modules.

use std::fmt::Write;

pub const SGR_RESET: &str = "\x1b[0m";
pub const FG_RESET: &str = "\x1b[39m";
pub const BG_RESET: &str = "\x1b[49m";

/// An ANSI color specification supporting 256-color palette and 24-bit
/// TrueColor RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnsiColor {
    Reset,
    Index(u8),
    Rgb(u8, u8, u8),
}

impl AnsiColor {
    /// Format the ANSI foreground escape sequence for this color.
    #[must_use]
    pub fn fg_code(&self) -> String {
        match self {
            Self::Reset => FG_RESET.to_string(),
            Self::Index(idx) => format!("\x1b[38;5;{idx}m"),
            Self::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
        }
    }

    /// Format the ANSI background escape sequence for this color.
    #[must_use]
    pub fn bg_code(&self) -> String {
        match self {
            Self::Reset => BG_RESET.to_string(),
            Self::Index(idx) => format!("\x1b[48;5;{idx}m"),
            Self::Rgb(r, g, b) => format!("\x1b[48;2;{r};{g};{b}m"),
        }
    }

    /// Write foreground escape code directly into a string buffer without
    /// intermediate allocations.
    pub fn write_fg<W: Write>(&self, out: &mut W) -> std::fmt::Result {
        match self {
            Self::Reset => write!(out, "{FG_RESET}"),
            Self::Index(idx) => write!(out, "\x1b[38;5;{idx}m"),
            Self::Rgb(r, g, b) => write!(out, "\x1b[38;2;{r};{g};{b}m"),
        }
    }

    /// Write background escape code directly into a string buffer without
    /// intermediate allocations.
    pub fn write_bg<W: Write>(&self, out: &mut W) -> std::fmt::Result {
        match self {
            Self::Reset => write!(out, "{BG_RESET}"),
            Self::Index(idx) => write!(out, "\x1b[48;5;{idx}m"),
            Self::Rgb(r, g, b) => write!(out, "\x1b[48;2;{r};{g};{b}m"),
        }
    }
}

/// An ANSI style combining foreground, background, and SGR text attributes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnsiStyle {
    pub fg: Option<AnsiColor>,
    pub bg: Option<AnsiColor>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl AnsiStyle {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }

    #[must_use]
    pub const fn fg(mut self, color: AnsiColor) -> Self {
        self.fg = Some(color);
        self
    }

    #[must_use]
    pub const fn bg(mut self, color: AnsiColor) -> Self {
        self.bg = Some(color);
        self
    }

    #[must_use]
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    #[must_use]
    pub const fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    #[must_use]
    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    #[must_use]
    pub const fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    /// Render a slice of text wrapped with this style's ANSI escape codes and
    /// reset.
    #[must_use]
    pub fn render(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 32);
        let _ = self.write_prefix(&mut out);
        out.push_str(text);
        out.push_str(SGR_RESET);
        out
    }

    /// Write the styling prefix into a string buffer.
    pub fn write_prefix<W: Write>(&self, out: &mut W) -> std::fmt::Result {
        if self.bold {
            write!(out, "{}", sgr_on(1))?;
        }
        if self.italic {
            write!(out, "{}", sgr_on(3))?;
        }
        if self.underline {
            write!(out, "{}", sgr_on(4))?;
        }
        if self.strikethrough {
            write!(out, "{}", sgr_on(9))?;
        }
        if let Some(fg) = self.fg {
            fg.write_fg(out)?;
        }
        if let Some(bg) = self.bg {
            bg.write_bg(out)?;
        }
        Ok(())
    }
}

/// Return the ANSI sequence to turn on an SGR modifier (e.g. 1=bold, 3=italic,
/// 4=underline, 9=strike).
#[must_use]
pub fn sgr_on(sgr: u32) -> String {
    format!("\x1b[{sgr}m")
}

/// Return the ANSI sequence to turn off an SGR modifier.
#[must_use]
pub fn sgr_off(sgr: u32) -> Option<&'static str> {
    match sgr {
        1 => Some("\x1b[22m"), // bold off
        3 => Some("\x1b[23m"), // italic off
        4 => Some("\x1b[24m"), // underline off
        9 => Some("\x1b[29m"), // strikethrough off
        _ => None,
    }
}

/// Return the numeric SGR code to turn off an SGR modifier.
#[must_use]
pub fn sgr_off_code(sgr: u32) -> Option<u32> {
    match sgr {
        1 => Some(22), // bold off
        3 => Some(23), // italic off
        4 => Some(24), // underline off
        9 => Some(29), // strikethrough off
        _ => None,
    }
}

/// Strip all ANSI escape sequences from a string.
#[must_use]
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// Convert ANSI escape sequences `\x1b[...m` to readable golden markup `{...}`.
#[must_use]
pub fn encode_sgr(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut last_copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            if i > last_copied {
                out.push_str(&s[last_copied..i]);
            }
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'm' {
                out.push('{');
                out.push_str(&s[i + 2..j]);
                out.push('}');
                i = j + 1;
                last_copied = i;
                continue;
            }
        }
        i += 1;
    }
    if last_copied < s.len() {
        out.push_str(&s[last_copied..]);
    }
    out
}

/// Convert readable golden markup `{...}` back to ANSI escape sequences
/// `\x1b[...m`.
#[must_use]
pub fn decode_sgr(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 32);
    let mut last_copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let mut j = i + 1;
            let mut is_sgr = true;
            while j < bytes.len() && bytes[j] != b'}' {
                if !matches!(bytes[j], b'0'..=b'9' | b';') {
                    is_sgr = false;
                    break;
                }
                j += 1;
            }
            if is_sgr && j < bytes.len() && bytes[j] == b'}' {
                out.push_str(&s[last_copied..i]);
                out.push_str("\x1b[");
                out.push_str(&s[i + 1..j]);
                out.push('m');
                i = j + 1;
                last_copied = i;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&s[last_copied..]);
    out
}

#[cfg(test)]
mod tests;
