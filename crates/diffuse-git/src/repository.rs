//! Pure Gitoxide repository handle and operations.

use std::path::Path;

use crate::GitError;

/// A pure-Gitoxide repository handle.
pub struct Repository {
    pub(crate) inner: gix::Repository,
}

impl Repository {
    /// Open a repository at the specified directory.
    pub fn open(path: &Path) -> Result<Self, GitError> {
        let inner = gix::open(path)?;
        Ok(Self { inner })
    }

    /// Discover a repository searching upwards from `path`.
    pub fn discover(path: &Path) -> Result<Self, GitError> {
        let inner = gix::discover(path)?;
        Ok(Self { inner })
    }

    /// The working tree root directory, if non-bare.
    pub fn work_dir(&self) -> Option<&Path> {
        self.inner.workdir()
    }

    /// The `.git` metadata directory.
    pub fn git_dir(&self) -> &Path {
        self.inner.git_dir()
    }

    /// Reference to the underlying `gix::Repository`.
    pub fn raw(&self) -> &gix::Repository {
        &self.inner
    }

    /// Mutable reference to the underlying `gix::Repository`.
    pub fn raw_mut(&mut self) -> &mut gix::Repository {
        &mut self.inner
    }

    /// Resolve HEAD commit ID.
    pub fn head_commit_id(&self) -> Option<gix::ObjectId> {
        self.inner.head_id().ok().map(|id| id.detach())
    }

    /// Resolve HEAD commit hex hash.
    pub fn head_commit_hash(&self) -> Option<String> {
        self.head_commit_id().map(|id| id.to_string())
    }

    /// Current branch name (shortened, e.g. "main").
    pub fn current_branch(&self) -> Option<String> {
        self.inner
            .head_name()
            .ok()
            .flatten()
            .map(|name| name.shorten().to_string())
    }

    /// Check if a revision specifier resolves to an existing commit.
    pub fn is_commit(&self, rev: &str) -> bool {
        if rev.is_empty() {
            return false;
        }
        let Ok(id) = self.inner.rev_parse_single(rev) else {
            return false;
        };
        let Ok(obj) = self.inner.find_object(id) else {
            return false;
        };
        obj.kind == gix::object::Kind::Commit
    }

    /// Resolve a revision spec to a short commit hash (typically 7 chars).
    pub fn rev_to_short_hash(&self, rev: &str) -> Option<String> {
        if rev.is_empty() {
            return None;
        }
        let id = self.inner.rev_parse_single(rev).ok()?;
        let obj = self.inner.find_object(id).ok()?;
        if obj.kind == gix::object::Kind::Commit {
            let hex = id.to_hex_with_len(7).to_string();
            Some(hex)
        } else {
            None
        }
    }

    /// Resolve a revision spec to a full 40-character hex commit hash.
    pub fn rev_to_full_hash(&self, rev: &str) -> Option<String> {
        if rev.is_empty() {
            return None;
        }
        let id = self.inner.rev_parse_single(rev).ok()?;
        let obj = self.inner.find_object(id).ok()?;
        if obj.kind == gix::object::Kind::Commit {
            Some(id.to_string())
        } else {
            None
        }
    }

    /// Resolve commit range string (e.g. `c1..c2`) from `rev_args`.
    pub fn resolve_commit_range(&self, rev_args: &[String]) -> Option<String> {
        let refs: Vec<&str> = rev_args
            .iter()
            .map(String::as_str)
            .filter(|t| !t.starts_with('-'))
            .collect();

        if refs.len() == 1 {
            let r = refs[0];
            if let Some(c) = r.strip_suffix("^!") {
                let h_old = self.rev_to_short_hash(&format!("{c}^"))?;
                let h_new = self.rev_to_short_hash(c)?;
                return Some(format!("{h_old}..{h_new}"));
            }
            if let Some((old_r, new_r)) = r.split_once("...") {
                let left = if old_r.is_empty() { "HEAD" } else { old_r };
                let right = if new_r.is_empty() { "HEAD" } else { new_r };
                let h_left = self.rev_to_short_hash(left)?;
                let h_right = self.rev_to_short_hash(right)?;
                return Some(format!("{h_left}...{h_right}"));
            }
            if let Some((old_r, new_r)) = r.split_once("..")
                && !old_r.ends_with('.')
                && !new_r.starts_with('.')
            {
                let left = if old_r.is_empty() { "HEAD" } else { old_r };
                let right = if new_r.is_empty() { "HEAD" } else { new_r };
                let h_left = self.rev_to_short_hash(left)?;
                let h_right = self.rev_to_short_hash(right)?;
                return Some(format!("{h_left}..{h_right}"));
            }
        }

        if refs.len() == 2 {
            let h1 = self.rev_to_short_hash(refs[0])?;
            let h2 = self.rev_to_short_hash(refs[1])?;
            return Some(format!("{h1}..{h2}"));
        }

        None
    }

    /// Check whether the commit range `left..right` contains exactly one
    /// commit.
    ///
    /// In a git DAG, `left..right` contains exactly one commit (`right`) if and
    /// only if `right` is a single-parent commit whose parent resolves to
    /// `left`.
    pub fn is_single_commit_range(&self, left: &str, right: &str) -> bool {
        let left = if left.is_empty() { "HEAD" } else { left };
        let right = if right.is_empty() { "HEAD" } else { right };

        let Ok(right_id) = self.inner.rev_parse_single(right) else {
            return false;
        };
        let Ok(right_obj) = self.inner.find_object(right_id) else {
            return false;
        };
        let right_commit_obj = match right_obj.kind {
            gix::object::Kind::Commit => right_obj,
            _ => match right_obj.peel_to_kind(gix::object::Kind::Commit) {
                Ok(peeled) => peeled,
                Err(_) => return false,
            },
        };
        let Ok(right_commit) = right_commit_obj.try_into_commit() else {
            return false;
        };
        let parents: Vec<_> = right_commit.parent_ids().collect();
        if parents.len() != 1 {
            return false;
        }

        let Ok(left_id) = self.inner.rev_parse_single(left) else {
            return false;
        };
        let left_commit_id = if let Ok(left_obj) = self.inner.find_object(left_id) {
            match left_obj.kind {
                gix::object::Kind::Commit => left_id.detach(),
                _ => match left_obj.peel_to_kind(gix::object::Kind::Commit) {
                    Ok(peeled) => peeled.id,
                    Err(_) => return false,
                },
            }
        } else {
            return false;
        };

        parents[0] == left_commit_id
    }

    /// Resolve the commit hash to scope review notes to.
    ///
    /// When reviewing a range, returns the second newest commit in the range
    /// (or the base commit). Otherwise returns HEAD.
    pub fn notes_commit(&self, rev_args: &[String]) -> Option<String> {
        let ref_pair = crate::types::RefPair::resolve(rev_args, false);

        if !ref_pair.worktree_new
            && let (Some(old), Some(new)) = (ref_pair.old_ref, ref_pair.new_ref)
        {
            // If new is a commit, try traversing its parent commits
            if let Some(new_id) = self.rev_to_full_hash(&new)
                && let Ok(id) = gix::ObjectId::from_hex(new_id.as_bytes())
                && let Ok(walk) = self.inner.rev_walk([id]).all()
            {
                let commits: Vec<_> = walk.filter_map(|info| info.ok()).take(2).collect();
                if commits.len() >= 2 {
                    return Some(commits[1].id.to_string());
                }
            }
            if let Some(h) = self.rev_to_full_hash(&old) {
                return Some(h);
            }
            if let Some(h) = self.rev_to_full_hash(&format!("{new}~1")) {
                return Some(h);
            }
            if let Some(h) = self.rev_to_full_hash(&new) {
                return Some(h);
            }
        }

        self.head_commit_hash()
    }

    /// Load a single blob from the index by path.
    pub fn load_index_blob(&self, path: &str) -> Option<crate::types::GitBlob> {
        crate::objects::load_index_blob(&self.inner, path)
    }

    /// Check if a path is considered binary by gitattributes or content.
    pub fn is_path_binary(&self, path: &str, data: &[u8]) -> bool {
        crate::objects::is_path_binary(&self.inner, path, data)
    }

    /// Load a single blob by reference and path.
    pub fn load_blob(&self, ref_str: &str, path: &str) -> Option<crate::types::GitBlob> {
        crate::objects::load_blob(&self.inner, ref_str, path)
    }

    /// Batch-load blobs by specifiers (e.g. `ref:path`, `:path`, or object id).
    pub fn load_blobs_batch(
        &self,
        specs: &[String],
    ) -> std::collections::HashMap<String, crate::types::GitBlob> {
        crate::objects::load_blobs_batch(&self.inner, specs)
    }

    /// Batch-load blobs as UTF-8 strings.
    pub fn load_blobs_text_batch(
        &self,
        specs: &[String],
    ) -> std::collections::HashMap<String, String> {
        crate::objects::load_blobs_text_batch(&self.inner, specs)
    }
}
