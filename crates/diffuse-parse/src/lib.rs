//! Unified diff parsing and core AST data structures.
//!
//! Models the diff hierarchy: `DiffFile` -> `Hunk` -> `Row`.

pub mod diff;
pub mod file;
pub mod hunk;
pub mod row;

pub use diff::{
    parse_diff, parse_diff_for_each, parse_diff_from_reader, parse_diff_lines,
    parse_diff_lines_with_raw, parse_diff_with_raw_from_reader, zero_hunk_section,
};
pub use file::{
    DiffFile, FileStatus, hunk_path, is_valid_diff_file, parse_diff_git_paths, unquote_git_path,
};
pub use hunk::{Hunk, add_count, del_count, parse_hunk_header};
pub use row::{
    AddLine, CtxLine, DelLine, GroupId, PeerGroupMeta, Row, RowGroup, RowKind, RunKind, Spans,
};

#[cfg(test)]
mod tests;
