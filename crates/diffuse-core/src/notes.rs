//! Note storage, persistence, and line anchoring for review notes.

use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    ffi::OsString,
    fmt, fs, io,
    num::{NonZero, NonZeroU32, ParseIntError},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::hash::{Hash, Hasher};

pub use crate::pipeline::note_runs;

/// Line number, 1-indexed, optional.
pub type LineNum = Option<std::num::NonZeroU32>;

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
}

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

/// The notes database: repo -> (`file_path` -> list of notes).
///
/// The top-level repo key is HEAD-scoped (see [`notes_key`]): notes saved
/// under one commit are invisible after the working tree is committed or
/// the branch changes, instead of lingering as stale annotations.
pub type NotesDB = HashMap<String, HashMap<String, Vec<ReviewNote>>>;

/// Extension-agnostic state directory for diffuse: `$XDG_STATE_HOME/diffuse`,
/// else `~/.local/state/diffuse`, else `.diffuse` relative to the cwd.
///
/// The engine is consumer-agnostic (see the root AGENTS.md) — its default
/// state never lives under an external consumer's private directory. A
/// consumer may point diffuse anywhere it likes via `--notes-file`; the
/// binary's own default stays its own.
pub fn default_state_dir() -> PathBuf {
    state_dir_from(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"))
}

fn state_dir_from(xdg_state_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    if let Some(x) = xdg_state_home {
        return PathBuf::from(x).join("diffuse");
    }
    if let Some(h) = home {
        return PathBuf::from(h)
            .join(".local")
            .join("state")
            .join("diffuse");
    }
    PathBuf::from(".diffuse")
}

/// Default notes database path (`$XDG_STATE_HOME/diffuse/notes.json`).
pub fn default_notes_path() -> PathBuf {
    default_state_dir().join("notes.json")
}

/// Default TUI resume-position path (`$XDG_STATE_HOME/diffuse/state.json`).
pub fn default_state_path() -> PathBuf {
    default_state_dir().join("state.json")
}

/// Current Unix time in milliseconds, for note `created_at`.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Generate a unique review note ID with millisecond timestamp, process ID,
/// and monotonic sequence counter.
pub fn generate_note_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}-{:x}", now_millis(), process::id(), count)
}

/// Resolve the HEAD commit hash of the repository containing `repo_root`.
/// Returns `None` outside a work tree or when the repo has no commits yet.
pub fn head_commit(repo_root: &Path) -> Option<String> {
    diffuse_git::Repository::open(repo_root)
        .ok()
        .and_then(|r| r.head_commit_hash())
}

/// Resolve the commit hash to scope review notes to.
///
/// When reviewing a commit range (e.g. `main..feature`, `A...B`, `HEAD^!`),
/// notes are keyed off the second-to-last (next newest) commit in the range.
/// When reviewing uncommitted changes against the working tree (or when the
/// range has no parent), notes fall back to the live HEAD commit.
pub fn notes_commit(repo_root: &Path, git_revs: &[String]) -> Option<String> {
    diffuse_git::Repository::open(repo_root)
        .ok()
        .and_then(|r| r.notes_commit(git_revs))
}

/// Notes-DB key for a repo at a given HEAD: `{repo}@{head}`.
///
/// Falls back to the bare repo name when HEAD is unknown (non-git
/// contexts, repos without commits yet) so notes keep working there.
/// Entries saved under a previous bare-repo key are simply never matched
/// again once a HEAD is known; they are left on disk untouched.
pub fn notes_key(repo: &str, head: Option<&str>) -> String {
    match head {
        Some(h) if !h.is_empty() => format!("{repo}@{h}"),
        _ => repo.to_string(),
    }
}

/// Convenience wrapper: resolve the target notes commit for `repo_root` under
/// `git_revs` (second-to-last commit when reviewing a range, otherwise HEAD)
/// and build the notes-DB key for `repo`.
pub fn notes_key_for_repo_args(repo_root: &Path, repo: &str, git_revs: &[String]) -> String {
    notes_key(repo, notes_commit(repo_root, git_revs).as_deref())
}

/// Convenience wrapper: resolve the live HEAD under `repo_root` and build
/// the notes-DB key for `repo`.
pub fn notes_key_for_repo(repo_root: &Path, repo: &str) -> String {
    notes_key_for_repo_args(repo_root, repo, &[])
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
struct StableHasher(u64);

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

impl DiffCoord {
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

/// Whether a row's line numbers exactly match a coordinate on both sides.
fn coord_matches(row: &crate::wire::OutputRow, coord: &DiffCoord) -> bool {
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

/// Load notes from disk. Returns an empty DB if the file does not exist or
/// fails to parse.
pub fn load_notes(path: &Path) -> NotesDB {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Save notes to disk.
pub fn save_notes(path: &Path, db: &NotesDB) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(db)?;
    fs::write(path, raw)
}

/// Get all notes for a specific repo and file.
pub fn notes_for_file<'a>(
    db: &'a NotesDB,
    repo: &str,
    file: &str,
) -> impl Iterator<Item = &'a ReviewNote> {
    db.get(repo)
        .and_then(|repo_map| repo_map.get(file))
        .into_iter()
        .flatten()
}

/// Check if a commit or range specification matches a candidate hash/range from
/// the database.
///
/// Supports prefix matching for single commits as well as dual-endpoint prefix
/// matching for commit ranges (`base..tip`). Single-commit targets never match
/// range candidates, and range targets never match single-commit candidates.
#[inline]
pub(crate) fn commit_spec_matches(target_spec: &str, candidate_hash: &str) -> bool {
    if let Some((target_base, target_tip)) = target_spec.split_once("..") {
        if let Some((c_base, c_tip)) = candidate_hash.split_once("..") {
            (target_base.starts_with(c_base) || c_base.starts_with(target_base))
                && (target_tip.starts_with(c_tip) || c_tip.starts_with(target_tip))
        } else {
            false
        }
    } else if candidate_hash.contains("..") {
        false
    } else {
        target_spec.starts_with(candidate_hash) || candidate_hash.starts_with(target_spec)
    }
}

/// Query notes for a specific file path.
///
/// Priority resolution order:
/// - Exact match on bucket `repo` (e.g. `repo@commit` or bare `repo`).
/// - If `repo` is commit-scoped (`repo@commit`), matches buckets with
///   compatible commit prefix.
/// - If `repo` is bare (`repo`), matches `{repo}@` buckets.
/// - If `repo` is `None` (standalone review): searches all buckets in `db` for
///   notes matching `file`.
pub fn query_notes_for_file<'a>(
    db: &'a NotesDB,
    repo: Option<&str>,
    file: &str,
) -> Vec<&'a ReviewNote> {
    if let Some(r) = repo {
        if let Some(repo_map) = db.get(r)
            && let Some(notes) = repo_map.get(file)
            && !notes.is_empty()
        {
            return notes.iter().collect();
        }
        if let Some((repo_name, commit)) = r.split_once('@') {
            let prefix = format!("{repo_name}@");
            return db
                .iter()
                .filter(|(k, _)| {
                    if let Some(hash) = k.strip_prefix(&prefix) {
                        commit_spec_matches(commit, hash)
                    } else {
                        false
                    }
                })
                .filter_map(|(_, repo_map)| repo_map.get(file))
                .flatten()
                .collect();
        } else {
            let prefix = format!("{r}@");
            return db
                .iter()
                .filter(|(k, _)| k.as_str() == r || k.starts_with(&prefix))
                .filter_map(|(_, repo_map)| repo_map.get(file))
                .flatten()
                .collect();
        }
    }
    db.values()
        .filter_map(|repo_map| repo_map.get(file))
        .flatten()
        .collect()
}

/// Query all distinct file paths that have notes for the given repo bucket (or
/// across all buckets if `repo` is None).
pub fn note_files_for_repo(db: &NotesDB, repo: Option<&str>) -> Vec<String> {
    let mut files = std::collections::HashSet::new();
    if let Some(r) = repo {
        if let Some(repo_map) = db.get(r) {
            for (f, notes) in repo_map {
                if !notes.is_empty() {
                    files.insert(f.clone());
                }
            }
        }
        if let Some((repo_name, commit)) = r.split_once('@') {
            let prefix = format!("{repo_name}@");
            for (k, repo_map) in db {
                if let Some(hash) = k.strip_prefix(&prefix)
                    && commit_spec_matches(commit, hash)
                {
                    for (f, notes) in repo_map {
                        if !notes.is_empty() {
                            files.insert(f.clone());
                        }
                    }
                }
            }
        } else {
            let prefix = format!("{r}@");
            for (k, repo_map) in db {
                if k.starts_with(&prefix) {
                    for (f, notes) in repo_map {
                        if !notes.is_empty() {
                            files.insert(f.clone());
                        }
                    }
                }
            }
        }
    } else {
        for repo_map in db.values() {
            for (f, notes) in repo_map {
                if !notes.is_empty() {
                    files.insert(f.clone());
                }
            }
        }
    }
    let mut sorted: Vec<String> = files.into_iter().collect();
    sorted.sort();
    sorted
}

/// Get all notes for a specific repo.
pub fn all_notes<'a>(db: &'a NotesDB, repo: &str) -> impl Iterator<Item = &'a ReviewNote> {
    db.get(repo)
        .into_iter()
        .flat_map(|repo_map| repo_map.values().flatten())
}

/// Return a copy of the database restricted to non-resolved notes.
///
/// Callers that load content only for notes the user can currently see (for
/// example, synthesizing context around an anchoring note) use this to derive
/// a content-scoped view; the authoritative source database is never modified.
/// Empty per-file and per-repo maps are pruned after removal.
#[must_use]
pub fn without_resolved(db: &NotesDB) -> NotesDB {
    let mut out = db.clone();
    for repo_map in out.values_mut() {
        for notes in repo_map.values_mut() {
            notes.retain(|n| !n.status.is_resolved());
        }
        repo_map.retain(|_, notes| !notes.is_empty());
    }
    out
}

/// Add a note to the database for a given repo.
pub fn add_note(db: &mut NotesDB, repo: &str, note: ReviewNote) {
    db.entry(repo.to_string())
        .or_default()
        .entry(note.file.clone())
        .or_default()
        .push(note);
}

/// Remove line notes overlapping the line range `[start, end]` on either
/// diff side. Returns the number of removed notes.
pub fn remove_notes_in_range(
    db: &mut NotesDB,
    repo: &str,
    file: &str,
    start: u32,
    end: u32,
) -> usize {
    let Some(repo_map) = db.get_mut(repo) else {
        return 0;
    };
    let Some(list) = repo_map.get_mut(file) else {
        return 0;
    };
    let before = list.len();
    list.retain(|n| {
        let note_range = match &n.location {
            NoteLocation::File => return true,
            NoteLocation::Line { coord } => {
                let num = coord.new_line_num.or(coord.old_line_num);
                match num {
                    Some(n) => (n.get(), n.get()),
                    None => return true,
                }
            }
            NoteLocation::Range { start: s, end: e } => {
                let s_num = s.new_line_num.or(s.old_line_num);
                let e_num = e.new_line_num.or(e.old_line_num);
                match (s_num, e_num) {
                    (Some(sn), Some(en)) => (sn.get(), en.get()),
                    _ => return true,
                }
            }
        };
        let (n_start, n_end) = note_range;
        n_end < start || n_start > end
    });
    let removed = before - list.len();
    if list.is_empty() {
        repo_map.remove(file);
    }
    removed
}

/// Remove all file-level notes for a file (notes whose location is
/// [`NoteLocation::File`]). Returns the number of removed notes.
pub fn remove_file_notes(db: &mut NotesDB, repo: &str, file: &str) -> usize {
    let Some(repo_map) = db.get_mut(repo) else {
        return 0;
    };
    let Some(list) = repo_map.get_mut(file) else {
        return 0;
    };
    let before = list.len();
    list.retain(|n| !n.location.is_file());
    let removed = before - list.len();
    if list.is_empty() {
        repo_map.remove(file);
    }
    removed
}

/// Remove a specific note matching the repo, file, and unique `note_id`.
/// Returns `true` if a note was removed, `false` otherwise.
pub fn remove_note_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str) -> bool {
    let Some(repo_map) = db.get_mut(repo) else {
        return false;
    };
    let Some(list) = repo_map.get_mut(file) else {
        return false;
    };
    let before = list.len();
    list.retain(|n| n.id != note_id);
    let removed = list.len() < before;
    if list.is_empty() {
        repo_map.remove(file);
    }
    removed
}

/// Update the text of a specific note matching the repo, file, and unique
/// `note_id`. Returns `true` if the note was found and updated, `false`
/// otherwise.
pub fn update_note_text(
    db: &mut NotesDB,
    repo: &str,
    file: &str,
    note_id: &str,
    new_text: String,
) -> bool {
    let Some(repo_map) = db.get_mut(repo) else {
        return false;
    };
    let Some(list) = repo_map.get_mut(file) else {
        return false;
    };
    if let Some(note) = list.iter_mut().find(|n| n.id == note_id) {
        note.text = new_text;
        true
    } else {
        false
    }
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

/// Set the lifecycle status of a specific note matching repo, file, and
/// `note_id`.
pub fn set_note_status_by_id(
    db: &mut NotesDB,
    repo: &str,
    file: &str,
    note_id: &str,
    status: NoteStatus,
) -> bool {
    let Some(repo_map) = db.get_mut(repo) else {
        return false;
    };
    let Some(list) = repo_map.get_mut(file) else {
        return false;
    };
    if let Some(note) = list.iter_mut().find(|n| n.id == note_id) {
        note.status = status;
        true
    } else {
        false
    }
}

/// Mark a specific note as resolved (`NoteStatus::Resolved`).
#[inline]
pub fn resolve_note_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str) -> bool {
    set_note_status_by_id(db, repo, file, note_id, NoteStatus::Resolved)
}

/// Reopen a specific note back to open status (`NoteStatus::Open`).
#[inline]
pub fn reopen_note_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str) -> bool {
    set_note_status_by_id(db, repo, file, note_id, NoteStatus::Open)
}

/// Alias for [`reopen_note_by_id`]: reset note back to open.
#[inline]
pub fn open_note_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str) -> bool {
    reopen_note_by_id(db, repo, file, note_id)
}

/// Reply to a specific note by prepending reply text and blockquoting original
/// text. Optionally updates the note's status.
pub fn reply_to_note_by_id(
    db: &mut NotesDB,
    repo: &str,
    file: &str,
    note_id: &str,
    reply: &str,
    status: Option<NoteStatus>,
) -> bool {
    let Some(repo_map) = db.get_mut(repo) else {
        return false;
    };
    let Some(list) = repo_map.get_mut(file) else {
        return false;
    };
    if let Some(note) = list.iter_mut().find(|n| n.id == note_id) {
        note.text = reply_to_note_text(&note.text, reply);
        if let Some(s) = status {
            note.status = s;
        }
        true
    } else {
        false
    }
}

/// Find a note and its file path by note ID within `repo`.
pub fn find_note_by_id<'a>(
    db: &'a NotesDB,
    repo: &str,
    note_id: &str,
) -> Option<(&'a str, &'a ReviewNote)> {
    let repo_map = db.get(repo)?;
    for (file, notes) in repo_map {
        if let Some(note) = notes.iter().find(|n| n.id == note_id) {
            return Some((file.as_str(), note));
        }
    }
    None
}

/// Set a note's lifecycle status searching across all files in `repo`.
pub fn set_note_status_in_repo(
    db: &mut NotesDB,
    repo: &str,
    note_id: &str,
    status: NoteStatus,
) -> bool {
    let Some(repo_map) = db.get_mut(repo) else {
        return false;
    };
    for notes in repo_map.values_mut() {
        if let Some(note) = notes.iter_mut().find(|n| n.id == note_id) {
            note.status = status;
            return true;
        }
    }
    false
}

/// Find a note by ID (or ID prefix) across buckets in `db`.
/// If `repo` is provided, searches that bucket first, then commit-scoped
/// buckets matching `{repo}@`, and finally falls back to all buckets in the
/// database. Matches exact note ID first; if none match, matches note ID
/// prefix.
pub fn find_note_in_db<'a>(
    db: &'a NotesDB,
    repo: Option<&str>,
    id_or_prefix: &str,
) -> Option<(&'a str, &'a str, &'a ReviewNote)> {
    let mut candidate_keys: Vec<&'a str> = Vec::new();
    if let Some(r) = repo {
        if let Some((k, _)) = db.get_key_value(r) {
            candidate_keys.push(k.as_str());
        }
        let pfx = format!("{r}@");
        for k in db.keys() {
            if k.starts_with(&pfx) && !candidate_keys.contains(&k.as_str()) {
                candidate_keys.push(k.as_str());
            }
        }
    }
    for k in db.keys() {
        if !candidate_keys.contains(&k.as_str()) {
            candidate_keys.push(k.as_str());
        }
    }

    // 1. Exact ID match pass
    for key in &candidate_keys {
        if let Some(repo_map) = db.get(*key) {
            for (file, notes) in repo_map {
                if let Some(note) = notes.iter().find(|n| n.id == id_or_prefix) {
                    return Some((*key, file.as_str(), note));
                }
            }
        }
    }

    // 2. Prefix match pass
    for key in &candidate_keys {
        if let Some(repo_map) = db.get(*key) {
            for (file, notes) in repo_map {
                if let Some(note) = notes.iter().find(|n| n.id.starts_with(id_or_prefix)) {
                    return Some((*key, file.as_str(), note));
                }
            }
        }
    }

    None
}

/// Mutate a note found by ID (or prefix) across buckets in `db`.
pub fn update_note_in_db<F>(
    db: &mut NotesDB,
    repo: Option<&str>,
    id_or_prefix: &str,
    mut f: F,
) -> bool
where
    F: FnMut(&mut ReviewNote),
{
    // First find where the note is located
    let found = find_note_in_db(db, repo, id_or_prefix)
        .map(|(k, file, note)| (k.to_string(), file.to_string(), note.id.clone()));

    if let Some((bucket, file, note_id)) = found
        && let Some(repo_map) = db.get_mut(&bucket)
        && let Some(notes) = repo_map.get_mut(&file)
        && let Some(note) = notes.iter_mut().find(|n| n.id == note_id)
    {
        f(note);
        return true;
    }
    false
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

#[cfg(test)]
mod tests;
