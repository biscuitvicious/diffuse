//! ANSI compound styling combining colors and text attribute modifiers.

use std::fmt::{self, Write};

use super::{
    color::AnsiColor,
    sgr::{SGR_RESET, sgr_on},
};

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

impl From<AnsiStyle> for yansi::Style {
    fn from(s: AnsiStyle) -> Self {
        let mut style = yansi::Style::new();
        if let Some(fg) = s.fg {
            style = style.fg(yansi::Color::from(fg));
        }
        if let Some(bg) = s.bg {
            style = style.bg(yansi::Color::from(bg));
        }
        if s.bold {
            style = style.bold();
        }
        if s.italic {
            style = style.italic();
        }
        if s.underline {
            style = style.underline();
        }
        if s.strikethrough {
            style = style.strike();
        }
        style
    }
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
    pub fn write_prefix<W: Write>(&self, out: &mut W) -> fmt::Result {
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
