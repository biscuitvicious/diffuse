//! Filesystem path resolution, unique ID generation, and git revision scoping.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

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

pub(crate) fn state_dir_from(xdg_state_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
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
