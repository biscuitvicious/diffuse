//! Gotoh Affine Gap Sequence Aligner with Zero-Allocation Workspace.
//!
//! Academic foundation:
//! - Osamu Gotoh (1982). "An Improved Algorithm for Matching Biological
//!   Sequences". Journal of Molecular Biology, 162(3), 705–708.
//! - Saul B. Needleman and Christian D. Wunsch (1970). "A General Method
//!   Applicable to the Search for Similarities in the Amino Acid Sequence of
//!   Two Proteins". Journal of Molecular Biology, 48(3), 443–453.
//!
//! # Strict Architectural Separation
//! Operates over generic slices `&[T]` where `T: PartialEq`.
//! Zero diffuse-domain dependencies.
//! Reusable [`AffineGapWorkspace`] avoids heap allocations across repeated
//! runs.

mod penalties;
mod steps;
mod workspace;

#[cfg(test)]
mod tests;

pub use penalties::AffineGapPenalties;
pub use steps::AffineAlignmentSteps;
pub use workspace::{AffineGapWorkspace, DiffuseAffineExt, align_affine};
