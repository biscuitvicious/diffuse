//! Note storage, persistence, and markdown rendering for review notes.

use crate::bin_pipeline::OutputRun;
use crate::merge::RunKind;
use crate::syntax::{
    ClassifiedToken, MD_SGR_BOLD, MD_SGR_ITALIC, MD_SGR_STRIKE, MD_SGR_UNDERLINE, classify_tokens,
    lang_profile, md_inner_tokens,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::OnceLock;

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

/// The notes database: repo -> (file_path -> list of notes).
pub type NotesDB = HashMap<String, HashMap<String, Vec<ReviewNote>>>;

/// Default notes path (`~/.pi/review/notes.json`).
pub fn default_notes_path() -> std::path::PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        std::path::PathBuf::from(home)
            .join(".pi")
            .join("review")
            .join("notes.json")
    } else {
        std::path::PathBuf::from(".pi/review/notes.json")
    }
}

/// Load notes from disk. Returns an empty DB if the file does not exist or fails to parse.
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
pub fn notes_for_file<'a>(db: &'a NotesDB, repo: &str, file: &str) -> Vec<&'a ReviewNote> {
    db.get(repo)
        .and_then(|repo_map| repo_map.get(file))
        .map(|list| list.iter().collect())
        .unwrap_or_default()
}

/// Get all notes for a specific repo.
pub fn all_notes<'a>(db: &'a NotesDB, repo: &str) -> Vec<&'a ReviewNote> {
    db.get(repo)
        .map(|repo_map| repo_map.values().flatten().collect())
        .unwrap_or_default()
}

/// Add a note to the database for a given repo.
pub fn add_note(db: &mut NotesDB, repo: &str, note: ReviewNote) {
    db.entry(repo.to_string())
        .or_default()
        .entry(note.file.clone())
        .or_default()
        .push(note);
}

/// Remove notes matching the repo, file, side, and overlapping line range [start, end].
/// Returns the number of removed notes.
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
pub fn render_note_markdown(text: &str) -> Vec<ClassifiedToken> {
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
            color: "muted".into(),
            sgr: None,
            text: fence[0].to_string(),
        });
        out.push(ClassifiedToken {
            color: "text".into(),
            sgr: None,
            text: "\n".into(),
        });
        if let Some(p) = profile {
            out.extend(classify_tokens(code, Some(p)));
        } else {
            out.push(ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: code.into(),
            });
        }
        if end_idx.is_some() {
            out.push(ClassifiedToken {
                color: "text".into(),
                sgr: None,
                text: "\n".into(),
            });
            out.push(ClassifiedToken {
                color: "muted".into(),
                sgr: None,
                text: "```".into(),
            });
        }
        return out;
    }

    static MD_INLINE: OnceLock<Regex> = OnceLock::new();
    let re = MD_INLINE.get_or_init(|| {
        Regex::new(
            r"(`[^`\n]+`|~~[^~\n]+~~|\*\*[^*\n]+\*\*|\*[^*\s\n][^*\n]*\*|\[[^\]\n]+\]\([^)\n]+\))",
        )
        .expect("the md inline")
    });

    let mut out = Vec::new();
    let mut last = 0usize;
    for m in re.find_iter(text) {
        if m.start() > last {
            out.push(ClassifiedToken {
                color: "warning".into(),
                sgr: None,
                text: text[last..m.start()].into(),
            });
        }
        let tok = m.as_str();
        if tok.starts_with('`') {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "`".into(),
            });
            out.push(ClassifiedToken {
                color: "syntaxString".into(),
                sgr: None,
                text: tok[1..tok.len() - 1].into(),
            });
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "`".into(),
            });
        } else if tok.starts_with("~~") {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "~~".into(),
            });
            out.extend(md_inner_tokens(
                &tok[2..tok.len() - 2],
                MD_SGR_STRIKE,
                "mdStrike",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "~~".into(),
            });
        } else if tok.starts_with("**") {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "**".into(),
            });
            out.extend(md_inner_tokens(
                &tok[2..tok.len() - 2],
                MD_SGR_BOLD,
                "mdBold",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "**".into(),
            });
        } else if tok.starts_with('[') {
            let close = tok.find("](").unwrap_or(0);
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "[".into(),
            });
            out.push(ClassifiedToken {
                color: "mdLink".into(),
                sgr: Some(MD_SGR_UNDERLINE),
                text: tok[1..close].into(),
            });
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "](".into(),
            });
            out.push(ClassifiedToken {
                color: "mdUrl".into(),
                sgr: Some(MD_SGR_UNDERLINE),
                text: tok[close + 2..tok.len() - 1].into(),
            });
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: ")".into(),
            });
        } else {
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "*".into(),
            });
            out.extend(md_inner_tokens(
                &tok[1..tok.len() - 1],
                MD_SGR_ITALIC,
                "mdItalic",
            ));
            out.push(ClassifiedToken {
                color: "accent".into(),
                sgr: None,
                text: "*".into(),
            });
        }
        last = m.end();
    }
    if last < text.len() {
        out.push(ClassifiedToken {
            color: "warning".into(),
            sgr: None,
            text: text[last..].into(),
        });
    }
    out
}

/// Convert note classified tokens to OutputRun wire format.
pub fn note_runs(text: &str) -> Vec<OutputRun> {
    render_note_markdown(text)
        .into_iter()
        .map(|t| OutputRun {
            kind: RunKind::Ctx,
            color: Some(t.color),
            sgr: t.sgr,
            text: t.text,
        })
        .collect()
}

#[cfg(test)]
mod tests;
