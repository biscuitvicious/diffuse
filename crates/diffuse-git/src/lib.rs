//! Pure Gitoxide repository operations, revision resolution, tree diffing,
//! and index mutation for diffuse.

pub mod diff;
pub mod mutation;
pub mod objects;
pub mod repository;
pub mod status;
pub mod types;

pub use diff::{
    FileDiffParams, FileDiffResult, RepoDiffResult, WhitespaceMode, diff_buffers, diff_repository,
    diff_text, generate_file_diff,
};
pub use mutation::{
    apply_hunk_to_buffer, create_stash_checkpoint, discard_file, discard_hunk, stage_file,
    stage_hunk, unstage_file, unstage_hunk,
};
pub use repository::Repository;
pub use status::{GitFileStatus, UntrackedOutput};
pub use types::{
    GitBlob, GitDiffOptions, GitError, RefPair, Repo, ReviewTarget, RevisionRange,
    parse_git_rev_and_pathspecs,
};

#[cfg(test)]
mod tests;
