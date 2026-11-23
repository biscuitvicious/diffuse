//! Output Note & Fence Models.
//!
//! Structured review notes and fenced syntax blocks serialized within
//! the wire contract.

use serde::{Deserialize, Serialize};

use super::{OutputRow, OutputRun};
use crate::notes::{LineHash, NoteStatus};

/// A review note attached to a file location (see
/// [`crate::notes::NoteLocation`]).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputNote {
    pub id: String,
    pub file: String,
    pub location: crate::notes::NoteLocation,
    pub text: String,
    pub created_at: u64,
    #[serde(
        serialize_with = "crate::wire::run::serialize_batched_runs",
        deserialize_with = "crate::wire::run::deserialize_batched_runs"
    )]
    pub runs: Vec<OutputRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<LineHash>,
    #[serde(default, skip_serializing_if = "NoteStatus::is_open")]
    pub status: NoteStatus,
    /// The output rows captured at note creation time — the original source
    /// the note was written against, preserved for display.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub original_source_rows: Vec<OutputRow>,
}

impl OutputNote {
    pub fn reverse(mut self) -> Self {
        self.location = self.location.reverse();
        self.runs = self.runs.into_iter().map(OutputRun::reverse).collect();
        self.original_source_rows = self
            .original_source_rows
            .into_iter()
            .map(OutputRow::reverse)
            .collect();
        self
    }
}

impl From<&OutputNote> for crate::notes::ReviewNote {
    fn from(n: &OutputNote) -> Self {
        crate::notes::ReviewNote {
            id: n.id.clone(),
            file: n.file.clone(),
            location: n.location,
            text: n.text.clone(),
            created_at: n.created_at,
            content_hash: n.content_hash,
            status: n.status,
            original_source_rows: n.original_source_rows.clone(),
        }
    }
}

/// A fenced syntax region in a file: the span of lines whose content
/// classifies with a different language than the file's.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputFence {
    pub start: u32,
    pub end: u32,
    pub lang: String,
}
