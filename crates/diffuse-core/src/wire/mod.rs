//! Diffuse Wire Contract Data Models & Serialization Schema.
//!
//! Defines the structured JSON wire contract output emitted by the engine
//! and consumed by external tooling, the CLI (`--json`), and the pi review
//! extension. The schema is frozen per PLAN.md §2.

pub mod file;
pub mod hunk;
pub mod jsonl;
pub mod note;
pub mod pipeline;
pub mod row;
pub mod run;

#[cfg(test)]
mod tests;

pub use diffuse_syntax::color::TokenColor;
pub use file::*;
pub use hunk::*;
pub use jsonl::{WireRecord, decode_jsonl_records, emit_file_jsonl};
pub use note::*;
pub use pipeline::*;
pub use row::*;
pub use run::*;

pub use crate::parse::{FileStatus, GroupId, PeerGroup, PeerGroupMeta};

#[cfg(test)]
pub(crate) use crate::counts::FileCounts;
#[cfg(test)]
pub(crate) use row::test_rows_pair_with;
