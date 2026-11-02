//! Relocation match representations, streaming iterators, and cohesion
//! filtering.

use std::{hash::Hash, iter::FusedIterator};

use rustc_hash::FxHashSet;

/// A matched relocation pairing a source position to a target position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RelocationMatch<P> {
    pub source_pos: P,
    pub target_pos: P,
}

impl<P> RelocationMatch<P> {
    /// Creates a new relocation match.
    pub const fn new(source_pos: P, target_pos: P) -> Self {
        Self {
            source_pos,
            target_pos,
        }
    }
}

/// Streaming iterator yielding relocation matches.
#[derive(Clone, Debug, Default)]
pub struct RelocationMatches<P> {
    matches: Vec<RelocationMatch<P>>,
    start: usize,
    end: usize,
}

impl<P> RelocationMatches<P> {
    /// Constructs a new streaming iterator from a vector of relocation matches.
    pub fn new(matches: Vec<RelocationMatch<P>>) -> Self {
        let end = matches.len();
        Self {
            matches,
            start: 0,
            end,
        }
    }

    /// Returns the remaining relocation matches as a slice.
    pub fn as_slice(&self) -> &[RelocationMatch<P>] {
        &self.matches[self.start..self.end]
    }

    /// Consumes the iterator and returns the underlying vector of matches.
    pub fn into_vec(self) -> Vec<RelocationMatch<P>> {
        self.matches
    }

    /// Returns the number of remaining elements.
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns true if there are no remaining elements.
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

impl<P: Copy> Iterator for RelocationMatches<P> {
    type Item = RelocationMatch<P>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start < self.end {
            let item = self.matches[self.start];
            self.start += 1;
            Some(item)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl<P: Copy> ExactSizeIterator for RelocationMatches<P> {}

impl<P: Copy> DoubleEndedIterator for RelocationMatches<P> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.start < self.end {
            self.end -= 1;
            Some(self.matches[self.end])
        } else {
            None
        }
    }
}

impl<P: Copy> FusedIterator for RelocationMatches<P> {}

/// Filters candidate matches based on spatial clustering and block cohesion
/// rules.
pub fn filter_cohesion_runs<P, I, F>(
    matches: &mut Vec<RelocationMatch<P>>,
    runs: impl IntoIterator<Item = I>,
    is_trivial: F,
) where
    P: Copy + Eq + Hash,
    I: IntoIterator<Item = P>,
    F: Fn(&P) -> bool,
{
    let moved_sources: FxHashSet<P> = matches.iter().map(|m| m.source_pos).collect();
    let mut unmark_positions: FxHashSet<P> = FxHashSet::default();

    for run_iter in runs {
        let run: Vec<P> = run_iter.into_iter().collect();
        if run.is_empty() {
            continue;
        }

        let first_pos = run[0];
        let first_moved = moved_sources.contains(&first_pos);
        let first_trivial = is_trivial(&first_pos);

        let has_nontrivial_move = run
            .iter()
            .any(|pos| moved_sources.contains(pos) && !is_trivial(pos));

        if !first_moved || first_trivial || !has_nontrivial_move {
            unmark_positions.extend(
                run.iter()
                    .copied()
                    .filter(|pos| moved_sources.contains(pos)),
            );
            continue;
        }

        // Block Move Cohesion Rule:
        // In a multi-line source run, isolated matches are coincidental
        // boilerplate. A run is only marked moved if:
        // - The ENTIRE deletion run moved, OR
        // - A contiguous prefix block of >= 2 lines starting at run[0] moved.
        // Otherwise, all lines in the run remain unmoved.
        if run.len() > 1 {
            let moved_in_run: Vec<P> = run
                .iter()
                .copied()
                .filter(|pos| moved_sources.contains(pos))
                .collect();
            if moved_in_run.len() < 2
                || !moved_sources.contains(&run[0])
                || moved_in_run.len() < run.len()
            {
                let prefix_len = run
                    .iter()
                    .take_while(|pos| moved_sources.contains(pos))
                    .count();
                if prefix_len < 2 || prefix_len < moved_in_run.len() {
                    unmark_positions.extend(moved_in_run);
                    continue;
                } else {
                    unmark_positions.extend(
                        run.iter()
                            .skip(prefix_len)
                            .copied()
                            .filter(|pos| moved_sources.contains(pos)),
                    );
                }
            }
        }

        // If only isolated trivial lines at the end of the run moved, unmark
        // them
        for i in 0..run.len() {
            let pos = run[i];
            if moved_sources.contains(&pos)
                && is_trivial(&pos)
                && i > 0
                && !moved_sources.contains(&run[i - 1])
            {
                unmark_positions.insert(pos);
            }
        }
    }

    matches.retain(|m| !unmark_positions.contains(&m.source_pos));
}
