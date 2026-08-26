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

/// A flattened visual row within the reviewer viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatRow {
    pub file_idx: usize,
    pub hunk_idx: usize,
    pub row_idx: usize,
    pub is_file_header: bool,
    pub is_hunk_divider: bool,
}
