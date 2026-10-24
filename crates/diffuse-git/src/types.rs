//! Canonical Git domain types for diffuse.

use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Options configuring git diff generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiffOptions {
    /// Number of context lines surrounding changes.
    pub context_lines: usize,
    /// Whether to detect renames and copies.
    pub find_renames: bool,
    /// Similarity threshold for rename detection (0..=100).
    pub rename_threshold: u32,
    /// Whether to ignore indentation changes (exact trim-equality diffing).
    pub ignore_indentation: bool,
}

impl Default for GitDiffOptions {
    fn default() -> Self {
        Self {
            context_lines: 4,
            find_renames: true,
            rename_threshold: 50,
            ignore_indentation: false,
        }
    }
}

/// A git blob retrieved from the object database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBlob {
    /// Object ID (hex string).
    pub id: String,
    /// Raw blob bytes.
    pub data: Vec<u8>,
    /// Whether the blob is considered binary by gitattributes, entry mode, or
    /// content inspection.
    pub is_binary: bool,
}

impl GitBlob {
    /// Construct a new blob.
    pub fn new(id: impl Into<String>, data: Vec<u8>, is_binary: bool) -> Self {
        Self {
            id: id.into(),
            data,
            is_binary,
        }
    }

    /// View blob data as UTF-8 string if valid and not binary.
    pub fn as_str(&self) -> Option<&str> {
        if self.is_binary {
            None
        } else {
            std::str::from_utf8(&self.data).ok()
        }
    }

    /// Size of blob data in bytes.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Whether this blob is binary.
    pub fn is_binary(&self) -> bool {
        self.is_binary
    }
}

/// A git repository store identity, holding its filesystem root and optional
/// display name override.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub root: Option<PathBuf>,
    pub name: Option<String>,
}

impl Repo {
    /// Create a repo context rooted at a directory path.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self {
            root: Some(root.into()),
            name: None,
        }
    }

    /// Create an unrooted repo context (e.g. for piped or non-git reviews).
    pub fn from_name(name: impl Into<String>) -> Self {
        Self {
            root: None,
            name: Some(name.into()),
        }
    }

    /// Discover a git repository worktree starting from `cwd`.
    pub fn discover(cwd: &Path) -> Option<Self> {
        let repo = gix::discover(cwd).ok()?;
        let root = repo
            .workdir()
            .map(Path::to_path_buf)
            .or_else(|| Some(repo.git_dir().to_path_buf()))?;
        Some(Self::from_root(root))
    }

    /// Set an explicit display name override.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set an optional display name override if Some.
    pub fn with_name_opt(mut self, name: Option<String>) -> Self {
        if let Some(n) = name {
            self.name = Some(n);
        }
        self
    }

    /// The filesystem path to the repository root, if rooted.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The repository name: returns explicit override if set, otherwise
    /// infers from root directory's file name, falling back to `"diffuse"`.
    pub fn name(&self) -> &str {
        if let Some(ref n) = self.name
            && !n.is_empty()
        {
            return n.as_str();
        }
        if let Some(ref root) = self.root
            && let Some(name) = root.file_name().and_then(|f| f.to_str())
            && !name.is_empty()
        {
            return name;
        }
        "diffuse"
    }

    /// Resolve the live HEAD commit of this repository, if rooted.
    pub fn head_commit(&self) -> Option<String> {
        let root = self.root.as_deref()?;
        let repo = gix::open(root).ok()?;
        let head_id = repo.head_id().ok()?;
        Some(head_id.to_string())
    }

    /// Resolve the active branch of this repository, if rooted.
    pub fn branch(&self) -> String {
        let Some(root) = self.root.as_deref() else {
            return String::new();
        };
        let Ok(repo) = gix::open(root) else {
            return String::new();
        };
        repo.head_name()
            .ok()
            .flatten()
            .map(|name| name.shorten().to_string())
            .unwrap_or_default()
    }

    /// Check if this repository is inside a git work tree.
    pub fn is_work_tree(&self) -> bool {
        let Some(root) = self.root.as_deref() else {
            return false;
        };
        gix::open(root)
            .map(|r| r.workdir().is_some())
            .unwrap_or(false)
    }

    /// Convenience: combine this repository with a revision range into a
    /// [`ReviewTarget`].
    pub fn with_target(
        self,
        range: Option<RevisionRange>,
        git_args: Vec<String>,
        staged: bool,
    ) -> ReviewTarget {
        ReviewTarget::new(self, range, git_args, staged)
    }
}

/// A pair of revision endpoints: (bottom / base, top / target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionRange(pub String, pub String);

impl RevisionRange {
    /// Create a new revision range from bottom and top endpoints.
    pub fn new(bottom: impl Into<String>, top: impl Into<String>) -> Self {
        Self(bottom.into(), top.into())
    }

    /// The bottom (base/old) revision endpoint.
    pub fn bottom(&self) -> &str {
        &self.0
    }

    /// The top (target/new) revision endpoint.
    pub fn top(&self) -> &str {
        &self.1
    }

    /// Resolve revision endpoints from git args in a repository.
    pub fn resolve(repo_root: &Path, git_args: &[String]) -> Option<Self> {
        let repo = crate::repository::Repository::open(repo_root).ok()?;
        let refs: Vec<&str> = git_args
            .iter()
            .map(String::as_str)
            .filter(|t| !t.starts_with('-'))
            .collect();

        if refs.len() == 1 {
            let r = refs[0];
            if let Some(c) = r.strip_suffix("^!") {
                let h_old = repo.rev_to_short_hash(&format!("{c}^"))?;
                let h_new = repo.rev_to_short_hash(c)?;
                return Some(Self(h_old, h_new));
            }
            if let Some((old_r, new_r)) = r.split_once("...") {
                let left = if old_r.is_empty() { "HEAD" } else { old_r };
                let right = if new_r.is_empty() { "HEAD" } else { new_r };
                let h_left = repo.rev_to_short_hash(left)?;
                let h_right = repo.rev_to_short_hash(right)?;
                return Some(Self(h_left, h_right));
            }
            if let Some((old_r, new_r)) = r.split_once("..")
                && !old_r.ends_with('.')
                && !new_r.starts_with('.')
            {
                let left = if old_r.is_empty() { "HEAD" } else { old_r };
                let right = if new_r.is_empty() { "HEAD" } else { new_r };
                let h_left = repo.rev_to_short_hash(left)?;
                let h_right = repo.rev_to_short_hash(right)?;
                return Some(Self(h_left, h_right));
            }
            return None;
        }

        if refs.len() == 2 {
            let h1 = repo.rev_to_short_hash(refs[0])?;
            let h2 = repo.rev_to_short_hash(refs[1])?;
            return Some(Self(h1, h2));
        }

        None
    }
}

impl fmt::Display for RevisionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.0, self.1)
    }
}

/// Resolved old/new ref endpoints for review comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefPair {
    pub worktree_new: bool,
    pub new_ref: Option<String>,
    pub old_ref: Option<String>,
}

impl RefPair {
    /// Resolve old/new git refs from base args.
    pub fn resolve(_cwd: &Path, base_args: &[String], staged: bool) -> Self {
        let refs: Vec<&str> = base_args
            .iter()
            .map(String::as_str)
            .filter(|t| !t.starts_with('-'))
            .collect();

        if refs.len() == 1 {
            let r = refs[0];
            if let Some(c) = r.strip_suffix("^!") {
                return Self {
                    worktree_new: false,
                    new_ref: Some(c.to_string()),
                    old_ref: Some(format!("{c}^")),
                };
            }
            if let Some((old_r, new_r)) = r.split_once("...") {
                let left = if old_r.is_empty() { "HEAD" } else { old_r };
                let right = if new_r.is_empty() { "HEAD" } else { new_r };
                return Self {
                    worktree_new: false,
                    new_ref: Some(right.to_string()),
                    old_ref: Some(left.to_string()),
                };
            }
            if let Some((old_r, new_r)) = r.split_once("..")
                && !old_r.ends_with('.')
                && !new_r.starts_with('.')
            {
                let left = if old_r.is_empty() { "HEAD" } else { old_r };
                let right = if new_r.is_empty() { "HEAD" } else { new_r };
                return Self {
                    worktree_new: false,
                    new_ref: Some(right.to_string()),
                    old_ref: Some(left.to_string()),
                };
            }
            return Self {
                worktree_new: false,
                new_ref: Some(r.to_string()),
                old_ref: Some(format!("{r}~1")),
            };
        }

        if refs.len() == 2 {
            return Self {
                worktree_new: false,
                new_ref: Some(refs[1].to_string()),
                old_ref: Some(refs[0].to_string()),
            };
        }

        Self {
            worktree_new: !staged,
            new_ref: if staged { Some(String::new()) } else { None },
            old_ref: if staged {
                Some("HEAD".to_string())
            } else {
                None
            },
        }
    }
}

/// The subject of a diffuse review: a repository and its revision target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewTarget {
    pub repo: Repo,
    pub range: Option<RevisionRange>,
    pub git_args: Vec<String>,
    pub rev_args: Vec<String>,
    pub pathspecs: Vec<String>,
    pub staged: bool,
}

impl ReviewTarget {
    /// Create a review target from a repo, optional range, preserved git args,
    /// and staged flag.
    pub fn new(
        repo: Repo,
        range: Option<RevisionRange>,
        git_args: Vec<String>,
        staged: bool,
    ) -> Self {
        let (rev_args, pathspecs) = parse_git_rev_and_pathspecs(repo.root(), &git_args);
        Self {
            repo,
            range,
            git_args,
            rev_args,
            pathspecs,
            staged,
        }
    }

    /// Create a review target from a repo, git arguments, and staged flag.
    pub fn from_repo_args(repo: Repo, git_args: Vec<String>, staged: bool) -> Self {
        let (rev_args, pathspecs) = parse_git_rev_and_pathspecs(repo.root(), &git_args);
        let range = repo
            .root()
            .and_then(|root| RevisionRange::resolve(root, &rev_args));
        Self {
            repo,
            range,
            git_args,
            rev_args,
            pathspecs,
            staged,
        }
    }

    /// Create a default review target for a repo with empty git arguments.
    pub fn from_repo(repo: Repo) -> Self {
        Self {
            repo,
            range: None,
            git_args: Vec::new(),
            rev_args: Vec::new(),
            pathspecs: Vec::new(),
            staged: false,
        }
    }

    /// Create a review target rooted at a directory with empty git arguments.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self::from_repo(Repo::from_root(root))
    }

    /// Whether this target is a commit-to-commit range rather than a working
    /// tree review.
    pub fn is_range(&self) -> bool {
        self.range.is_some()
    }

    /// The revision range, if this target compares two commits.
    pub fn range(&self) -> Option<&RevisionRange> {
        self.range.as_ref()
    }

    /// The human-readable display name of the repository.
    pub fn repo_name(&self) -> &str {
        self.repo.name()
    }

    /// The filesystem root of the repository, if rooted.
    pub fn repo_root(&self) -> Option<&Path> {
        self.repo.root()
    }

    /// Formatted commit range string (e.g. `c1..c2`), or `None` if reviewing
    /// working tree.
    pub fn commit_range(&self) -> Option<String> {
        if let Some(root) = self.repo.root()
            && let Ok(repo) = crate::repository::Repository::open(root)
            && let Some(r) = repo.resolve_commit_range(&self.rev_args)
        {
            return Some(r);
        }
        self.range.as_ref().map(ToString::to_string)
    }

    /// Resolves the single commit hash if `rev_args` targets a single commit.
    pub fn single_commit(&self) -> Option<String> {
        if self.rev_args.len() == 1 {
            let r = &self.rev_args[0];
            if !r.contains("..")
                && !r.contains("...")
                && !r.ends_with("^!")
                && let Some(root) = self.repo.root()
                && let Ok(repo) = crate::repository::Repository::open(root)
            {
                return repo.rev_to_full_hash(r);
            }
        }
        None
    }

    /// Target commit hash to scope review notes to.
    pub fn notes_commit(&self) -> Option<String> {
        let root = self.repo.root()?;
        let repo = crate::repository::Repository::open(root).ok()?;
        if !self.rev_args.is_empty() {
            if let Some(single) = self.single_commit() {
                return Some(single);
            }
            repo.notes_commit(&self.rev_args)
        } else {
            self.repo.head_commit()
        }
    }

    /// Scoped notes database key: `{repo_name}@{notes_commit}` or bare
    /// `{repo_name}`.
    pub fn notes_key(&self) -> String {
        let repo = self.repo_name();
        if let Some(commit) = self.notes_commit() {
            format!("{repo}@{commit}")
        } else {
            repo.to_string()
        }
    }

    /// Check whether a file path matches this target's pathspecs.
    pub fn matches_path(&self, file_path: &str, current_dir: &Path) -> bool {
        if self.pathspecs.is_empty() {
            return true;
        }

        let file_clean = file_path.trim_start_matches("./");

        self.pathspecs.iter().any(|spec| {
            let spec_clean = spec.trim_start_matches("./").trim_end_matches('/');

            if file_clean == spec_clean {
                return true;
            }

            if file_clean.starts_with(&format!("{spec_clean}/")) {
                return true;
            }

            if let Some(root) = self.repo.root()
                && let Ok(rel) = current_dir.join(spec).strip_prefix(root)
            {
                let rel_str = rel.to_string_lossy();
                let rel_clean = rel_str.trim_start_matches("./").trim_end_matches('/');
                if file_clean == rel_clean || file_clean.starts_with(&format!("{rel_clean}/")) {
                    return true;
                }
            }

            if file_clean.ends_with(&format!("/{spec_clean}")) {
                return true;
            }

            false
        })
    }

    /// Active branch name.
    pub fn branch(&self) -> String {
        self.repo.branch()
    }

    /// Whether the repository is inside a git work tree.
    pub fn is_work_tree(&self) -> bool {
        self.repo.is_work_tree()
    }

    /// Whether the review target diffs against the staged index.
    pub fn is_staged(&self) -> bool {
        self.staged
    }

    /// Alias for [`ReviewTarget::is_staged`].
    pub fn is_cached(&self) -> bool {
        self.is_staged()
    }
}

/// Separate git CLI arguments into revisions and pathspecs.
pub fn parse_git_rev_and_pathspecs(
    repo_root: Option<&Path>,
    git_args: &[String],
) -> (Vec<String>, Vec<String>) {
    if let Some(pos) = git_args.iter().position(|a| a == "--") {
        let revs = git_args[..pos]
            .iter()
            .filter(|a| !a.starts_with('-'))
            .cloned()
            .collect();
        let paths = git_args[pos + 1..].to_vec();
        return (revs, paths);
    }

    let mut revs = Vec::new();
    let mut paths = Vec::new();

    let repo = repo_root.and_then(|r| crate::repository::Repository::open(r).ok());

    for arg in git_args {
        if arg.starts_with('-') {
            continue;
        }
        if arg.contains("..") || arg.contains("...") || arg.ends_with("^!") {
            revs.push(arg.clone());
            continue;
        }
        if let Some(ref r) = repo {
            let is_commit = r.is_commit(arg);
            if is_commit && revs.len() < 2 && !r.work_dir().is_some_and(|w| w.join(arg).exists()) {
                revs.push(arg.clone());
            } else {
                paths.push(arg.clone());
            }
        } else {
            paths.push(arg.clone());
        }
    }

    (revs, paths)
}
