//! Note storage, persistence, and line anchoring for review notes.

use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    ffi::OsString,
    fmt, fs, io,
    num::{NonZero, ParseIntError},
    path::{Path, PathBuf},
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
/// state never lives under a consumer's directory (e.g. `~/.pi/review/`). A
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
