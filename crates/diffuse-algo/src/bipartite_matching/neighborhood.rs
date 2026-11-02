//! Spatial neighborhood query traits and positioned coordinate items.

use std::hash::Hash;

/// A keyed item associated with a spatial position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PositionedItem<K, P> {
    pub key: K,
    pub position: P,
}

impl<K, P> PositionedItem<K, P> {
    /// Creates a new positioned item.
    pub const fn new(key: K, position: P) -> Self {
        Self { key, position }
    }
}

/// Spatial neighborhood query trait for coordinate spaces.
pub trait SpatialNeighborhood<P> {
    /// Returns true if `next` immediately follows `prev` in spatial sequence.
    fn is_contiguous_successor(&self, prev: &P, next: &P) -> bool;

    /// Returns the immediate predecessor position, if one exists in the
    /// neighborhood.
    fn predecessor(&self, pos: &P) -> Option<P> {
        let _ = pos;
        None
    }

    /// Returns the immediate successor position, if one exists in the
    /// neighborhood.
    fn successor(&self, pos: &P) -> Option<P> {
        let _ = pos;
        None
    }
}

impl SpatialNeighborhood<usize> for () {
    fn is_contiguous_successor(&self, prev: &usize, next: &usize) -> bool {
        prev.saturating_add(1) == *next
    }

    fn predecessor(&self, pos: &usize) -> Option<usize> {
        pos.checked_sub(1)
    }

    fn successor(&self, pos: &usize) -> Option<usize> {
        pos.checked_add(1)
    }
}

/// Spatial neighborhood implementation for 2D hunk/row grid coordinates
/// `(hunk_idx, row_idx)`.
#[derive(Clone, Copy, Debug)]
pub struct HunkGridNeighborhood<'a> {
    hunk_lens: &'a [usize],
}

impl<'a> HunkGridNeighborhood<'a> {
    /// Creates a new grid neighborhood bounded by hunk row lengths.
    pub const fn new(hunk_lens: &'a [usize]) -> Self {
        Self { hunk_lens }
    }
}

impl SpatialNeighborhood<(usize, usize)> for HunkGridNeighborhood<'_> {
    fn is_contiguous_successor(&self, prev: &(usize, usize), next: &(usize, usize)) -> bool {
        prev.0 == next.0 && prev.1.saturating_add(1) == next.1
    }

    fn predecessor(&self, pos: &(usize, usize)) -> Option<(usize, usize)> {
        let (hi, ri) = *pos;
        if hi < self.hunk_lens.len() && ri > 0 {
            Some((hi, ri - 1))
        } else {
            None
        }
    }

    fn successor(&self, pos: &(usize, usize)) -> Option<(usize, usize)> {
        let (hi, ri) = *pos;
        if let Some(&len) = self.hunk_lens.get(hi)
            && ri + 1 < len
        {
            return Some((hi, ri + 1));
        }
        None
    }
}
