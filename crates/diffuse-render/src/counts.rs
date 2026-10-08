//! Shared mod-count display cells: one canonical segmentation, order, and
//! color palette for `−dels`, `+adds`, `~mods`, `≈movs`, and `💬notes`
//! wherever counts render — status bars, file banners, and the CLI pretty
//! printer alike.

use smallvec::SmallVec;

use crate::{
    core::counts::{CountKind, FileCounts},
    theme::ThemePalette,
};
use yansi::Color;

/// Highlight color for mod-count segments resolved from a [`ThemePalette`].
#[must_use]
pub fn count_color(kind: CountKind, palette: &ThemePalette) -> Color {
    match kind {
        CountKind::Dels => palette.count_dels,
        CountKind::Adds => palette.count_adds,
        CountKind::Mods => palette.count_mods,
        CountKind::Movs => palette.count_movs,
        CountKind::Notes => palette.count_notes,
        CountKind::InProgressNotes => Color::Rgb(60, 180, 240),
        CountKind::ResolvedNotes => palette.count_adds,
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
pub fn count_cells(counts: &FileCounts, palette: &ThemePalette) -> SmallVec<[CountCell; 7]> {
    counts
        .segments()
        .iter()
        .map(|seg| CountCell {
            marker: seg.marker,
            value: seg.value,
            color: count_color(seg.kind, palette),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_cells_order_and_colors() {
        let palette = ThemePalette::default_palette();
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
            &palette,
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
                palette.count_dels,
                palette.count_adds,
                palette.count_mods,
                palette.count_movs,
                palette.count_notes,
                Color::Rgb(60, 180, 240),
                palette.count_adds,
            ]
        );
    }

    #[test]
    fn test_count_cells_zero_counts_omitted() {
        let palette = ThemePalette::default_palette();
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
            &palette,
        );
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].marker, "+");
        assert_eq!(cells[0].value, 5);
        assert_eq!(cells[0].color, palette.count_adds);
    }
}
