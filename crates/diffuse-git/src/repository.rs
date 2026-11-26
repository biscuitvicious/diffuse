//! Pure Gitoxide repository handle and operations.

use std::path::Path;

use crate::GitError;

/// Category of a git revision reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionKind {
    LocalBranch,
    RemoteBranch,
    Tag,
    Special,
}

impl RevisionKind {
    /// Human-readable label for shell completion help.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalBranch => "Local branch",
            Self::RemoteBranch => "Remote branch",
            Self::Tag => "Tag",
            Self::Special => "Commit reference",
        }
    }
}

/// A git revision reference candidate with its classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionCandidate {
    pub name: String,
    pub kind: RevisionKind,
}

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

    /// List all local and remote branches, tags, and HEAD for completion.
    pub fn revision_candidates(&self) -> Vec<RevisionCandidate> {
        let mut candidates = Vec::new();
        candidates.push(RevisionCandidate {
            name: "HEAD".to_string(),
            kind: RevisionKind::Special,
        });

        if let Ok(platform) = self.inner.references()
            && let Ok(iter) = platform.all()
        {
            for reference in iter.flatten() {
                let full_name = reference.name().as_bstr();
                let kind = if full_name.starts_with(b"refs/heads/") {
                    RevisionKind::LocalBranch
                } else if full_name.starts_with(b"refs/remotes/") {
                    if full_name.ends_with(b"/HEAD") {
                        continue;
                    }
                    RevisionKind::RemoteBranch
                } else if full_name.starts_with(b"refs/tags/") {
                    RevisionKind::Tag
                } else {
                    continue;
                };

                candidates.push(RevisionCandidate {
                    name: reference.name().shorten().to_string(),
                    kind,
                });
            }
        }
        candidates
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

    /// Try to resolve a user-friendly symbolic name (branch, tag, or HEAD) for
    /// a commit revision. If no symbolic reference is found, falls back to the
    /// short commit hash.
    pub fn resolve_commit_display_name(&self, rev: &str) -> Option<String> {
        let rev = rev.trim();
        if rev.is_empty() {
            return None;
        }

        // 1. Direct HEAD check
        if rev.eq_ignore_ascii_case("HEAD") {
            return Some("HEAD".to_string());
        }

        // 2. Check if rev itself resolves to a reference (branch, tag, or
        //    remote branch)
        if let Ok(Some(r)) = self.inner.try_find_reference(rev) {
            let full_name = r.name().as_bstr();
            if full_name.starts_with(b"refs/heads/")
                || full_name.starts_with(b"refs/tags/")
                || full_name.starts_with(b"refs/remotes/")
            {
                return Some(r.name().shorten().to_string());
            }
            if full_name == b"HEAD" {
                return Some("HEAD".to_string());
            }
        }

        // 3. Fall back to short commit hash
        self.rev_to_short_hash(rev)
    }

    /// Resolve commit range string (e.g. `c1..c2` or `c2^!`) from `git_revs`.
    ///
    /// If a range `start..end` represents exactly one commit and `end^!` is the
    /// same range based on git semantics (i.e. `end` is a single-parent commit
    /// whose parent is `start`), formatted as `end^!`. Otherwise formatted as
    /// `start..end` (or `start...end` for three-dot).
    pub fn resolve_commit_range(&self, git_revs: &[String]) -> Option<String> {
        let refs: Vec<&str> = git_revs
            .iter()
            .map(String::as_str)
            .filter(|t| !t.starts_with('-'))
            .collect();

        if refs.len() == 1 {
            let r = refs[0];
            if let Some(c) = r.strip_suffix("^!") {
                let name = self
                    .resolve_commit_display_name(c)
                    .or_else(|| self.rev_to_short_hash(c))?;
                return Some(format!("{name}^!"));
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
                if self.is_single_commit_range(left, right) {
                    let name = self
                        .resolve_commit_display_name(right)
                        .or_else(|| self.rev_to_short_hash(right))?;
                    return Some(format!("{name}^!"));
                }
                let h_right = self.rev_to_short_hash(right)?;
                let h_left = self.rev_to_short_hash(left)?;
                return Some(format!("{h_left}..{h_right}"));
            }
        }

        if refs.len() == 2 {
            if self.is_single_commit_range(refs[0], refs[1]) {
                let name = self
                    .resolve_commit_display_name(refs[1])
                    .or_else(|| self.rev_to_short_hash(refs[1]))?;
                return Some(format!("{name}^!"));
            }
            let h2 = self.rev_to_short_hash(refs[1])?;
            let h1 = self.rev_to_short_hash(refs[0])?;
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

    /// Create or update a lightweight tag pointing to `target` (e.g. revision
    /// specifier or commit ID). If `target` is empty, points to `HEAD`.
    pub fn create_tag(&self, tag_name: &str, target: &str) -> Result<(), GitError> {
        let target = if target.is_empty() { "HEAD" } else { target };
        let id = self.inner.rev_parse_single(target)?.detach();
        let full_name = format!("refs/tags/{tag_name}");
        self.inner.reference(
            full_name,
            id,
            gix::refs::transaction::PreviousValue::Any,
            format!("tag: created {tag_name}"),
        )?;
        Ok(())
    }

    /// Resolve the commit or range scope to attach review notes to.
    ///
    /// - Working tree review (empty args): returns HEAD.
    /// - Single commit diff (`c^!` or `p..c` where `p` is `c`'s parent):
    ///   returns commit parent `p`.
    /// - Explicit single commit reference (`c`): returns commit `c`.
    /// - Multi-commit range (`c0..c3`): returns `<base>..<tip~1>` (parent of
    ///   tip).
    /// - Explicit worktree range (`<base>..`): returns `<base>..<head>` (parent
    ///   of worktree tip).
    pub fn notes_commit(&self, git_revs: &[String]) -> Option<String> {
        if git_revs.is_empty() {
            return self.head_commit_hash();
        }

        // Single ref without range syntax: resolves to that single commit
        if git_revs.len() == 1 {
            let r = &git_revs[0];
            if let Some(c) = r.strip_suffix("^!") {
                return self.rev_to_full_hash(&format!("{c}^"));
            }
            if !r.contains("..") && !r.contains("...") {
                return self.rev_to_full_hash(r);
            }
        }

        let ref_pair = crate::types::RefPair::resolve(git_revs, false);

        if ref_pair.worktree_new {
            let head = self.head_commit_hash()?;
            if let Some(old) = ref_pair.old_ref
                && let Some(old_hash) = self.rev_to_full_hash(&old)
                && old_hash != head
            {
                return Some(format!("{old_hash}..{head}"));
            }
            return Some(head);
        }

        if let (Some(old), Some(new)) = (ref_pair.old_ref, ref_pair.new_ref) {
            let old_hash = self.rev_to_full_hash(&old)?;
            let new_hash = self.rev_to_full_hash(&new)?;

            // Check if this range contains exactly one commit:
            if self.is_single_commit_range(&old, &new) {
                return Some(old_hash);
            }

            // Multi-commit range: base..parent_of_tip
            if let Ok(id) = gix::ObjectId::from_hex(new_hash.as_bytes())
                && let Ok(walk) = self.inner.rev_walk([id]).all()
            {
                let commits: Vec<_> = walk.filter_map(|info| info.ok()).take(2).collect();
                if commits.len() >= 2 {
                    let tip_parent = commits[1].id.to_string();
                    return Some(format!("{old_hash}..{tip_parent}"));
                }
            }

            return Some(format!("{old_hash}..{new_hash}"));
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
