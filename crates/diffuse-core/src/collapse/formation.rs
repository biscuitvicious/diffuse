//! Peer reflow formation spans within a change cluster.

use std::ops::Range;

use crate::parse::GroupId;

/// A contiguous formation span within a change cluster, pairing `old_range`
/// against `new_range` under a shared [`GroupId`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formation {
    /// Unique sequential group id within this cluster.
    pub id: GroupId,
    /// Range of old row indices in the cluster: `old_rows[old_range]`.
    pub old_range: Range<usize>,
    /// Range of new row indices in the cluster: `new_rows[new_range]`.
    pub new_range: Range<usize>,
}

impl Formation {
    /// Number of old-side rows in this formation.
    #[inline]
    pub fn old_count(&self) -> usize {
        self.old_range.len()
    }

    /// Number of new-side rows in this formation.
    #[inline]
    pub fn new_count(&self) -> usize {
        self.new_range.len()
    }
}
