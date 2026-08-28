//! Note storage, persistence, and markdown rendering for review notes.

use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{
    merge::RunKind,
    syntax::{
        classifier::{ClassifiedToken, classify_tokens, for_each_markdown_token},
        registry::lang_profile,
    },
    wire::OutputRun,
};

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
}

/// The notes database: repo -> (`file_path` -> list of notes).
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
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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

/// Render/tokenize review note markdown into `ClassifiedToken`s.
/// Note text uses `"warning"` for plain text, code fences with syntax colors,
/// and inline markdown (bold/italic/strike/link/code).
pub fn render_note_markdown<'a>(text: &'a str) -> Vec<ClassifiedToken<'a>> {
    static FENCE: OnceLock<Regex> = OnceLock::new();
    let fence_re =
        FENCE.get_or_init(|| Regex::new(r"^\s*```([A-Za-z0-9_+-]*)\s*$").expect("note fence"));
    if let Some(fence) = fence_re.captures(text) {
        let lang = fence[1].to_lowercase();
        let rest = &text[fence.get(0).unwrap().end()..];
        let body = rest.strip_prefix('\n').unwrap_or(rest);
        let end_idx = body.rfind("```");
        let code = match end_idx {
            Some(idx) => &body[..idx],
            None => body,
        };
        let profile = lang_profile(&lang);
        let mut out = Vec::new();
        out.push(ClassifiedToken {
            color: "muted",
            sgr: None,
            text: fence.get(0).unwrap().as_str(),
        });
        out.push(ClassifiedToken {
            color: "text",
            sgr: None,
            text: "\n",
        });
        if let Some(p) = profile {
            out.extend(classify_tokens(code, Some(p)));
        } else {
            out.push(ClassifiedToken {
                color: "text",
                sgr: None,
                text: code,
            });
        }
        if end_idx.is_some() {
            out.push(ClassifiedToken {
                color: "text",
                sgr: None,
                text: "\n",
            });
            out.push(ClassifiedToken {
                color: "muted",
                sgr: None,
                text: "```",
            });
        }
        return out;
    }

    let mut out = Vec::new();
    for_each_markdown_token(text, |mut t| {
        if t.color == "text" {
            t.color = "warning";
        }
        out.push(t);
    });
    out
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

/// Convert note classified tokens to `OutputRun` wire format.
pub fn note_runs(text: &str) -> Vec<OutputRun> {
    static FENCE: OnceLock<Regex> = OnceLock::new();
    let fence_re =
        FENCE.get_or_init(|| Regex::new(r"^\s*```([A-Za-z0-9_+-]*)\s*$").expect("note fence"));
    let normalized = if fence_re.is_match(text) {
        Cow::Borrowed(text)
    } else {
        normalize_note_text(text)
    };
    render_note_markdown(&normalized)
        .into_iter()
        .map(|t| OutputRun {
            kind: RunKind::Ctx,
            color: Some(std::borrow::Cow::Borrowed(t.color)),
            sgr: t.sgr,
            text: t.text.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
