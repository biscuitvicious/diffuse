//! Shared mod-count display cells: one canonical segmentation, order, and
//! color palette for `−dels`, `+adds`, `~mods`, `≈movs`, and `💬notes`
//! wherever counts render — status bars, file banners, and the CLI pretty
//! printer alike.

use smallvec::SmallVec;

use crate::{
    core::counts::{CountKind, FileCounts},
    theme::UiPalette,
};
use yansi::Color;

/// Highlight color for mod-count segments resolved from the scoped
/// [`UiPalette`].
#[must_use]
pub fn count_color(kind: CountKind, ui: &UiPalette) -> Color {
    match kind {
        CountKind::Dels => ui.count_dels,
        CountKind::Adds => ui.count_adds,
        CountKind::Mods => ui.count_mods,
        CountKind::Movs => ui.count_movs,
        CountKind::Notes => ui.count_notes,
        CountKind::InProgressNotes => Color::Rgb(60, 180, 240),
        CountKind::ResolvedNotes => ui.count_adds,
    }
}

/// One display cell of the shared mod-count rendering: marker glyph, value,
/// and bucket color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountCell {
    pub marker: &'static str,
    pub value: usize,
    pub color: Color,
}

/// Shared count cells: same segments, order, and colors everywhere counts
/// render (status bars, file banners, pretty printer).
pub fn count_cells(counts: &FileCounts, ui: &UiPalette) -> SmallVec<[CountCell; 7]> {
    counts
        .segments()
        .iter()
        .map(|seg| CountCell {
            marker: seg.marker,
            value: seg.value,
            color: count_color(seg.kind, ui),
        })
        .collect()
}

/// Format mod counts as a plain text string (`−dels`, `+adds`, `~mods`,
/// `≈movs`, `💬notes`), omitting zero counts. Returns an empty string when
/// all counts are zero.
#[must_use]
pub fn format_counts_plain(counts: &FileCounts) -> String {
    counts
        .segments()
        .iter()
        .map(|seg| format!("{}{}", seg.marker, seg.value))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_cells_order_and_colors() {
        let ui = &crate::theme::DiffuseTheme::DEFAULT.scoped_palette().ui;
        let cells = count_cells(
            &FileCounts {
                dels: 7,
                adds: 3,
                mods: 1,
                movs: 2,
                notes: 4,
                in_progress_notes: 1,
                resolved_notes: 2,
            },
            ui,
        );
        let texts: Vec<String> = cells
            .iter()
            .map(|c| format!("{}{}", c.marker, c.value))
            .collect();
        assert_eq!(texts, vec!["−7", "+3", "~1", "≈2", "💬4", "🔄1", "✅2"]);
        let colors: Vec<Color> = cells.iter().map(|c| c.color).collect();
        assert_eq!(
            colors,
            vec![
                ui.count_dels,
                ui.count_adds,
                ui.count_mods,
                ui.count_movs,
                ui.count_notes,
                Color::Rgb(60, 180, 240),
                ui.count_adds,
            ]
        );
        // The bundled dark asset carries real count colors (not the
        // `Color::Primary` default), so the parity above is meaningful.
        assert_ne!(ui.count_adds, Color::Primary);
    }

    #[test]
    fn test_count_cells_zero_counts_omitted() {
        let ui = &crate::theme::DiffuseTheme::DEFAULT.scoped_palette().ui;
        let cells = count_cells(
            &FileCounts {
                dels: 0,
                adds: 5,
                mods: 0,
                movs: 0,
                notes: 0,
                in_progress_notes: 0,
                resolved_notes: 0,
            },
            ui,
        );
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].marker, "+");
        assert_eq!(cells[0].value, 5);
        assert_eq!(cells[0].color, ui.count_adds);
    }

    #[test]
    fn test_format_counts_plain() {
        let counts = FileCounts {
            dels: 3,
            adds: 12,
            mods: 2,
            movs: 1,
            notes: 4,
            in_progress_notes: 1,
            resolved_notes: 2,
        };
        assert_eq!(format_counts_plain(&counts), "−3 +12 ~2 ≈1 💬4 🔄1 ✅2");

        let empty = FileCounts::default();
        assert_eq!(format_counts_plain(&empty), "");
    }
}
