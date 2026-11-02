//! Scope-indexed palette architecture.
//!
//! Replaces flat color permutations with two orthogonal axes: containment
//! [`Scope`] and semantic [`DiffKind`]. Lookup is $O(1)$ pointer arithmetic:
//! `palette[Scope::Row][DiffKind::Add].surface.bg`.
//!
//! This module intentionally coexists with the legacy flat
//! [`super::ThemePalette`](super::ThemePalette) until the render-pipeline and
//! TUI migrations (phases 5-7) repoint their consumers; the scoped
//! [`ThemePalette`] is therefore only reachable through this module until
//! those phases complete.

use std::ops;

use yansi::Color;

use super::super::{
    diff::{DiffKind, DiffPalette},
    scope::Scope,
    surface::SurfaceColors,
};

/// Styling for one containment scope: banner surfaces plus one [`DiffPalette`]
/// per diff kind, dual-indexed by `palette[scope][kind]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ScopePalette {
    /// File header banner surface.
    pub file_header: SurfaceColors,
    /// Hunk divider banner surface.
    pub hunk_divider: SurfaceColors,
    /// Per-kind diff styling, indexed by [`DiffKind`].
    pub diffs: [DiffPalette; DiffKind::COUNT],
}

impl ops::Index<DiffKind> for ScopePalette {
    type Output = DiffPalette;

    #[inline]
    fn index(&self, kind: DiffKind) -> &Self::Output {
        &self.diffs[kind as usize]
    }
}

impl ScopePalette {
    /// Construct a scope palette from its banner surfaces and diff palettes.
    #[inline]
    #[must_use]
    pub const fn new(
        file_header: SurfaceColors,
        hunk_divider: SurfaceColors,
        diffs: [DiffPalette; DiffKind::COUNT],
    ) -> Self {
        Self {
            file_header,
            hunk_divider,
            diffs,
        }
    }
}

/// UI chrome and status surface colors outside the diff-grid scope axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct UiPalette {
    /// Application background canvas.
    pub bg_base: Color,
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

/// Complete scoped theme palette: one [`ScopePalette`] per containment scope
/// plus UI chrome, resolved at load time into flat, `Copy`, array-indexed
/// storage for zero-allocation rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ThemePalette {
    /// One palette per containment scope, ordered by [`Scope`] discriminant.
    pub scopes: [ScopePalette; Scope::COUNT],
    /// UI chrome outside the scope axis.
    pub ui: UiPalette,
}

impl ops::Index<Scope> for ThemePalette {
    type Output = ScopePalette;

    #[inline]
    fn index(&self, scope: Scope) -> &Self::Output {
        &self.scopes[scope as usize]
    }
}

impl ThemePalette {
    /// Construct a theme palette from its scope arrays and UI chrome.
    #[inline]
    #[must_use]
    pub const fn new(scopes: [ScopePalette; Scope::COUNT], ui: UiPalette) -> Self {
        Self { scopes, ui }
    }

    /// The palette for a containment scope.
    #[inline]
    #[must_use]
    pub const fn scope(&self, scope: Scope) -> &ScopePalette {
        &self.scopes[scope as usize]
    }

    /// The palette for a diff kind within a containment scope, resolving the
    /// double axis in a single call.
    #[inline]
    #[must_use]
    pub const fn diff(&self, scope: Scope, kind: DiffKind) -> &DiffPalette {
        &self.scopes[scope as usize].diffs[kind as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        super::super::styled_color::{GutterColors, StyledColor},
        *,
    };

    #[test]
    fn test_scope_palette_indexes_each_diff_kind() {
        let mut palette = ScopePalette::default();
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
    fn test_theme_palette_scope_and_diff_accessors_agree() {
        let mut scopes = [ScopePalette::default(); Scope::COUNT];
        scopes[Scope::Row as usize].file_header =
            SurfaceColors::new(Color::Fixed(234), Color::Fixed(231));
        scopes[Scope::Hunk as usize].diffs[DiffKind::Del as usize].gutter =
            GutterColors::from_styled(
                StyledColor::new(Color::Fixed(203)),
                StyledColor::new(Color::Fixed(88)),
            );
        let palette = ThemePalette::new(scopes, UiPalette::default());

        assert_eq!(
            palette[Scope::Row].file_header.bg,
            Color::Fixed(234),
            "acceptance: direct, zero-allocation double-axis access"
        );
        assert_eq!(
            palette[Scope::Row].file_header,
            palette.scope(Scope::Row).file_header,
            "Index<Scope> and scope() resolve identically"
        );
        assert_eq!(
            &palette[Scope::Hunk][DiffKind::Del],
            palette.diff(Scope::Hunk, DiffKind::Del),
            "double indexing and diff() resolve identically"
        );
        assert_eq!(
            palette[Scope::Hunk][DiffKind::Del].gutter.num.color,
            Color::Fixed(203)
        );
        assert_eq!(
            palette[Scope::Hunk][DiffKind::Del].gutter.icon.color,
            Color::Fixed(88)
        );
        assert_eq!(
            palette[Scope::None][DiffKind::MovSrc],
            DiffPalette::default()
        );
        assert_eq!(
            palette[Scope::File][DiffKind::MovTgt],
            DiffPalette::default()
        );
    }

    #[test]
    fn test_theme_palette_is_copy_and_flat() {
        let palette = ThemePalette::default();
        // Copy semantics: the palette is a flat value, safe to pass by value
        // into renderers with zero heap allocation.
        let copy = palette;
        assert_eq!(copy, palette);
        // The storage is a flat array: every scope slot is contiguous and the
        // discriminant is the index.
        for (i, scope) in Scope::cascade().into_iter().enumerate().rev() {
            assert_eq!(copy.scopes[i], palette[scope]);
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
