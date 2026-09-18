//! Repository, revision range, and review target identity.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use crate::{
    git::{get_current_branch, is_inside_work_tree},
    notes::{head_commit, notes_commit, notes_key},
};

#[cfg(test)]
mod tests;

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
        if is_inside_work_tree(cwd) {
            let root = crate::git::run_git(cwd, &["rev-parse", "--show-toplevel"])
                .map(|s| PathBuf::from(s.trim()))
                .unwrap_or_else(|| cwd.to_path_buf());
            Some(Self::from_root(root))
        } else {
            None
        }
    }

    /// Set an explicit display name override (e.g. from `--repo <name>`).
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

    /// The repository name: returns the explicit override if set, otherwise
    /// infers from the root directory's file name, falling back to `"diffuse"`.
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
        self.root.as_deref().and_then(head_commit)
    }

    /// Resolve the active branch of this repository, if rooted.
    pub fn branch(&self) -> String {
        self.root
            .as_deref()
            .map(get_current_branch)
            .unwrap_or_default()
    }

    /// Check if this repository is inside a git work tree.
    pub fn is_work_tree(&self) -> bool {
        self.root
            .as_deref()
            .map(is_inside_work_tree)
            .unwrap_or(false)
    }

    /// Convenience: combine this repository with a revision range into a
    /// [`ReviewTarget`].
    pub fn with_target(self, range: Option<RevisionRange>, git_args: Vec<String>) -> ReviewTarget {
        ReviewTarget {
            repo: self,
            range,
            git_args,
        }
    }
}

/// A pair of revision endpoints: (bottom / base, top / target).
///
/// In `c1..c2`, `.0` is `c1` (bottom/base) and `.1` is `c2` (top/target).
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
    ///
    /// Returns `Some(RevisionRange(bottom, top))` for commit-to-commit ranges
    /// (e.g. `c1..c2`, `c1...c2`, `c1 c2`, `c^!`).
    /// Returns `None` for comparisons against the working tree (empty args,
    /// `--cached`, etc.).
    pub fn resolve(repo_root: &Path, git_args: &[String]) -> Option<Self> {
        let refs: Vec<&str> = git_args
            .iter()
            .map(std::string::String::as_str)
            .filter(|t| *t != "--cached" && !t.starts_with('-'))
            .collect();

        if refs.len() == 1 {
            let r = refs[0];
            if let Some(c) = r.strip_suffix("^!") {
                let h_old = rev_to_short_hash(repo_root, &format!("{c}^"))?;
                let h_new = rev_to_short_hash(repo_root, c)?;
                return Some(Self(h_old, h_new));
            }
            if let Some((old_r, new_r)) = r.split_once("...") {
                let left = if old_r.is_empty() { "HEAD" } else { old_r };
                let right = if new_r.is_empty() { "HEAD" } else { new_r };
                let h_left = rev_to_short_hash(repo_root, left)?;
                let h_right = rev_to_short_hash(repo_root, right)?;
                return Some(Self(h_left, h_right));
            }
            if let Some((old_r, new_r)) = r.split_once("..")
                && !old_r.ends_with('.')
                && !new_r.starts_with('.')
            {
                let left = if old_r.is_empty() { "HEAD" } else { old_r };
                let right = if new_r.is_empty() { "HEAD" } else { new_r };
                let h_left = rev_to_short_hash(repo_root, left)?;
                let h_right = rev_to_short_hash(repo_root, right)?;
                return Some(Self(h_left, h_right));
            }
            return None;
        }

        if refs.len() == 2 {
            let h1 = rev_to_short_hash(repo_root, refs[0])?;
            let h2 = rev_to_short_hash(repo_root, refs[1])?;
            return Some(Self(h1, h2));
        }

        None
    }
}

fn rev_to_short_hash(cwd: &Path, rev: &str) -> Option<String> {
    if rev.is_empty() {
        return None;
    }
    crate::git::run_git(cwd, &["rev-parse", "--short", &format!("{rev}^{{commit}}")])
        .map(|s| s.trim().to_string())
}

impl fmt::Display for RevisionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.0, self.1)
    }
}

/// The subject of a diffuse review: a repository and its revision target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewTarget {
    pub repo: Repo,
    pub range: Option<RevisionRange>,
    pub git_args: Vec<String>,
}

impl ReviewTarget {
    /// Create a review target from a repo, optional range, and preserved git
    /// args.
    pub fn new(repo: Repo, range: Option<RevisionRange>, git_args: Vec<String>) -> Self {
        Self {
            repo,
            range,
            git_args,
        }
    }

    /// Create a review target from a repo and git arguments, automatically
    /// resolving whether it represents a commit range or working tree review.
    pub fn from_repo_args(repo: Repo, git_args: Vec<String>) -> Self {
        let range = repo
            .root()
            .and_then(|root| RevisionRange::resolve(root, &git_args));
        Self {
            repo,
            range,
            git_args,
        }
    }

    /// Create a default review target for a repo with empty git arguments
    /// (working tree review).
    pub fn from_repo(repo: Repo) -> Self {
        Self {
            repo,
            range: None,
            git_args: Vec::new(),
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
            && let Some(r) = crate::git::resolve_commit_range(root, &self.git_args)
        {
            return Some(r);
        }
        self.range.as_ref().map(ToString::to_string)
    }

    /// Target commit hash to scope review notes to:
    /// - Range: second-to-last commit in range.
    /// - Working tree: live HEAD commit.
    /// - Unrooted: None.
    pub fn notes_commit(&self) -> Option<String> {
        self.repo
            .root()
            .and_then(|root| notes_commit(root, &self.git_args))
    }

    /// Scoped notes database key: `{repo_name}@{notes_commit}` or bare
    /// `{repo_name}`.
    pub fn notes_key(&self) -> String {
        notes_key(self.repo_name(), self.notes_commit().as_deref())
    }

    /// Active branch name.
    pub fn branch(&self) -> String {
        self.repo.branch()
    }

    /// Whether the repository is inside a git work tree.
    pub fn is_work_tree(&self) -> bool {
        self.repo.is_work_tree()
    }
}
