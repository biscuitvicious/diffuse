//! Unified diff parsing and core AST data structures.
//!
//! Models the diff hierarchy: `DiffFile` -> `Hunk` -> `Row`.

use std::num::NonZero;

use serde::{Deserialize, Serialize};

/// A 1-based source line number.
pub type LineNum = NonZero<u32>;

/// A diff row kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    Add,
    Del,
    Ctx,
    File,
    Mod,
    Mov,
    #[serde(skip_serializing)]
    Sep,
}

impl RowKind {
    pub fn reverse(&self) -> RowKind {
        match self {
            RowKind::Add => RowKind::Del,
            RowKind::Del => RowKind::Add,
            RowKind::Mod => RowKind::Mod,
            RowKind::Mov => RowKind::Mov,
            RowKind::Ctx | RowKind::Sep => RowKind::Ctx,
            RowKind::File => RowKind::File,
        }
    }
}

/// A line deletion: (`line_num`, text, moved).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
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
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
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
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
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
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Row {
    Ctx(CtxLine),
    Del(DelLine),
    Add(AddLine),
    Mod {
        old_line: DelLine,
        new_line: AddLine,
    },
    MovSrc(DelLine),
    MovTgt(AddLine),
    #[serde(skip_serializing)]
    Sep(String),
}

/// Change spans between old and new lines.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
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
            Row::Ctx(_) | Row::Sep(_) => RowKind::Ctx,
            Row::Del(_) => RowKind::Del,
            Row::Add(_) => RowKind::Add,
            Row::Mod { .. } => RowKind::Mod,
            Row::MovSrc(_) | Row::MovTgt(_) => RowKind::Mov,
        }
    }

    #[inline]
    pub fn text(&self) -> &str {
        match self {
            Row::Ctx(ctx_line) => &ctx_line.text,
            Row::Del(del_line) | Row::MovSrc(del_line) => &del_line.text,
            Row::Add(add_line) | Row::MovTgt(add_line) => &add_line.text,
            Row::Mod { new_line, .. } => &new_line.text,
            Row::Sep(s) => s.as_str(),
        }
    }

    #[inline]
    pub fn del_text(&self) -> Option<&str> {
        match self {
            Row::Ctx(ctx_line) => Some(&ctx_line.text),
            Row::Del(del_line) | Row::MovSrc(del_line) => Some(&del_line.text),
            Row::Add(add_line) | Row::MovTgt(add_line) => add_line.del_text.as_deref(),
            Row::Mod { old_line, .. } => Some(&old_line.text),
            Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn old_line_num(&self) -> Option<LineNum> {
        match self {
            Row::Ctx(ctx_line) => Some(ctx_line.old_pos),
            Row::Del(del_line) | Row::MovSrc(del_line) => Some(del_line.num),
            Row::Mod { old_line, .. } => Some(old_line.num),
            Row::Add(_) | Row::MovTgt(_) | Row::Sep(_) => None,
        }
    }

    #[inline]
    pub fn new_line_num(&self) -> Option<LineNum> {
        match self {
            Row::Ctx(ctx_line) => Some(ctx_line.new_pos),
            Row::Add(add_line) | Row::MovTgt(add_line) => Some(add_line.num),
            Row::Mod { new_line, .. } => Some(new_line.num),
            Row::Del(_) | Row::MovSrc(_) | Row::Sep(_) => None,
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
            Row::Add(add_line) | Row::MovTgt(add_line) if !add_line.spans.is_empty() => {
                Some(&add_line.spans)
            }
            Row::Mod { new_line, .. } if !new_line.spans.is_empty() => Some(&new_line.spans),
            _ => None,
        }
    }

    #[inline]
    pub fn is_moved(&self) -> bool {
        matches!(self, Row::MovSrc(_) | Row::MovTgt(_))
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
            Row::MovSrc(del_line) => Row::MovTgt(AddLine::new(del_line.num, del_line.text.clone())),
            Row::MovTgt(add_line) => Row::MovSrc(DelLine::new(
                add_line.num,
                add_line
                    .del_text
                    .as_deref()
                    .unwrap_or(&add_line.text)
                    .to_string(),
            )),
            Row::Sep(s) => Row::Sep(s.clone()),
        }
    }
}

/// A hunk: the `@@ -old +new @@` region of a diff.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
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
            Row::Add(add_line) | Row::MovTgt(add_line) => {
                let lines = add_line.text.matches('\n').count() as u32;
                max_new = max_new.max(add_line.num.get() + lines);
                has_new = true;
            }
            Row::Mod { new_line, .. } => {
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Untracked,
    ModeChanged,
}

impl FileStatus {
    pub fn reverse(&self) -> FileStatus {
        match self {
            FileStatus::Added => FileStatus::Deleted,
            FileStatus::Deleted => FileStatus::Added,
            FileStatus::Modified => FileStatus::Modified,
            FileStatus::Renamed => FileStatus::Renamed,
            FileStatus::Untracked => FileStatus::Untracked,
            FileStatus::ModeChanged => FileStatus::ModeChanged,
        }
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

fn unquote_git_path(raw: &str) -> String {
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
fn hunk_path(sign: &str, line: &str) -> Option<String> {
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

fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32, &str)> {
    let rest = line.strip_prefix("@@ -")?;
    let space = rest.find(' ')?;
    let old_part = &rest[..space];
    let rest_after_space = &rest[space + 1..];
    let new_part_full = rest_after_space.strip_prefix('+')?;
    let at_at = new_part_full.find(" @@")?;
    let new_part = &new_part_full[..at_at];
    let section = &new_part_full[at_at + 3..];

    let (old_start, old_count) = if let Some(comma) = old_part.find(',') {
        (
            old_part[..comma].parse().ok()?,
            old_part[comma + 1..].parse().unwrap_or(1),
        )
    } else {
        (old_part.parse().ok()?, 1)
    };

    let (new_start, new_count) = if let Some(comma) = new_part.find(',') {
        (
            new_part[..comma].parse().ok()?,
            new_part[comma + 1..].parse().unwrap_or(1),
        )
    } else {
        (new_part.parse().ok()?, 1)
    };

    Some((old_start, old_count, new_start, new_count, section))
}

fn parse_diff_git_paths(rest: &str) -> (Option<String>, Option<String>) {
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

/// Parse a `git diff` into files, hunks, and rows. The `git diff -U0`
/// flavor: hunks carry only add/del rows; the context is synthesized later.
#[tracing::instrument(level = "trace", ret, skip(raw), fields(diff_len = raw.len()))]
pub fn parse_diff(raw: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut cur: Option<DiffFile> = None;
    let mut oi: u32 = 1;
    let mut ni: u32 = 1;

    for line in raw.split('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(c) = cur.take() {
                files.push(c);
            }
            let (_p_a, p_b) = parse_diff_git_paths(rest);
            cur = Some(DiffFile {
                path: p_b.unwrap_or_default(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: FileStatus::Modified,
                is_binary: false,
                old_bytes: None,
                new_bytes: None,
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
        } else if let Some(mode) = line.strip_prefix("old mode ") {
            if let Some(c) = cur.as_mut() {
                c.old_mode = Some(mode.trim().to_string());
                if c.status == FileStatus::Modified {
                    c.status = FileStatus::ModeChanged;
                }
            }
        } else if let Some(mode) = line.strip_prefix("new mode ") {
            if let Some(c) = cur.as_mut() {
                c.new_mode = Some(mode.trim().to_string());
                if c.status == FileStatus::Modified {
                    c.status = FileStatus::ModeChanged;
                }
            }
        } else if let Some(rest) = line.strip_prefix("mode change ") {
            if let Some(c) = cur.as_mut() {
                if let Some((old_m, new_m)) = rest.split_once("->") {
                    c.old_mode = Some(old_m.trim().to_string());
                    c.new_mode = Some(new_m.trim().to_string());
                } else {
                    c.new_mode = Some(rest.trim().to_string());
                }
                if c.status == FileStatus::Modified {
                    c.status = FileStatus::ModeChanged;
                }
            }
        } else if line.starts_with("new file mode ") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Added;
            }
        } else if line.starts_with("deleted file mode ") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Deleted;
            }
        } else if line.starts_with("untracked file mode") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Untracked;
            }
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            if let Some(c) = cur.as_mut() {
                c.old_path = Some(unquote_git_path(rest));
                c.status = FileStatus::Renamed;
            }
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            if let Some(c) = cur.as_mut() {
                c.path = unquote_git_path(rest);
                c.status = FileStatus::Renamed;
            }
        } else if line.starts_with("GIT binary patch")
            || line.starts_with("Binary files ")
            || line == "Binary files differ"
        {
            if let Some(c) = cur.as_mut() {
                c.is_binary = true;
                if line.contains(" /dev/null and ") {
                    c.status = FileStatus::Added;
                } else if line.contains(" and /dev/null differ") {
                    c.status = FileStatus::Deleted;
                }
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
                if c.path.is_empty() {
                    c.path = p;
                }
            }
        } else if line.starts_with("@@ ") {
            if let Some((old_start, old_count, new_start, new_count, section_str)) =
                parse_hunk_header(line)
            {
                oi = old_start;
                ni = new_start;
                if let Some(c) = cur.as_mut() {
                    let section = section_str.trim().to_string();
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
                let row = Row::Ctx(CtxLine::new(old_num, new_num, rest));
                hunk.rows.push(row.clone());
                raw_hunk.rows.push(row);
                oi += 1;
                ni += 1;
            }
        }
    }
    if let Some(c) = cur.take() {
        files.push(c);
    }
    files.retain(|f| {
        !f.path.is_empty()
            && (!f.hunks.is_empty()
                || f.status == FileStatus::Renamed
                || f.status == FileStatus::Untracked
                || f.status == FileStatus::ModeChanged
                || f.is_binary)
    });

    crate::ensure!(
        files.iter().all(|f| {
            !f.path.is_empty()
                && (!f.hunks.is_empty()
                    || f.status == FileStatus::Renamed
                    || f.status == FileStatus::Untracked
                    || f.status == FileStatus::ModeChanged
                    || f.is_binary)
        }),
        "All parsed DiffFiles must have non-empty paths and non-empty hunks or be renamed/untracked/mode changed/binary"
    );

    files
}

#[cfg(test)]
mod tests;
