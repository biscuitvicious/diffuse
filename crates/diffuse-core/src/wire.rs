//! Diffuse Wire Contract Data Models & Serialization Schema.
//!
//! Defines the structured JSON wire contract output emitted by the engine
//! and consumed by external tooling, the CLI (`--json`), and the pi review
//! extension. The schema is frozen per PLAN.md §2.

use serde::{Deserialize, Serialize};

use crate::{
    counts::FileCounts,
    merge::RunKind,
    notes::Side as NoteSide,
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

fn is_zero_usize(val: &usize) -> bool {
    *val == 0
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

/// A single file in the wire contract output, carrying its reconstructed
/// content, hunks, pre-classified syntax runs, and attached review notes.
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
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub bytes: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub old_bytes: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub new_bytes: usize,
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
}

impl OutputFile {
    pub fn reverse(&self) -> Self {
        let status = match self.status.as_str() {
            "added" => "deleted",
            "deleted" => "added",
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
            bytes: self.bytes,
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
        }
    }

    pub fn reconstruct(&self) -> DiffFile {
        let status = match self.status.as_str() {
            "added" => FileStatus::Added,
            "deleted" => FileStatus::Deleted,
            "renamed" => FileStatus::Renamed,
            "untracked" => FileStatus::Untracked,
            "mode changed" => FileStatus::ModeChanged,
            _ => FileStatus::Modified,
        };
        DiffFile {
            path: self.path.clone(),
            old_path: self.old_path.clone(),
            old_mode: self.old_mode.clone(),
            new_mode: self.new_mode.clone(),
            status,
            is_binary: self.is_binary,
            bytes: self.bytes,
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

    pub fn old_byte_size(&self) -> usize {
        if self.old_bytes > 0 {
            self.old_bytes
        } else if !self.old_content.is_empty() {
            self.old_content
                .iter()
                .map(|s| s.len() + 1)
                .sum::<usize>()
                .saturating_sub(1)
        } else {
            0
        }
    }

    pub fn new_byte_size(&self) -> usize {
        if self.new_bytes > 0 {
            self.new_bytes
        } else if !self.new_content.is_empty() {
            self.new_content
                .iter()
                .map(|s| s.len() + 1)
                .sum::<usize>()
                .saturating_sub(1)
        } else {
            0
        }
    }

    pub fn byte_size(&self) -> usize {
        if self.status == "deleted" {
            self.old_byte_size()
        } else {
            self.new_byte_size()
        }
    }

    /// Formats the file size, displaying `from_size -> to_size` if the
    /// formatted values differ, or a single size if unchanged (or if
    /// added/deleted/untracked).
    pub fn display_size(&self) -> String {
        let new_fmt = format_byte_size(self.new_byte_size());
        let old_fmt = format_byte_size(self.old_byte_size());
        if self.status == "added" || self.status == "untracked" {
            new_fmt
        } else if self.status == "deleted" {
            old_fmt
        } else if old_fmt != new_fmt {
            format!("{old_fmt} -> {new_fmt}")
        } else {
            new_fmt
        }
    }

    /// Format status description for banners, markdown summary, and pretty
    /// printing, incorporating mode changes and byte size (e.g. "mode
    /// 100644 -> 100755, 1.2kB", "1kB -> 2kB", "1.2kB", etc.).
    pub fn display_status(&self) -> String {
        let size_str = self.display_size();
        let mode_suffix = match (&self.old_mode, &self.new_mode) {
            (Some(old_m), Some(new_m)) if old_m != new_m => {
                Some(format!("mode {old_m} -> {new_m}"))
            }
            _ => None,
        };

        if self.is_binary {
            return match (self.status.as_str(), mode_suffix) {
                ("renamed", Some(mode_str)) => format!("binary, {mode_str}, {size_str}"),
                ("renamed", None) => format!("binary, {size_str}"),
                ("added", Some(mode_str)) => format!("added, binary, {mode_str}, {size_str}"),
                ("added", None) => format!("added, binary, {size_str}"),
                ("deleted", Some(mode_str)) => format!("deleted, binary, {mode_str}, {size_str}"),
                ("deleted", None) => format!("deleted, binary, {size_str}"),
                (_, Some(mode_str)) => format!("binary, {mode_str}, {size_str}"),
                (_, None) => format!("binary, {size_str}"),
            };
        }

        match (self.status.as_str(), mode_suffix) {
            ("renamed", Some(mode_str)) => format!("{mode_str}, {size_str}"),
            ("renamed", None) => size_str,
            ("added", Some(mode_str)) => format!("added, {mode_str}, {size_str}"),
            ("added", None) => format!("added, {size_str}"),
            ("deleted", Some(mode_str)) => format!("deleted, {mode_str}, {size_str}"),
            ("deleted", None) => format!("deleted, {size_str}"),
            ("mode changed" | "modified", Some(mode_str)) => {
                if self.counts.adds == 0 && self.counts.dels == 0 && self.counts.mods == 0 {
                    format!("{mode_str}, {size_str}")
                } else {
                    format!("modified, {mode_str}, {size_str}")
                }
            }
            ("modified", None) => size_str,
            (s, Some(mode_str)) => format!("{s}, {mode_str}, {size_str}"),
            (s, None) => format!("{s}, {size_str}"),
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
