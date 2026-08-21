//! Unified diff parsing and core AST data structures.
//!
//! Models the diff hierarchy: `DiffFile` -> `Hunk` -> `Row`.

use regex::Regex;
use std::{num::NonZero, sync::OnceLock};

/// A 1-based source line number.
pub type LineNum = NonZero<u32>;

/// A diff row kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    Add,
    Del,
    Ctx,
    Sep,
    File,
    Mod,
}

impl RowKind {
    pub fn reverse(&self) -> RowKind {
        match self {
            RowKind::Add => RowKind::Del,
            RowKind::Del => RowKind::Add,
            RowKind::Mod => RowKind::Mod,
            RowKind::Ctx => RowKind::Ctx,
            RowKind::Sep => RowKind::Sep,
            RowKind::File => RowKind::File,
        }
    }
}

/// A line deletion: (`line_num`, text).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct DelLine {
    pub num: LineNum,
    pub text: String,
}

impl DelLine {
    #[inline]
    pub fn new(num: LineNum, text: impl Into<String>) -> Self {
        Self {
            num,
            text: text.into(),
        }
    }
}

/// A line addition: (`line_num`, text, spans, optional donor `del_text`).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct AddLine {
    pub num: LineNum,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub del_text: Option<String>,
    #[serde(default, skip_serializing_if = "Spans::is_empty")]
    pub spans: Spans,
}

impl AddLine {
    #[inline]
    pub fn new(num: LineNum, text: impl Into<String>) -> Self {
        Self {
            num,
            text: text.into(),
            del_text: None,
            spans: Spans::default(),
        }
    }

    #[inline]
    pub fn with_spans(num: LineNum, text: impl Into<String>, spans: Spans) -> Self {
        Self {
            num,
            text: text.into(),
            del_text: None,
            spans,
        }
    }

    #[inline]
    pub fn with_donor(
        num: LineNum,
        text: impl Into<String>,
        del_text: impl Into<String>,
        spans: Spans,
    ) -> Self {
        Self {
            num,
            text: text.into(),
            del_text: Some(del_text.into()),
            spans,
        }
    }
}

/// A context line: (`old_pos`, `new_pos`, text).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CtxLine {
    pub old_pos: LineNum,
    pub new_pos: LineNum,
    pub text: String,
}

impl CtxLine {
    #[inline]
    pub fn new(old_pos: LineNum, new_pos: LineNum, text: impl Into<String>) -> Self {
        Self {
            old_pos,
            new_pos,
            text: text.into(),
        }
    }
}

/// A diff row.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Row {
    Ctx(CtxLine),
    Del(DelLine),
    Add(AddLine),
    Mod {
        old_line: DelLine,
        new_line: AddLine,
    },
    Mov {
        old_line: DelLine,
        new_line: AddLine,
    },
    Sep(String),
}

/// Change spans between old and new lines.
#[derive(Clone, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Spans {
    pub del: Vec<(usize, usize)>,
    pub add: Vec<(usize, usize)>,
}

impl Spans {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.del.is_empty() && self.add.is_empty()
    }

    #[inline]
    pub fn del_char_len(&self) -> usize {
        self.del.iter().map(|(s, e)| e.saturating_sub(*s)).sum()
    }

    #[inline]
    pub fn add_char_len(&self) -> usize {
        self.add.iter().map(|(s, e)| e.saturating_sub(*s)).sum()
    }

    pub fn reverse(&self) -> Self {
        Spans {
            del: self.add.clone(),
            add: self.del.clone(),
        }
    }
}

impl Row {
    #[inline]
    pub fn kind(&self) -> RowKind {
        match self {
            Row::Ctx(_) => RowKind::Ctx,
            Row::Del(_) => RowKind::Del,
            Row::Add(_) | Row::Mov { .. } => RowKind::Add,
            Row::Mod { .. } => RowKind::Mod,
            Row::Sep(_) => RowKind::Sep,
        }
    }

    #[inline]
    pub fn text(&self) -> &str {
        match self {
            Row::Ctx(ctx_line) => &ctx_line.text,
            Row::Del(del_line) => &del_line.text,
            Row::Add(add_line) => &add_line.text,
            Row::Mod { new_line, .. } | Row::Mov { new_line, .. } => &new_line.text,
            Row::Sep(text) => text.as_str(),
        }
    }

    #[inline]
    pub fn del_text(&self) -> Option<&str> {
        match self {
            Row::Ctx(ctx_line) => Some(&ctx_line.text),
            Row::Del(del_line) => Some(&del_line.text),
            Row::Add(add_line) => add_line.del_text.as_deref(),
            Row::Mod { old_line, .. } | Row::Mov { old_line, .. } => Some(&old_line.text),
            Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn old_line_num(&self) -> Option<LineNum> {
        match self {
            Row::Ctx(ctx_line) => Some(ctx_line.old_pos),
            Row::Del(del_line) => Some(del_line.num),
            Row::Mod { old_line, .. } | Row::Mov { old_line, .. } => Some(old_line.num),
            Row::Add(_) | Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn new_line_num(&self) -> Option<LineNum> {
        match self {
            Row::Ctx(ctx_line) => Some(ctx_line.new_pos),
            Row::Add(add_line) => Some(add_line.num),
            Row::Mod { new_line, .. } | Row::Mov { new_line, .. } => Some(new_line.num),
            Row::Del(_) | Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn old_line(&self) -> Option<u32> {
        self.old_line_num().map(std::num::NonZero::get)
    }

    #[inline]
    pub fn new_line(&self) -> Option<u32> {
        self.new_line_num().map(std::num::NonZero::get)
    }

    #[inline]
    pub fn spans(&self) -> Option<&Spans> {
        match self {
            Row::Add(add_line) if !add_line.spans.is_empty() => Some(&add_line.spans),
            Row::Mod { new_line, .. } => Some(&new_line.spans),
            Row::Mov { new_line, .. } if !new_line.spans.is_empty() => Some(&new_line.spans),
            _ => None,
        }
    }

    #[inline]
    pub fn is_moved(&self) -> bool {
        matches!(self, Row::Mov { .. })
    }

    pub fn reverse(&self) -> Self {
        match self {
            Row::Ctx(ctx_line) => Row::Ctx(CtxLine {
                old_pos: ctx_line.new_pos,
                new_pos: ctx_line.old_pos,
                text: ctx_line.text.clone(),
            }),
            Row::Del(del_line) => Row::Add(AddLine::new(del_line.num, del_line.text.clone())),
            Row::Add(add_line) => Row::Del(DelLine::new(
                add_line.num,
                add_line
                    .del_text
                    .as_deref()
                    .unwrap_or(&add_line.text)
                    .to_string(),
            )),
            Row::Mod { old_line, new_line } => Row::Mod {
                old_line: DelLine::new(new_line.num, new_line.text.clone()),
                new_line: AddLine::with_spans(
                    old_line.num,
                    old_line.text.clone(),
                    new_line.spans.reverse(),
                ),
            },
            Row::Mov { old_line, new_line } => Row::Mov {
                old_line: DelLine::new(new_line.num, new_line.text.clone()),
                new_line: AddLine::with_spans(
                    old_line.num,
                    old_line.text.clone(),
                    new_line.spans.reverse(),
                ),
            },
            Row::Sep(s) => Row::Sep(s.clone()),
        }
    }
}

/// A hunk: the `@@ -old +new @@` region of a diff.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub section: String,
    /// Base rows (add/del only, parsed from `git diff -U0`).
    pub rows: Vec<Row>,
    pub above_ctx: u32,
    pub below_ctx: u32,
}

impl Hunk {
    pub fn reverse(&self) -> Self {
        Hunk {
            old_start: self.new_start,
            old_count: self.new_count,
            new_start: self.old_start,
            new_count: self.old_count,
            section: self.section.clone(),
            rows: self.rows.iter().map(Row::reverse).collect(),
            above_ctx: self.above_ctx,
            below_ctx: self.below_ctx,
        }
    }
}

/// Row-derived new-side count: the *extent* from the hunk start to the last
/// new-side line, not a row count. Accurate for the @ header counts too.
pub fn add_count(h: &Hunk) -> u32 {
    let mut max_new = h.new_start;
    let mut has_new = false;
    for r in &h.rows {
        match r {
            Row::Add(add_line) => {
                let lines = add_line.text.matches('\n').count() as u32;
                max_new = max_new.max(add_line.num.get() + lines);
                has_new = true;
            }
            Row::Mod { new_line, .. } | Row::Mov { new_line, .. } => {
                let lines = new_line.text.matches('\n').count() as u32;
                max_new = max_new.max(new_line.num.get() + lines);
                has_new = true;
            }
            _ => {}
        }
    }
    if has_new {
        (max_new - h.new_start + 1).max(1)
    } else {
        0
    }
}

pub fn del_count(h: &Hunk) -> u32 {
    h.rows.iter().filter(|r| matches!(r, Row::Del(_))).count() as u32
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Untracked,
}

impl FileStatus {
    pub fn reverse(&self) -> FileStatus {
        match self {
            FileStatus::Added => FileStatus::Deleted,
            FileStatus::Deleted => FileStatus::Added,
            FileStatus::Modified => FileStatus::Modified,
            FileStatus::Renamed => FileStatus::Renamed,
            FileStatus::Untracked => FileStatus::Untracked,
        }
    }
}

/// A parsed diff file.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize)]
pub struct DiffFile {
    /// Repo-relative path of the NEW side (old path for deletions).
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
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
    pub fn reverse(&self) -> Self {
        DiffFile {
            path: self.old_path.clone().unwrap_or_else(|| self.path.clone()),
            old_path: if self.old_path.is_some() {
                Some(self.path.clone())
            } else {
                None
            },
            status: self.status.reverse(),
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

fn unquote_git_path(raw: &str) -> String {
    let mut s = raw;
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
fn hunk_path(sign: &str, line: &str) -> Option<String> {
    let p = &line[4..];
    if p == "/dev/null" {
        return None;
    }
    let stripped = p
        .strip_prefix("b/")
        .or_else(|| p.strip_prefix("a/"))
        .unwrap_or(p);
    let _ = sign;
    Some(unquote_git_path(stripped))
}

/// Parse a `git diff` into files, hunks, and rows. The `git diff -U0`
/// flavor: hunks carry only add/del rows; the context is synthesized later.
pub fn parse_diff(raw: &str) -> Vec<DiffFile> {
    static HUNK_RE: OnceLock<Regex> = OnceLock::new();
    let hunk_re = HUNK_RE
        .get_or_init(|| Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$").unwrap());
    let mut files: Vec<DiffFile> = Vec::new();
    let mut cur: Option<DiffFile> = None;
    let mut oi: u32 = 1;
    let mut ni: u32 = 1;

    for line in raw.split('\n') {
        if line.starts_with("diff --git ") {
            if let Some(c) = cur.take() {
                files.push(c);
            }
            cur = Some(DiffFile {
                path: String::new(),
                old_path: None,
                status: FileStatus::Modified,
                new_line_count: 0,
                old_line_count: 0,
                new_content: Vec::new(),
                old_content: Vec::new(),
                hunks: Vec::new(),
                raw_hunks: Vec::new(),
                no_content: false,
            });
        } else if cur.is_none() {
            continue;
        } else if line.starts_with("new file mode") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Added;
            }
        } else if line.starts_with("untracked file mode") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Untracked;
            }
        } else if line.starts_with("deleted file mode") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Deleted;
            }
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            if let Some(c) = cur.as_mut() {
                c.old_path = Some(unquote_git_path(rest).replacen("a/", "", 1));
                c.status = FileStatus::Renamed;
            }
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            if let Some(c) = cur.as_mut() {
                c.path = unquote_git_path(rest);
            }
        } else if line.starts_with("GIT binary patch") || line.starts_with("Binary files ") {
            if let Some(c) = cur.as_mut() {
                c.hunks.clear(); // binary: nothing to review
                c.raw_hunks.clear();
            }
        } else if line.starts_with("+++ ") || line.starts_with("--- ") {
            let is_new = line.starts_with("+++ ");
            let p = hunk_path(if is_new { "+++" } else { "---" }, line);
            if is_new {
                if let (Some(c), Some(p)) = (cur.as_mut(), p) {
                    c.path = p;
                }
            } else if let (Some(c), Some(p)) = (cur.as_mut(), p) {
                // Deleted files have `+++ /dev/null` (no new path) — the old
                // path is the only path; renames carry it via `rename to`.
                if c.old_path.is_none() {
                    c.old_path = Some(p.clone());
                }
                if c.path.is_empty() {
                    c.path = p;
                }
            }
        } else if line.starts_with("@@ ") {
            if let Some(caps) = hunk_re.captures(line) {
                let old_start: u32 = caps[1].parse().unwrap_or(1);
                let old_count: u32 = caps.get(2).map_or(1, |m| m.as_str().parse().unwrap_or(1));
                let new_start: u32 = caps[3].parse().unwrap_or(1);
                let new_count: u32 = caps.get(4).map_or(1, |m| m.as_str().parse().unwrap_or(1));
                oi = old_start;
                ni = new_start;
                if let Some(c) = cur.as_mut() {
                    let section = caps.get(5).map_or("", |m| m.as_str()).trim().to_string();
                    c.hunks.push(Hunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        section: section.clone(),
                        rows: Vec::new(),
                        above_ctx: 0,
                        below_ctx: 0,
                    });
                    c.raw_hunks.push(Hunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        section,
                        rows: Vec::new(),
                        above_ctx: 0,
                        below_ctx: 0,
                    });
                }
            }
        } else if let Some(c) = cur.as_mut() {
            // Row lines (`+`, `-`, ` `) append to the hunk they belong to —
            // the same hunk object `cur.hunks.last_mut()` that was pushed at
            // the `@@` header (the TS holds one reference; Rust appends in
            // place).
            let hunk = match c.hunks.last_mut() {
                Some(h) => h,
                None => continue,
            };
            let raw_hunk = match c.raw_hunks.last_mut() {
                Some(h) => h,
                None => continue,
            };
            if let Some(rest) = line.strip_prefix('+') {
                let num = LineNum::new(ni).unwrap_or(LineNum::MIN);
                let row = Row::Add(AddLine::new(num, rest));
                hunk.rows.push(row.clone());
                raw_hunk.rows.push(row);
                ni += 1;
            } else if let Some(rest) = line.strip_prefix('-') {
                let num = LineNum::new(oi).unwrap_or(LineNum::MIN);
                let row = Row::Del(DelLine::new(num, rest));
                hunk.rows.push(row.clone());
                raw_hunk.rows.push(row);
                oi += 1;
            } else if let Some(rest) = line.strip_prefix(' ') {
                let old_num = LineNum::new(oi).unwrap_or(LineNum::MIN);
                let new_num = LineNum::new(ni).unwrap_or(LineNum::MIN);
                raw_hunk
                    .rows
                    .push(Row::Ctx(CtxLine::new(old_num, new_num, rest)));
                oi += 1;
                ni += 1;
            }
        }
    }
    if let Some(c) = cur.take() {
        files.push(c);
    }
    files.retain(|f| !f.path.is_empty() && !f.hunks.is_empty());
    files
}

#[cfg(test)]
mod tests;
