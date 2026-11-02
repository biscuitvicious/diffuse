//! Canonical Git domain types for diffuse.

pub mod error;
pub mod options;
pub mod range;
pub mod repo;
pub mod target;

pub use error::GitError;
pub use options::{GitBlob, GitDiffOptions};
pub use range::{RefPair, RevisionRange};
pub use repo::Repo;
pub use target::{ReviewTarget, parse_git_rev_and_pathspecs};
