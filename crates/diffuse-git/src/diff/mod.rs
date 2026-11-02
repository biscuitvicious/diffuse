//! Pure in-process unified diff generator with trim-equality semantics.

pub mod file;
pub mod hunk;
pub mod line;
pub mod repo;

pub use file::{FileDiffParams, FileDiffResult, generate_file_diff};
pub use hunk::{AstHunkConsumer, diff_buffers, diff_text};
pub use line::{DiffLine, DiffLines, WhitespaceMode, split_lines};
pub use repo::{RepoDiffResult, diff_paths, diff_repository};
