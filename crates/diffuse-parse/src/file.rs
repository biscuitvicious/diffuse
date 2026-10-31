//! Unified diff file models, status enumerations, and path header parsers.

use serde::{Deserialize, Serialize};

use crate::hunk::Hunk;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Deleted,
    #[default]
    Modified,
    Renamed,
    Untracked,
    #[serde(rename = "mode changed")]
    ModeChanged,
    Unmodified,
}

impl FileStatus {
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            FileStatus::Added => "added",
            FileStatus::Deleted => "deleted",
            FileStatus::Modified => "modified",
            FileStatus::Renamed => "renamed",
            FileStatus::Untracked => "untracked",
            FileStatus::ModeChanged => "mode changed",
            FileStatus::Unmodified => "unmodified",
        }
    }

    pub fn reverse(&self) -> FileStatus {
        match self {
            FileStatus::Added => FileStatus::Deleted,
            FileStatus::Deleted => FileStatus::Added,
            FileStatus::Modified => FileStatus::Modified,
            FileStatus::Renamed => FileStatus::Renamed,
            FileStatus::Untracked => FileStatus::Untracked,
            FileStatus::ModeChanged => FileStatus::ModeChanged,
            FileStatus::Unmodified => FileStatus::Unmodified,
        }
    }
}

impl std::fmt::Display for FileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for FileStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "added" => Ok(FileStatus::Added),
            "deleted" => Ok(FileStatus::Deleted),
            "modified" => Ok(FileStatus::Modified),
            "renamed" => Ok(FileStatus::Renamed),
            "untracked" => Ok(FileStatus::Untracked),
            "mode changed" | "modechanged" => Ok(FileStatus::ModeChanged),
            "unmodified" => Ok(FileStatus::Unmodified),
            _ => Err(format!("unknown file status: {s}")),
        }
    }
}

impl PartialEq<&str> for FileStatus {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<FileStatus> for &str {
    #[inline]
    fn eq(&self, other: &FileStatus) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<str> for FileStatus {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<FileStatus> for str {
    #[inline]
    fn eq(&self, other: &FileStatus) -> bool {
        self == other.as_str()
    }
}

impl From<&str> for FileStatus {
    #[inline]
    fn from(s: &str) -> Self {
        s.parse().unwrap_or(FileStatus::Modified)
    }
}

/// A parsed diff file.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct DiffFile {
    /// Repo-relative path of the NEW side (old path for deletions).
    pub path: String,
    pub old_path: Option<String>,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    pub status: FileStatus,
    pub is_binary: bool,
    /// The section carried `---`/`+++` unified-diff file headers: a
    /// complete content section. Zero-hunk files are real files only when
    /// this is set (whitespace-suppressed changes, pure renames aside).
    pub has_file_headers: bool,
    pub old_bytes: Option<usize>,
    pub new_bytes: Option<usize>,
    pub new_line_count: usize,
    pub old_line_count: usize,
    /// Content of the new side (worktree), for context synthesis.
    pub new_content: Vec<String>,
    /// Content of the old side (HEAD/ref), for deleted-file context.
    pub old_content: Vec<String>,
    pub hunks: Vec<Hunk>,
    pub raw_hunks: Vec<Hunk>,
    /// True when neither side's content could be loaded (no context source).
    pub no_content: bool,
}

impl DiffFile {
    pub fn new(path: impl Into<String>) -> Self {
        DiffFile {
            path: path.into(),
            old_path: None,
            old_mode: None,
            new_mode: None,
            status: FileStatus::Modified,
            is_binary: false,
            has_file_headers: false,
            old_bytes: None,
            new_bytes: None,
            new_line_count: 0,
            old_line_count: 0,
            new_content: Vec::new(),
            old_content: Vec::new(),
            hunks: Vec::new(),
            raw_hunks: Vec::new(),
            no_content: false,
        }
    }

    pub fn reverse(&self) -> Self {
        DiffFile {
            path: self.old_path.clone().unwrap_or_else(|| self.path.clone()),
            old_path: if self.old_path.is_some() {
                Some(self.path.clone())
            } else {
                None
            },
            old_mode: self.new_mode.clone(),
            new_mode: self.old_mode.clone(),
            status: self.status.reverse(),
            is_binary: self.is_binary,
            has_file_headers: self.has_file_headers,
            old_bytes: self.new_bytes,
            new_bytes: self.old_bytes,
            new_line_count: self.old_line_count,
            old_line_count: self.new_line_count,
            new_content: self.old_content.clone(),
            old_content: self.new_content.clone(),
            hunks: self.hunks.iter().map(Hunk::reverse).collect(),
            raw_hunks: self.raw_hunks.iter().map(Hunk::reverse).collect(),
            no_content: self.no_content,
        }
    }
}

pub fn unquote_git_path(raw: &str) -> String {
    let mut s = raw.trim_end_matches(['\r', '\n']);
    if s.starts_with('"') {
        let end = if s.ends_with('"') {
            s.len() - 1
        } else {
            s.len()
        };
        s = &s[1..end];
        return s
            .replace("\\t", "\t")
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\");
    }
    s.to_string()
}

/// The `+++`/`---` path (minus the a/ b/ prefix, unquoted).
pub fn hunk_path(sign: &str, line: &str) -> Option<String> {
    let p_full = &line[4..];
    let p = if let Some(rest) = p_full.strip_prefix('"') {
        if let Some(end_quote) = rest.find('"') {
            &p_full[..=end_quote + 1]
        } else {
            p_full.trim_end_matches(['\r', '\n'])
        }
    } else {
        p_full
            .split('\t')
            .next()
            .unwrap_or(p_full)
            .trim_end_matches(['\r', '\n'])
    };

    if p == "/dev/null" {
        return None;
    }
    let stripped = if p.starts_with('"') {
        if let Some(rest) = p.strip_prefix("\"b/") {
            format!("\"{rest}")
        } else if let Some(rest) = p.strip_prefix("\"a/") {
            format!("\"{rest}")
        } else {
            p.to_string()
        }
    } else {
        p.strip_prefix("b/")
            .or_else(|| p.strip_prefix("a/"))
            .unwrap_or(p)
            .to_string()
    };
    let _ = sign;
    Some(unquote_git_path(&stripped))
}

pub fn parse_diff_git_paths(rest: &str) -> (Option<String>, Option<String>) {
    let mut rest = rest.trim();
    if rest.is_empty() {
        return (None, None);
    }
    let mut parts = Vec::new();
    while !rest.is_empty() {
        if rest.starts_with('"') {
            if let Some(end_quote) = rest[1..].find('"') {
                let quoted = &rest[..=end_quote + 1];
                parts.push(quoted);
                rest = rest[end_quote + 2..].trim_start();
            } else {
                parts.push(rest);
                break;
            }
        } else {
            let next_space = rest.find(' ').unwrap_or(rest.len());
            parts.push(&rest[..next_space]);
            rest = rest[next_space..].trim_start();
        }
    }

    if parts.len() >= 2 {
        let clean_a = parts[0]
            .strip_prefix("a/")
            .or_else(|| parts[0].strip_prefix("\"a/"));
        let clean_b = parts[parts.len() - 1]
            .strip_prefix("b/")
            .or_else(|| parts[parts.len() - 1].strip_prefix("\"b/"));
        (clean_a.map(unquote_git_path), clean_b.map(unquote_git_path))
    } else {
        (None, None)
    }
}

#[inline]
pub fn is_valid_diff_file(f: &DiffFile) -> bool {
    !f.path.is_empty()
        && (!f.hunks.is_empty()
            || f.has_file_headers
            || f.status == FileStatus::Renamed
            || f.status == FileStatus::Untracked
            || f.status == FileStatus::ModeChanged
            || f.status == FileStatus::Unmodified
            || f.is_binary)
}
