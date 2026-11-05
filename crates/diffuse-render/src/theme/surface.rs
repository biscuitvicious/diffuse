//! Three-layer composite surface color model for diffuse rendering.
//!
//! Visual elements (rows, headers, dividers, badges) are rendered via three
//! distinct composite layers:
//! 1. Canvas background (`bg`)
//! 2. Optional interior highlight or emphasis fill (`fill`)
//! 3. Foreground text or content glyphs (`fg`)
//!
//! Banner surfaces additionally carry a `path` color (the file-header
//! filename foreground); unset paths resolve to the foreground at cascade
//! time, so banners without path data share the foreground color.

use yansi::{Color, Style};

/// Three-layer composite surface colors for background, optional interior fill,
/// and foreground glyphs, plus the banner path color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceColors {
    /// Layer 1: Base surface canvas background.
    pub bg: Color,
    /// Layer 2: Optional interior emphasis fill (e.g. word diff highlight,
    /// pill/badge background, or selection highlight).
    pub fill: Option<Color>,
    /// Layer 3: Foreground text or primary content glyphs.
    pub fg: Color,
    /// Banner path text color (file-header filename); `None` resolves to the
    /// foreground at cascade time.
    pub path: Option<Color>,
}

impl SurfaceColors {
    /// Construct a new surface without interior fill.
    #[inline]
    #[must_use]
    pub const fn new(bg: Color, fg: Color) -> Self {
        Self {
            bg,
            fill: None,
            fg,
            path: None,
        }
    }

    /// Construct a new surface with an interior emphasis fill.
    #[inline]
    #[must_use]
    pub const fn with_fill(bg: Color, fill: Color, fg: Color) -> Self {
        Self {
            bg,
            fill: Some(fill),
            fg,
            path: None,
        }
    }

    /// The surface without path data: `path` falls back to the foreground.
    #[inline]
    #[must_use]
    pub const fn fg_or_path(&self) -> Color {
        match self.path {
            Some(path) => path,
            None => self.fg,
        }
    }

    /// Return an ANSI [`Style`] setting the canvas background color.
    #[inline]
    #[must_use]
    pub const fn bg_style(&self) -> Style {
        Style::new().bg(self.bg)
    }

    /// Return an ANSI [`Style`] for the interior fill color, if present.
    #[inline]
    #[must_use]
    pub const fn fill_style(&self) -> Option<Style> {
        match self.fill {
            Some(fill) => Some(Style::new().bg(fill)),
            None => None,
        }
    }

    /// Return an ANSI [`Style`] setting the fill background if present, or
    /// falling back to the canvas background.
    #[inline]
    #[must_use]
    pub const fn fill_or_bg_style(&self) -> Style {
        match self.fill {
            Some(fill) => Style::new().bg(fill),
            None => Style::new().bg(self.bg),
        }
    }

    /// Return an ANSI [`Style`] setting the foreground text color.
    #[inline]
    #[must_use]
    pub const fn fg_style(&self) -> Style {
        Style::new().fg(self.fg)
    }

    /// Return an ANSI [`Style`] combining the canvas background and foreground
    /// text colors.
    #[inline]
    #[must_use]
    pub const fn text_style(&self) -> Style {
        Style::new().bg(self.bg).fg(self.fg)
    }

    /// Return an ANSI [`Style`] combining the fill color (or canvas fallback)
    /// and foreground text colors.
    #[inline]
    #[must_use]
    pub const fn text_with_fill_style(&self) -> Style {
        match self.fill {
            Some(fill) => Style::new().bg(fill).fg(self.fg),
            None => Style::new().bg(self.bg).fg(self.fg),
        }
    }
}

impl Default for SurfaceColors {
    #[inline]
    fn default() -> Self {
        Self {
            bg: Color::Primary,
            fill: None,
            fg: Color::Primary,
            path: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_colors_new() {
        let sc = SurfaceColors::new(Color::Fixed(233), Color::Fixed(120));
        assert_eq!(sc.bg, Color::Fixed(233));
        assert_eq!(sc.fill, None);
        assert_eq!(sc.fg, Color::Fixed(120));

        assert_eq!(sc.bg_style(), Style::new().bg(Color::Fixed(233)));
        assert_eq!(sc.fill_style(), None);
        assert_eq!(sc.fill_or_bg_style(), Style::new().bg(Color::Fixed(233)));
        assert_eq!(sc.fg_style(), Style::new().fg(Color::Fixed(120)));
        assert_eq!(
            sc.text_style(),
            Style::new().bg(Color::Fixed(233)).fg(Color::Fixed(120))
        );
        assert_eq!(
            sc.text_with_fill_style(),
            Style::new().bg(Color::Fixed(233)).fg(Color::Fixed(120))
        );
    }

    #[test]
    fn test_surface_colors_with_fill() {
        let sc = SurfaceColors::with_fill(Color::Fixed(233), Color::Fixed(30), Color::Fixed(120));
        assert_eq!(sc.bg, Color::Fixed(233));
        assert_eq!(sc.fill, Some(Color::Fixed(30)));
        assert_eq!(sc.fg, Color::Fixed(120));

        assert_eq!(sc.bg_style(), Style::new().bg(Color::Fixed(233)));
        assert_eq!(sc.fill_style(), Some(Style::new().bg(Color::Fixed(30))));
        assert_eq!(sc.fill_or_bg_style(), Style::new().bg(Color::Fixed(30)));
        assert_eq!(sc.fg_style(), Style::new().fg(Color::Fixed(120)));
        assert_eq!(
            sc.text_style(),
            Style::new().bg(Color::Fixed(233)).fg(Color::Fixed(120))
        );
        assert_eq!(
            sc.text_with_fill_style(),
            Style::new().bg(Color::Fixed(30)).fg(Color::Fixed(120))
        );
    }

    #[test]
    fn test_surface_colors_path_layer() {
        // Phase 6: banner path color; unset paths resolve to the foreground.
        let unset = SurfaceColors::new(Color::Fixed(233), Color::Fixed(120));
        assert_eq!(unset.path, None);
        assert_eq!(unset.fg_or_path(), Color::Fixed(120));

        let with_path = SurfaceColors {
            path: Some(Color::Fixed(251)),
            ..unset
        };
        assert_eq!(with_path.fg_or_path(), Color::Fixed(251));

        let defaulted = SurfaceColors::default();
        assert_eq!(defaulted.path, None);
        assert_eq!(defaulted.fg_or_path(), Color::Primary);
    }

    #[test]
    fn test_surface_colors_default() {
        let sc = SurfaceColors::default();
        assert_eq!(sc.bg, Color::Primary);
        assert_eq!(sc.fill, None);
        assert_eq!(sc.fg, Color::Primary);
    }
}
