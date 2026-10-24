//! Pure Gitoxide repository operations, revision resolution, tree diffing,
//! and index mutation for diffuse.

pub mod objects;
pub mod repository;
pub mod status;
pub mod types;

pub use repository::Repository;
pub use status::{GitFileStatus, UntrackedOutput};
pub use types::{
    GitBlob, GitDiffOptions, GitError, RefPair, Repo, ReviewTarget, RevisionRange,
    parse_git_rev_and_pathspecs,
};

#[cfg(test)]
mod tests;
