//! Shared mod-count display cells: one canonical segmentation, order, and
//! color palette for `−dels`, `+adds`, `~mods`, `≈movs`, and `💬notes`
//! wherever counts render — status bars, file banners, and the CLI pretty
//! printer alike.

use smallvec::SmallVec;

use crate::core::counts::{CountKind, FileCounts};

/// Highlight palette for mod-count segments by bucket.
pub fn count_color(kind: CountKind) -> u8 {
    match kind {
        CountKind::Dels => 203,
        CountKind::Adds => 120,
        CountKind::Mods => 214,
        CountKind::Movs => 147,
        CountKind::Notes => 220,
    }
}

/// One display cell of the shared mod-count rendering: marker glyph, value,
/// and bucket color.
pub struct CountCell {
    pub marker: &'static str,
    pub value: usize,
    pub color: u8,
}

/// Shared count cells: same segments, order, and colors everywhere counts
/// render (status bars, file banners, pretty printer).
pub fn count_cells(counts: &FileCounts) -> SmallVec<[CountCell; 5]> {
    counts
        .segments()
        .iter()
        .map(|seg| CountCell {
            marker: seg.marker,
            value: seg.value,
            color: count_color(seg.kind),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_cells_order_and_colors() {
        let cells = count_cells(&FileCounts {
            dels: 7,
            adds: 3,
            mods: 1,
            movs: 2,
            notes: 4,
        });
        let texts: Vec<String> = cells
            .iter()
            .map(|c| format!("{}{}", c.marker, c.value))
            .collect();
        assert_eq!(texts, vec!["−7", "+3", "~1", "≈2", "💬4"]);
        let colors: Vec<u8> = cells.iter().map(|c| c.color).collect();
        assert_eq!(colors, vec![203, 120, 214, 147, 220]);
    }

    #[test]
    fn test_count_cells_zero_counts_omitted() {
        let cells = count_cells(&FileCounts {
            dels: 0,
            adds: 5,
            mods: 0,
            movs: 0,
            notes: 0,
        });
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].marker, "+");
        assert_eq!(cells[0].value, 5);
    }
}
