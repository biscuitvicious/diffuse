//! Git repository identity and filesystem context.

use std::path::{Path, PathBuf};

use super::{range::RevisionRange, target::ReviewTarget};

pub(crate) fn canonicalize_path(path: &Path) -> PathBuf {
    match path.canonicalize() {
        Ok(p) => {
            let s = p.to_string_lossy();
            if let Some(stripped) = s.strip_prefix(r"\\?\") {
                PathBuf::from(stripped)
            } else {
                p
            }
        }
        Err(_) => path.to_path_buf(),
    }
}

/// A git repository store identity, holding its filesystem root and optional
/// review key override.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Repo {
    pub root: Option<PathBuf>,
    pub key: Option<String>,
}

impl Repo {
    /// Create a repo context rooted at a directory path.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let p = root.into();
        Self {
            root: Some(canonicalize_path(&p)),
            key: None,
        }
    }

    /// Create an unrooted repo context with an explicit review key.
    pub fn from_key(key: impl Into<String>) -> Self {
        Self {
            root: None,
            key: Some(key.into()),
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

    /// Set an explicit review key override.
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Set an optional review key override if Some.
    pub fn with_key_opt(mut self, key: Option<String>) -> Self {
        if let Some(k) = key {
            self.key = Some(k);
        }
        self
    }

    /// The filesystem path to the repository root, if rooted.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The repository key used for notes lookup.
    ///
    /// - If explicit `key` is set, returns that key.
    /// - Else if `root` is set, returns the canonical root path string.
    /// - Else returns `None` (no notes functionality available).
    pub fn key(&self) -> Option<&str> {
        if let Some(ref k) = self.key
            && !k.is_empty()
        {
            return Some(k.as_str());
        }
        if let Some(ref root) = self.root {
            return root.to_str();
        }
        None
    }

    /// Human-readable display name for presentation in TUI and export.
    ///
    /// - If explicit `key` is set: returns its basename if it looks like a
    ///   path, otherwise the key itself.
    /// - Else if `root` is set: returns root directory's file name.
    /// - Else returns `"<unnamed>"`.
    pub fn display_name(&self) -> &str {
        if let Some(ref k) = self.key
            && !k.is_empty()
        {
            if let Some(name) = Path::new(k).file_name().and_then(|f| f.to_str())
                && !name.is_empty()
            {
                return name;
            }
            return k.as_str();
        }
        if let Some(ref root) = self.root
            && let Some(name) = root.file_name().and_then(|f| f.to_str())
            && !name.is_empty()
        {
            return name;
        }
        "<unnamed>"
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
        git_revs: Vec<String>,
        staged: bool,
    ) -> ReviewTarget {
        ReviewTarget::new(self, range, git_revs, Vec::new(), staged)
    }
}
