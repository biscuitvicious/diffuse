//! One-for-one port of `review.ts`'s `parseDiff` machinery (PLAN.md §3):
//! `RowKind`/`Row`/`Hunk`/`DiffFile`, `unquoteGitPath`, `hunkPath`,
//! `parseDiff`, `addCount`, `delCount`. The comments carry the *why* —
//! ported verbatim from the TypeScript.

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

/// A diff row. Fields mirror `review.ts`'s `Row` interface.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize)]
pub struct Row {
    pub kind: RowKind,
    pub text: String,
    /// Line in the NEW file (None for pure-deletion rows).
    pub new_line: Option<u32>,
    /// Line in the OLD file (None for pure-addition rows).
    pub old_line: Option<u32>,
    /// Cached word-level change spans (char offsets into `text`).
    pub spans: Option<Spans>,
    /// Exact-content match on the other side: the line moved, it did not change.
    pub moved: bool,
    /// The paired old text (mod rows) — for the struck-through removed words.
    pub del_text: Option<String>,
}

/// Word-level change spans: char-offset ranges into the del/add text.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    pub fn new(
        kind: RowKind,
        text: impl Into<String>,
        new_line: Option<u32>,
        old_line: Option<u32>,
    ) -> Self {
        Row {
            kind,
            text: text.into(),
            new_line,
            old_line,
            spans: None,
            moved: false,
            del_text: None,
        }
    }

    pub fn reverse(&self) -> Self {
        Row {
            kind: self.kind.reverse(),
            text: self.del_text.clone().unwrap_or_else(|| self.text.clone()),
            new_line: self.old_line,
            old_line: self.new_line,
            spans: self.spans.as_ref().map(|s| s.reverse()),
            moved: self.moved,
            del_text: if self.kind == RowKind::Mod || self.del_text.is_some() {
                Some(self.text.clone())
            } else {
                None
            },
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
            rows: self.rows.iter().map(|r| r.reverse()).collect(),
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
    let mut scan = |r: &Row| {
        if r.kind == RowKind::Add || r.kind == RowKind::Mod {
            let lines = r.text.matches('\n').count() as u32;
            max_new = max_new.max(r.new_line.unwrap_or(0) + lines);
            has_new = true;
        }
    };
    for r in &h.rows {
        scan(r);
    }
    if has_new {
        (max_new - h.new_start + 1).max(1)
    } else {
        0
    }
}

pub fn del_count(h: &Hunk) -> u32 {
    h.rows.iter().filter(|r| r.kind == RowKind::Del).count() as u32
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
            hunks: self.hunks.iter().map(|h| h.reverse()).collect(),
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
    static HUNK_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let hunk_re = HUNK_RE.get_or_init(|| {
        regex::Regex::new(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$").unwrap()
    });
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
                no_content: false,
            });
        } else if cur.is_none() {
            continue;
        } else if line.starts_with("new file mode") {
            if let Some(c) = cur.as_mut() {
                c.status = FileStatus::Added;
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
                    c.hunks.push(Hunk {
                        old_start,
                        old_count,
                        new_start,
                        new_count,
                        section: caps.get(5).map_or("", |m| m.as_str()).trim().to_string(),
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
            if let Some(rest) = line.strip_prefix('+') {
                hunk.rows.push(Row::new(RowKind::Add, rest, Some(ni), None));
                ni += 1;
            } else if let Some(rest) = line.strip_prefix('-') {
                hunk.rows.push(Row::new(RowKind::Del, rest, None, Some(oi)));
                oi += 1;
            } else if line.starts_with(' ') {
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
