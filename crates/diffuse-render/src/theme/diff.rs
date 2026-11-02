//! Diff-kind axis and per-kind palette containers.
//!
//! [`DiffKind`] unifies all diff row variants (context, additions, deletions,
//! modifications, and relocation sides) under one palette shape so every kind
//! is styled identically via $O(1)$ array indexing.

use diffuse_core::{parse::RowKind, wire::MovSide};

use super::{styled_color::GutterColors, surface::SurfaceColors, syntax::SyntaxPalette};

/// Semantic diff row variant including relocation side asymmetry.
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
    /// Relocation source (removed from its original position).
    MovSrc = 4,
    /// Relocation target (re-inserted at its new position).
    MovTgt = 5,
}

impl DiffKind {
    /// Number of diff kinds indexed by the scoped palettes.
    pub const COUNT: usize = 6;

    /// Map a wire row to its semantic diff kind. Relocation rows without a
    /// declared side resolve to the target variant.
    #[inline]
    #[must_use]
    pub fn from_row(kind: RowKind, mov_side: Option<MovSide>) -> Self {
        match (kind, mov_side) {
            (RowKind::Add, _) => Self::Add,
            (RowKind::Del, _) => Self::Del,
            (RowKind::Mod, _) => Self::Mod,
            (RowKind::Mov, Some(MovSide::Src)) => Self::MovSrc,
            (RowKind::Mov, _) => Self::MovTgt,
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
            DiffKind::MovSrc => "mov_src",
            DiffKind::MovTgt => "mov_tgt",
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
        assert_eq!(DiffKind::from_row(RowKind::Ctx, None), DiffKind::Ctx);
        assert_eq!(DiffKind::from_row(RowKind::Add, None), DiffKind::Add);
        assert_eq!(DiffKind::from_row(RowKind::Del, None), DiffKind::Del);
        assert_eq!(DiffKind::from_row(RowKind::Mod, None), DiffKind::Mod);
        assert_eq!(
            DiffKind::from_row(RowKind::Mov, Some(MovSide::Src)),
            DiffKind::MovSrc
        );
        assert_eq!(
            DiffKind::from_row(RowKind::Mov, Some(MovSide::Tgt)),
            DiffKind::MovTgt
        );
        // Relocation rows without a declared side resolve to the target.
        assert_eq!(DiffKind::from_row(RowKind::Mov, None), DiffKind::MovTgt);
        // Structural rows (file headers, separators) style as context.
        assert_eq!(DiffKind::from_row(RowKind::File, None), DiffKind::Ctx);
        assert_eq!(DiffKind::from_row(RowKind::Sep, None), DiffKind::Ctx);
        assert_eq!(
            DiffKind::from_row(RowKind::Add, Some(MovSide::Tgt)),
            DiffKind::Add
        );
        assert_eq!(
            DiffKind::from_row(RowKind::Del, Some(MovSide::Src)),
            DiffKind::Del
        );
        assert_eq!(DiffKind::default(), DiffKind::Ctx);
        assert_eq!(DiffKind::COUNT, 6);
    }

    #[test]
    fn test_diff_kind_names_match_toml_keys() {
        let names = [
            DiffKind::default().name(),
            DiffKind::Add.name(),
            DiffKind::Del.name(),
            DiffKind::Mod.name(),
            DiffKind::MovSrc.name(),
            DiffKind::MovTgt.name(),
        ];
        assert_eq!(names, ["ctx", "add", "del", "mod", "mov_src", "mov_tgt"]);
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
