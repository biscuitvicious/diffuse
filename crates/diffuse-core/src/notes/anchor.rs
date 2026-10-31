//! Row coordinates, structural content hashes, and diff line anchor
//! verification.

use std::{
    fmt,
    hash::{Hash, Hasher},
    num::{NonZero, NonZeroU32, ParseIntError},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::note::{LineNum, NoteLocation, ReviewNote};

/// Coordinate in a diff row identified by optional old/new line numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffCoord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_line_num: LineNum,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_line_num: LineNum,
}

impl DiffCoord {
    /// Construct a coordinate directly from `LineNum` values.
    ///
    /// # Preconditions
    /// At least one of `old` or `new` must be `Some` (DbC fail-fast
    /// precondition).
    #[inline]
    #[must_use]
    pub fn new(old: LineNum, new: LineNum) -> Self {
        crate::require!(
            old.is_some() || new.is_some(),
            "DiffCoord requires at least one line number (old, new) = ({old:?}, {new:?})"
        );
        Self {
            old_line_num: old,
            new_line_num: new,
        }
    }

    /// Construct a coordinate on the new side.
    #[inline]
    #[must_use]
    pub fn new_side(new: u32) -> Self {
        Self::from_numbers(None, Some(new)).expect("new line number must be non-zero")
    }

    /// Construct a coordinate on the old side.
    #[inline]
    #[must_use]
    pub fn old_side(old: u32) -> Self {
        Self::from_numbers(Some(old), None).expect("old line number must be non-zero")
    }

    /// Construct a coordinate on both sides.
    #[inline]
    #[must_use]
    pub fn both(old: u32, new: u32) -> Self {
        Self::from_numbers(Some(old), Some(new)).expect("line numbers must be non-zero")
    }

    /// Construct a coordinate from raw line numbers, capturing both sides
    /// when available. Zero (0-indexed sentinel) values are dropped. Returns
    /// `None` when both sides are absent — the not-both-`None` invariant is
    /// upheld by construction.
    #[inline]
    #[must_use]
    pub fn from_numbers(old: Option<u32>, new: Option<u32>) -> Option<Self> {
        fn nz(v: Option<u32>) -> LineNum {
            v.filter(|v| *v > 0).and_then(NonZeroU32::new)
        }
        let (old, new) = (nz(old), nz(new));
        (old.is_some() || new.is_some()).then_some(Self {
            old_line_num: old,
            new_line_num: new,
        })
    }

    /// Whether the row's coordinates lie within the span `start..=end` on a
    /// side both bounds share. Mirrors the reviewer selection semantics: a
    /// row qualifies through a side it shares with fully bounded span
    /// bounds; rows without a number on any fully bounded side are outside.
    #[must_use]
    pub fn row_in_span(row: &crate::wire::OutputRow, start: Self, end: Self) -> bool {
        let within = |n: u32, lo: LineNum, hi: LineNum| match (lo, hi) {
            (Some(lo), Some(hi)) => {
                let (lo, hi) = (lo.get().min(hi.get()), lo.get().max(hi.get()));
                n >= lo && n <= hi
            }
            _ => false,
        };
        row.old_line
            .is_some_and(|n| within(n, start.old_line_num, end.old_line_num))
            || row
                .new_line
                .is_some_and(|n| within(n, start.new_line_num, end.new_line_num))
    }

    /// Construct a coordinate from an output row, capturing both old and new
    /// line numbers when available (mod/ctx rows carry both). `None` when the
    /// row has no line numbers at all.
    #[inline]
    #[must_use]
    pub fn from_row(row: &crate::wire::OutputRow) -> Option<Self> {
        Self::from_numbers(row.old_line, row.new_line)
    }

    /// Human-readable coordinate label: `old 12` for a pre-image-only
    /// coordinate (when there is no post-image line), or plain `12` when
    /// a post-image line number is present (defaulting to showing the
    /// post-image line without labelling it).
    #[inline]
    #[must_use]
    pub fn label(self) -> String {
        match (self.old_line_num, self.new_line_num) {
            (_, Some(n)) => format!("{}", n.get()),
            (Some(o), None) => format!("old {}", o.get()),
            (None, None) => String::new(),
        }
    }
}

/// A deterministic 32-bit short hash of trimmed line content.
///
/// Transparently wraps a [`NonZero<u32>`] so that `Option<LineHash>` has
/// zero-cost niche optimization (occupying exactly 4 bytes with no
/// discriminant). Serializes to and deserializes from an 8-character
/// hexadecimal string.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct LineHash(pub NonZero<u32>);

/// A stable FNV-1a 64-bit [`std::hash::Hasher`].
///
/// std's `DefaultHasher` (SipHash-1-3) carries no cross-compiler-version
/// stability guarantee; persisted content hashes must remain valid across
/// toolchain updates, so derived [`Hash`] streams are folded through this
/// fixed implementation instead. Integer writes use fixed little-endian
/// encodings for platform independence.
#[derive(Default)]
pub(crate) struct StableHasher(u64);

impl Hasher for StableHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.write(&[i]);
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_u128(&mut self, i: u128) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.write(&(i as u64).to_le_bytes());
    }

    #[inline]
    fn write_i8(&mut self, i: i8) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_i16(&mut self, i: i16) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_i32(&mut self, i: i32) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_i64(&mut self, i: i64) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_i128(&mut self, i: i128) {
        self.write(&i.to_le_bytes());
    }

    #[inline]
    fn write_isize(&mut self, i: isize) {
        self.write(&(i as i64).to_le_bytes());
    }
}

impl LineHash {
    /// Parse a line hash from a hexadecimal string (up to 8 hex chars).
    pub fn from_hex(hex: &str) -> Result<Self, ParseIntError> {
        let val = u32::from_str_radix(hex.trim(), 16)?;
        let val = if val == 0 { 1 } else { val };
        // SAFETY: val is guaranteed non-zero.
        Ok(Self(unsafe { NonZero::new_unchecked(val) }))
    }

    /// Complete structural content hash over a value's derived [`Hash`]
    /// stream.
    ///
    /// Every field, enum discriminant, and sequence length prefix of the
    /// value participates, so structurally distinct values cannot collide
    /// by encoding ambiguity. The stream is folded through a
    /// [`StableHasher`], whose digest is identical across compiler versions
    /// and platforms, keeping persisted hashes valid across toolchain
    /// updates.
    pub fn of_hashable<H: Hash>(value: &H) -> Self {
        let mut hasher = StableHasher::default();
        value.hash(&mut hasher);
        let full = hasher.finish();
        let folded = (full ^ (full >> 32)) as u32;
        let val = if folded == 0 { 1 } else { folded };
        // SAFETY: val is guaranteed non-zero.
        Self(unsafe { NonZero::new_unchecked(val) })
    }

    /// Content hash over original source rows: the complete structural hash
    /// of the row slice. `None` when there are no rows.
    pub fn of_rows(rows: &[crate::wire::OutputRow]) -> Option<Self> {
        (!rows.is_empty()).then(|| Self::of_hashable(&rows))
    }

    /// The raw non-zero integer representation.
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// Return the formatted 8-character hexadecimal string.
    pub fn to_hex(self) -> String {
        format!("{:08x}", self.0.get())
    }
}

impl fmt::Display for LineHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0.get())
    }
}

impl fmt::LowerHex for LineHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0.get())
    }
}

impl Serialize for LineHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LineHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LineHashVisitor;

        impl<'de> serde::de::Visitor<'de> for LineHashVisitor {
            type Value = LineHash;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a 1-8 character hexadecimal string")
            }

            fn visit_str<E>(self, v: &str) -> Result<LineHash, E>
            where
                E: serde::de::Error,
            {
                LineHash::from_hex(v).map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(LineHashVisitor)
    }
}

/// Whether a row's line numbers exactly match a coordinate on both sides.
pub(crate) fn coord_matches(row: &crate::wire::OutputRow, coord: &DiffCoord) -> bool {
    row.old_line == coord.old_line_num.map(|n| n.get())
        && row.new_line == coord.new_line_num.map(|n| n.get())
}

/// Check whether a note anchors against a row slice. Anchoring semantics:
///
/// 1. `File` -> unanchored.
/// 2. `Line(coord)` -> the first row whose old AND new line numbers both
///    exactly match `coord` locates the anchor; the hash is computed over that
///    single row afterwards.
/// 3. `Range(start, end)` -> the first row matching `start` opens the window;
///    scanning forward, every row is included until (and including) the first
///    row matching `end`; the hash is computed over the whole window
///    afterwards.
///
/// Notes never re-anchor at different lines. The window-located rows are
/// hashed with [`LineHash::of_rows`] after the coordinates are found, and
/// the computed hash must equal the note's stored `content_hash`. Any
/// failure (missing hash, missing coordinates, hash mismatch) leaves the
/// note unanchored.
fn anchors_by_coord(note: &ReviewNote, rows: &[&crate::wire::OutputRow]) -> bool {
    let Some(expected) = note.content_hash else {
        return false;
    };
    let located: Option<Vec<&crate::wire::OutputRow>> = match &note.location {
        NoteLocation::File => None,
        NoteLocation::Line { coord } => rows
            .iter()
            .position(|r| coord_matches(r, coord))
            .map(|i| vec![rows[i]]),
        NoteLocation::Range { start, end } => rows
            .iter()
            .position(|r| coord_matches(r, start))
            .and_then(|first| {
                rows[first..]
                    .iter()
                    .position(|r| coord_matches(r, end))
                    .map(|len| (first, first + len))
            })
            .map(|(first, last)| {
                // The window is the span's visible rows: everything between
                // the boundary matches that the span's sides contain, exactly
                // as a reviewer selection captures them. Hidden rows without
                // a number on a bounded side are not part of the capture.
                rows[first..=last]
                    .iter()
                    .copied()
                    .filter(|r| DiffCoord::row_in_span(r, *start, *end))
                    .collect()
            }),
    };
    let Some(window) = located.as_deref() else {
        return false;
    };
    LineHash::of_hashable(&window) == expected
}

/// Check whether a note anchors in the current output rows of a file.
pub fn note_anchors_in_rows<'r>(
    note: &ReviewNote,
    rows: impl IntoIterator<Item = &'r crate::wire::OutputRow>,
) -> bool {
    let rows: Vec<&crate::wire::OutputRow> = rows.into_iter().collect();
    anchors_by_coord(note, &rows)
}

/// Gap-note content gate: whether the note's stored original source rows
/// still reproduce the plain file content at the location's exact primary
/// coordinates (one stored row per line, primary-side text equal).
///
/// Used as a pre-filter before synthesizing context hunks for notes outside
/// the diff (pipeline gap synthesis, unmodified-file inclusion), so stale
/// notes no longer resurrect hunks. The authoritative coordinate + hash
/// anchor check runs later over the real output rows via
/// [`note_anchors_in_rows`].
pub fn note_anchors_in_context_lines<S: AsRef<str>>(note: &ReviewNote, file_lines: &[S]) -> bool {
    let primary = |c: &DiffCoord| c.new_line_num.or(c.old_line_num).map(|n| n.get());
    let (start, end) = match &note.location {
        NoteLocation::File => return false,
        NoteLocation::Line { coord } => match primary(coord) {
            Some(n) => (n, n),
            None => return false,
        },
        NoteLocation::Range { start, end } => match (primary(start), primary(end)) {
            (Some(s), Some(e)) => (s.min(e), e.max(s)),
            _ => return false,
        },
    };
    if start == 0 || end == 0 || end as usize > file_lines.len() {
        return false;
    }
    if note.original_source_rows.is_empty() {
        return false;
    }
    let new_side = note
        .location
        .primary_coord()
        .is_some_and(|c| c.new_line_num.is_some());
    let row_num = |row: &crate::wire::OutputRow| {
        if new_side { row.new_line } else { row.old_line }
    };
    // The captured rows must cover the recorded span boundaries on the
    // primary side.
    let covers_span = note
        .original_source_rows
        .first()
        .is_some_and(|r| row_num(r) == Some(start))
        && note
            .original_source_rows
            .last()
            .is_some_and(|r| row_num(r) == Some(end));
    if !covers_span {
        return false;
    }
    // Every stored row must match the content line at its own primary-side
    // coordinate. Rows without a primary-side number are malformed captures
    // and fail the gate; interleaved hidden rows (deletions inside a
    // selected range) legitimately leave gaps in the numbering.
    note.original_source_rows.iter().all(|row| {
        let num = if new_side { row.new_line } else { row.old_line };
        let Some(num) = num else {
            return false;
        };
        if num == 0 || num as usize > file_lines.len() {
            return false;
        }
        let text = if new_side {
            row.new_text()
        } else {
            row.old_text()
        };
        text == file_lines[(num - 1) as usize].as_ref()
    })
}
