//! TUI Domain Types — selection, navigation coordinates, and flat visual row
//! models.

/// A contiguous line selection within a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub file_idx: usize,
    pub start: u32,
    pub end: u32,
    pub side_is_new: bool,
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
            | FlatRowKind::BottomGapDivider { hunk_idx } => Some(hunk_idx),
        }
    }

    #[inline]
    pub fn diff_row_idx(&self) -> Option<usize> {
        match self.kind {
            FlatRowKind::DiffRow { row_idx, .. } => Some(row_idx),
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
        matches!(self.kind, FlatRowKind::DiffRow { .. })
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
}
