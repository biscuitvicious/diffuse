//! Unified diff parsing and core AST data structures.
//!
//! Re-exports canonical types and parsing routines from `diffuse-parse`
//! for backward compatibility across the diffuse workspace.

pub use diffuse_parse::*;

#[cfg(test)]
mod tests;
