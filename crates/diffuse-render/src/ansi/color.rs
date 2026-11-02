//! ANSI 256-color and TrueColor models with formatting and extensions.

use std::fmt::{self, Write};

use yansi::{Color, Style};

use super::sgr::{BG_RESET, FG_RESET};

/// An ANSI color specification supporting 256-color palette and 24-bit
/// TrueColor RGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnsiColor {
    Reset,
    Index(u8),
    Rgb(u8, u8, u8),
}

impl From<AnsiColor> for yansi::Color {
    fn from(color: AnsiColor) -> Self {
        match color {
            AnsiColor::Reset => yansi::Color::Primary,
            AnsiColor::Index(idx) => yansi::Color::Fixed(idx),
            AnsiColor::Rgb(r, g, b) => yansi::Color::Rgb(r, g, b),
        }
    }
}

impl From<yansi::Color> for AnsiColor {
    fn from(color: yansi::Color) -> Self {
        match color {
            yansi::Color::Primary => AnsiColor::Reset,
            yansi::Color::Fixed(idx) => AnsiColor::Index(idx),
            yansi::Color::Rgb(r, g, b) => AnsiColor::Rgb(r, g, b),
            yansi::Color::Black => AnsiColor::Index(0),
            yansi::Color::Red => AnsiColor::Index(1),
            yansi::Color::Green => AnsiColor::Index(2),
            yansi::Color::Yellow => AnsiColor::Index(3),
            yansi::Color::Blue => AnsiColor::Index(4),
            yansi::Color::Magenta => AnsiColor::Index(5),
            yansi::Color::Cyan => AnsiColor::Index(6),
            yansi::Color::White => AnsiColor::Index(7),
            yansi::Color::BrightBlack => AnsiColor::Index(8),
            yansi::Color::BrightRed => AnsiColor::Index(9),
            yansi::Color::BrightGreen => AnsiColor::Index(10),
            yansi::Color::BrightYellow => AnsiColor::Index(11),
            yansi::Color::BrightBlue => AnsiColor::Index(12),
            yansi::Color::BrightMagenta => AnsiColor::Index(13),
            yansi::Color::BrightCyan => AnsiColor::Index(14),
            yansi::Color::BrightWhite => AnsiColor::Index(15),
        }
    }
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

    /// Format the ANSI italic foreground escape sequence for this color (fused
    /// SGR).
    #[must_use]
    pub fn italic_fg_code(&self) -> String {
        match self {
            Self::Reset => "\x1b[3;39m".to_string(),
            Self::Index(idx) => format!("\x1b[3;38;5;{idx}m"),
            Self::Rgb(r, g, b) => format!("\x1b[3;38;2;{r};{g};{b}m"),
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
    pub fn write_fg<W: Write>(&self, out: &mut W) -> fmt::Result {
        match self {
            Self::Reset => write!(out, "{FG_RESET}"),
            Self::Index(idx) => write!(out, "\x1b[38;5;{idx}m"),
            Self::Rgb(r, g, b) => write!(out, "\x1b[38;2;{r};{g};{b}m"),
        }
    }

    /// Write background escape code directly into a string buffer without
    /// intermediate allocations.
    pub fn write_bg<W: Write>(&self, out: &mut W) -> fmt::Result {
        match self {
            Self::Reset => write!(out, "{BG_RESET}"),
            Self::Index(idx) => write!(out, "\x1b[48;5;{idx}m"),
            Self::Rgb(r, g, b) => write!(out, "\x1b[48;2;{r};{g};{b}m"),
        }
    }
}

/// Extension trait providing styling code generation and helpers on
/// [`yansi::Color`].
pub trait ColorExt {
    /// Format the ANSI foreground escape sequence for this color.
    fn fg_code(&self) -> String;

    /// Format the ANSI italic foreground escape sequence for this color (fused
    /// SGR).
    fn italic_fg_code(&self) -> String;

    /// Format the ANSI background escape sequence for this color.
    fn bg_code(&self) -> String;

    /// Build a [`yansi::Style`] setting this color as the foreground.
    fn fg_style(&self) -> Style;

    /// Build a [`yansi::Style`] setting this color as the background.
    fn bg_style(&self) -> Style;

    /// Write foreground escape code directly into a string buffer without
    /// intermediate allocations.
    fn write_fg<W: Write>(&self, out: &mut W) -> fmt::Result;

    /// Write background escape code directly into a string buffer without
    /// intermediate allocations.
    fn write_bg<W: Write>(&self, out: &mut W) -> fmt::Result;
}

impl ColorExt for Color {
    #[inline]
    fn fg_code(&self) -> String {
        format!("{}", Style::new().fg(*self).prefix())
    }

    #[inline]
    fn italic_fg_code(&self) -> String {
        format!("{}", Style::new().italic().fg(*self).prefix())
    }

    #[inline]
    fn bg_code(&self) -> String {
        format!("{}", Style::new().bg(*self).prefix())
    }

    #[inline]
    fn fg_style(&self) -> Style {
        Style::new().fg(*self)
    }

    #[inline]
    fn bg_style(&self) -> Style {
        Style::new().bg(*self)
    }

    #[inline]
    fn write_fg<W: Write>(&self, out: &mut W) -> fmt::Result {
        write!(out, "{}", Style::new().fg(*self).prefix())
    }

    #[inline]
    fn write_bg<W: Write>(&self, out: &mut W) -> fmt::Result {
        write!(out, "{}", Style::new().bg(*self).prefix())
    }
}
