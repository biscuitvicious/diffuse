//! Diffuse Wire Contract Data Models & Serialization Schema.
//!
//! Defines the structured JSON wire contract output emitted by the engine
//! and consumed by external tooling, the CLI (`--json`), and the pi review
//! extension. The schema is frozen per PLAN.md §2.

use serde::{Deserialize, Serialize};

use crate::{
    counts::FileCounts,
    merge::RunKind,
    notes::{LineHash, Side as NoteSide},
    parse::{AddLine, CtxLine, DelLine, DiffFile, FileStatus, Hunk, LineNum, Row, RowKind, Spans},
};

pub mod jsonl;

/// Identifies which side of a moved line an `OutputRow` represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MovSide {
    Src,
    Tgt,
}

/// The top-level wire contract payload containing processed file diffs, branch
/// name, and untracked file metadata.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineOutput {
    pub files: Vec<OutputFile>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub branch: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub untracked_skipped: Vec<String>,
}

impl PipelineOutput {
    pub fn reverse(&self) -> Self {
        PipelineOutput {
            files: self.files.iter().map(OutputFile::reverse).collect(),
            branch: self.branch.clone(),
            untracked_skipped: self.untracked_skipped.clone(),
        }
    }

    pub fn reconstruct(&self) -> Vec<DiffFile> {
        self.files.iter().map(OutputFile::reconstruct).collect()
    }

    /// Computes the aggregated line-level modification counts summed across
    /// all files in this output payload, including the total number of review
    /// notes.
    pub fn total_counts(&self) -> FileCounts {
        let mut total = FileCounts::default();
        for f in &self.files {
            total.adds += f.counts.adds;
            total.dels += f.counts.dels;
            total.mods += f.counts.mods;
            total.movs += f.counts.movs;
            total.notes += f.counts.notes;
        }
        total
    }

    /// Computes the longest common directory prefix shared across all files in
    /// the diff (both `path` and `old_path`), if there are 2 or more files.
    /// Returns `Some(prefix)` with a trailing slash (e.g.
    /// `"crates/diffuse-core/src/"`) or `None` if there is no shared
    /// directory prefix or fewer than 2 files.
    pub fn common_directory_prefix(&self) -> Option<String> {
        if self.files.len() < 2 {
            return None;
        }

        let mut all_paths: Vec<&str> = Vec::new();
        for f in &self.files {
            all_paths.push(&f.path);
            if let Some(ref old) = f.old_path {
                all_paths.push(old);
            }
        }

        let split_paths: Vec<Vec<&str>> =
            all_paths.iter().map(|p| p.split('/').collect()).collect();

        // If any path has only 1 component (top-level file), there is no common
        // directory prefix
        if split_paths.iter().any(|p| p.len() <= 1) {
            return None;
        }

        let mut common_dirs = Vec::new();
        let min_dirs_len = split_paths.iter().map(|p| p.len() - 1).min().unwrap_or(0);

        for dir_idx in 0..min_dirs_len {
            let seg = split_paths[0][dir_idx];
            if split_paths.iter().all(|p| p[dir_idx] == seg) {
                common_dirs.push(seg);
            } else {
                break;
            }
        }

        if common_dirs.is_empty() {
            None
        } else {
            let mut prefix = common_dirs.join("/");
            prefix.push('/');
            Some(prefix)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSize(pub usize);

impl std::fmt::Debug for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_byte_size(self.0))
    }
}

impl std::fmt::Display for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format_byte_size(self.0))
    }
}

pub fn format_byte_size(size: usize) -> String {
    let units = ["B", "kB", "MB", "GB", "TB", "PB"];
    let (size, unit) = units
        .into_iter()
        .enumerate()
        .rev()
        .map(|(i, u)| (size as f64 / (1u64 << (i * 10)) as f64, u))
        .find(|&(size, _)| size >= 0.5)
        .unwrap_or((size as f64, "B"));

    let mut formatted = format!("{:.1}", size);

    // Trim trailing zeros after the decimal point
    if formatted.contains('.') {
        formatted = formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
    }

    format!("{}{}", formatted, unit)
}

pub const NON_EXISTENT_SYMBOL: &str = "×";
pub const INCEPTION_SYMBOL: &str = NON_EXISTENT_SYMBOL;
pub const DESTRUCTION_SYMBOL: &str = NON_EXISTENT_SYMBOL;
pub const UNKNOWN_SYMBOL: &str = "?";
pub const ARROW_SYMBOL: &str = "➞";

/// The engine's complete, rendered output for a single changed file.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_mode: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_binary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_bytes: Option<usize>,
    pub new_line_count: usize,
    pub old_line_count: usize,
    #[serde(default, skip_serializing)]
    pub new_content: Vec<String>,
    #[serde(default, skip_serializing)]
    pub old_content: Vec<String>,
    pub hunks: Vec<OutputHunk>,
    #[serde(default, skip_serializing)]
    pub raw_hunks: Vec<OutputHunk>,
    pub counts: FileCounts,
    /// The fence regions (the wire's syntax-region spans — line 1-indexed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fences: Vec<OutputFence>,
    /// The review notes attached to this file (with engine-classified markdown
    /// runs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<OutputNote>,
    /// Pre-classified syntax token runs for all context/content lines of the
    /// file.
    #[serde(default, skip_serializing)]
    pub content_runs: Vec<Vec<OutputRun>>,
    /// Pre-assembled row sequence for default context.
    #[serde(default, skip_serializing)]
    pub rows: Vec<OutputRow>,
    /// Source descriptor for on-demand lazy rendering (internal engine use
    /// only).
    #[serde(default, skip_serializing)]
    pub raw_source: Option<RawFileSource>,
}

/// Source information to allow on-demand rendering of an `OutputFile`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum RawFileSource {
    /// Can be re-loaded directly from git repository on demand.
    Git {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        old_path: Option<String>,
        status: String,
    },
    /// In-memory diff fallback for non-git diffs (piped diffs, synthetic diffs,
    /// unit tests).
    Raw {
        path: String,
        diff: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        new_content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        old_content: Option<String>,
    },
}

impl OutputFile {
    /// Create a lightweight collapsed stub that skips pipeline execution.
    pub fn collapsed_stub(
        f: &DiffFile,
        counts: FileCounts,
        raw_source: Option<RawFileSource>,
    ) -> Self {
        let status = match f.status {
            FileStatus::Added => "added".to_string(),
            FileStatus::Deleted => "deleted".to_string(),
            FileStatus::Modified => "modified".to_string(),
            FileStatus::Renamed => "renamed".to_string(),
            FileStatus::Untracked => "untracked".to_string(),
            FileStatus::ModeChanged => "mode changed".to_string(),
            FileStatus::Note => "note".to_string(),
        };
        Self {
            path: f.path.clone(),
            old_path: f.old_path.clone(),
            old_mode: f.old_mode.clone(),
            new_mode: f.new_mode.clone(),
            status,
            is_binary: f.is_binary,
            old_bytes: f.old_bytes,
            new_bytes: f.new_bytes,
            new_line_count: f.new_line_count,
            old_line_count: f.old_line_count,
            new_content: Vec::new(),
            old_content: Vec::new(),
            hunks: Vec::new(),
            raw_hunks: Vec::new(),
            counts,
            fences: Vec::new(),
            notes: Vec::new(),
            content_runs: Vec::new(),
            rows: Vec::new(),
            raw_source,
        }
    }

    /// Whether this file's full diff rows and hunks are currently loaded in
    /// memory.
    pub fn is_loaded(&self) -> bool {
        !self.hunks.is_empty()
            || !self.rows.is_empty()
            || self.is_binary
            || (self.counts.adds == 0 && self.counts.dels == 0 && self.counts.mods == 0)
    }

    /// Release heavy row and syntax allocations when collapsed, keeping only
    /// metadata.
    pub fn release_rendered_memory(&mut self) {
        self.rows.clear();
        self.rows.shrink_to_fit();
        self.hunks.clear();
        self.hunks.shrink_to_fit();
        self.raw_hunks.clear();
        self.raw_hunks.shrink_to_fit();
        self.new_content.clear();
        self.new_content.shrink_to_fit();
        self.old_content.clear();
        self.old_content.shrink_to_fit();
        self.content_runs.clear();
        self.content_runs.shrink_to_fit();
        self.fences.clear();
        self.fences.shrink_to_fit();
    }

    pub fn reverse(&self) -> Self {
        let status = match self.status.as_str() {
            "added" => "deleted",
            "deleted" => "added",
            "untracked" => "deleted",
            s => s,
        };
        OutputFile {
            path: self.old_path.clone().unwrap_or_else(|| self.path.clone()),
            old_path: if self.old_path.is_some() {
                Some(self.path.clone())
            } else {
                None
            },
            old_mode: self.new_mode.clone(),
            new_mode: self.old_mode.clone(),
            status: status.to_string(),
            is_binary: self.is_binary,
            old_bytes: self.new_bytes,
            new_bytes: self.old_bytes,
            new_line_count: self.old_line_count,
            old_line_count: self.new_line_count,
            new_content: self.old_content.clone(),
            old_content: self.new_content.clone(),
            hunks: self.hunks.iter().map(OutputHunk::reverse).collect(),
            raw_hunks: self.raw_hunks.iter().map(OutputHunk::reverse).collect(),
            counts: self.counts.reverse(),
            fences: self.fences.clone(),
            notes: self.notes.iter().map(OutputNote::reverse).collect(),
            content_runs: self
                .content_runs
                .iter()
                .map(|runs| runs.iter().map(OutputRun::reverse).collect())
                .collect(),
            rows: self.rows.iter().map(OutputRow::reverse).collect(),
            raw_source: self.raw_source.clone(),
        }
    }

    pub fn reconstruct(&self) -> DiffFile {
        let status = match self.status.as_str() {
            "added" => FileStatus::Added,
            "deleted" => FileStatus::Deleted,
            "renamed" => FileStatus::Renamed,
            "untracked" => FileStatus::Untracked,
            "mode changed" => FileStatus::ModeChanged,
            "note" => FileStatus::Note,
            _ => FileStatus::Modified,
        };
        DiffFile {
            path: self.path.clone(),
            old_path: self.old_path.clone(),
            old_mode: self.old_mode.clone(),
            new_mode: self.new_mode.clone(),
            status,
            is_binary: self.is_binary,
            old_bytes: self.old_bytes,
            new_bytes: self.new_bytes,
            new_line_count: self.new_line_count,
            old_line_count: self.old_line_count,
            new_content: self.new_content.clone(),
            old_content: self.old_content.clone(),
            hunks: self.hunks.iter().map(OutputHunk::reconstruct).collect(),
            raw_hunks: self.raw_hunks.iter().map(OutputHunk::reconstruct).collect(),
            no_content: self.new_content.is_empty() && self.old_content.is_empty(),
        }
    }

    #[inline]
    pub fn old_byte_size(&self) -> Option<usize> {
        self.old_bytes
    }

    #[inline]
    pub fn new_byte_size(&self) -> Option<usize> {
        self.new_bytes
    }

    #[inline]
    pub fn byte_size(&self) -> Option<usize> {
        if self.status == "deleted" {
            self.old_bytes
        } else {
            self.new_bytes
        }
    }

    /// Formats the file size, displaying `from_size ➞ to_size` if the
    /// formatted values differ, or a single size if unchanged (or if
    /// added/deleted/untracked).
    pub fn display_size(&self) -> String {
        if self.old_bytes.is_none() && self.new_bytes.is_none() {
            return String::new();
        }
        let new_fmt = self.new_bytes.map(format_byte_size);
        let old_fmt = self.old_bytes.map(format_byte_size);
        if self.status == "added" || self.status == "untracked" {
            let new_str = new_fmt.unwrap_or_else(|| UNKNOWN_SYMBOL.to_string());
            format!("{INCEPTION_SYMBOL} {ARROW_SYMBOL} {new_str}")
        } else if self.status == "deleted" {
            let old_str = old_fmt.unwrap_or_else(|| UNKNOWN_SYMBOL.to_string());
            format!("{old_str} {ARROW_SYMBOL} {DESTRUCTION_SYMBOL}")
        } else {
            match (old_fmt, new_fmt) {
                (Some(o), Some(n)) if o != n => format!("{o} {ARROW_SYMBOL} {n}"),
                (Some(_), Some(n)) => n,
                (Some(o), None) => format!("{o} {ARROW_SYMBOL} {UNKNOWN_SYMBOL}"),
                (None, Some(n)) => format!("{UNKNOWN_SYMBOL} {ARROW_SYMBOL} {n}"),
                (None, None) => String::new(),
            }
        }
    }

    /// Format status description for banners, markdown summary, and pretty
    /// printing, incorporating mode changes and byte size (e.g. "mode
    /// 100644 ➞ 100755, 1.2kB", "1kB ➞ 2kB", "1.2kB", etc.).
    pub fn display_status(&self) -> String {
        let size_str = self.display_size();
        let mode_suffix = match (&self.old_mode, &self.new_mode) {
            (Some(old_m), Some(new_m)) if old_m != new_m => {
                Some(format!("mode {old_m} {ARROW_SYMBOL} {new_m}"))
            }
            _ => None,
        };

        if self.is_binary {
            let mut parts = Vec::new();
            if self.status == "untracked" {
                parts.push("untracked");
            }
            parts.push("binary");
            if let Some(ref mode_str) = mode_suffix {
                parts.push(mode_str.as_str());
            }
            if !size_str.is_empty() {
                parts.push(&size_str);
            }
            return parts.join(", ");
        }

        match (self.status.as_str(), mode_suffix) {
            ("renamed", Some(mode_str)) => {
                if size_str.is_empty() {
                    mode_str
                } else {
                    format!("{mode_str}, {size_str}")
                }
            }
            ("renamed", None) => {
                if size_str.is_empty() {
                    "renamed".to_string()
                } else {
                    size_str
                }
            }
            ("added", Some(mode_str)) => {
                if size_str.is_empty() {
                    format!("added, {mode_str}")
                } else {
                    format!("{mode_str}, {size_str}")
                }
            }
            ("added", None) => {
                if size_str.is_empty() {
                    "added".to_string()
                } else {
                    size_str
                }
            }
            ("deleted", Some(mode_str)) => {
                if size_str.is_empty() {
                    format!("deleted, {mode_str}")
                } else {
                    format!("{mode_str}, {size_str}")
                }
            }
            ("deleted", None) => {
                if size_str.is_empty() {
                    "deleted".to_string()
                } else {
                    size_str
                }
            }
            ("mode changed" | "modified", Some(mode_str)) => {
                let prefix =
                    if self.counts.adds == 0 && self.counts.dels == 0 && self.counts.mods == 0 {
                        mode_str
                    } else {
                        format!("modified, {mode_str}")
                    };
                if size_str.is_empty() {
                    prefix
                } else {
                    format!("{prefix}, {size_str}")
                }
            }
            ("modified", None) => {
                if size_str.is_empty() {
                    "modified".to_string()
                } else {
                    size_str
                }
            }
            (s, Some(mode_str)) => {
                if size_str.is_empty() {
                    format!("{s}, {mode_str}")
                } else {
                    format!("{s}, {mode_str}, {size_str}")
                }
            }
            (s, None) => {
                if size_str.is_empty() {
                    s.to_string()
                } else {
                    format!("{s}, {size_str}")
                }
            }
        }
    }

    /// Format the file's path for display, using rename syntax
    /// `old -> new` when renamed. If `common_prefix` is supplied (e.g.
    /// `"crates/diffuse-core/src/"`), that prefix is stripped and replaced with
    /// `"…/"`.
    pub fn display_path_with_prefix(&self, common_prefix: Option<&str>) -> String {
        let (old, new) = match (self.old_path.as_deref(), common_prefix) {
            (Some(old), Some(prefix)) if old != self.path => {
                let stripped_old = if let Some(stripped) = old.strip_prefix(prefix) {
                    stripped.to_string()
                } else {
                    old.to_string()
                };
                let stripped_new = if let Some(stripped) = self.path.strip_prefix(prefix) {
                    stripped.to_string()
                } else {
                    self.path.clone()
                };
                (Some(stripped_old), stripped_new)
            }
            (Some(old), None) if old != self.path => (Some(old.to_string()), self.path.clone()),
            (None | Some(_), Some(prefix)) => {
                let stripped_new = if let Some(stripped) = self.path.strip_prefix(prefix) {
                    stripped.to_string()
                } else {
                    self.path.clone()
                };
                (None, stripped_new)
            }
            _ => (None, self.path.clone()),
        };

        if let Some(ref old_p) = old {
            format_rename_path(old_p, &new)
        } else {
            new
        }
    }

    /// Format the file's path for display in ANSI styled banners.
    pub fn display_path_styled(&self, common_prefix: Option<&str>, fg_code: &str) -> String {
        let (old, new) = match (self.old_path.as_deref(), common_prefix) {
            (Some(old), Some(prefix)) if old != self.path => {
                let stripped_old = if let Some(stripped) = old.strip_prefix(prefix) {
                    stripped.to_string()
                } else {
                    old.to_string()
                };
                let stripped_new = if let Some(stripped) = self.path.strip_prefix(prefix) {
                    stripped.to_string()
                } else {
                    self.path.clone()
                };
                (Some(stripped_old), stripped_new)
            }
            (Some(old), None) if old != self.path => (Some(old.to_string()), self.path.clone()),
            (None | Some(_), Some(prefix)) => {
                let stripped_new = if let Some(stripped) = self.path.strip_prefix(prefix) {
                    stripped.to_string()
                } else {
                    self.path.clone()
                };
                (None, stripped_new)
            }
            _ => (None, self.path.clone()),
        };

        if let Some(ref old_p) = old {
            format_rename_path_styled(old_p, &new, fg_code)
        } else {
            new
        }
    }

    /// Format the file's path for display in Markdown.
    pub fn display_path_markdown(&self) -> String {
        if let Some(ref old) = self.old_path
            && old != &self.path
        {
            format_rename_path_markdown(old, &self.path)
        } else {
            self.path.clone()
        }
    }

    /// Format the file's path for display, using rename syntax
    /// `old -> new` when renamed.
    pub fn display_path(&self) -> String {
        self.display_path_with_prefix(None)
    }
}

/// Formats a rename from `old_path` to `new_path`.
/// The shared directory prefix on the `to` path does not render.
pub fn format_rename_path(old_path: &str, new_path: &str) -> String {
    if old_path == new_path {
        return new_path.to_string();
    }

    let old_parts: Vec<&str> = old_path.split('/').collect();
    let new_parts: Vec<&str> = new_path.split('/').collect();

    let mut common_len = 0;
    while common_len + 1 < old_parts.len()
        && common_len + 1 < new_parts.len()
        && old_parts[common_len] == new_parts[common_len]
    {
        common_len += 1;
    }

    if common_len > 0 {
        let new_rem = new_parts[common_len..].join("/");
        format!("{old_path} -> {new_rem}")
    } else {
        format!("{old_path} -> {new_path}")
    }
}

/// Formats a rename from `old_path` to `new_path` with ANSI styling:
/// - The shared directory path and trailing slash on the *from* path are
///   underlined and muted with a hint of desaturated purple.
/// - The shared directory path on the *to* path does not render.
pub fn format_rename_path_styled(old_path: &str, new_path: &str, fg_code: &str) -> String {
    if old_path == new_path {
        return new_path.to_string();
    }

    let old_parts: Vec<&str> = old_path.split('/').collect();
    let new_parts: Vec<&str> = new_path.split('/').collect();

    let mut common_len = 0;
    while common_len + 1 < old_parts.len()
        && common_len + 1 < new_parts.len()
        && old_parts[common_len] == new_parts[common_len]
    {
        common_len += 1;
    }

    if common_len > 0 {
        let prefix = old_parts[..common_len].join("/");
        let old_rem = old_parts[common_len..].join("/");
        let new_rem = new_parts[common_len..].join("/");
        format!("\x1b[4m\x1b[38;5;139m{prefix}/\x1b[24m{fg_code}{old_rem} -> {new_rem}")
    } else {
        format!("{old_path} -> {new_path}")
    }
}

/// Formats a rename from `old_path` to `new_path` for GitHub-Flavored Markdown:
/// - The shared directory path and trailing slash on the *from* path are
///   underlined (`<u>.../</u>`).
/// - The shared directory path on the *to* path does not render.
pub fn format_rename_path_markdown(old_path: &str, new_path: &str) -> String {
    if old_path == new_path {
        return new_path.to_string();
    }

    let old_parts: Vec<&str> = old_path.split('/').collect();
    let new_parts: Vec<&str> = new_path.split('/').collect();

    let mut common_len = 0;
    while common_len + 1 < old_parts.len()
        && common_len + 1 < new_parts.len()
        && old_parts[common_len] == new_parts[common_len]
    {
        common_len += 1;
    }

    if common_len > 0 {
        let prefix = old_parts[..common_len].join("/");
        let old_rem = old_parts[common_len..].join("/");
        let new_rem = new_parts[common_len..].join("/");
        format!("<u>{prefix}/</u>{old_rem} -> {new_rem}")
    } else {
        format!("{old_path} -> {new_path}")
    }
}

/// A hunk in the wire contract: change coordinates and its collapsed rows.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub rows: Vec<OutputRow>,
}

impl OutputHunk {
    pub fn reverse(&self) -> Self {
        OutputHunk {
            old_start: self.new_start,
            old_count: self.new_count,
            new_start: self.old_start,
            new_count: self.old_count,
            rows: self.rows.iter().map(OutputRow::reverse).collect(),
        }
    }

    /// Aggregated line-level change counts for this hunk's rows, mirroring
    /// [`crate::counts::file_counts`] row semantics (moved sources and
    /// targets both count as relocations; context and separators excluded).
    /// Notes attach at file level, so `notes` is always 0 — use
    /// [`OutputHunk::note_count`] to attribute a file's notes to this hunk.
    pub fn counts(&self) -> FileCounts {
        let mut counts = FileCounts::default();
        for r in &self.rows {
            match r.kind {
                RowKind::Del => counts.dels += 1,
                RowKind::Add => counts.adds += 1,
                RowKind::Mod => counts.mods += 1,
                RowKind::Mov => counts.movs += 1,
                _ => {}
            }
        }
        counts
    }

    /// Number of `notes` attached to lines in this hunk. A note counts
    /// toward the single hunk where its selection range ends (the hunk's
    /// coordinate range on the note's own side contains `end`), mirroring
    /// where note callouts anchor. Selection ranges spanning several hunks
    /// are therefore never double-counted.
    pub fn note_count(&self, notes: &[OutputNote]) -> usize {
        notes
            .iter()
            .filter(|n| {
                let (start, count) = match n.side {
                    NoteSide::New => (self.new_start, self.new_count),
                    NoteSide::Old => (self.old_start, self.old_count),
                };
                count > 0 && n.end >= start && n.end < start + count
            })
            .count()
    }

    /// Row counts for this hunk with `notes` attributed via
    /// [`OutputHunk::note_count`].
    pub fn counts_with_notes(&self, notes: &[OutputNote]) -> FileCounts {
        let mut counts = self.counts();
        counts.notes = self.note_count(notes);
        counts
    }

    pub fn reconstruct(&self) -> Hunk {
        Hunk {
            old_start: self.old_start,
            old_count: self.old_count,
            new_start: self.new_start,
            new_count: self.new_count,
            section: String::new(),
            rows: self.rows.iter().map(OutputRow::reconstruct).collect(),
            above_ctx: 0,
            below_ctx: 0,
        }
    }
}

/// A row in the wire-contract output: the collapse row + its merged token runs.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRow {
    pub kind: RowKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mov_side: Option<MovSide>,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub del_text: Option<String>,
    pub spans: Spans,
    pub runs: Vec<OutputRun>,
    /// The mod is wholesale (the del block + the add block render as two
    /// separate blocks, never merged) — the renderer's old isWholesaleMod
    /// decision, baked by the engine so the TS merge machinery can go.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wholesale: bool,
}

impl From<&OutputRow> for Row {
    fn from(r: &OutputRow) -> Self {
        match r.kind {
            RowKind::Ctx => {
                let old_pos = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let new_pos = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                Row::Ctx(CtxLine::new(old_pos, new_pos, &r.text))
            }
            RowKind::Del => {
                let num = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                Row::Del(DelLine::new(num, &r.text))
            }
            RowKind::Add => {
                let num = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                if let Some(ref dt) = r.del_text {
                    Row::Add(AddLine::with_donor(num, &r.text, dt, r.spans.clone()))
                } else {
                    Row::Add(AddLine::with_spans(num, &r.text, r.spans.clone()))
                }
            }
            RowKind::Mod => {
                let old_num = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let new_num = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let del_text = r.del_text.as_deref().unwrap_or(&r.text);
                Row::Mod {
                    old_line: DelLine::new(old_num, del_text),
                    new_line: AddLine::with_spans(new_num, &r.text, r.spans.clone()),
                }
            }
            RowKind::Mov => match r.mov_side {
                Some(MovSide::Src) => {
                    let old_num = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                    Row::MovSrc(DelLine::new(old_num, &r.text))
                }
                Some(MovSide::Tgt) | None => {
                    let new_num = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                    let del_text = r.del_text.as_deref().unwrap_or(&r.text);
                    Row::MovTgt(AddLine::with_donor(
                        new_num,
                        &r.text,
                        del_text,
                        r.spans.clone(),
                    ))
                }
            },
            RowKind::File | RowKind::Sep => {
                Row::Ctx(CtxLine::new(LineNum::MIN, LineNum::MIN, r.text.clone()))
            }
        }
    }
}

impl OutputRow {
    pub fn reverse(&self) -> Self {
        OutputRow {
            kind: self.kind.reverse(),
            mov_side: self.mov_side.map(|s| match s {
                MovSide::Src => MovSide::Tgt,
                MovSide::Tgt => MovSide::Src,
            }),
            text: self.del_text.clone().unwrap_or_else(|| self.text.clone()),
            new_line: self.old_line,
            old_line: self.new_line,
            del_text: if self.kind == RowKind::Mod || self.del_text.is_some() {
                Some(self.text.clone())
            } else {
                None
            },
            spans: self.spans.reverse(),
            runs: self.runs.iter().map(OutputRun::reverse).collect(),
            wholesale: self.wholesale,
        }
    }

    #[inline]
    pub fn reconstruct(&self) -> Row {
        self.into()
    }
}

use std::borrow::Cow;

/// A classified token run in an [`OutputRow`]: kind, optional theme color key,
/// SGR attribute modifier, and text.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputRun {
    pub kind: RunKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Cow<'static, str>>,
    /// The SGR attribute (the markdown emphasis — bold/italic/strike/
    /// underline) the renderer applies with the color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sgr: Option<u32>,
    pub text: Cow<'static, str>,
}

impl OutputRun {
    pub fn reverse(&self) -> Self {
        OutputRun {
            kind: self.kind.reverse(),
            color: self.color.clone(),
            sgr: self.sgr,
            text: self.text.clone(),
        }
    }
}

/// A review note attached to a file location.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputNote {
    pub id: String,
    pub file: String,
    pub side: NoteSide,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub created_at: u64,
    pub runs: Vec<OutputRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_hash: Option<LineHash>,
}

impl OutputNote {
    pub fn reverse(&self) -> Self {
        OutputNote {
            id: self.id.clone(),
            file: self.file.clone(),
            side: self.side.reverse(),
            start: self.start,
            end: self.end,
            text: self.text.clone(),
            created_at: self.created_at,
            runs: self.runs.iter().map(OutputRun::reverse).collect(),
            line_hash: self.line_hash,
        }
    }
}

/// A fenced syntax region in a file: the span of lines whose content
/// classifies with a different language than the file's.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFence {
    pub start: u32,
    pub end: u32,
    pub lang: String,
}
