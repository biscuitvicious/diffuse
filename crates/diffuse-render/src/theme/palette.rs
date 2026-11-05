//! Cursor- and diff-kind-indexed theme palette.
//!
//! Replaces flat color permutations with two orthogonal axes: containment
//! [`Cursor`] and semantic [`DiffKind`]. Lookup is $O(1)$ pointer arithmetic:
//! `palette[Cursor::Row][DiffKind::Add].surface.bg`.
//!
//! [`ThemePalette`] is the complete resolved theme: one [`CursorPalette`] per
//! cursor tier plus the cursor-independent [`UiPalette`] chrome.

use std::ops;

use diffuse_core::parse::RowKind;
use diffuse_syntax::color::TokenColor;
use yansi::Color;

use super::{
    cursor::Cursor,
    diff::{DiffKind, DiffPalette},
    surface::SurfaceColors,
};

/// Styling for one cursor tier: banner surfaces, the visual-selection band
/// surface, and one [`DiffPalette`] per diff kind, dual-indexed by
/// `palette[cursor][kind]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CursorPalette {
    /// File header banner surface.
    pub file_header: SurfaceColors,
    /// Hunk divider banner surface.
    pub hunk_divider: SurfaceColors,
    /// Visual-selection band surface for diff rows at this tier.
    pub band: SurfaceColors,
    /// Per-kind diff styling, indexed by [`DiffKind`].
    pub diffs: [DiffPalette; DiffKind::COUNT],
}

impl ops::Index<DiffKind> for CursorPalette {
    type Output = DiffPalette;

    #[inline]
    fn index(&self, kind: DiffKind) -> &Self::Output {
        &self.diffs[kind as usize]
    }
}

impl CursorPalette {
    /// Construct a cursor-tier palette from its banner surfaces, band
    /// surface, and diff palettes.
    #[inline]
    #[must_use]
    pub const fn new(
        file_header: SurfaceColors,
        hunk_divider: SurfaceColors,
        band: SurfaceColors,
        diffs: [DiffPalette; DiffKind::COUNT],
    ) -> Self {
        Self {
            file_header,
            hunk_divider,
            band,
            diffs,
        }
    }
}

/// UI chrome and status surface colors outside the diff-grid cursor axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct UiPalette {
    /// Application background canvas.
    pub bg_base: Color,
    /// Base diff text foreground (terminal default when unset).
    pub text: Color,
    /// Modal / overlay background canvas.
    pub bg_modal: Color,

    /// Warning indicator color.
    pub warning: Color,
    /// Review note indicator color.
    pub note: Color,
    /// General accent color.
    pub accent: Color,

    // Status bar & ribbon
    /// Status bar background.
    pub status_bar_bg: Color,
    /// Status bar text foreground.
    pub status_bar_fg: Color,
    /// Status bar dimmed text.
    pub status_bar_dim: Color,
    /// Status bar accent highlights.
    pub status_bar_accent: Color,
    /// Ribbon divider glyph color.
    pub ribbon_divider: Color,
    /// Ribbon text color.
    pub ribbon_text: Color,

    // Header bar
    /// Header bar background.
    pub header_bar_bg: Color,
    /// Header bar foreground.
    pub header_bar_fg: Color,
    /// Repository name color.
    pub header_repo: Color,
    /// Branch name color.
    pub header_branch: Color,
    /// Header dimmed text.
    pub header_dim: Color,
    /// Staged indicator color.
    pub header_staged: Color,
    /// Header path prefix color.
    pub header_prefix: Color,

    // Scrollbar
    /// Scrollbar thumb color.
    pub scrollbar_thumb: Color,
    /// Scrollbar track color.
    pub scrollbar_track: Color,

    // Modals & overlays
    /// Modal border color.
    pub modal_border: Color,
    /// Modal title color.
    pub modal_title: Color,
    /// Modal body text color.
    pub modal_text: Color,
    /// Modal dimmed text color.
    pub modal_dim: Color,
    /// Modal selection background.
    pub modal_selected_bg: Color,
    /// Modal selection foreground.
    pub modal_selected_fg: Color,

    // Count highlights
    /// Deletion count highlight.
    pub count_dels: Color,
    /// Addition count highlight.
    pub count_adds: Color,
    /// Modification count highlight.
    pub count_mods: Color,
    /// Relocation count highlight.
    pub count_movs: Color,
    /// Note count highlight.
    pub count_notes: Color,
}

/// Complete theme palette: one [`CursorPalette`] per cursor tier plus UI
/// chrome, resolved at load time into flat, `Copy`, array-indexed storage for
/// zero-allocation rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ThemePalette {
    /// One palette per cursor tier, ordered by [`Cursor`] discriminant.
    pub tiers: [CursorPalette; Cursor::COUNT],
    /// UI chrome outside the cursor axis.
    pub ui: UiPalette,
}

impl ops::Index<Cursor> for ThemePalette {
    type Output = CursorPalette;

    #[inline]
    fn index(&self, cursor: Cursor) -> &Self::Output {
        &self.tiers[cursor as usize]
    }
}

impl ThemePalette {
    /// Construct a theme palette from its cursor-tier arrays and UI chrome.
    #[inline]
    #[must_use]
    pub const fn new(tiers: [CursorPalette; Cursor::COUNT], ui: UiPalette) -> Self {
        Self { tiers, ui }
    }

    /// The palette for a cursor tier.
    #[inline]
    #[must_use]
    pub const fn tier(&self, cursor: Cursor) -> &CursorPalette {
        &self.tiers[cursor as usize]
    }

    /// The palette for a diff kind within a cursor tier, resolving the
    /// double axis in a single call.
    #[inline]
    #[must_use]
    pub const fn diff(&self, cursor: Cursor, kind: DiffKind) -> &DiffPalette {
        &self.tiers[cursor as usize].diffs[kind as usize]
    }

    /// Resolve syntax / markdown token color in the context of a specific row
    /// kind at the canonical rest tier ([`Cursor::Rest`]).
    #[inline]
    #[must_use]
    pub fn token_color(&self, color: TokenColor, row_kind: RowKind) -> Option<Color> {
        let kind = DiffKind::from_row(row_kind);
        self.tiers[Cursor::Rest as usize].diffs[kind as usize]
            .syntax
            .token_color(color)
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::styled_color::{GutterColors, StyledColor};

    use super::*;

    #[test]
    fn test_cursor_palette_indexes_each_diff_kind() {
        let mut palette = CursorPalette::default();
        palette.diffs[DiffKind::Add as usize].surface =
            SurfaceColors::new(Color::Fixed(30), Color::Fixed(120));
        assert_eq!(
            palette[DiffKind::Add].surface.bg,
            Color::Fixed(30),
            "DiffKind discriminant indexes the matching slot"
        );
        assert_eq!(
            palette[DiffKind::Ctx],
            DiffPalette::default(),
            "untouched slots stay at default"
        );
    }

    #[test]
    fn test_theme_palette_cursor_and_diff_accessors_agree() {
        let mut tiers = [CursorPalette::default(); Cursor::COUNT];
        tiers[Cursor::Row as usize].file_header =
            SurfaceColors::new(Color::Fixed(234), Color::Fixed(231));
        tiers[Cursor::Hunk as usize].diffs[DiffKind::Del as usize].gutter =
            GutterColors::from_styled(
                StyledColor::new(Color::Fixed(203)),
                StyledColor::new(Color::Fixed(88)),
            );
        let palette = ThemePalette::new(tiers, UiPalette::default());

        assert_eq!(
            palette[Cursor::Row].file_header.bg,
            Color::Fixed(234),
            "acceptance: direct, zero-allocation double-axis access"
        );
        assert_eq!(
            palette[Cursor::Row].file_header,
            palette.tier(Cursor::Row).file_header,
            "Index<Cursor> and tier() resolve identically"
        );
        assert_eq!(
            &palette[Cursor::Hunk][DiffKind::Del],
            palette.diff(Cursor::Hunk, DiffKind::Del),
            "double indexing and diff() resolve identically"
        );
        assert_eq!(
            palette[Cursor::Hunk][DiffKind::Del].gutter.num.color,
            Color::Fixed(203)
        );
        assert_eq!(
            palette[Cursor::Hunk][DiffKind::Del].gutter.icon.color,
            Color::Fixed(88)
        );
        assert_eq!(palette[Cursor::Rest][DiffKind::Mov], DiffPalette::default());
        assert_eq!(palette[Cursor::File][DiffKind::Mod], DiffPalette::default());
    }

    #[test]
    fn test_theme_palette_is_copy_and_flat() {
        let palette = ThemePalette::default();
        // Copy semantics: the palette is a flat value, safe to pass by value
        // into renderers with zero heap allocation.
        let copy = palette;
        assert_eq!(copy, palette);
        // The storage is a flat array: every cursor-tier slot is contiguous
        // and the discriminant is the index.
        for (i, cursor) in Cursor::cascade().into_iter().enumerate().rev() {
            assert_eq!(copy.tiers[i], palette[cursor]);
        }
    }

    #[test]
    fn test_ui_palette_default_fields() {
        let ui = UiPalette::default();
        assert_eq!(ui.bg_base, Color::Primary);
        assert_eq!(ui.scrollbar_thumb, Color::Primary);
        assert_eq!(ui.count_adds, Color::Primary);
        assert_eq!(ui.modal_selected_fg, Color::Primary);
    }
}
