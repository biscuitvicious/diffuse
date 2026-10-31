//! Pure in-process unified diff generator with trim-equality semantics.

use diffuse_parse::{AddLine, CtxLine, DelLine, Hunk, Row};
use gix::{
    bstr::ByteSlice,
    diff::blob::{
        Algorithm, InternedInput, TokenSource, UnifiedDiff, diff_with_slider_heuristics,
        unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader},
    },
};
use std::hash::{Hash, Hasher};

/// Whitespace comparison semantics for line diffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhitespaceMode {
    /// Exact line equality including all whitespace and indentation.
    Exact,
    /// Canonical diffuse trim-equality: compare `line.trim()` for leading and
    /// trailing whitespace equality, while preserving internal token
    /// spacing.
    #[default]
    TrimEquality,
}

/// A line wrapper that hashes and compares lines according to a
/// [`WhitespaceMode`].
#[derive(Clone, Copy, Debug)]
pub struct DiffLine<'a> {
    pub bytes: &'a [u8],
    pub mode: WhitespaceMode,
}

impl<'a> AsRef<[u8]> for DiffLine<'a> {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.bytes
    }
}

impl<'a> PartialEq for DiffLine<'a> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        match self.mode {
            WhitespaceMode::Exact => self.bytes == other.bytes,
            WhitespaceMode::TrimEquality => self.bytes.trim() == other.bytes.trim(),
        }
    }
}

impl<'a> Eq for DiffLine<'a> {}

impl<'a> Hash for DiffLine<'a> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self.mode {
            WhitespaceMode::Exact => self.bytes.hash(state),
            WhitespaceMode::TrimEquality => self.bytes.trim().hash(state),
        }
    }
}

/// Tokenizer yielding [`DiffLine`] items from byte slices.
#[derive(Clone, Copy)]
pub struct DiffLines<'a> {
    bytes: &'a [u8],
    mode: WhitespaceMode,
}

impl<'a> DiffLines<'a> {
    pub fn new(bytes: &'a [u8], mode: WhitespaceMode) -> Self {
        Self { bytes, mode }
    }
}

impl<'a> Iterator for DiffLines<'a> {
    type Item = DiffLine<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.bytes.is_empty() {
            return None;
        }
        let line_len = match self.bytes.find_byte(b'\n') {
            Some(pos) => pos + 1,
            None => self.bytes.len(),
        };
        let (line, rest) = self.bytes.split_at(line_len);
        self.bytes = rest;
        Some(DiffLine {
            bytes: line,
            mode: self.mode,
        })
    }
}

impl<'a> TokenSource for DiffLines<'a> {
    type Token = DiffLine<'a>;
    type Tokenizer = Self;

    #[inline]
    fn tokenize(&self) -> Self::Tokenizer {
        *self
    }

    #[inline]
    fn estimate_tokens(&self) -> u32 {
        self.bytes.lines().count() as u32
    }
}

/// Split a byte slice into line slices preserving line endings.
pub fn split_lines(mut bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    while !bytes.is_empty() {
        let line_len = match bytes.find_byte(b'\n') {
            Some(pos) => pos + 1,
            None => bytes.len(),
        };
        let (line, rest) = bytes.split_at(line_len);
        lines.push(line);
        bytes = rest;
    }
    lines
}

/// Hunk consumer accumulating structured [`diffuse_parse::Hunk`] instances
/// and unified diff text.
pub struct AstHunkConsumer<'a> {
    pub hunks: Vec<Hunk>,
    pub patch: String,
    old_lines: &'a [&'a [u8]],
    new_lines: &'a [&'a [u8]],
}

impl<'a> AstHunkConsumer<'a> {
    pub fn new(old_lines: &'a [&'a [u8]], new_lines: &'a [&'a [u8]]) -> Self {
        Self {
            hunks: Vec::new(),
            patch: String::new(),
            old_lines,
            new_lines,
        }
    }
}

impl<'a> ConsumeHunk for AstHunkConsumer<'a> {
    type Out = (Vec<Hunk>, String);

    fn consume_hunk(
        &mut self,
        header: HunkHeader,
        lines: &[(DiffLineKind, &[u8])],
    ) -> std::io::Result<()> {
        let mut hunk = Hunk {
            old_start: header.before_hunk_start,
            old_count: header.before_hunk_len,
            new_start: header.after_hunk_start,
            new_count: header.after_hunk_len,
            section: String::new(),
            rows: Vec::with_capacity(lines.len()),
            above_ctx: 0,
            below_ctx: 0,
        };

        use std::fmt::Write;
        let _ = writeln!(
            self.patch,
            "@@ -{},{} +{},{} @@",
            header.before_hunk_start,
            header.before_hunk_len,
            header.after_hunk_start,
            header.after_hunk_len
        );

        let mut old_pos = header.before_hunk_start;
        let mut new_pos = header.after_hunk_start;

        for &(kind, fallback_bytes) in lines {
            let line_bytes = match kind {
                DiffLineKind::Context => {
                    if new_pos > 0 && (new_pos as usize) <= self.new_lines.len() {
                        self.new_lines[(new_pos - 1) as usize]
                    } else if old_pos > 0 && (old_pos as usize) <= self.old_lines.len() {
                        self.old_lines[(old_pos - 1) as usize]
                    } else {
                        fallback_bytes
                    }
                }
                DiffLineKind::Remove => {
                    if old_pos > 0 && (old_pos as usize) <= self.old_lines.len() {
                        self.old_lines[(old_pos - 1) as usize]
                    } else {
                        fallback_bytes
                    }
                }
                DiffLineKind::Add => {
                    if new_pos > 0 && (new_pos as usize) <= self.new_lines.len() {
                        self.new_lines[(new_pos - 1) as usize]
                    } else {
                        fallback_bytes
                    }
                }
            };

            let text = line_bytes.to_str_lossy();
            let text_trimmed = text.trim_end_matches(['\r', '\n']);

            match kind {
                DiffLineKind::Context => {
                    self.patch.push(' ');
                    self.patch.push_str(&text);
                    if !text.ends_with('\n') {
                        self.patch.push('\n');
                    }
                    hunk.rows.push(Row::Ctx(CtxLine::new(
                        old_pos,
                        new_pos,
                        text_trimmed.to_string(),
                    )));
                    old_pos += 1;
                    new_pos += 1;
                }
                DiffLineKind::Remove => {
                    self.patch.push('-');
                    self.patch.push_str(&text);
                    if !text.ends_with('\n') {
                        self.patch.push('\n');
                    }
                    hunk.rows
                        .push(Row::Del(DelLine::new(old_pos, text_trimmed.to_string())));
                    old_pos += 1;
                }
                DiffLineKind::Add => {
                    self.patch.push('+');
                    self.patch.push_str(&text);
                    if !text.ends_with('\n') {
                        self.patch.push('\n');
                    }
                    hunk.rows
                        .push(Row::Add(AddLine::new(new_pos, text_trimmed.to_string())));
                    new_pos += 1;
                }
            }
        }

        self.hunks.push(hunk);
        Ok(())
    }

    fn finish(self) -> Self::Out {
        (self.hunks, self.patch)
    }
}

/// Compute unified diff hunks and patch text between two byte buffers.
pub fn diff_buffers(
    old: &[u8],
    new: &[u8],
    context: u32,
    mode: WhitespaceMode,
) -> (Vec<Hunk>, String) {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let old_source = DiffLines::new(old, mode);
    let new_source = DiffLines::new(new, mode);
    let input = InternedInput::new(old_source, new_source);
    let diff = diff_with_slider_heuristics(Algorithm::Histogram, &input);
    let consumer = AstHunkConsumer::new(&old_lines, &new_lines);
    let unified = UnifiedDiff::new(&diff, &input, consumer, ContextSize::symmetrical(context));
    unified.consume().unwrap_or_default()
}

/// Convenience wrapper for diffing string slices.
pub fn diff_text(old: &str, new: &str, context: u32, mode: WhitespaceMode) -> (Vec<Hunk>, String) {
    diff_buffers(old.as_bytes(), new.as_bytes(), context, mode)
}

/// Parameters for generating a unified diff and AST for a single file.
#[derive(Debug, Clone)]
pub struct FileDiffParams<'a> {
    pub path: &'a str,
    pub old_path: Option<&'a str>,
    pub old_bytes: Option<&'a [u8]>,
    pub new_bytes: Option<&'a [u8]>,
    pub status: diffuse_parse::FileStatus,
    pub old_mode: Option<&'a str>,
    pub new_mode: Option<&'a str>,
    pub is_binary: bool,
    pub context: u32,
    pub mode: WhitespaceMode,
}

/// The result of diffing a single file: parsed AST and raw patch text.
#[derive(Debug, Clone)]
pub struct FileDiffResult {
    pub file: diffuse_parse::DiffFile,
    pub patch: String,
}

/// Generate structured [`diffuse_parse::DiffFile`] AST and unified diff patch
/// text for a single file.
pub fn generate_file_diff(params: FileDiffParams<'_>) -> FileDiffResult {
    use diffuse_parse::{DiffFile, FileStatus};

    let effective_old = params.old_path.unwrap_or(params.path);
    let mut patch = format!("diff --git a/{effective_old} b/{}\n", params.path);

    match params.status {
        FileStatus::Deleted => {
            patch.push_str(&format!(
                "deleted file mode {}\n",
                params.old_mode.unwrap_or("100644")
            ));
            patch.push_str(&format!("--- a/{effective_old}\n+++ /dev/null\n"));
        }
        FileStatus::Added => {
            patch.push_str(&format!(
                "new file mode {}\n",
                params.new_mode.unwrap_or("100644")
            ));
            patch.push_str(&format!("--- /dev/null\n+++ b/{}\n", params.path));
        }
        FileStatus::Untracked => {
            patch.push_str(&format!(
                "untracked file mode {}\n",
                params.new_mode.unwrap_or("100644")
            ));
            patch.push_str(&format!("--- /dev/null\n+++ b/{}\n", params.path));
        }
        FileStatus::Renamed => {
            patch.push_str(&format!(
                "rename from {effective_old}\nrename to {}\n",
                params.path
            ));
            patch.push_str(&format!("--- a/{effective_old}\n+++ b/{}\n", params.path));
        }
        FileStatus::Modified | FileStatus::ModeChanged | FileStatus::Unmodified => {
            if let (Some(old_m), Some(new_m)) = (params.old_mode, params.new_mode)
                && old_m != new_m
            {
                patch.push_str(&format!("old mode {old_m}\nnew mode {new_m}\n"));
            }
            patch.push_str(&format!("--- a/{effective_old}\n+++ b/{}\n", params.path));
        }
    }

    if params.is_binary {
        patch.push_str(&format!(
            "Binary files a/{effective_old} and b/{} differ\n",
            params.path
        ));
        let mut file = DiffFile::new(params.path);
        file.old_path = params.old_path.map(ToString::to_string);
        file.status = params.status;
        file.old_mode = params.old_mode.map(ToString::to_string);
        file.new_mode = params.new_mode.map(ToString::to_string);
        file.is_binary = true;
        file.has_file_headers = true;
        file.old_bytes = params.old_bytes.map(|b| b.len());
        file.new_bytes = params.new_bytes.map(|b| b.len());
        return FileDiffResult { file, patch };
    }

    let (old_buf, new_buf): (&[u8], &[u8]) = match params.status {
        FileStatus::Added | FileStatus::Untracked => (b"", params.new_bytes.unwrap_or(b"")),
        FileStatus::Deleted => (params.old_bytes.unwrap_or(b""), b""),
        _ => (
            params.old_bytes.unwrap_or(b""),
            params.new_bytes.unwrap_or(b""),
        ),
    };

    let (hunks, hunk_patch) = diff_buffers(old_buf, new_buf, params.context, params.mode);
    patch.push_str(&hunk_patch);

    let mut file = DiffFile::new(params.path);
    file.old_path = params.old_path.map(ToString::to_string);
    file.status = params.status;
    file.old_mode = params.old_mode.map(ToString::to_string);
    file.new_mode = params.new_mode.map(ToString::to_string);
    file.is_binary = false;
    file.has_file_headers = true;
    file.old_bytes = params.old_bytes.map(|b| b.len());
    file.new_bytes = params.new_bytes.map(|b| b.len());
    file.raw_hunks = hunks.clone();
    file.hunks = hunks;

    FileDiffResult { file, patch }
}

/// Complete result of diffing a repository against a target.
#[derive(Debug, Clone, Default)]
pub struct RepoDiffResult {
    pub diff: String,
    pub files: Vec<diffuse_parse::DiffFile>,
    pub new_content: std::collections::HashMap<String, String>,
    pub old_content: std::collections::HashMap<String, String>,
    pub branch: Option<String>,
    pub untracked_skipped: Vec<String>,
}

/// Compute unified diff for a repository against a review target natively.
pub fn diff_repository(
    repo: &crate::Repository,
    target: &crate::ReviewTarget,
    context: u32,
    mode: WhitespaceMode,
) -> Result<RepoDiffResult, crate::GitError> {
    use diffuse_parse::FileStatus;
    use std::fs;

    let statuses = repo.file_statuses(target)?;
    let mut files = Vec::new();
    let mut diff = String::new();
    let mut new_content_map = std::collections::HashMap::new();
    let mut old_content_map = std::collections::HashMap::new();

    let (old_ref, new_ref, worktree_new) = if let Some(ref r) = target.range {
        (Some(r.0.clone()), Some(r.1.clone()), false)
    } else if target.staged {
        (Some("HEAD".to_string()), None, false)
    } else {
        (None, None, true)
    };

    let repo_root = repo.work_dir().unwrap_or_else(|| repo.git_dir());

    for s in statuses {
        if !crate::status::pathspecs_match(&s.path, &target.pathspecs)
            && !s
                .old_path
                .as_deref()
                .is_some_and(|op| crate::status::pathspecs_match(op, &target.pathspecs))
        {
            continue;
        }

        let old_lookup = s.old_path.as_deref().unwrap_or(&s.path);
        let old_bytes: Option<Vec<u8>> = if s.status == FileStatus::Added {
            None
        } else if let Some(ref r) = old_ref {
            repo.load_blob(r, old_lookup).map(|b| b.data)
        } else {
            repo.load_index_blob(old_lookup).map(|b| b.data)
        };

        let new_bytes: Option<Vec<u8>> = if s.status == FileStatus::Deleted {
            None
        } else if worktree_new {
            if s.old_mode.as_deref() == Some("160000") || s.new_mode.as_deref() == Some("160000") {
                if let Ok(sub) = gix::open(repo_root.join(&s.path))
                    && let Ok(head) = sub.head_id()
                {
                    Some(format!("Subproject commit {head}\n").into_bytes())
                } else {
                    None
                }
            } else {
                fs::read(repo_root.join(&s.path)).ok()
            }
        } else if let Some(ref r) = new_ref {
            repo.load_blob(r, &s.path).map(|b| b.data)
        } else if target.staged {
            repo.load_index_blob(&s.path).map(|b| b.data)
        } else {
            fs::read(repo_root.join(&s.path)).ok()
        };

        let active_bytes = new_bytes.as_deref().or(old_bytes.as_deref()).unwrap_or(b"");
        let is_binary = repo.is_path_binary(&s.path, active_bytes)
            || old_bytes.as_ref().is_some_and(|b| b.contains(&0))
            || new_bytes.as_ref().is_some_and(|b| b.contains(&0));

        let res = generate_file_diff(FileDiffParams {
            path: &s.path,
            old_path: s.old_path.as_deref(),
            old_bytes: old_bytes.as_deref(),
            new_bytes: new_bytes.as_deref(),
            status: s.status,
            old_mode: s.old_mode.as_deref(),
            new_mode: s.new_mode.as_deref(),
            is_binary,
            context,
            mode,
        });

        let mode_changed = match (s.old_mode.as_deref(), s.new_mode.as_deref()) {
            (Some(o), Some(n)) => o != n,
            _ => false,
        };

        if res.file.hunks.is_empty()
            && old_bytes.as_deref() == new_bytes.as_deref()
            && (s.status == FileStatus::Modified || s.status == FileStatus::Unmodified)
            && !mode_changed
            && !is_binary
        {
            continue;
        }

        if !diff.is_empty() && !diff.ends_with('\n') {
            diff.push('\n');
        }
        diff.push_str(&res.patch);

        if !is_binary {
            if let Some(ob) = &old_bytes
                && let Ok(text) = std::str::from_utf8(ob)
            {
                old_content_map.insert(old_lookup.to_string(), text.to_string());
            }
            if let Some(nb) = &new_bytes
                && let Ok(text) = std::str::from_utf8(nb)
            {
                new_content_map.insert(s.path.clone(), text.to_string());
            }
        }

        files.push(res.file);
    }

    let mut untracked_skipped = Vec::new();
    if !target.staged && worktree_new {
        let untracked = repo.untracked_files(&target.pathspecs)?;
        untracked_skipped = untracked.skipped_binary;

        for f in untracked.files {
            let rel = f.path;
            let content_bytes = match fs::read(repo_root.join(&rel)) {
                Ok(b) => b,
                Err(_) => continue,
            };

            let is_binary = repo.is_path_binary(&rel, &content_bytes) || content_bytes.contains(&0);
            let res = generate_file_diff(FileDiffParams {
                path: &rel,
                old_path: None,
                old_bytes: None,
                new_bytes: Some(&content_bytes),
                status: FileStatus::Untracked,
                old_mode: None,
                new_mode: Some("100644"),
                is_binary,
                context,
                mode,
            });

            if !diff.is_empty() && !diff.ends_with('\n') {
                diff.push('\n');
            }
            diff.push_str(&res.patch);

            if !is_binary && let Ok(text) = String::from_utf8(content_bytes) {
                new_content_map.insert(rel, text);
            }

            files.push(res.file);
        }
    }

    let branch = repo.current_branch();

    Ok(RepoDiffResult {
        diff,
        files,
        new_content: new_content_map,
        old_content: old_content_map,
        branch,
        untracked_skipped,
    })
}

/// Compute unified diff between two files or two directory trees directly on
/// disk.
pub fn diff_paths(
    old_path: &std::path::Path,
    new_path: &std::path::Path,
    context: u32,
    mode: WhitespaceMode,
) -> std::io::Result<RepoDiffResult> {
    use diffuse_parse::FileStatus;
    use std::{fs, path::PathBuf};

    if !old_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path not found: {}", old_path.display()),
        ));
    }
    if !new_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("path not found: {}", new_path.display()),
        ));
    }

    if old_path.is_file() && new_path.is_file() {
        let old_bytes = fs::read(old_path)?;
        let new_bytes = fs::read(new_path)?;
        let is_binary = old_bytes.contains(&0) || new_bytes.contains(&0);
        let path_str = new_path.to_string_lossy();
        let old_path_str = old_path.to_string_lossy();

        let res = generate_file_diff(FileDiffParams {
            path: &path_str,
            old_path: Some(&old_path_str),
            old_bytes: Some(&old_bytes),
            new_bytes: Some(&new_bytes),
            status: FileStatus::Modified,
            old_mode: None,
            new_mode: None,
            is_binary,
            context,
            mode,
        });

        let mut new_content_map = std::collections::HashMap::new();
        let mut old_content_map = std::collections::HashMap::new();
        if !is_binary {
            if let Ok(text) = String::from_utf8(new_bytes) {
                new_content_map.insert(path_str.to_string(), text);
            }
            if let Ok(text) = String::from_utf8(old_bytes) {
                old_content_map.insert(old_path_str.to_string(), text);
            }
        }

        return Ok(RepoDiffResult {
            diff: res.patch,
            files: vec![res.file],
            new_content: new_content_map,
            old_content: old_content_map,
            branch: None,
            untracked_skipped: Vec::new(),
        });
    }

    if old_path.is_dir() && new_path.is_dir() {
        fn collect_rel_files(
            root: &std::path::Path,
            cur: &std::path::Path,
            acc: &mut Vec<PathBuf>,
        ) -> std::io::Result<()> {
            for entry in fs::read_dir(cur)? {
                let entry = entry?;
                let path = entry.path();
                let file_name = entry.file_name();
                if file_name == ".git" {
                    continue;
                }
                if path.is_dir() {
                    collect_rel_files(root, &path, acc)?;
                } else if path.is_file()
                    && let Ok(rel) = path.strip_prefix(root)
                {
                    acc.push(rel.to_path_buf());
                }
            }
            Ok(())
        }

        let mut old_files = Vec::new();
        let mut new_files = Vec::new();
        collect_rel_files(old_path, old_path, &mut old_files)?;
        collect_rel_files(new_path, new_path, &mut new_files)?;

        let mut all_paths: std::collections::BTreeSet<PathBuf> = std::collections::BTreeSet::new();
        all_paths.extend(old_files);
        all_paths.extend(new_files);

        let mut files = Vec::new();
        let mut diff = String::new();
        let mut new_content_map = std::collections::HashMap::new();
        let mut old_content_map = std::collections::HashMap::new();

        for rel in all_paths {
            let rel_str = rel.to_string_lossy().to_string();
            let p_old = old_path.join(&rel);
            let p_new = new_path.join(&rel);

            let has_old = p_old.is_file();
            let has_new = p_new.is_file();

            let old_bytes = if has_old {
                Some(fs::read(&p_old)?)
            } else {
                None
            };
            let new_bytes = if has_new {
                Some(fs::read(&p_new)?)
            } else {
                None
            };

            let (status, is_binary) = match (&old_bytes, &new_bytes) {
                (Some(o), Some(n)) => {
                    if o == n {
                        // Unmodified, skip from diff
                        continue;
                    }
                    (FileStatus::Modified, o.contains(&0) || n.contains(&0))
                }
                (None, Some(n)) => (FileStatus::Added, n.contains(&0)),
                (Some(o), None) => (FileStatus::Deleted, o.contains(&0)),
                (None, None) => continue,
            };

            let res = generate_file_diff(FileDiffParams {
                path: &rel_str,
                old_path: None,
                old_bytes: old_bytes.as_deref(),
                new_bytes: new_bytes.as_deref(),
                status,
                old_mode: None,
                new_mode: None,
                is_binary,
                context,
                mode,
            });

            if !diff.is_empty() && !diff.ends_with('\n') {
                diff.push('\n');
            }
            diff.push_str(&res.patch);

            if !is_binary {
                if let Some(nb) = new_bytes
                    && let Ok(text) = String::from_utf8(nb)
                {
                    new_content_map.insert(rel_str.clone(), text);
                }
                if let Some(ob) = old_bytes
                    && let Ok(text) = String::from_utf8(ob)
                {
                    old_content_map.insert(rel_str, text);
                }
            }

            files.push(res.file);
        }

        return Ok(RepoDiffResult {
            diff,
            files,
            new_content: new_content_map,
            old_content: old_content_map,
            branch: None,
            untracked_skipped: Vec::new(),
        });
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "cannot diff file with directory: '{}' and '{}'",
            old_path.display(),
            new_path.display()
        ),
    ))
}
