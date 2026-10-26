//! Diffuse Wire Contract Data Models & Serialization Schema.
//!
//! Defines the structured JSON wire contract output emitted by the engine
//! and consumed by external tooling, the CLI (`--json`), and the pi review
//! extension. The schema is frozen per PLAN.md §2.

use std::{
    fmt::{self, Debug, Display},
    ops::Not,
    sync::Arc,
};

use diffuse_syntax::profile::LangProfile;
use serde::{Deserialize, Serialize};

use crate::{
    counts::FileCounts,
    merge::RunKind,
    notes::{LineHash, NoteStatus, Side as NoteSide},
    parse::{AddLine, CtxLine, DelLine, DiffFile, Hunk, Row, RowKind, Spans},
};

pub use crate::parse::{FileStatus, RowGroup};

pub mod jsonl;
pub use jsonl::{WireRecord, decode_jsonl_records, emit_file_jsonl};

#[cfg(test)]
mod tests;

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
            total.in_progress_notes += f.counts.in_progress_notes;
            total.resolved_notes += f.counts.resolved_notes;
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

    /// Validates cross-pipeline invariants: all files are valid and total
    /// counts match.
    pub fn validate_invariants(&self) -> Result<(), String> {
        for f in &self.files {
            f.validate_invariants()?;
        }
        let total = self.total_counts();
        let mut expected = FileCounts::default();
        for f in &self.files {
            expected.adds += f.counts.adds;
            expected.dels += f.counts.dels;
            expected.mods += f.counts.mods;
            expected.movs += f.counts.movs;
            expected.notes += f.counts.notes;
            expected.in_progress_notes += f.counts.in_progress_notes;
            expected.resolved_notes += f.counts.resolved_notes;
        }
        if total != expected {
            return Err("PipelineOutput total_counts mismatch".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSize(pub usize);

impl Debug for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", format_byte_size(self.0))
    }
}

impl Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

    // Decimals only below 5.0 adjusted units; at and above, integers.
    let formatted = if size < 5.0 {
        let mut trimmed = format!("{:.1}", size);
        // Trim trailing zeros after the decimal point
        if trimmed.contains('.') {
            trimmed = trimmed
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string();
        }
        trimmed
    } else {
        format!("{:.0}", size)
    };

    format!("{}{}", formatted, unit)
}

pub const NON_EXISTENT_SYMBOL: &str = "×";
pub const UNKNOWN_SYMBOL: &str = "?";
pub const ARROW_SYMBOL: &str = "➞";

/// The engine's complete, rendered output for a single changed file.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct OutputFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_mode: Option<String>,
    pub status: FileStatus,
    #[serde(default, skip_serializing_if = "Not::not")]
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
    /// unit tests). The diff is shared (`Arc`) — never cloned per file — so
    /// retaining it for on-demand reload costs one pointer per file.
    Raw {
        path: String,
        diff: Arc<String>,
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
        Self {
            path: f.path.clone(),
            old_path: f.old_path.clone(),
            old_mode: f.old_mode.clone(),
            new_mode: f.new_mode.clone(),
            status: f.status,
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
        OutputFile {
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
        DiffFile {
            path: self.path.clone(),
            old_path: self.old_path.clone(),
            old_mode: self.old_mode.clone(),
            new_mode: self.new_mode.clone(),
            status: self.status,
            is_binary: self.is_binary,
            has_file_headers: false,
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
        if self.status == "deleted" || (self.status == "untracked" && self.new_bytes.is_none()) {
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
        if self.status == "added" {
            new_fmt.unwrap_or_else(|| UNKNOWN_SYMBOL.to_string())
        } else if self.status == "deleted" {
            old_fmt.unwrap_or_else(|| UNKNOWN_SYMBOL.to_string())
        } else if self.status == "untracked" {
            new_fmt
                .or(old_fmt)
                .unwrap_or_else(|| UNKNOWN_SYMBOL.to_string())
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
            // Added/renamed/deleted share the generic shape below; only the
            // mode-change prefix is special.
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
            ("modified" | "unmodified", None) => {
                if size_str.is_empty() {
                    self.status.to_string()
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

    /// Validates core invariants on this OutputFile: path presence, counts
    /// conservation, fence bounds & disjointness, and internal hunk
    /// invariants.
    pub fn validate_invariants(&self) -> Result<(), String> {
        if self.path.is_empty() {
            return Err("OutputFile path must not be empty".to_string());
        }
        if self.status == "renamed" && self.old_path.is_none() {
            return Err(format!("{}: renamed file must have old_path", self.path));
        }

        for (i, fence) in self.fences.iter().enumerate() {
            if fence.start > fence.end || fence.end > self.new_line_count as u32 {
                return Err(format!(
                    "{}: fence {i} out of bounds [{}..={}] (new_line_count={})",
                    self.path, fence.start, fence.end, self.new_line_count
                ));
            }
            if i > 0 && self.fences[i - 1].end + 1 >= fence.start {
                return Err(format!(
                    "{}: fences overlap or touch: fence {} ends at {}, next starts at {}",
                    self.path,
                    i - 1,
                    self.fences[i - 1].end,
                    fence.start
                ));
            }
        }

        if self.is_loaded() {
            let mut adds = 0usize;
            let mut dels = 0usize;
            let mut mods = 0usize;
            let mut movs = 0usize;
            for h in &self.hunks {
                for r in &h.rows {
                    match r.kind {
                        RowKind::Mov => {
                            if r.mov_side != Some(MovSide::Src) {
                                movs += 1;
                            }
                        }
                        RowKind::Add => adds += 1,
                        RowKind::Del => dels += 1,
                        RowKind::Mod => mods += 1,
                        _ => {}
                    }
                }
            }
            if self.counts.adds != adds
                || self.counts.dels != dels
                || self.counts.mods != mods
                || self.counts.movs != movs
            {
                return Err(format!(
                    "{}: counts mismatch (counts: {:?}, walked: +{adds} -{dels} ~{mods} ^{movs})",
                    self.path, self.counts
                ));
            }

            let ext = self.path.rsplit('.').next().unwrap_or("");
            let profile = diffuse_syntax::registry::lang_profile(ext);

            for (h_idx, h) in self.hunks.iter().enumerate() {
                if let Err(e) = h.validate_invariants_with_profile(profile) {
                    return Err(format!("{}: hunk {h_idx}: {e}", self.path));
                }
            }

            for (h_idx, h) in self.raw_hunks.iter().enumerate() {
                if let Err(e) = h.validate_invariants_with_profile(profile) {
                    return Err(format!("{}: raw_hunk {h_idx}: {e}", self.path));
                }
            }
        }

        Ok(())
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
#[derive(Default)]
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
    /// Notes attach at file level, so `notes` is always 0 — note counts
    /// are never attributed to hunks.
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

    pub fn validate_invariants(&self) -> Result<(), String> {
        self.validate_invariants_with_profile(None)
    }

    pub fn validate_invariants_with_profile(
        &self,
        profile: Option<&LangProfile>,
    ) -> Result<(), String> {
        let mut last_new = None;

        for (row_idx, r) in self.rows.iter().enumerate() {
            // New line number monotonicity
            if let Some(nl) = r.new_line {
                if let Some(prev) = last_new
                    && nl <= prev
                {
                    return Err(format!(
                        "Hunk row {row_idx}: new_line non-monotonic: {nl} <= {prev}"
                    ));
                }
                last_new = Some(nl);
            }

            // Line number invariants per row kind
            match r.kind {
                RowKind::Add => {
                    if r.new_line.is_none() || r.old_line.is_some() {
                        return Err(format!(
                            "Hunk row {row_idx}: Add row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                }
                RowKind::Del => {
                    if r.old_line.is_none() || r.new_line.is_some() {
                        return Err(format!(
                            "Hunk row {row_idx}: Del row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                    if r.runs.iter().any(|run| run.kind != RunKind::Del) {
                        return Err(format!("Hunk row {row_idx}: Del row has non-Del runs"));
                    }
                }
                RowKind::Mod => {
                    if r.new_line.is_none() || r.old_line.is_none() {
                        return Err(format!(
                            "Hunk row {row_idx}: Mod row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                    if r.wholesale {
                        return Err(format!("Hunk row {row_idx}: Mod row must not be wholesale"));
                    }
                }
                RowKind::Ctx => {
                    if r.new_line.is_none() || r.old_line.is_none() {
                        return Err(format!(
                            "Hunk row {row_idx}: Ctx row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                    if r.runs.iter().any(|run| run.kind != RunKind::Ctx) {
                        return Err(format!("Hunk row {row_idx}: Ctx row has non-Ctx runs"));
                    }
                }
                RowKind::Mov => {
                    if r.mov_side == Some(MovSide::Src) {
                        if r.old_line.is_none() || r.new_line.is_some() {
                            return Err(format!(
                                "Hunk row {row_idx}: MovSrc row invalid line numbers (new={:?}, old={:?})",
                                r.new_line, r.old_line
                            ));
                        }
                        if r.runs
                            .iter()
                            .any(|run| run.kind != RunKind::Del && run.kind != RunKind::MovedSrc)
                        {
                            return Err(format!(
                                "Hunk row {row_idx}: MovSrc row has non-del-like runs"
                            ));
                        }
                    } else if r.new_line.is_none() || r.old_line.is_some() {
                        return Err(format!(
                            "Hunk row {row_idx}: MovTgt row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                }
                _ => {}
            }

            // Spans bounds and ordering
            let spans = r.spans();
            let new_char_len = r.new_text().chars().count();
            let mut prev_add_end = 0;
            for &(s, e) in &spans.add {
                if s > e || e > new_char_len || s < prev_add_end {
                    return Err(format!(
                        "Hunk row {row_idx}: invalid add span [{s}..{e}] for text length {new_char_len}"
                    ));
                }
                prev_add_end = e;
            }

            let old_char_len = r.old_text().chars().count();
            let mut prev_del_end = 0;
            for &(s, e) in &spans.del {
                if s > e || e > old_char_len || s < prev_del_end {
                    return Err(format!(
                        "Hunk row {row_idx}: invalid del span [{s}..{e}] for old text length {old_char_len}"
                    ));
                }
                prev_del_end = e;
            }

            // Disk-exact invariant:
            // Forward: non-del-like runs concatenate to row.new_text()
            if r.kind == RowKind::Mod || (r.kind == RowKind::Add && r.is_aux()) {
                let kept: String = r
                    .runs
                    .iter()
                    .filter(|x| !x.kind.is_del_like())
                    .map(|x| x.text.as_ref())
                    .collect();
                if kept != r.new_text() {
                    return Err(format!(
                        "Hunk row {row_idx}: forward text mismatch: kept {kept:?} != text {:?}",
                        r.new_text()
                    ));
                }
            }
            // Reverse: non-add-like runs concatenate to row.old_text()
            if r.kind == RowKind::Mod {
                let rev: String = r
                    .runs
                    .iter()
                    .filter(|x| !x.kind.is_add_like())
                    .map(|x| x.text.as_ref())
                    .collect();
                let old_text = r.old_text();
                if rev != old_text {
                    return Err(format!(
                        "Hunk row {row_idx}: reverse text mismatch: rev {rev:?} != old_text {old_text:?}"
                    ));
                }
            }
        }

        // Reflow groups invariant within the hunk
        self.validate_reflow_groups(profile)?;

        Ok(())
    }

    fn validate_reflow_groups(&self, profile: Option<&LangProfile>) -> Result<(), String> {
        let strip_ws = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();

        let mut anchors = rustc_hash::FxHashMap::default();
        for (i, r) in self.rows.iter().enumerate() {
            if r.group == Some(RowGroup::Anchor) {
                let anchor_num = r.new_line.or(r.old_line).unwrap_or(0);
                anchors.insert(anchor_num, i);
            }
        }

        for (i, r) in self.rows.iter().enumerate() {
            if let Some(RowGroup::Aux { anchor }) = r.group
                && !anchors.contains_key(&anchor)
            {
                return Err(format!(
                    "Row {i} is Aux pointing to nonexistent anchor line {anchor}"
                ));
            }
        }

        for (&anchor_num, &anchor_idx) in &anchors {
            let anchor_row = &self.rows[anchor_idx];
            if anchor_row.old_line.is_none() {
                continue;
            }

            let mut group_rows: Vec<&OutputRow> = Vec::new();
            for r in &self.rows {
                if r.group == Some(RowGroup::Anchor) {
                    let num = r.new_line.or(r.old_line).unwrap_or(0);
                    if num == anchor_num {
                        group_rows.push(r);
                    }
                } else if let Some(RowGroup::Aux { anchor }) = r.group
                    && anchor == anchor_num
                {
                    group_rows.push(r);
                }
            }

            let aux_adds: Vec<&&OutputRow> = group_rows
                .iter()
                .filter(|r| r.kind == RowKind::Add)
                .collect();
            let has_aux_dels = group_rows.iter().any(|r| {
                r.kind == RowKind::Del
                    || (r.kind == RowKind::Mov && r.mov_side == Some(MovSide::Src))
            });

            if !aux_adds.is_empty() && !has_aux_dels {
                let is_import =
                    profile.is_some_and(|p| p.is_import_statement(&anchor_row.old_text()));

                let anchor_moved_src: String = anchor_row
                    .runs
                    .iter()
                    .filter(|run| run.kind == RunKind::MovedSrc)
                    .map(|run| run.text.as_ref())
                    .collect();
                let aux_non_add: String = aux_adds
                    .iter()
                    .flat_map(|aux| &aux.runs)
                    .filter(|run| run.kind == RunKind::MovedTgt || run.kind == RunKind::Del)
                    .map(|run| run.text.as_ref())
                    .collect();

                if is_import {
                    let expected_old: String = anchor_row
                        .runs
                        .iter()
                        .filter(|run| run.kind == RunKind::Ctx || run.kind == RunKind::MovedSrc)
                        .map(|run| run.text.as_ref())
                        .collect();
                    let mut reconstructed_old = String::new();
                    for r in &group_rows {
                        if r.kind == RowKind::Mod {
                            reconstructed_old.extend(
                                r.runs
                                    .iter()
                                    .filter(|run| run.kind == RunKind::Ctx)
                                    .map(|run| run.text.as_ref()),
                            );
                        } else if r.kind == RowKind::Add {
                            reconstructed_old.extend(
                                r.runs
                                    .iter()
                                    .filter(|run| {
                                        run.kind == RunKind::MovedTgt || run.kind == RunKind::Del
                                    })
                                    .map(|run| run.text.as_ref()),
                            );
                        }
                    }
                    if strip_ws(&reconstructed_old) != strip_ws(&expected_old) {
                        return Err(format!(
                            "Anchor line {anchor_num} old-text reconstruction mismatch:\n  expected: {expected_old:?}\n  got:      {reconstructed_old:?}"
                        ));
                    }
                    if strip_ws(&anchor_moved_src) != strip_ws(&aux_non_add) {
                        return Err(format!(
                            "Anchor line {anchor_num} moved-text conservation mismatch:\n  anchor MovedSrc: {anchor_moved_src:?}\n  aux non-add:     {aux_non_add:?}"
                        ));
                    }
                } else {
                    let mut s1: Vec<char> = anchor_moved_src
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    let mut s2: Vec<char> =
                        aux_non_add.chars().filter(|c| !c.is_whitespace()).collect();
                    s1.sort_unstable();
                    s2.sort_unstable();
                    if s1 != s2 {
                        return Err(format!(
                            "Anchor line {anchor_num} moved-text character conservation mismatch:\n  anchor MovedSrc chars: {s1:?}\n  aux non-add chars:     {s2:?}"
                        ));
                    }
                }

                // Suffix aux add line numbers strictly ascending
                let mut last_aux_line = None;
                for aux in group_rows.iter().filter(|r| r.kind == RowKind::Add) {
                    if let Some(nl) = aux.new_line {
                        if let Some(prev) = last_aux_line
                            && nl <= prev
                        {
                            return Err(format!("Aux row line {nl} <= previous aux line {prev}"));
                        }
                        last_aux_line = Some(nl);
                    }
                }
            }
        }

        Ok(())
    }
}

/// Optional trailing metadata flags on a serialized compact row tuple.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowFlags {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mov_side: Option<MovSide>,
    #[serde(default, skip_serializing_if = "Not::not")]
    pub wholesale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<RowGroup>,
}

impl RowFlags {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.mov_side.is_none() && !self.wholesale && self.group.is_none()
    }
}

/// A row in the wire-contract output: the collapse row + its merged token runs.
///
/// Serialized as a positional tuple: `[kind, old_line, new_line, runs,
/// flags?]`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OutputRow {
    pub kind: RowKind,
    pub mov_side: Option<MovSide>,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub runs: Vec<OutputRun>,
    pub wholesale: bool,
    pub group: Option<RowGroup>,
}

/// Test support across the engine's row-level tests: whether the rendered
/// row stream links old line `old` with new line `new` — as a Mod, or as an
/// adjacent MovSrc/MovTgt pair. Representation is engine policy; the
/// pairing is the contract.
#[cfg(test)]
pub(crate) fn test_rows_pair_with(rows: &[&OutputRow], old: u32, new: u32) -> bool {
    if rows
        .iter()
        .any(|r| r.kind == RowKind::Mod && r.old_line == Some(old) && r.new_line == Some(new))
    {
        return true;
    }
    rows.windows(2).any(|w| {
        w[0].kind == RowKind::Mov
            && w[0].mov_side == Some(MovSide::Src)
            && w[0].old_line == Some(old)
            && w[1].kind == RowKind::Mov
            && w[1].mov_side == Some(MovSide::Tgt)
            && w[1].new_line == Some(new)
    })
}

impl Serialize for OutputRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let flags = RowFlags {
            mov_side: self.mov_side,
            wholesale: self.wholesale,
            group: self.group,
        };
        let len = if flags.is_empty() { 4 } else { 5 };
        let mut seq = serializer.serialize_seq(Some(len))?;
        seq.serialize_element(&self.kind)?;
        seq.serialize_element(&self.old_line)?;
        seq.serialize_element(&self.new_line)?;
        seq.serialize_element(&self.runs)?;
        if !flags.is_empty() {
            seq.serialize_element(&flags)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for OutputRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OutputRowVisitor;

        impl<'de> serde::de::Visitor<'de> for OutputRowVisitor {
            type Value = OutputRow;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a compact row array [kind, old_line, new_line, runs, flags?]")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let kind: RowKind = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let old_line: Option<u32> = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                let new_line: Option<u32> = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(2, &self))?;
                let runs: Vec<OutputRun> = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;
                let flags: Option<RowFlags> = seq.next_element()?;

                let (mov_side, wholesale, group) = match flags {
                    Some(f) => (f.mov_side, f.wholesale, f.group),
                    None => (None, false, None),
                };

                Ok(OutputRow {
                    kind,
                    mov_side,
                    new_line,
                    old_line,
                    runs,
                    wholesale,
                    group,
                })
            }
        }

        deserializer.deserialize_seq(OutputRowVisitor)
    }
}

impl From<&OutputRow> for Row {
    fn from(r: &OutputRow) -> Self {
        let cached_runs: Vec<(RunKind, String)> = r
            .runs
            .iter()
            .map(|run| (run.kind, run.text.to_string()))
            .collect();
        let spans = r.spans();
        match r.kind {
            RowKind::Ctx => {
                let old_pos = r.old_line.expect("Ctx row must have old line");
                let new_pos = r.new_line.expect("Ctx row must have new line");
                Row::Ctx(CtxLine::new(old_pos, new_pos, r.new_text()))
            }
            RowKind::Del => {
                let num = r.old_line.expect("Del row must have old line");
                let del = DelLine::new(num, r.old_text());
                if let Some(RowGroup::Aux { anchor }) = r.group {
                    Row::AuxDel {
                        anchor,
                        line: del,
                        spans,
                        cached_runs,
                    }
                } else {
                    Row::Del(del)
                }
            }
            RowKind::Add => {
                let num = r.new_line.expect("Add row must have new line");
                let add = AddLine::new(num, r.new_text());
                if let Some(RowGroup::Aux { anchor }) = r.group {
                    Row::AuxAdd {
                        anchor,
                        line: add,
                        spans,
                        cached_runs,
                    }
                } else {
                    Row::Add(add)
                }
            }
            RowKind::Mod => {
                let old_num = r.old_line.expect("Mod row must have old line");
                let new_num = r.new_line.expect("Mod row must have new line");
                let old_line = DelLine::new(old_num, r.old_text());
                let new_line = AddLine::new(new_num, r.new_text());
                let is_anchor = r.group == Some(RowGroup::Anchor);
                Row::Mod {
                    old_line,
                    new_line,
                    spans,
                    cached_runs,
                    is_anchor,
                }
            }
            RowKind::Mov => match r.mov_side {
                Some(MovSide::Src) => {
                    let old_num = r.old_line.expect("MovSrc row must have old line");
                    let line = DelLine::new(old_num, r.old_text());
                    Row::MovSrc {
                        line,
                        spans,
                        cached_runs,
                    }
                }
                Some(MovSide::Tgt) | None => {
                    let new_num = r.new_line.expect("MovTgt row must have new line");
                    let line = AddLine::new(new_num, r.new_text());
                    Row::MovTgt {
                        line,
                        spans,
                        cached_runs,
                    }
                }
            },
            RowKind::File | RowKind::Sep => Row::Sep(r.new_text()),
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
            new_line: self.old_line,
            old_line: self.new_line,
            runs: self.runs.iter().map(OutputRun::reverse).collect(),
            wholesale: self.wholesale,
            group: self.group,
        }
    }

    #[inline]
    pub fn reconstruct(&self) -> Row {
        self.into()
    }

    /// Whether this row is an auxiliary row within a multiline cluster.
    #[inline]
    pub fn is_aux(&self) -> bool {
        matches!(self.group, Some(RowGroup::Aux { .. }))
    }

    /// Computes the new-side line text projected across this row's runs (Ctx +
    /// Add).
    #[inline]
    pub fn new_text(&self) -> String {
        runs_new_text(&self.runs)
    }

    /// Computes the old-side line text projected across this row's runs (Ctx +
    /// Del).
    #[inline]
    pub fn old_text(&self) -> String {
        runs_old_text(&self.runs)
    }

    /// Main line text for display: old text for Del / MovSrc, new text for Ctx
    /// / Add / Mod / MovTgt.
    #[inline]
    pub fn text(&self) -> String {
        match self.kind {
            RowKind::Del => self.old_text(),
            RowKind::Mov if self.mov_side == Some(MovSide::Src) => self.old_text(),
            _ => self.new_text(),
        }
    }

    /// Derives change spans on demand from runs.
    #[inline]
    pub fn spans(&self) -> Spans {
        derive_spans_from_runs(&self.runs)
    }
}

/// Derives [`Spans`] change intervals from a slice of [`OutputRun`]s.
pub fn derive_spans_from_runs(runs: &[OutputRun]) -> Spans {
    let mut add = Vec::new();
    let mut del = Vec::new();
    let mut new_offset = 0usize;
    let mut old_offset = 0usize;

    for r in runs {
        let char_len = r.text.chars().count();
        match r.kind {
            RunKind::Ctx => {
                new_offset += char_len;
                old_offset += char_len;
            }
            RunKind::Add => {
                let start = new_offset;
                new_offset += char_len;
                add.push((start, new_offset));
            }
            // Moved-out source text left this line exactly like struck
            // text (old-side coverage is identical); only renderers
            // distinguish the two. See `RunKind::is_del_like`.
            RunKind::Del | RunKind::MovedSrc => {
                let start = old_offset;
                old_offset += char_len;
                del.push((start, old_offset));
            }
            // Moved-in target text is new-side-only (like filled text for
            // offset purposes) but lands in no span bucket — it is shared,
            // not novel. See `RunKind::is_add_like`.
            RunKind::MovedTgt => {
                new_offset += char_len;
            }
        }
    }

    let merge_spans = |spans: Vec<(usize, usize)>| -> Vec<(usize, usize)> {
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (s, e) in spans {
            if let Some(last) = merged.last_mut()
                && last.1 == s
            {
                last.1 = e;
            } else {
                merged.push((s, e));
            }
        }
        merged
    };

    Spans {
        del: merge_spans(del),
        add: merge_spans(add),
    }
}

use std::borrow::Cow;

/// A classified token run in an [`OutputRow`]: kind, optional theme color key,
pub use diffuse_syntax::color::TokenColor;

/// An individual colored text run within an [`OutputRow`].
///
/// Encodes syntax highlighting and intra-line diff highlights as a
/// sequence of uniform character slices carrying run kind (add/del/ctx),
/// color classification (syntax/theme token), optional markdown SGR attribute
/// modifier, and text.
///
/// Serialized compactly on the wire:
/// - 1 element: `"+text"` for unstyled runs (`TokenColor::Default`, no SGR).
/// - 2 elements: `["+text", "kwd"]` for styled runs without SGR.
/// - 3 elements: `["+text", null, 4]` or `["+text", "kwd", 1]` when SGR is
///   present.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OutputRun {
    pub kind: RunKind,
    pub color: TokenColor,
    /// The SGR attribute (the markdown emphasis — bold/italic/strike/
    /// underline) the renderer applies with the color.
    pub sgr: Option<u32>,
    pub text: Cow<'static, str>,
}

impl Serialize for OutputRun {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut baked = String::with_capacity(1 + self.text.len());
        baked.push(self.kind.sign_char());
        baked.push_str(&self.text);

        if let Some(sgr) = self.sgr {
            let mut seq = serializer.serialize_seq(Some(3))?;
            seq.serialize_element(&baked)?;
            seq.serialize_element(&self.color)?;
            seq.serialize_element(&sgr)?;
            seq.end()
        } else if !self.color.is_default() {
            let mut seq = serializer.serialize_seq(Some(2))?;
            seq.serialize_element(&baked)?;
            seq.serialize_element(&self.color)?;
            seq.end()
        } else {
            serializer.serialize_str(&baked)
        }
    }
}

impl<'de> Deserialize<'de> for OutputRun {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OutputRunVisitor;

        impl<'de> serde::de::Visitor<'de> for OutputRunVisitor {
            type Value = OutputRun;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a compact run string or an array [baked_text, color, sgr?]")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                let mut chars = v.chars();
                let sign = chars.next().ok_or_else(|| E::custom("empty run string"))?;
                let kind = RunKind::from_sign_char(sign)
                    .ok_or_else(|| E::custom(format!("invalid run sign char '{sign}'")))?;
                let text = Cow::Owned(chars.as_str().to_string());
                Ok(OutputRun {
                    kind,
                    color: TokenColor::Default,
                    sgr: None,
                    text,
                })
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                use serde::de::Error;
                let baked: String = seq
                    .next_element()?
                    .ok_or_else(|| Error::custom("missing baked run string"))?;
                let mut chars = baked.chars();
                let sign = chars
                    .next()
                    .ok_or_else(|| Error::custom("empty baked run string"))?;
                let kind = RunKind::from_sign_char(sign)
                    .ok_or_else(|| Error::custom(format!("invalid run sign char '{sign}'")))?;
                let text = Cow::Owned(chars.as_str().to_string());

                let color: TokenColor = seq.next_element()?.unwrap_or_default();
                let sgr_opt: Option<Option<u32>> = seq.next_element()?;
                let sgr = sgr_opt.flatten();

                Ok(OutputRun {
                    kind,
                    color,
                    sgr,
                    text,
                })
            }
        }

        deserializer.deserialize_any(OutputRunVisitor)
    }
}

impl OutputRun {
    #[inline]
    pub fn new(kind: RunKind, text: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            color: TokenColor::Default,
            sgr: None,
            text: text.into(),
        }
    }

    #[inline]
    pub fn sign_char(&self) -> char {
        self.kind.sign_char()
    }

    pub fn reverse(&self) -> Self {
        OutputRun {
            kind: self.kind.reverse(),
            color: self.color,
            sgr: self.sgr,
            text: self.text.clone(),
        }
    }
}

/// Project the forward/new text from an iterator of [`OutputRun`]s (Ctx +
/// Add; del-like runs — struck or moved-out — are old-side only).
pub fn runs_new_text<'a, I>(runs: I) -> String
where
    I: IntoIterator<Item = &'a OutputRun>,
{
    runs.into_iter()
        .filter(|r| !r.kind.is_del_like())
        .map(|r| r.text.as_ref())
        .collect()
}

/// Project the reverse/old text from an iterator of [`OutputRun`]s (Ctx + Del
/// + moved-out source; new-side-only runs are excluded).
pub fn runs_old_text<'a, I>(runs: I) -> String
where
    I: IntoIterator<Item = &'a OutputRun>,
{
    runs.into_iter()
        .filter(|r| !r.kind.is_add_like())
        .map(|r| r.text.as_ref())
        .collect()
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
    #[serde(default, skip_serializing_if = "NoteStatus::is_open")]
    pub status: NoteStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_lines: Option<String>,
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
            status: self.status,
            source_lines: self.source_lines.clone(),
        }
    }
}

impl From<&OutputNote> for crate::notes::ReviewNote {
    fn from(n: &OutputNote) -> Self {
        crate::notes::ReviewNote {
            id: n.id.clone(),
            file: n.file.clone(),
            side: n.side,
            start: n.start,
            end: n.end,
            text: n.text.clone(),
            created_at: n.created_at,
            line_hash: n.line_hash,
            status: n.status,
            source_lines: n.source_lines.clone(),
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
