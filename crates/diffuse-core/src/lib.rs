//! diffuse-core — the diff-engine token-stream backend.

pub use cluster::dp::{
    DEFAULT_CLUSTER_BANDWIDTH_MIN, DEFAULT_CLUSTER_BANDWIDTH_SLACK, DEFAULT_CLUSTER_BUDGET,
    DEFAULT_CLUSTER_PRODUCT_CAP,
};
pub use git::{DEFAULT_CONTEXT_LINES, DEFAULT_EXPAND_LINES, ENGINE_CONTEXT};
pub use lcs::DEFAULT_TOKEN_PRODUCT_CAP;
pub use notes::{LineHash, NoteStatus, NotesDB, ReviewNote, Side};
pub use parse::RowGroup;
pub use pipeline::{PipelineInput, PipelineOptions};
pub use repo::{Repo, ReviewTarget, RevisionRange, parse_git_rev_and_pathspecs};
pub use token::{TokenClass, WhitespaceKind};
pub mod apply;
pub mod cluster;
pub mod collapse;
pub mod counts;
pub mod dbc;
pub mod git;
pub mod lcs;
pub mod logging;
pub mod merge;
pub mod moves;
pub mod notes;
pub mod old_content;
pub mod parse;
pub mod pipeline;
pub mod reflow;
pub mod repo;
pub mod scope;
pub mod shift;
pub mod similarity;
pub mod streaming;
pub mod tape;
pub mod token;
pub mod visible;
pub mod wire;
