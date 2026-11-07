//! Styled color token and gutter formatting models.
//!
//! Encapsulates a [`Color`] with optional text attributes (bold, italic, dim,
//! underline) for terminal formatting, and pairs line number and gutter icon
//! colors into [`GutterColors`].

use yansi::{Color, Style};

use crate::{ansi::ColorExt as _, span::StyledSpan};

/// A color combined with text attribute modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyledColor {
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
    pub underline: bool,
}

impl StyledColor {
    /// Construct a new unadorned styled color.
    #[inline]
    #[must_use]
    pub const fn new(color: Color) -> Self {
        Self {
            color,
            bold: false,
            italic: false,
            dim: false,
            underline: false,
        }
    }

    /// Construct a styled color with bold attribute enabled.
    #[inline]
    #[must_use]
    pub const fn bold(color: Color) -> Self {
        Self {
            color,
            bold: true,
            italic: false,
            dim: false,
            underline: false,
        }
    }

    /// Construct a styled color with dim attribute enabled.
    #[inline]
    #[must_use]
    pub const fn dim(color: Color) -> Self {
        Self {
            color,
            bold: false,
            italic: false,
            dim: true,
            underline: false,
        }
    }

    /// Construct a styled color with italic attribute enabled.
    #[inline]
    #[must_use]
    pub const fn italic(color: Color) -> Self {
        Self {
            color,
            bold: false,
            italic: true,
            dim: false,
            underline: false,
        }
    }

    /// Construct a styled color with underline attribute enabled.
    #[inline]
    #[must_use]
    pub const fn underline(color: Color) -> Self {
        Self {
            color,
            bold: false,
            italic: false,
            dim: false,
            underline: true,
        }
    }

    /// Set the bold modifier.
    #[inline]
    #[must_use]
    pub const fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }

    /// Set the italic modifier.
    #[inline]
    #[must_use]
    pub const fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// Set the dim modifier.
    #[inline]
    #[must_use]
    pub const fn with_dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }

    /// Set the underline modifier.
    #[inline]
    #[must_use]
    pub const fn with_underline(mut self, underline: bool) -> Self {
        self.underline = underline;
        self
    }

    /// Format the ANSI foreground escape sequence, fusing this color's
    /// attribute flags into a single leading SGR group before the color
    /// parameters (`\x1b[3;38;5;203m` for italic fixed 203). Unadorned
    /// colors emit the plain foreground sequence unchanged.
    #[must_use]
    pub fn fg_code(&self) -> String {
        let fg = self.color.fg_code();
        if self.bold || self.italic || self.dim || self.underline {
            let mut attrs = String::new();
            if self.bold {
                attrs.push_str("1;");
            }
            if self.italic {
                attrs.push_str("3;");
            }
            if self.dim {
                attrs.push_str("2;");
            }
            if self.underline {
                attrs.push_str("4;");
            }
            // fg has the shape `\x1b[<params>m`; splice the attribute group
            // in front of the color parameters.
            let params = fg
                .strip_prefix("\x1b[")
                .and_then(|p| p.strip_suffix('m'))
                .unwrap_or("39");
            return format!("\x1b[{attrs}{params}m");
        }
        fg
    }

    /// Project this gutter color onto a renderer-agnostic [`StyledSpan`]
    /// riding on `bg`, preserving the bold, italic, dim, and underline
    /// flags the ANSI [`Self::fg_code`] path fuses into its escape.
    /// The flags carry no retained SGR ids: the CLI gutter emitter
    /// reproduces the fused spelling from the flags themselves.
    #[inline]
    #[must_use]
    pub fn span(&self, text: impl Into<String>, bg: Option<Color>) -> StyledSpan {
        StyledSpan {
            text: text.into(),
            fg: Some(self.color),
            bg,
            bold: self.bold,
            dim: self.dim,
            italic: self.italic,
            underline: self.underline,
            strikethrough: false,
            sgrs: None,
        }
    }

    /// Build a [`yansi::Style`] combining the color and enabled modifiers.
    #[inline]
    #[must_use]
    pub fn style(&self) -> Style {
        let mut s = Style::new().fg(self.color);
        if self.bold {
            s = s.bold();
        }
        if self.italic {
            s = s.italic();
        }
        if self.dim {
            s = s.dim();
        }
        if self.underline {
            s = s.underline();
        }
        s
    }
}

impl From<Color> for StyledColor {
    #[inline]
    fn from(color: Color) -> Self {
        Self::new(color)
    }
}

impl Default for StyledColor {
    #[inline]
    fn default() -> Self {
        Self::new(Color::Primary)
    }
}

/// Gutter styling pairing line coordinate number and icon glyph colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GutterColors {
    pub num: StyledColor,
    pub icon: StyledColor,
}

impl GutterColors {
    /// Construct gutter colors from styled colors.
    #[inline]
    #[must_use]
    pub const fn from_styled(num: StyledColor, icon: StyledColor) -> Self {
        Self { num, icon }
    }

    /// Construct gutter colors from plain colors with no text modifiers.
    #[inline]
    #[must_use]
    pub const fn from_colors(num: Color, icon: Color) -> Self {
        Self {
            num: StyledColor::new(num),
            icon: StyledColor::new(icon),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fg_code_fuses_attribute_flags() {
        // Unadorned: identical to the color's plain fg code.
        assert_eq!(
            StyledColor::new(Color::Fixed(203)).fg_code(),
            Color::Fixed(203).fg_code()
        );
        assert_eq!(
            StyledColor::new(Color::Primary).fg_code(),
            "\x1b[39m",
            "unadorned reset color keeps its plain reset sequence"
        );
        // Italic-only: byte-identical to the legacy fused italic code.
        assert_eq!(
            StyledColor::new(Color::Fixed(203))
                .with_italic(true)
                .fg_code(),
            "\x1b[3;38;5;203m"
        );
        assert_eq!(
            StyledColor::default().with_italic(true).fg_code(),
            "\x1b[3;39m"
        );
        // Bold-only, dim-only, underline-only.
        assert_eq!(
            StyledColor::new(Color::Fixed(88)).with_bold(true).fg_code(),
            "\x1b[1;38;5;88m"
        );
        assert_eq!(
            StyledColor::new(Color::Fixed(88)).with_dim(true).fg_code(),
            "\x1b[2;38;5;88m"
        );
        assert_eq!(
            StyledColor::new(Color::Fixed(88))
                .with_underline(true)
                .fg_code(),
            "\x1b[4;38;5;88m"
        );
        // Combined flags fuse into one sequence.
        assert_eq!(
            StyledColor::new(Color::Fixed(88))
                .with_bold(true)
                .with_italic(true)
                .with_underline(true)
                .fg_code(),
            "\x1b[1;3;4;38;5;88m"
        );
        // RGB colors fuse the same way.
        assert_eq!(
            StyledColor::new(Color::Rgb(1, 2, 3))
                .with_bold(true)
                .fg_code(),
            "\x1b[1;38;2;1;2;3m"
        );
    }

    #[test]
    fn test_styled_color_constructors() {
        let base = StyledColor::new(Color::Fixed(120));
        assert_eq!(base.color, Color::Fixed(120));
        assert!(!base.bold && !base.italic && !base.dim && !base.underline);
        assert_eq!(base.style(), Style::new().fg(Color::Fixed(120)));

        let b = StyledColor::bold(Color::Fixed(120));
        assert!(b.bold && !b.italic && !b.dim && !b.underline);
        assert_eq!(b.style(), Style::new().fg(Color::Fixed(120)).bold());

        let i = StyledColor::italic(Color::Fixed(120));
        assert!(!i.bold && i.italic && !i.dim && !i.underline);
        assert_eq!(i.style(), Style::new().fg(Color::Fixed(120)).italic());

        let d = StyledColor::dim(Color::Fixed(120));
        assert!(!d.bold && !d.italic && d.dim && !d.underline);
        assert_eq!(d.style(), Style::new().fg(Color::Fixed(120)).dim());

        let u = StyledColor::underline(Color::Fixed(120));
        assert!(!u.bold && !u.italic && !u.dim && u.underline);
        assert_eq!(u.style(), Style::new().fg(Color::Fixed(120)).underline());
    }

    #[test]
    fn test_styled_color_modifiers_and_chaining() {
        let sc = StyledColor::new(Color::Fixed(203))
            .with_bold(true)
            .with_italic(true)
            .with_dim(true)
            .with_underline(true);
        assert!(sc.bold && sc.italic && sc.dim && sc.underline);
        assert_eq!(
            sc.style(),
            Style::new()
                .fg(Color::Fixed(203))
                .bold()
                .italic()
                .dim()
                .underline()
        );

        let unstyled = sc
            .with_bold(false)
            .with_italic(false)
            .with_dim(false)
            .with_underline(false);
        assert!(!unstyled.bold && !unstyled.italic && !unstyled.dim && !unstyled.underline);
        assert_eq!(unstyled.style(), Style::new().fg(Color::Fixed(203)));
    }

    #[test]
    fn test_styled_color_from_color_and_default() {
        let sc: StyledColor = Color::Fixed(42).into();
        assert_eq!(sc, StyledColor::new(Color::Fixed(42)));

        let def = StyledColor::default();
        assert_eq!(def, StyledColor::new(Color::Primary));
    }

    #[test]
    fn test_gutter_colors() {
        let gc = GutterColors::from_colors(Color::Fixed(203), Color::Fixed(120));
        assert_eq!(gc.num, StyledColor::new(Color::Fixed(203)));
        assert_eq!(gc.icon, StyledColor::new(Color::Fixed(120)));

        let styled_gc = GutterColors::from_styled(
            StyledColor::bold(Color::Fixed(203)),
            StyledColor::dim(Color::Fixed(120)),
        );
        assert_eq!(styled_gc.num, StyledColor::bold(Color::Fixed(203)));
        assert_eq!(styled_gc.icon, StyledColor::dim(Color::Fixed(120)));

        let def_gc = GutterColors::default();
        assert_eq!(def_gc.num, StyledColor::default());
        assert_eq!(def_gc.icon, StyledColor::default());
    }
}
