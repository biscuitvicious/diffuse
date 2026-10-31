//! Review note domain model, location, and lifecycle status.

use std::{borrow::Cow, num::NonZeroU32};

use serde::{Deserialize, Serialize};

use super::anchor::{DiffCoord, LineHash};

/// Line number, 1-indexed, optional.
pub type LineNum = Option<std::num::NonZeroU32>;

/// Where a review note is anchored.
///
/// File notes attach to the file as a whole and render on the file header
/// banner; line notes attach to a diff row coordinate and re-anchor by content
/// hash across reloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[derive(Default)]
pub enum NoteLocation {
    /// The file as a whole; rendered on the file header banner.
    #[default]
    File,
    /// Single diff row anchor.
    Line { coord: DiffCoord },
    /// Multi-row range anchor.
    Range { start: DiffCoord, end: DiffCoord },
}

impl NoteLocation {
    /// Construct a single-row line location from a coordinate.
    #[inline]
    #[must_use]
    pub const fn line(coord: DiffCoord) -> Self {
        Self::Line { coord }
    }

    /// Construct a multi-row range location from start and end coordinates.
    #[inline]
    #[must_use]
    pub const fn range(start: DiffCoord, end: DiffCoord) -> Self {
        Self::Range { start, end }
    }

    /// Whether this note targets the file as a whole.
    #[inline]
    pub const fn is_file(&self) -> bool {
        matches!(self, Self::File)
    }

    /// The coordinate anchoring this location on its primary side: the line
    /// coordinate, or the range start for ranges.
    #[inline]
    #[must_use]
    pub const fn primary_coord(&self) -> Option<DiffCoord> {
        match self {
            Self::File => None,
            Self::Line { coord } | Self::Range { start: coord, .. } => Some(*coord),
        }
    }

    /// Reverse old/new line numbers for the location.
    #[inline]
    pub fn reverse(self) -> Self {
        match self {
            Self::File => Self::File,
            Self::Line { coord } => Self::Line {
                coord: DiffCoord {
                    old_line_num: coord.new_line_num,
                    new_line_num: coord.old_line_num,
                },
            },
            Self::Range { start, end } => Self::Range {
                start: DiffCoord {
                    old_line_num: start.new_line_num,
                    new_line_num: start.old_line_num,
                },
                end: DiffCoord {
                    old_line_num: end.new_line_num,
                    new_line_num: end.old_line_num,
                },
            },
        }
    }

    /// Human-readable location label: `old 12` for a pre-image-only line,
    /// plain `12` or `1-3` for post-image numbers, or `12-old 15`
    /// spanning a cross-side coordinate pair. File-level notes label as
    /// `file`.
    #[inline]
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::File => "file".to_string(),
            Self::Line { coord } => coord.label(),
            Self::Range { start, end } => format!("{}-{}", start.label(), end.label()),
        }
    }
}

/// Lifecycle status of a review note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NoteStatus {
    /// Active open feedback (default).
    #[default]
    Open,
    /// Note is actively being addressed.
    InProgress,
    /// Note has been resolved / completed.
    Resolved,
}

impl NoteStatus {
    #[inline]
    pub const fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    #[inline]
    pub const fn is_in_progress(&self) -> bool {
        matches!(self, Self::InProgress)
    }

    #[inline]
    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved)
    }

    /// Whether this status counts as active/open (Open or InProgress).
    #[inline]
    pub const fn counts_as_open(&self) -> bool {
        matches!(self, Self::Open | Self::InProgress)
    }

    /// Next status in the cyclic toggle: Open -> InProgress -> Resolved ->
    /// Open.
    #[inline]
    pub const fn cycle(&self) -> Self {
        match self {
            Self::Open => Self::InProgress,
            Self::InProgress => Self::Resolved,
            Self::Resolved => Self::Open,
        }
    }

    /// Display glyph corresponding to the status.
    #[inline]
    pub const fn glyph(&self) -> &'static str {
        match self {
            Self::Open => "💬",
            Self::InProgress => "🔄",
            Self::Resolved => "✅",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewNote {
    pub id: String,
    pub file: String,
    pub location: NoteLocation,
    pub text: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<LineHash>,
    /// Note lifecycle status: Open (default), InProgress, or Resolved.
    #[serde(default, skip_serializing_if = "NoteStatus::is_open")]
    pub status: NoteStatus,
    /// The output rows captured at note creation time — the original source
    /// the note was written against, preserved for display and hashed into
    /// the content hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub original_source_rows: Vec<crate::wire::OutputRow>,
}

impl Default for ReviewNote {
    fn default() -> Self {
        Self {
            id: String::new(),
            file: String::new(),
            location: NoteLocation::default(),
            text: String::new(),
            created_at: 0,
            content_hash: None,
            status: NoteStatus::Open,
            original_source_rows: Vec::new(),
        }
    }
}

impl ReviewNote {
    /// First line number of the location (preferring the new side), 0 for
    /// file notes or empty coordinates.
    #[inline]
    pub fn start(&self) -> u32 {
        match &self.location {
            NoteLocation::Line { coord } => coord
                .new_line_num
                .or(coord.old_line_num)
                .map_or(0, NonZeroU32::get),
            NoteLocation::Range { start, .. } => start
                .new_line_num
                .or(start.old_line_num)
                .map_or(0, NonZeroU32::get),
            NoteLocation::File => 0,
        }
    }

    /// Last line number of the location (preferring the new side), 0 for
    /// file notes or empty coordinates.
    #[inline]
    pub fn end(&self) -> u32 {
        match &self.location {
            NoteLocation::Line { coord } => coord
                .new_line_num
                .or(coord.old_line_num)
                .map_or(0, NonZeroU32::get),
            NoteLocation::Range { end, .. } => end
                .new_line_num
                .or(end.old_line_num)
                .map_or(0, NonZeroU32::get),
            NoteLocation::File => 0,
        }
    }
}

/// Resolve the `(side, start, end)` line span of a note location for hunk
/// synthesis and row targeting. Prefers new-side numbering when a new line
/// number exists on the coordinate; falls back to old-side numbering.
/// `None` for file notes or coordinates with no line number at all.
pub fn note_line_span(location: &NoteLocation) -> Option<(crate::visible::Side, u32, u32)> {
    let coord_num = |c: &DiffCoord| -> Option<(crate::visible::Side, u32)> {
        if let Some(n) = c.new_line_num {
            Some((crate::visible::Side::New, n.get()))
        } else {
            c.old_line_num.map(|n| (crate::visible::Side::Old, n.get()))
        }
    };
    match *location {
        NoteLocation::File => None,
        NoteLocation::Line { coord } => coord_num(&coord).map(|(side, n)| (side, n, n)),
        NoteLocation::Range { start, end } => {
            let (side_a, n_a) = coord_num(&start)?;
            let (side_b, n_b) = coord_num(&end)?;
            if side_a != side_b {
                return None;
            }
            Some((side_a, n_a.min(n_b), n_a.max(n_b)))
        }
    }
}

/// Normalize non-fenced note text for inline callout rendering:
/// - Strips leading and trailing whitespace
/// - Collapses all runs of whitespace (spaces, tabs, newlines) into a single
///   space
/// - Returns `Cow::Borrowed` when no whitespace normalization is needed
pub fn normalize_note_text<'a>(text: &'a str) -> Cow<'a, str> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Cow::Borrowed("");
    }

    let mut prev_is_space = false;
    let mut needs_rebuild = false;
    for b in trimmed.bytes() {
        if b == b' ' {
            if prev_is_space {
                needs_rebuild = true;
                break;
            }
            prev_is_space = true;
        } else if b.is_ascii_whitespace() {
            needs_rebuild = true;
            break;
        } else {
            prev_is_space = false;
        }
    }

    if !needs_rebuild {
        return Cow::Borrowed(trimmed);
    }

    let mut words = trimmed.split_whitespace();
    let first = words.next().unwrap_or("");
    let mut out = String::with_capacity(trimmed.len());
    out.push_str(first);
    for w in words {
        out.push(' ');
        out.push_str(w);
    }
    Cow::Owned(out)
}

/// Prepend a reply text to an existing note, blockquoting the previous note
/// text.
pub fn reply_to_note_text(original: &str, reply: &str) -> String {
    let quoted = original
        .lines()
        .map(|l| format!("> {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{reply}\n\n{quoted}")
}
