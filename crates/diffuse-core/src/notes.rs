//! Note storage, persistence, and line anchoring for review notes.

use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    ffi::OsString,
    fmt, fs, io,
    num::{NonZero, ParseIntError},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use crate::pipeline::note_runs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    New,
    Old,
}

impl Side {
    pub fn reverse(&self) -> Side {
        match self {
            Side::New => Side::Old,
            Side::Old => Side::New,
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
    pub side: Side,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_hash: Option<LineHash>,
    /// Note lifecycle status: Open (default), InProgress, or Resolved.
    #[serde(default, skip_serializing_if = "NoteStatus::is_open")]
    pub status: NoteStatus,
    /// Exact original source lines selected when the note was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_lines: Option<String>,
}

impl Default for ReviewNote {
    fn default() -> Self {
        Self {
            id: String::new(),
            file: String::new(),
            side: Side::New,
            start: 1,
            end: 1,
            text: String::new(),
            created_at: 0,
            line_hash: None,
            status: NoteStatus::Open,
            source_lines: None,
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

/// Extract a 1-indexed line range `[start, end]` from text content.
/// Returns `None` if `start == 0`, `start > end`, or the range is out of
/// bounds.
pub fn extract_source_lines(content: &str, start: u32, end: u32) -> Option<String> {
    if start == 0 || end < start {
        return None;
    }
    let lines: Vec<&str> = content.lines().collect();
    let s = (start as usize).saturating_sub(1);
    let e = (end as usize).min(lines.len());
    if s >= lines.len() || s >= e {
        return None;
    }
    Some(lines[s..e].join("\n"))
}

/// Synthesize source lines for a given file and coordinate range.
///
/// For [`Side::New`], attempts to read the file directly on disk (or relative
/// to `repo_root`). For [`Side::Old`], queries git for `HEAD:<file_path>` if
/// inside a repository, falling back to disk. Returns `None` if the file cannot
/// be accessed or the range is invalid.
pub fn synthesize_source_lines(
    repo_root: Option<&Path>,
    file_path: &str,
    side: Side,
    start: u32,
    end: u32,
) -> Option<String> {
    match side {
        Side::New => {
            let path = Path::new(file_path);
            let content = if path.is_file() {
                fs::read_to_string(path).ok()
            } else if let Some(root) = repo_root {
                let candidate = root.join(file_path);
                if candidate.is_file() {
                    fs::read_to_string(candidate).ok()
                } else {
                    None
                }
            } else {
                None
            };
            content.and_then(|c| extract_source_lines(&c, start, end))
        }
        Side::Old => {
            if let Some(root) = repo_root
                && let Some(git_out) =
                    crate::git::run_git(root, &["show", &format!("HEAD:{file_path}")])
            {
                extract_source_lines(&git_out, start, end)
            } else {
                synthesize_source_lines(repo_root, file_path, Side::New, start, end)
            }
        }
    }
}

/// Resolve the HEAD commit hash of the repository containing `repo_root`.
/// Returns `None` outside a work tree or when the repo has no commits yet.
pub fn head_commit(repo_root: &Path) -> Option<String> {
    crate::git::run_git(repo_root, &["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve the commit hash to scope review notes to.
///
/// When reviewing a commit range (e.g. `main..feature`, `A...B`, `HEAD^!`),
/// notes are keyed off the second-to-last (next newest) commit in the range.
/// When reviewing uncommitted changes against the working tree (or when the
/// range has no parent), notes fall back to the live HEAD commit.
pub fn notes_commit(repo_root: &Path, git_args: &[String]) -> Option<String> {
    let ref_pair = crate::git::resolve_refs(repo_root, git_args, false);
    if !ref_pair.worktree_new
        && let (Some(old), Some(new)) = (ref_pair.old_ref, ref_pair.new_ref)
    {
        // Try resolving the second newest commit in the range via git rev-list:
        let range = format!("{old}..{new}");
        if let Some(revs) = crate::git::run_git(repo_root, &["rev-list", "-n", "2", &range]) {
            let lines: Vec<&str> = revs.lines().filter(|l| !l.trim().is_empty()).collect();
            if lines.len() >= 2 {
                return Some(lines[1].trim().to_string());
            }
        }
        // Single commit in range: the commit immediately preceding `new` in the
        // range is `old`.
        if let Some(h) =
            crate::git::run_git(repo_root, &["rev-parse", &format!("{old}^{{commit}}")])
        {
            let h = h.trim().to_string();
            if !h.is_empty() {
                return Some(h);
            }
        }
        if let Some(h) =
            crate::git::run_git(repo_root, &["rev-parse", &format!("{new}~1^{{commit}}")])
        {
            let h = h.trim().to_string();
            if !h.is_empty() {
                return Some(h);
            }
        }
        if let Some(h) =
            crate::git::run_git(repo_root, &["rev-parse", &format!("{new}^{{commit}}")])
        {
            let h = h.trim().to_string();
            if !h.is_empty() {
                return Some(h);
            }
        }
    }

    head_commit(repo_root)
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
/// `git_args` (second-to-last commit when reviewing a range, otherwise HEAD)
/// and build the notes-DB key for `repo`.
pub fn notes_key_for_repo_args(repo_root: &Path, repo: &str, git_args: &[String]) -> String {
    notes_key(repo, notes_commit(repo_root, git_args).as_deref())
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

impl LineHash {
    /// Compute a deterministic line hash from trimmed line content.
    ///
    /// Uses FNV-1a 32-bit over `content.trim().as_bytes()`. If the hash
    /// evaluates to 0, wraps to 1 to maintain the non-zero niche invariant.
    pub fn from_content(content: &str) -> Self {
        let mut hash: u32 = 0x811c9dc5;
        for byte in content.trim().as_bytes() {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x01000193);
        }
        let val = if hash == 0 { 1 } else { hash };
        // SAFETY: val is guaranteed non-zero.
        Self(unsafe { NonZero::new_unchecked(val) })
    }

    /// Parse a line hash from a hexadecimal string (up to 8 hex chars).
    pub fn from_hex(hex: &str) -> Result<Self, ParseIntError> {
        let val = u32::from_str_radix(hex.trim(), 16)?;
        let val = if val == 0 { 1 } else { val };
        // SAFETY: val is guaranteed non-zero.
        Ok(Self(unsafe { NonZero::new_unchecked(val) }))
    }

    /// The raw non-zero integer representation.
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    /// Check if this hash matches the trimmed content of a line.
    pub fn matches_content(self, content: &str) -> bool {
        Self::from_content(content) == self
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
                formatter.write_str("a 1-8 character hexadecimal string or u32 integer")
            }

            fn visit_str<E>(self, v: &str) -> Result<LineHash, E>
            where
                E: serde::de::Error,
            {
                LineHash::from_hex(v).map_err(serde::de::Error::custom)
            }

            fn visit_u64<E>(self, v: u64) -> Result<LineHash, E>
            where
                E: serde::de::Error,
            {
                let val = if (v as u32) == 0 { 1 } else { v as u32 };
                // SAFETY: val is guaranteed non-zero.
                Ok(LineHash(unsafe { NonZero::new_unchecked(val) }))
            }
        }

        deserializer.deserialize_any(LineHashVisitor)
    }
}

/// Convenience alias: compute the deterministic line hash from line content.
pub fn hash_line_content(content: &str) -> LineHash {
    LineHash::from_content(content)
}

/// Check whether a note anchors cleanly against the content of its target line.
///
/// If the note has a recorded `line_hash`, returns `true` only if the hash
/// of `line_content` matches. Notes without a hash (legacy) always anchor
/// by line number.
pub fn note_anchors_cleanly(note: &ReviewNote, line_content: &str) -> bool {
    match note.line_hash {
        Some(expected) => expected.matches_content(line_content),
        None => true,
    }
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

/// Query notes for a file from `db`.
///
/// If `repo` is provided:
/// - First checks an exact match on `repo`.
/// - If not found or empty, checks commit-scoped keys matching `{repo}@`.
///
/// If still not found or `repo` is `None`:
/// - Searches all buckets in `db` for notes matching `file`.
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
        let prefix = format!("{r}@");
        let matching: Vec<&'a ReviewNote> = db
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .filter_map(|(_, repo_map)| repo_map.get(file))
            .flatten()
            .collect();
        if !matching.is_empty() {
            return matching;
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
        if files.is_empty() {
            let base_r = r.split('@').next().unwrap_or(r);
            let base_prefix = format!("{base_r}@");
            for (k, repo_map) in db {
                if k == base_r || k.starts_with(&base_prefix) {
                    for (f, notes) in repo_map {
                        if !notes.is_empty() {
                            files.insert(f.clone());
                        }
                    }
                }
            }
        }
    }
    if files.is_empty() {
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

/// Add a note to the database for a given repo.
pub fn add_note(db: &mut NotesDB, repo: &str, note: ReviewNote) {
    db.entry(repo.to_string())
        .or_default()
        .entry(note.file.clone())
        .or_default()
        .push(note);
}

/// Remove notes matching the repo, file, side, and overlapping line range
/// [start, end]. Returns the number of removed notes.
pub fn remove_notes(
    db: &mut NotesDB,
    repo: &str,
    file: &str,
    side: Side,
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
    list.retain(|n| !(n.side == side && n.start <= end && n.end >= start));
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
