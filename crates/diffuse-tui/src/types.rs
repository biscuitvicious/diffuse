//! TUI Domain Types — selection, navigation coordinates, and flat visual row
//! models.

use diffuse_core::{ModSide, notes::DiffCoord, wire::OutputRow};

/// A contiguous line selection within a file, bounded by diff-row
/// coordinates in display order. Each bound carries both the old and new
/// line numbers when the selected rows provide them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub file_idx: usize,
    pub start: DiffCoord,
    pub end: DiffCoord,
}

/// Unique identifier for a modified row within a file, stable across
/// context expansions and collapses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModRowKey {
    pub file_idx: usize,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

impl ModRowKey {
    #[inline]
    pub fn new(file_idx: usize, row: &OutputRow) -> Self {
        Self {
            file_idx,
            old_line: row.old_line,
            new_line: row.new_line,
        }
    }
}

impl Selection {
    /// Whether a row lies within this selection span.
    ///
    /// Delegates to [`DiffCoord::row_in_span`].
    #[inline]
    #[must_use]
    pub fn contains_row(&self, row: &OutputRow) -> bool {
        DiffCoord::row_in_span(row, self.start, self.end)
    }

    /// `file:old 12-new 15`-style location label for the selection span
    /// (plain numbers on the new side, `old` prefix only for pre-image
    /// coordinates).
    #[must_use]
    pub fn loc_label(&self, file_path: &str) -> String {
        if self.start == self.end {
            format!("{file_path}:{}", self.start.label())
        } else {
            format!("{file_path}:{}-{}", self.start.label(), self.end.label())
        }
    }
}

/// Snapshot of cursor position, coordinate anchor, and vertical screen offset
/// used to retain exact viewport position across full or granular diff reloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorSnapshot {
    pub file_path: String,
    pub file_idx: usize,
    pub is_file_header: bool,
    pub is_hunk_divider: bool,
    pub hunk_idx: Option<usize>,
    pub target_new: Option<u32>,
    pub target_old: Option<u32>,
    pub screen_y: usize,
}

/// The first flat row of a hunk's content region (the jump target for hunk
/// navigation, mirroring the reference `hunkStarts`).
#[derive(Clone, Copy, Debug)]
pub struct HunkStart {
    pub flat: usize,
    pub file: usize,
    pub hunk: usize,
}

/// A hunk's allocated context envelope (above/below counts),
/// used for the divider `ctx N▲ M▼` counters.
#[derive(Clone, Copy, Debug)]
pub struct HunkMeta {
    pub above: usize,
    pub below: usize,
}

/// The functional role and payload of a flattened visual row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatRowKind {
    /// File header banner at the top of a file's diff section.
    FileHeader,
    /// Raw hunk header divider (`@@ -lo,count +lo,count @@`) in raw diff mode.
    HunkDivider { hunk_idx: usize },
    /// Diff modification or context line within a hunk.
    DiffRow { hunk_idx: usize, row_idx: usize },
    /// An expanded modification line half (either Del pre-image or Add
    /// post-image).
    ExpandedMod {
        hunk_idx: usize,
        row_idx: usize,
        side: ModSide,
    },
    /// Trailing gap divider (`──── N more lines: ▲ ────`) at the end of a file.
    BottomGapDivider { hunk_idx: usize },
}

/// A flattened visual row within the reviewer viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatRow {
    pub file_idx: usize,
    pub kind: FlatRowKind,
}

impl FlatRow {
    #[inline]
    pub fn file_header(file_idx: usize) -> Self {
        Self {
            file_idx,
            kind: FlatRowKind::FileHeader,
        }
    }

    #[inline]
    pub fn hunk_divider(file_idx: usize, hunk_idx: usize) -> Self {
        Self {
            file_idx,
            kind: FlatRowKind::HunkDivider { hunk_idx },
        }
    }

    #[inline]
    pub fn diff_row(file_idx: usize, hunk_idx: usize, row_idx: usize) -> Self {
        Self {
            file_idx,
            kind: FlatRowKind::DiffRow { hunk_idx, row_idx },
        }
    }

    #[inline]
    pub fn expanded_mod(file_idx: usize, hunk_idx: usize, row_idx: usize, side: ModSide) -> Self {
        Self {
            file_idx,
            kind: FlatRowKind::ExpandedMod {
                hunk_idx,
                row_idx,
                side,
            },
        }
    }

    #[inline]
    pub fn bottom_gap(file_idx: usize, hunk_idx: usize) -> Self {
        Self {
            file_idx,
            kind: FlatRowKind::BottomGapDivider { hunk_idx },
        }
    }

    #[inline]
    pub fn hunk_idx(&self) -> Option<usize> {
        match self.kind {
            FlatRowKind::FileHeader => None,
            FlatRowKind::HunkDivider { hunk_idx }
            | FlatRowKind::DiffRow { hunk_idx, .. }
            | FlatRowKind::ExpandedMod { hunk_idx, .. }
            | FlatRowKind::BottomGapDivider { hunk_idx } => Some(hunk_idx),
        }
    }

    #[inline]
    pub fn diff_row_idx(&self) -> Option<usize> {
        match self.kind {
            FlatRowKind::DiffRow { row_idx, .. } | FlatRowKind::ExpandedMod { row_idx, .. } => {
                Some(row_idx)
            }
            _ => None,
        }
    }

    #[inline]
    pub fn mod_side(&self) -> Option<ModSide> {
        match self.kind {
            FlatRowKind::ExpandedMod { side, .. } => Some(side),
            _ => None,
        }
    }

    #[inline]
    pub fn is_file_header(&self) -> bool {
        matches!(self.kind, FlatRowKind::FileHeader)
    }

    #[inline]
    pub fn is_hunk_divider(&self) -> bool {
        matches!(self.kind, FlatRowKind::HunkDivider { .. })
    }

    #[inline]
    pub fn is_bottom_gap_divider(&self) -> bool {
        matches!(self.kind, FlatRowKind::BottomGapDivider { .. })
    }

    #[inline]
    pub fn is_diff_row(&self) -> bool {
        matches!(
            self.kind,
            FlatRowKind::DiffRow { .. } | FlatRowKind::ExpandedMod { .. }
        )
    }

    #[inline]
    pub fn is_expanded_mod(&self) -> bool {
        matches!(self.kind, FlatRowKind::ExpandedMod { .. })
    }
}

/// Sticky hunk divider layout metadata for viewport rendering and hit-testing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StickyHunkInfo {
    pub file_idx: usize,
    pub hunk_idx: usize,
    pub flat_row: usize,
    pub hunk_vis: usize,
    pub hunk_len: usize,
    pub sticky_lines_count: usize,
    pub sticky_start_seg: usize,
}

/// Sticky file header layout metadata for viewport rendering and hit-testing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StickyHeaderInfo {
    pub file_idx: usize,
    pub banner_row: usize,
    pub banner_vis: usize,
    pub banner_len: usize,
    pub sticky_lines_count: usize,
    pub sticky_start_seg: usize,
    pub hunk: Option<StickyHunkInfo>,
}

impl StickyHeaderInfo {
    #[inline]
    pub fn total_sticky_lines(&self) -> usize {
        self.sticky_lines_count + self.hunk.map_or(0, |h| h.sticky_lines_count)
    }

    /// Number of sticky header lines that obscure content above `row_idx`.
    /// When on the file banner itself, 0 lines are sticky; when on the hunk
    /// divider, only the banner lines are sticky.
    #[inline]
    pub fn effective_sticky_lines_for_row(&self, row_idx: usize) -> usize {
        if row_idx == self.banner_row {
            0
        } else if self.hunk.is_some_and(|h| row_idx == h.flat_row) {
            self.sticky_lines_count
        } else {
            self.total_sticky_lines()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expanded_mod_flat_row_methods() {
        let del = FlatRow::expanded_mod(1, 2, 3, ModSide::Del);
        assert!(del.is_diff_row());
        assert!(del.is_expanded_mod());
        assert!(!del.is_file_header());
        assert!(!del.is_hunk_divider());
        assert!(!del.is_bottom_gap_divider());
        assert_eq!(del.hunk_idx(), Some(2));
        assert_eq!(del.diff_row_idx(), Some(3));
        assert_eq!(del.mod_side(), Some(ModSide::Del));

        let add = FlatRow::expanded_mod(1, 2, 3, ModSide::Add);
        assert!(add.is_diff_row());
        assert!(add.is_expanded_mod());
        assert_eq!(add.mod_side(), Some(ModSide::Add));

        let diff = FlatRow::diff_row(1, 2, 3);
        assert!(diff.is_diff_row());
        assert!(!diff.is_expanded_mod());
        assert_eq!(diff.mod_side(), None);
    }
}
