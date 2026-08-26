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
}

/// A single file in the wire contract output, carrying its reconstructed
/// content, hunks, pre-classified syntax runs, and attached review notes.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFile {
    pub path: String,
    pub status: String,
    pub new_line_count: usize,
    pub old_line_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_content: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_runs: Vec<Vec<OutputRun>>,
    /// Pre-assembled row sequence for default context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
            path: self.path.clone(),
            status: status.to_string(),
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
            _ => FileStatus::Modified,
        };
        DiffFile {
            path: self.path.clone(),
            old_path: None,
            status,
            new_line_count: self.new_line_count,
            old_line_count: self.old_line_count,
            new_content: self.new_content.clone(),
            old_content: self.old_content.clone(),
            hunks: self.hunks.iter().map(OutputHunk::reconstruct).collect(),
            raw_hunks: self.raw_hunks.iter().map(OutputHunk::reconstruct).collect(),
            no_content: self.new_content.is_empty() && self.old_content.is_empty(),
        }
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
    pub moved: bool,
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
            RowKind::Add if r.moved => {
                let old_num = r.old_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let new_num = r.new_line.and_then(LineNum::new).unwrap_or(LineNum::MIN);
                let del_text = r.del_text.as_deref().unwrap_or(&r.text);
                Row::Mov {
                    old_line: DelLine::new(old_num, del_text),
                    new_line: AddLine::with_spans(new_num, &r.text, r.spans.clone()),
                }
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
            moved: self.moved,
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
    pub text: String,
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
