//! Output File Model & Rename Path Formatting.
//!
//! Encodes the primary per-file diff metadata, hunks, pre-classified context
//! runs, and display formatting for paths, modes, and sizes.

use std::{ops::Not, sync::Arc};

use serde::{Deserialize, Serialize};

use super::{MovSide, OutputFence, OutputHunk, OutputNote, OutputRow, OutputRun, format_byte_size};
use crate::{
    counts::FileCounts,
    merge::RunKind,
    parse::{DiffFile, FileStatus, RowKind},
};

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

    pub fn reverse(mut self) -> Self {
        if let Some(old) = self.old_path {
            self.old_path = Some(std::mem::replace(&mut self.path, old));
        }
        std::mem::swap(&mut self.old_mode, &mut self.new_mode);
        std::mem::swap(&mut self.old_bytes, &mut self.new_bytes);
        std::mem::swap(&mut self.old_line_count, &mut self.new_line_count);
        std::mem::swap(&mut self.old_content, &mut self.new_content);
        self.status = self.status.reverse();
        self.hunks = self.hunks.into_iter().map(OutputHunk::reverse).collect();
        self.raw_hunks = self
            .raw_hunks
            .into_iter()
            .map(OutputHunk::reverse)
            .collect();
        self.counts = self.counts.reverse();
        self.notes = self.notes.into_iter().map(OutputNote::reverse).collect();
        self.content_runs = self
            .content_runs
            .into_iter()
            .map(|runs| runs.into_iter().map(OutputRun::reverse).collect())
            .collect();
        self.rows = self.rows.into_iter().map(OutputRow::reverse).collect();
        self
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

    /// Formats the file size, displaying the current size alongside the change
    /// delta if modified (e.g. `24kB +3.3kB` or `21kB −1.2kB`), a single size
    /// if unchanged, or `+size` / `−size` for added, untracked, and deleted
    /// files.
    pub fn display_size(&self) -> String {
        if self.old_bytes.is_none() && self.new_bytes.is_none() {
            return String::new();
        }
        let new_fmt = self.new_bytes.map(format_byte_size);
        let old_fmt = self.old_bytes.map(format_byte_size);
        if self.status == "added" {
            match new_fmt {
                Some(n) => format!("+{n}"),
                None => UNKNOWN_SYMBOL.to_string(),
            }
        } else if self.status == "deleted" {
            match old_fmt {
                Some(o) => format!("−{o}"),
                None => UNKNOWN_SYMBOL.to_string(),
            }
        } else if self.status == "untracked" {
            let fmt = new_fmt.or(old_fmt);
            match fmt {
                Some(s) => format!("+{s}"),
                None => UNKNOWN_SYMBOL.to_string(),
            }
        } else {
            match (self.old_bytes, self.new_bytes) {
                (Some(o), Some(n)) => {
                    let base = format_byte_size(n);
                    let delta = n as i64 - o as i64;
                    if delta > 0 {
                        let delta_fmt = format_byte_size(delta as usize);
                        format!("{base} +{delta_fmt}")
                    } else if delta < 0 {
                        let delta_fmt = format_byte_size((-delta) as usize);
                        format!("{base} −{delta_fmt}")
                    } else {
                        base
                    }
                }
                (Some(o), None) => {
                    let o_fmt = format_byte_size(o);
                    format!("{o_fmt} {ARROW_SYMBOL} {UNKNOWN_SYMBOL}")
                }
                (None, Some(n)) => {
                    let n_fmt = format_byte_size(n);
                    format!("{UNKNOWN_SYMBOL} {ARROW_SYMBOL} {n_fmt}")
                }
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

            let mut sum_hunk_counts = FileCounts::default();
            for (h_idx, h) in self.hunks.iter().enumerate() {
                let hc = h.counts();
                if hc.adds > self.counts.adds
                    || hc.dels > self.counts.dels
                    || hc.mods > self.counts.mods
                    || hc.movs > self.counts.movs
                {
                    return Err(format!(
                        "{}: hunk {h_idx} counts ({hc:?}) exceed file counts ({:?})",
                        self.path, self.counts
                    ));
                }
                sum_hunk_counts.adds += hc.adds;
                sum_hunk_counts.dels += hc.dels;
                sum_hunk_counts.mods += hc.mods;
                sum_hunk_counts.movs += hc.movs;
            }
            if sum_hunk_counts.adds != self.counts.adds
                || sum_hunk_counts.dels != self.counts.dels
                || sum_hunk_counts.mods != self.counts.mods
                || sum_hunk_counts.movs != self.counts.movs
            {
                return Err(format!(
                    "{}: hunk counts sum ({sum_hunk_counts:?}) does not match file counts ({:?})",
                    self.path, self.counts
                ));
            }

            let ext = self.path.rsplit('.').next().unwrap_or("");
            let profile = diffuse_syntax::registry::lang_profile(ext);

            // Universal closed-economy thermodynamic invariant:
            // Across the entire file, the multiset of characters in
            // RunKind::MovedSrc must exactly equal the multiset of characters
            // in RunKind::MovedTgt. (Note: characters rather than token runs
            // are checked here because `OutputRun` on the new side is
            // partitioned into syntactic highlight tokens, whereas `MovedSrc`
            // on the old side is emitted whole).
            let mut file_moved_src = Vec::new();
            let mut file_moved_tgt = Vec::new();
            for h in &self.hunks {
                for r in &h.rows {
                    for run in &r.runs {
                        match run.kind {
                            RunKind::MovedSrc => file_moved_src
                                .extend(run.text.chars().filter(|c| !c.is_whitespace())),
                            RunKind::MovedTgt => file_moved_tgt
                                .extend(run.text.chars().filter(|c| !c.is_whitespace())),
                            _ => {}
                        }
                    }
                }
            }
            file_moved_src.sort_unstable();
            file_moved_tgt.sort_unstable();
            if file_moved_src != file_moved_tgt {
                return Err(format!(
                    "{}: file-level moved character conservation mismatch (∑ MovedSrc ≢ ∑ MovedTgt):\n  MovedSrc: {:?}\n  MovedTgt: {:?}",
                    self.path, file_moved_src, file_moved_tgt
                ));
            }

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
