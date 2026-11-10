//! Git plumbing, ref resolution, and repo status loader for diffuse.
//!
//! Port of TypeScript's `resolveRefs`, `loadReview`, and `rustBackendLoad`.

pub mod blob;
pub mod load;
pub mod stream;

use std::{collections::HashMap, path::Path};

pub use blob::{load_git_blob, load_git_blobs_batch, load_git_blobs_native};
pub use diffuse_git::{
    RefPair, Repository, RevisionCandidate, RevisionKind, WhitespaceMode, diff_paths,
    diff_repository, generate_file_diff,
};
pub use load::{load_git_repo, load_git_repo_with_notes, load_single_file_diff};
pub use stream::stream_git_repo_to_producer;

/// Fixed unified-diff context width used when generating diffs from git (`-U`).
///
/// This is the *engine* band: the DP/shift engine needs enough context around
/// changes to slide hunks and anchor against syntax. It is separate from the
/// *visual* context the user controls via `-U/--context` — the pipeline's last
/// step normalizes the engine's context down (or up) to the requested visual
/// width.
pub const ENGINE_CONTEXT: usize = 4;

/// Default *visual* context lines (`-U/--context` default). Displayed context,
/// applied by the pipeline's final normalization step.
pub const DEFAULT_CONTEXT_LINES: usize = 4;

/// Default number of lines to expand up and down when activating a hidden gap
/// separator.
pub const DEFAULT_EXPAND_LINES: usize = 10;

/// Check if the directory is inside a git repository or worktree.
pub fn is_inside_work_tree(cwd: &Path) -> bool {
    gix::discover(cwd).is_ok()
}

/// Bare unified-diff header (no hunks) for a file surfaced solely so review
/// notes can be attached to it; used for unmodified reference files that carry
/// review notes without an anchorable hunk.
pub(crate) fn synthetic_unmodified_diff(file_path: &str) -> String {
    format!(
        "diff --git a/{file_path} b/{file_path}\nunmodified file mode 100644\n--- a/{file_path}\n+++ b/{file_path}\n"
    )
}

/// Get current repository branch name via native gix.
pub fn get_current_branch(cwd: &Path) -> String {
    diffuse_git::Repository::discover(cwd)
        .ok()
        .and_then(|r| r.current_branch())
        .unwrap_or_default()
}

/// Resolve old/new git refs from base args (e.g. `main..feature`, `HEAD^!`,
/// `main...feature`).
pub fn resolve_refs(_cwd: &Path, base_args: &[String], staged: bool) -> RefPair {
    RefPair::resolve(base_args, staged)
}

/// Resolves commit range short hashes from base args (e.g. `main..feature` ->
/// `1a2b3c4..5d6e7f8`). Returns `None` if the diff is not a commit-to-commit
/// range (e.g. diff against working tree).
pub fn resolve_commit_range(cwd: &Path, base_args: &[String]) -> Option<String> {
    diffuse_git::Repository::discover(cwd)
        .ok()
        .and_then(|r| r.resolve_commit_range(base_args))
}

#[derive(Debug, Clone)]
pub struct GitFileItem {
    pub path: String,
    pub status: String,
    pub old_path: Option<String>,
    pub new_content: Option<String>,
    pub old_content: Option<String>,
    pub no_content: bool,
}

#[derive(Debug)]
pub struct LoadGitResult {
    pub diff: String,
    pub new_content: HashMap<String, String>,
    pub old_content: HashMap<String, String>,
    pub branch: String,
    pub untracked_skipped: Vec<String>,
    pub commit_range: Option<String>,
}

#[cfg(test)]
mod tests;
