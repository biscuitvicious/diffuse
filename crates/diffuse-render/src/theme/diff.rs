//! Diff-kind axis and per-kind palette containers.
//!
//! [`DiffKind`] unifies all diff row variants (context, additions, deletions,
//! modifications, relocations) under one palette shape so every kind is
//! styled identically via $O(1)$ array indexing.

use diffuse_core::parse::RowKind;

use super::{styled_color::GutterColors, surface::SurfaceColors, syntax::SyntaxPalette};

/// Semantic palette channel for a diff row. Relocation sides share the
/// single `mov` channel: sides differ in wire line numbers and gutter
/// glyphs, never in theme colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum DiffKind {
    /// Unmodified context row.
    #[default]
    Ctx = 0,
    /// Post-image addition row.
    Add = 1,
    /// Pre-image deletion row.
    Del = 2,
    /// Paired modification row (struck pre-image, filled post-image).
    Mod = 3,
    /// Relocation row (either side).
    Mov = 4,
}

impl DiffKind {
    /// Number of diff kinds indexed by the scoped palettes.
    pub const COUNT: usize = 5;

    /// Map a wire row to its palette channel. Structural rows (file
    /// headers, separators) style as context.
    #[inline]
    #[must_use]
    pub fn from_row(kind: RowKind) -> Self {
        match kind {
            RowKind::Add => Self::Add,
            RowKind::Del => Self::Del,
            RowKind::Mod => Self::Mod,
            RowKind::Mov => Self::Mov,
            _ => Self::Ctx,
        }
    }

    /// Canonical TOML key for this kind.
    #[inline]
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            DiffKind::Ctx => "ctx",
            DiffKind::Add => "add",
            DiffKind::Del => "del",
            DiffKind::Mod => "mod",
            DiffKind::Mov => "mov",
        }
    }
}

/// Complete styling for a diff row kind: its composite surface, gutter
/// colors, and syntax token palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DiffPalette {
    /// Composite background / fill / foreground colors.
    pub surface: SurfaceColors,
    /// Gutter number and icon colors.
    pub gutter: GutterColors,
    /// Syntax and markdown token colors for this row kind.
    pub syntax: SyntaxPalette,
}

impl DiffPalette {
    /// Construct a complete diff palette.
    #[inline]
    #[must_use]
    pub const fn new(surface: SurfaceColors, gutter: GutterColors, syntax: SyntaxPalette) -> Self {
        Self {
            surface,
            gutter,
            syntax,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yansi::Color;

    #[test]
    fn test_diff_kind_from_row_matches_wire_variants() {
        assert_eq!(DiffKind::from_row(RowKind::Ctx), DiffKind::Ctx);
        assert_eq!(DiffKind::from_row(RowKind::Add), DiffKind::Add);
        assert_eq!(DiffKind::from_row(RowKind::Del), DiffKind::Del);
        assert_eq!(DiffKind::from_row(RowKind::Mod), DiffKind::Mod);
        // Both relocation sides share the single mov channel: palette
        // identity is side-independent (sides differ in wire line numbers
        // and gutter glyphs, never in theme colors).
        assert_eq!(DiffKind::from_row(RowKind::Mov), DiffKind::Mov);
        // Structural rows (file headers, separators) style as context.
        assert_eq!(DiffKind::from_row(RowKind::File), DiffKind::Ctx);
        assert_eq!(DiffKind::from_row(RowKind::Sep), DiffKind::Ctx);
        assert_eq!(DiffKind::default(), DiffKind::Ctx);
        assert_eq!(DiffKind::COUNT, 5);
    }

    #[test]
    fn test_diff_kind_names_match_toml_keys() {
        let names = [
            DiffKind::default().name(),
            DiffKind::Add.name(),
            DiffKind::Del.name(),
            DiffKind::Mod.name(),
            DiffKind::Mov.name(),
        ];
        assert_eq!(names, ["ctx", "add", "del", "mod", "mov"]);
    }

    #[test]
    fn test_diff_palette_composition() {
        let palette = DiffPalette::new(
            SurfaceColors::with_fill(Color::Fixed(233), Color::Fixed(30), Color::Fixed(120)),
            GutterColors::from_colors(Color::Fixed(203), Color::Fixed(38)),
            SyntaxPalette::dark(),
        );
        assert_eq!(palette.surface.bg, Color::Fixed(233));
        assert_eq!(palette.surface.fill, Some(Color::Fixed(30)));
        assert_eq!(palette.gutter.num.color, Color::Fixed(203));
        assert_eq!(palette.gutter.icon.color, Color::Fixed(38));
        assert_eq!(palette.syntax.keyword, SyntaxPalette::dark().keyword);

        let default = DiffPalette::default();
        assert_eq!(default.surface, SurfaceColors::default());
        assert_eq!(default.gutter, GutterColors::default());
        assert_eq!(default.syntax, SyntaxPalette::default());
    }
}
