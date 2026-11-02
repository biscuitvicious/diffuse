//! Invertible Alignment Tape & Reversible Run Algebra.
//!
//! Represents a token-level alignment between two lines or blocks as a
//! strongly typed, monoidal tape of atomic operations (`Keep`, `Strike`,
//! `Fill`, `Relocate`).
//!
//! Guarantees by construction:
//! - `project_old(&tape) == old_text`
//! - `project_new(&tape) == new_text`
//! - Invertibility: reverse of a tape is symmetric
//! - No lossy coordinate guessing

pub mod builder;
pub mod chunk;
pub mod core;
pub mod spans;

#[cfg(test)]
mod tests;

pub use chunk::TapeChunk;
pub use core::AlignmentTape;
