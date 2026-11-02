//! Standard Git error enumeration.

/// Standard error type for diffuse-git operations, wrapping underlying errors
/// from Gitoxide and I/O.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git error: {0}")]
    Gix(#[from] gix::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Git operation failed: {0}")]
    OperationFailed(String),

    #[error("Repository not found: {0}")]
    NotFound(String),

    #[error("Invalid reference: {0}")]
    InvalidRef(String),
}
