//! Generic bipartite matching and spatial coalescence engine.
//!
//! Provides multiset bipartite matching, spatial neighborhood expansion,
//! and cluster cohesion filtering decoupled from diff domain structures.

pub mod matcher;
pub mod matches;
pub mod neighborhood;

#[cfg(test)]
mod tests;

pub use matcher::{
    BipartiteMatcher, DiffuseBipartiteExt, expand_neighborhood_matches, greedy_multiset_match,
};
pub use matches::{RelocationMatch, RelocationMatches, filter_cohesion_runs};
pub use neighborhood::{HunkGridNeighborhood, PositionedItem, SpatialNeighborhood};
