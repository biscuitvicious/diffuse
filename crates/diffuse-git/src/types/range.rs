//! Git revision ranges and resolved ref pairs.

use std::{fmt, path::Path};

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
    /// Return the revision range if this pair represents a commit-to-commit
    /// comparison.
    pub fn revision_range(&self) -> Option<RevisionRange> {
        if !self.worktree_new
            && let (Some(old), Some(new)) = (&self.old_ref, &self.new_ref)
            && !old.is_empty()
            && !new.is_empty()
        {
            Some(RevisionRange(old.clone(), new.clone()))
        } else {
            None
        }
    }

    /// Resolve old/new git refs from base args.
    pub fn resolve(base_args: &[String], staged: bool) -> Self {
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
            if staged {
                return Self {
                    worktree_new: false,
                    new_ref: Some(String::new()),
                    old_ref: Some(r.to_string()),
                };
            }
            return Self {
                worktree_new: true,
                new_ref: None,
                old_ref: Some(r.to_string()),
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
