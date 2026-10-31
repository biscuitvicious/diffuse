//! Git working tree status, directory traversal (dirwalk), and whitespace
//! reconciliation.

use std::{
    fs,
    ops::ControlFlow,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

use gix::bstr::{BString, ByteSlice};

use crate::{GitError, Repository, ReviewTarget, objects::peel_to_tree};

/// Canonical 8,000-byte null-byte binary check matching Git and Gitoxide.
pub fn is_binary_buf(buf: &[u8]) -> bool {
    let check = &buf[..buf.len().min(8000)];
    check.contains(&0)
}

/// Helper to test if a relative path matches command-line pathspecs,
/// honoring exclusion patterns (e.g. `:(exclude)<pat>`, `:!<pat>`, `^<pat>`).
pub(crate) fn pathspecs_match(path: &str, pathspecs: &[String]) -> bool {
    if pathspecs.is_empty() {
        return true;
    }

    let file_clean = path.trim_start_matches("./");

    let path_matches_pattern = |pat: &str| -> bool {
        let clean = pat.trim_start_matches("./").trim_end_matches('/');
        file_clean == clean || file_clean.starts_with(&format!("{clean}/"))
    };

    let mut has_includes = false;
    let mut matched_include = false;

    for ps in pathspecs {
        if let Some(pat) = ps.strip_prefix(":(exclude)") {
            if path_matches_pattern(pat) {
                return false;
            }
        } else if let Some(pat) = ps.strip_prefix(":!") {
            if path_matches_pattern(pat) {
                return false;
            }
        } else if let Some(pat) = ps.strip_prefix('^') {
            if path_matches_pattern(pat) {
                return false;
            }
        } else {
            has_includes = true;
            if !matched_include && path_matches_pattern(ps) {
                matched_include = true;
            }
        }
    }

    !has_includes || matched_include
}

/// Result of untracked file discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedOutput {
    /// Discovered untracked files formatted as synthetic
    /// [`diffuse_parse::DiffFile`]s.
    pub files: Vec<diffuse_parse::DiffFile>,
    /// Repository-relative paths of untracked files skipped due to binary
    /// content.
    pub skipped_binary: Vec<String>,
}

/// Status of a file relative to index, worktree, or commit tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileStatus {
    /// Repository-relative path of the file (new path for additions/renames).
    pub path: String,
    /// Previous path if renamed.
    pub old_path: Option<String>,
    /// High-level file status.
    pub status: diffuse_parse::FileStatus,
    /// Whether the file is classified as binary.
    pub is_binary: bool,
    /// Mode on old side (octal string, e.g. "100644").
    pub old_mode: Option<String>,
    /// Mode on new side (octal string, e.g. "100755").
    pub new_mode: Option<String>,
}

impl GitFileStatus {
    /// Convert this status item into a canonical [`diffuse_parse::DiffFile`].
    pub fn to_diff_file(&self) -> diffuse_parse::DiffFile {
        let mut f = diffuse_parse::DiffFile::new(&self.path);
        f.old_path = self.old_path.clone();
        f.status = self.status;
        f.is_binary = self.is_binary;
        f.old_mode = self.old_mode.clone();
        f.new_mode = self.new_mode.clone();
        f.has_file_headers = true;
        f
    }
}

impl Repository {
    /// The root path of the working directory, returning an error if bare.
    pub fn root_path(&self) -> Result<&Path, GitError> {
        self.work_dir().ok_or_else(|| {
            GitError::OperationFailed("repository has no working directory".to_string())
        })
    }

    /// Discover untracked repository-relative file paths matching optional
    /// `pathspecs`.
    ///
    /// Respects `.gitignore` rules hierarchically, `.git/info/exclude`, and
    /// global gitignore.
    pub fn untracked_paths(&self, pathspecs: &[String]) -> Result<Vec<PathBuf>, GitError> {
        let index = self.inner.index_or_empty()?;
        let should_interrupt = AtomicBool::new(false);
        let options = self
            .inner
            .dirwalk_options()?
            .emit_untracked(gix::dir::walk::EmissionMode::Matching);

        let mut collect = gix::dir::walk::delegate::Collect::default();
        let patterns: Vec<&gix::bstr::BStr> =
            pathspecs.iter().map(|p| p.as_bytes().as_bstr()).collect();

        self.inner
            .dirwalk(&index, patterns, &should_interrupt, options, &mut collect)?;

        let mut untracked = Vec::new();
        for (entry, _) in collect.into_entries_by_path() {
            if entry.status == gix::dir::entry::Status::Untracked
                && entry.disk_kind.map(|k| !k.is_dir()).unwrap_or(true)
            {
                untracked.push(PathBuf::from(entry.rela_path.to_string()));
            }
        }
        Ok(untracked)
    }

    /// Discover untracked files and synthesize [`diffuse_parse::DiffFile`]
    /// records for text files, separating binary files into
    /// `skipped_binary`.
    pub fn untracked_files(&self, pathspecs: &[String]) -> Result<UntrackedOutput, GitError> {
        let paths = self.untracked_paths(pathspecs)?;
        let workdir = self.root_path()?;
        let mut files = Vec::new();
        let mut skipped_binary = Vec::new();

        for rel_buf in paths {
            let rel_str = rel_buf.to_string_lossy().into_owned();
            let abs_path = workdir.join(&rel_buf);

            let bytes = match fs::read(&abs_path) {
                Ok(b) => b,
                Err(_) => continue,
            };

            // Detect binary via 8,000-byte null check
            if is_binary_buf(&bytes) {
                skipped_binary.push(rel_str);
                continue;
            }

            let text = match String::from_utf8(bytes) {
                Ok(t) => t,
                Err(_) => {
                    skipped_binary.push(rel_str);
                    continue;
                }
            };

            let line_count = text.lines().count();
            let mut synthetic_diff = format!(
                "diff --git a/{rel_str} b/{rel_str}\nuntracked file mode 100644\n--- /dev/null\n+++ b/{rel_str}\n"
            );
            if line_count > 0 {
                synthetic_diff.push_str(&format!("@@ -0,0 +1,{line_count} @@\n"));
                for line in text.lines() {
                    synthetic_diff.push('+');
                    synthetic_diff.push_str(line);
                    synthetic_diff.push('\n');
                }
            }

            let mut parsed = diffuse_parse::parse_diff(&synthetic_diff);
            for f in &mut parsed {
                f.status = diffuse_parse::FileStatus::Untracked;
                f.new_bytes = Some(text.len());
            }
            files.extend(parsed);
        }

        Ok(UntrackedOutput {
            files,
            skipped_binary,
        })
    }

    /// Query high-level file status entries for a given [`ReviewTarget`].
    pub fn file_statuses(&self, target: &ReviewTarget) -> Result<Vec<GitFileStatus>, GitError> {
        if let Some(range) = &target.range {
            return self.status_commit_range(&range.0, &range.1, &target.pathspecs);
        }
        if target.staged {
            return self.status_staged(&target.pathspecs);
        }
        self.status_unstaged(&target.pathspecs)
    }

    /// Diff two historical commits or refspecs and report file modifications,
    /// additions, deletions, and renames.
    pub fn status_commit_range(
        &self,
        old_rev: &str,
        new_rev: &str,
        pathspecs: &[String],
    ) -> Result<Vec<GitFileStatus>, GitError> {
        let old_id = self.inner.rev_parse_single(old_rev.trim())?.detach();
        let new_id = self.inner.rev_parse_single(new_rev.trim())?.detach();

        let old_tree = peel_to_tree(&self.inner, old_id)
            .ok_or_else(|| GitError::InvalidRef(format!("failed to peel {old_rev} to tree")))?;
        let new_tree = peel_to_tree(&self.inner, new_id)
            .ok_or_else(|| GitError::InvalidRef(format!("failed to peel {new_rev} to tree")))?;

        let changes = self
            .inner
            .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)?;

        let mut statuses = Vec::new();
        for change in changes {
            let (path, old_path, status, old_mode, new_mode) = match change {
                gix::object::tree::diff::ChangeDetached::Addition {
                    location,
                    entry_mode,
                    ..
                } => {
                    if entry_mode.is_tree() {
                        continue;
                    }
                    (
                        location.to_string(),
                        None,
                        diffuse_parse::FileStatus::Added,
                        None,
                        Some(format!("{:o}", entry_mode.value())),
                    )
                }
                gix::object::tree::diff::ChangeDetached::Deletion {
                    location,
                    entry_mode,
                    ..
                } => {
                    if entry_mode.is_tree() {
                        continue;
                    }
                    (
                        location.to_string(),
                        None,
                        diffuse_parse::FileStatus::Deleted,
                        Some(format!("{:o}", entry_mode.value())),
                        None,
                    )
                }
                gix::object::tree::diff::ChangeDetached::Modification {
                    location,
                    previous_entry_mode,
                    entry_mode,
                    previous_id,
                    id,
                } => {
                    if entry_mode.is_tree() || previous_entry_mode.is_tree() {
                        continue;
                    }
                    let st = if previous_id == id && previous_entry_mode != entry_mode {
                        diffuse_parse::FileStatus::ModeChanged
                    } else {
                        diffuse_parse::FileStatus::Modified
                    };
                    (
                        location.to_string(),
                        None,
                        st,
                        Some(format!("{:o}", previous_entry_mode.value())),
                        Some(format!("{:o}", entry_mode.value())),
                    )
                }
                gix::object::tree::diff::ChangeDetached::Rewrite {
                    source_location,
                    location,
                    entry_mode,
                    ..
                } => (
                    location.to_string(),
                    Some(source_location.to_string()),
                    diffuse_parse::FileStatus::Renamed,
                    None,
                    Some(format!("{:o}", entry_mode.value())),
                ),
            };

            if pathspecs_match(&path, pathspecs)
                || old_path
                    .as_ref()
                    .is_some_and(|p| pathspecs_match(p, pathspecs))
            {
                statuses.push(GitFileStatus {
                    path,
                    old_path,
                    status,
                    is_binary: false,
                    old_mode,
                    new_mode,
                });
            }
        }

        Ok(statuses)
    }

    /// Query staged changes comparing `HEAD^{tree}` to the current index.
    pub fn status_staged(&self, pathspecs: &[String]) -> Result<Vec<GitFileStatus>, GitError> {
        let head_tree_id = self
            .inner
            .head_tree_id_or_empty()
            .map_err(|e| GitError::OperationFailed(format!("failed to get HEAD tree: {e}")))?;
        let index = self.inner.index_or_empty()?;

        let mut statuses = Vec::new();
        self.inner.tree_index_status(
            &head_tree_id,
            &index,
            None,
            gix::status::tree_index::TrackRenames::AsConfigured,
            |change, _, _| {
                let (path, old_path, status, old_mode, new_mode) = match change {
                    gix::diff::index::ChangeRef::Addition {
                        location,
                        entry_mode,
                        ..
                    } => (
                        location.to_string(),
                        None,
                        diffuse_parse::FileStatus::Added,
                        None,
                        Some(format!("{:o}", entry_mode.bits())),
                    ),
                    gix::diff::index::ChangeRef::Deletion {
                        location,
                        entry_mode,
                        ..
                    } => (
                        location.to_string(),
                        None,
                        diffuse_parse::FileStatus::Deleted,
                        Some(format!("{:o}", entry_mode.bits())),
                        None,
                    ),
                    gix::diff::index::ChangeRef::Modification {
                        location,
                        previous_entry_mode,
                        entry_mode,
                        previous_id,
                        id,
                        ..
                    } => {
                        let st = if previous_id == id && previous_entry_mode != entry_mode {
                            diffuse_parse::FileStatus::ModeChanged
                        } else {
                            diffuse_parse::FileStatus::Modified
                        };
                        (
                            location.to_string(),
                            None,
                            st,
                            Some(format!("{:o}", previous_entry_mode.bits())),
                            Some(format!("{:o}", entry_mode.bits())),
                        )
                    }
                    gix::diff::index::ChangeRef::Rewrite {
                        source_location,
                        location,
                        entry_mode,
                        ..
                    } => (
                        location.to_string(),
                        Some(source_location.to_string()),
                        diffuse_parse::FileStatus::Renamed,
                        None,
                        Some(format!("{:o}", entry_mode.bits())),
                    ),
                };

                if pathspecs_match(&path, pathspecs)
                    || old_path
                        .as_ref()
                        .is_some_and(|p| pathspecs_match(p, pathspecs))
                {
                    statuses.push(GitFileStatus {
                        path,
                        old_path,
                        status,
                        is_binary: false,
                        old_mode,
                        new_mode,
                    });
                }

                Ok(ControlFlow::Continue(()))
            },
        )?;

        Ok(statuses)
    }

    /// Query unstaged changes comparing the current index to the active working
    /// tree.
    pub fn status_unstaged(&self, pathspecs: &[String]) -> Result<Vec<GitFileStatus>, GitError> {
        let platform = self
            .inner
            .status(gix::progress::Discard)?
            .index_worktree_rewrites(Some(gix::diff::Rewrites::default()))
            .untracked_files(gix::status::UntrackedFiles::None);

        let patterns: Vec<BString> = pathspecs
            .iter()
            .map(|s| BString::from(s.as_bytes()))
            .collect();

        let iter = platform.into_index_worktree_iter(patterns)?;
        let mut statuses = Vec::new();

        for item_res in iter {
            let item = item_res?;
            let summary = match item.summary() {
                Some(s) => s,
                None => continue,
            };

            let path = item.rela_path().to_string();
            let mut old_path = None;
            let mut old_mode = None;
            let new_mode = match &item {
                gix::status::index_worktree::Item::Modification {
                    status:
                        gix::status::plumbing::index_as_worktree::EntryStatus::Change(
                            gix::status::plumbing::index_as_worktree::Change::Type {
                                worktree_mode,
                            },
                        ),
                    ..
                } => Some(format!("{:o}", worktree_mode.bits())),
                _ => None,
            };

            let file_status = match summary {
                gix::status::plumbing::index_as_worktree_with_renames::Summary::Removed => {
                    diffuse_parse::FileStatus::Deleted
                }
                gix::status::plumbing::index_as_worktree_with_renames::Summary::TypeChange => {
                    diffuse_parse::FileStatus::ModeChanged
                }
                gix::status::plumbing::index_as_worktree_with_renames::Summary::Modified
                | gix::status::plumbing::index_as_worktree_with_renames::Summary::Conflict => {
                    diffuse_parse::FileStatus::Modified
                }
                gix::status::plumbing::index_as_worktree_with_renames::Summary::IntentToAdd
                | gix::status::plumbing::index_as_worktree_with_renames::Summary::Added => {
                    diffuse_parse::FileStatus::Added
                }
                gix::status::plumbing::index_as_worktree_with_renames::Summary::Renamed
                | gix::status::plumbing::index_as_worktree_with_renames::Summary::Copied => {
                    if let gix::status::index_worktree::Item::Rewrite { source, .. } = &item {
                        old_path = Some(source.rela_path().to_string());
                    }
                    diffuse_parse::FileStatus::Renamed
                }
            };

            if let gix::status::index_worktree::Item::Modification { entry, .. } = &item {
                old_mode = Some(format!("{:o}", entry.mode.bits()));
            }

            statuses.push(GitFileStatus {
                path,
                old_path,
                status: file_status,
                is_binary: false,
                old_mode,
                new_mode,
            });
        }

        Ok(statuses)
    }
}
