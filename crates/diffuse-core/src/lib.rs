//! diffuse-core — the diff-engine token-stream backend.

pub use diffuse_syntax as syntax;

pub mod algo;
pub mod apply;
pub mod cluster;
pub mod collapse;
pub mod counts;
pub mod git;
pub mod lcs;
pub mod merge;
pub mod moves;
pub mod notes;
pub mod old_content;
pub mod parse;
pub mod pipeline;
pub mod scope;
pub mod shift;
pub mod similarity;
pub mod streaming;
pub mod visible;
pub mod wire;
