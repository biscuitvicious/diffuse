//! Generic bipartite matching and spatial coalescence engine.
//!
//! Provides multiset bipartite matching, spatial neighborhood expansion,
//! and cluster cohesion filtering decoupled from diff domain structures.

use std::{hash::Hash, iter::FusedIterator};

use rustc_hash::{FxHashMap, FxHashSet};

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

/// Greedily matches source and target items by key equality into relocation
/// matches.
pub fn greedy_multiset_match<K, P>(
    sources: &[PositionedItem<K, P>],
    targets: &[PositionedItem<K, P>],
) -> Vec<RelocationMatch<P>>
where
    K: Eq + Hash + Clone,
    P: Copy + Eq + Hash,
{
    let mut counts: FxHashMap<&K, usize> = FxHashMap::default();
    let mut targets_by_key: FxHashMap<&K, Vec<P>> = FxHashMap::default();

    for target in targets {
        *counts.entry(&target.key).or_insert(0) += 1;
        targets_by_key
            .entry(&target.key)
            .or_default()
            .push(target.position);
    }

    if targets_by_key.is_empty() {
        return Vec::new();
    }

    let mut target_cursor: FxHashMap<&K, usize> = FxHashMap::default();
    let mut used_targets: FxHashSet<P> = FxHashSet::default();
    let mut matches: Vec<RelocationMatch<P>> = Vec::new();

    for source in sources {
        let n = counts.get(&source.key).copied().unwrap_or(0);
        if n == 0 {
            continue;
        }
        counts.insert(&source.key, n - 1);

        let cursor = target_cursor.entry(&source.key).or_insert(0);
        if let Some(positions) = targets_by_key.get(&source.key) {
            while *cursor < positions.len() {
                let pos = positions[*cursor];
                *cursor += 1;
                if !used_targets.contains(&pos) {
                    used_targets.insert(pos);
                    matches.push(RelocationMatch::new(source.position, pos));
                    break;
                }
            }
        }
    }

    matches
}

/// Expands existing matches to adjacent items satisfying predicate conditions.
pub fn expand_neighborhood_matches<P, N, Fs, Ft>(
    matches: &mut Vec<RelocationMatch<P>>,
    candidate_sources: &[P],
    neighborhood_source: &N,
    neighborhood_target: &N,
    can_expand_source: Fs,
    can_expand_target: Ft,
) where
    P: Copy + Eq + Hash,
    N: SpatialNeighborhood<P>,
    Fs: Fn(&P) -> bool,
    Ft: Fn(&P) -> bool,
{
    let mut pair_of: FxHashMap<P, P> = FxHashMap::default();
    let mut used_targets: FxHashSet<P> = FxHashSet::default();

    for m in matches.iter() {
        pair_of.insert(m.source_pos, m.target_pos);
        used_targets.insert(m.target_pos);
    }

    for &src in candidate_sources {
        if !can_expand_source(&src) {
            continue;
        }

        let mut target_candidate: Option<P> = None;

        // Check predecessor
        if let Some(pred_src) = neighborhood_source.predecessor(&src)
            && let Some(&pred_tgt) = pair_of.get(&pred_src)
            && let Some(cand_tgt) = neighborhood_target.successor(&pred_tgt)
            && !used_targets.contains(&cand_tgt)
            && can_expand_target(&cand_tgt)
        {
            target_candidate = Some(cand_tgt);
        }

        // Check successor
        if target_candidate.is_none()
            && let Some(succ_src) = neighborhood_source.successor(&src)
            && let Some(&succ_tgt) = pair_of.get(&succ_src)
            && let Some(cand_tgt) = neighborhood_target.predecessor(&succ_tgt)
            && !used_targets.contains(&cand_tgt)
            && can_expand_target(&cand_tgt)
        {
            target_candidate = Some(cand_tgt);
        }

        if let Some(cand_tgt) = target_candidate {
            used_targets.insert(cand_tgt);
            pair_of.insert(src, cand_tgt);
            matches.push(RelocationMatch::new(src, cand_tgt));
        }
    }
}

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

/// Extension trait for ergonomic bipartite matching operations.
pub trait DiffuseBipartiteExt<K, P> {
    /// Computes greedy multiset relocation matches against candidate targets.
    fn greedy_bipartite_match(&self, targets: &[PositionedItem<K, P>]) -> Vec<RelocationMatch<P>>;
}

impl<K, P> DiffuseBipartiteExt<K, P> for [PositionedItem<K, P>]
where
    K: Eq + Hash + Clone,
    P: Copy + Eq + Hash,
{
    fn greedy_bipartite_match(&self, targets: &[PositionedItem<K, P>]) -> Vec<RelocationMatch<P>> {
        greedy_multiset_match(self, targets)
    }
}

/// Generic bipartite matching builder and solver.
#[derive(Clone, Debug, Default)]
pub struct BipartiteMatcher<K, P> {
    sources: Vec<PositionedItem<K, P>>,
    targets: Vec<PositionedItem<K, P>>,
}

impl<K, P> BipartiteMatcher<K, P> {
    /// Creates an empty bipartite matcher.
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            targets: Vec::new(),
        }
    }

    /// Creates an empty bipartite matcher with preallocated capacity.
    pub fn with_capacity(source_cap: usize, target_cap: usize) -> Self {
        Self {
            sources: Vec::with_capacity(source_cap),
            targets: Vec::with_capacity(target_cap),
        }
    }

    /// Adds a source item.
    pub fn add_source(&mut self, key: K, position: P) -> &mut Self {
        self.sources.push(PositionedItem::new(key, position));
        self
    }

    /// Adds a target item.
    pub fn add_target(&mut self, key: K, position: P) -> &mut Self {
        self.targets.push(PositionedItem::new(key, position));
        self
    }

    /// Returns the recorded source items.
    pub fn sources(&self) -> &[PositionedItem<K, P>] {
        &self.sources
    }

    /// Returns the recorded target items.
    pub fn targets(&self) -> &[PositionedItem<K, P>] {
        &self.targets
    }
}

impl<K, P> BipartiteMatcher<K, P>
where
    K: Eq + Hash + Clone,
    P: Copy + Eq + Hash,
{
    /// Computes greedy multiset matches between sources and targets.
    pub fn match_multiset(&self) -> Vec<RelocationMatch<P>> {
        greedy_multiset_match(&self.sources, &self.targets)
    }

    /// Solves the bipartite matching problem with neighborhood expansion and
    /// spatial cohesion filtering.
    #[allow(clippy::too_many_arguments)]
    pub fn solve<N, Fs, Ft, I, Ftriv>(
        &self,
        candidate_sources: &[P],
        neighborhood_source: &N,
        neighborhood_target: &N,
        can_expand_source: Fs,
        can_expand_target: Ft,
        runs: impl IntoIterator<Item = I>,
        is_trivial: Ftriv,
    ) -> RelocationMatches<P>
    where
        N: SpatialNeighborhood<P>,
        Fs: Fn(&P) -> bool,
        Ft: Fn(&P) -> bool,
        I: IntoIterator<Item = P>,
        Ftriv: Fn(&P) -> bool,
    {
        let mut matches = self.match_multiset();
        expand_neighborhood_matches(
            &mut matches,
            candidate_sources,
            neighborhood_source,
            neighborhood_target,
            can_expand_source,
            can_expand_target,
        );
        filter_cohesion_runs(&mut matches, runs, is_trivial);
        RelocationMatches::new(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greedy_multiset_matching_basic() {
        let mut matcher = BipartiteMatcher::new();
        matcher.add_source("fn main()", (0, 1));
        matcher.add_source("println!();", (0, 2));

        matcher.add_target("println!();", (1, 10));
        matcher.add_target("fn main()", (1, 9));

        let matches = matcher.match_multiset();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0], RelocationMatch::new((0, 1), (1, 9)));
        assert_eq!(matches[1], RelocationMatch::new((0, 2), (1, 10)));
    }

    #[test]
    fn test_empty_targets_or_sources() {
        let sources = [PositionedItem::new(10, 0)];
        let targets: [PositionedItem<i32, usize>; 0] = [];
        let matches = sources.as_slice().greedy_bipartite_match(&targets);
        assert!(matches.is_empty());

        let empty_sources: [PositionedItem<i32, usize>; 0] = [];
        let targets2 = [PositionedItem::new(10, 1)];
        let matches2 = empty_sources.as_slice().greedy_bipartite_match(&targets2);
        assert!(matches2.is_empty());
    }

    #[test]
    fn test_multiset_duplicate_keys_and_excess() {
        let mut matcher = BipartiteMatcher::with_capacity(3, 2);
        matcher.add_source("x = 1", 100);
        matcher.add_source("x = 1", 101);
        matcher.add_source("x = 1", 102);

        matcher.add_target("x = 1", 200);
        matcher.add_target("x = 1", 201);

        assert_eq!(matcher.sources().len(), 3);
        assert_eq!(matcher.targets().len(), 2);

        let matches = matcher.match_multiset();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0], RelocationMatch::new(100, 200));
        assert_eq!(matches[1], RelocationMatch::new(101, 201));
    }

    #[test]
    fn test_spatial_neighborhood_implementations() {
        // Test 1D ()
        let n1d = ();
        assert!(n1d.is_contiguous_successor(&5, &6));
        assert!(!n1d.is_contiguous_successor(&5, &7));
        assert_eq!(n1d.predecessor(&5), Some(4));
        assert_eq!(n1d.predecessor(&0), None);
        assert_eq!(n1d.successor(&5), Some(6));

        // Test HunkGridNeighborhood
        let hunk_lens = [5, 10];
        let grid = HunkGridNeighborhood::new(&hunk_lens);
        assert!(grid.is_contiguous_successor(&(0, 2), &(0, 3)));
        assert!(!grid.is_contiguous_successor(&(0, 2), &(1, 3)));
        assert!(!grid.is_contiguous_successor(&(0, 2), &(0, 4)));

        assert_eq!(grid.predecessor(&(0, 0)), None);
        assert_eq!(grid.predecessor(&(0, 2)), Some((0, 1)));
        assert_eq!(grid.predecessor(&(2, 1)), None); // out of bounds hunk

        assert_eq!(grid.successor(&(0, 4)), None); // at hunk end
        assert_eq!(grid.successor(&(0, 3)), Some((0, 4)));
        assert_eq!(grid.successor(&(2, 0)), None); // out of bounds hunk
    }

    #[test]
    fn test_neighborhood_expansion() {
        let hunk_lens = [10, 10];
        let grid = HunkGridNeighborhood::new(&hunk_lens);

        // Predecessor expansion:
        // Source (0, 1) matched with Target (1, 5).
        // Candidate source (0, 2) is blank. Target (1, 6) is blank and
        // available.
        let mut matches = vec![RelocationMatch::new((0, 1), (1, 5))];
        let candidates = [(0, 2)];

        expand_neighborhood_matches(
            &mut matches,
            &candidates,
            &grid,
            &grid,
            |p| p.1 == 2, // can expand source
            |p| p.1 == 6, // can expand target
        );

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[1], RelocationMatch::new((0, 2), (1, 6)));

        // Successor expansion:
        // Source (0, 4) matched with Target (1, 8).
        // Candidate source (0, 3) is blank. Target (1, 7) is blank and
        // available.
        let candidates_succ = [(0, 3)];
        expand_neighborhood_matches(
            &mut matches,
            &candidates_succ,
            &grid,
            &grid,
            |p| p.1 == 3,
            |p| p.1 == 7,
        );
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[2], RelocationMatch::new((0, 3), (1, 7)));

        // Target not meeting predicate should not expand
        let candidates_fail = [(0, 4)];
        expand_neighborhood_matches(
            &mut matches,
            &candidates_fail,
            &grid,
            &grid,
            |_| true,
            |_| false, // target rejected
        );
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_cohesion_run_filtering() {
        // Run 1: First line did not move -> all unmarked
        let mut matches1 = vec![
            RelocationMatch::new((0, 1), (1, 1)),
            RelocationMatch::new((0, 2), (1, 2)),
        ];
        let run1 = [(0, 0), (0, 1), (0, 2)];
        filter_cohesion_runs(&mut matches1, [run1], |_| false);
        assert!(matches1.is_empty());

        // Run 2: First line is trivial -> all unmarked
        let mut matches2 = vec![
            RelocationMatch::new((0, 0), (1, 0)),
            RelocationMatch::new((0, 1), (1, 1)),
        ];
        let run2 = [(0, 0), (0, 1)];
        filter_cohesion_runs(&mut matches2, [run2], |p| p.1 == 0);
        assert!(matches2.is_empty());

        // Run 3: Entire run moved -> all retained
        let mut matches3 = vec![
            RelocationMatch::new((0, 0), (1, 0)),
            RelocationMatch::new((0, 1), (1, 1)),
            RelocationMatch::new((0, 2), (1, 2)),
        ];
        let run3 = [(0, 0), (0, 1), (0, 2)];
        filter_cohesion_runs(&mut matches3, [run3], |_| false);
        assert_eq!(matches3.len(), 3);

        // Run 4: Prefix >= 2 moved -> prefix retained, rest unmarked
        let mut matches4 = vec![
            RelocationMatch::new((0, 0), (1, 0)),
            RelocationMatch::new((0, 1), (1, 1)),
        ];
        let run4 = [(0, 0), (0, 1), (0, 2), (0, 3)];
        filter_cohesion_runs(&mut matches4, [run4], |_| false);
        assert_eq!(matches4.len(), 2);

        // Run 5: Prefix < 2 moved -> all unmarked
        let mut matches5 = vec![RelocationMatch::new((0, 0), (1, 0))];
        let run5 = [(0, 0), (0, 1), (0, 2)];
        filter_cohesion_runs(&mut matches5, [run5], |_| false);
        assert!(matches5.is_empty());

        // Run 6: Isolated moved lines after unmoved line -> all unmarked
        let mut matches6 = vec![
            RelocationMatch::new((0, 0), (1, 0)),
            RelocationMatch::new((0, 2), (1, 2)),
        ];
        let run6 = [(0, 0), (0, 1), (0, 2)];
        filter_cohesion_runs(&mut matches6, [run6], |_| false);
        assert!(matches6.is_empty());

        // Run 7: Empty run
        let mut matches7 = vec![RelocationMatch::new((0, 0), (1, 0))];
        let empty_run: [(usize, usize); 0] = [];
        filter_cohesion_runs(&mut matches7, [empty_run], |_| false);
        assert_eq!(matches7.len(), 1);

        // Run 8: Isolated moved line after unmoved line causes entire run to be
        // unmarked
        let mut matches8 = vec![
            RelocationMatch::new((0, 0), (1, 0)),
            RelocationMatch::new((0, 1), (1, 1)),
            RelocationMatch::new((0, 3), (1, 3)),
        ];
        let run8 = [(0, 0), (0, 1), (0, 2), (0, 3)];
        filter_cohesion_runs(&mut matches8, [run8], |_| false);
        assert!(matches8.is_empty());
    }

    #[test]
    fn test_relocation_matches_iterator() {
        let m = vec![
            RelocationMatch::new(1, 10),
            RelocationMatch::new(2, 20),
            RelocationMatch::new(3, 30),
        ];
        let mut iter = RelocationMatches::new(m.clone());

        assert_eq!(iter.len(), 3);
        assert!(!iter.is_empty());
        assert_eq!(iter.as_slice().len(), 3);
        assert_eq!(iter.size_hint(), (3, Some(3)));

        assert_eq!(iter.next(), Some(RelocationMatch::new(1, 10)));
        assert_eq!(iter.len(), 2);

        assert_eq!(iter.next_back(), Some(RelocationMatch::new(3, 30)));
        assert_eq!(iter.len(), 1);

        assert_eq!(iter.next(), Some(RelocationMatch::new(2, 20)));
        assert_eq!(iter.len(), 0);
        assert!(iter.is_empty());

        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);

        let iter2 = RelocationMatches::new(m.clone());
        assert_eq!(iter2.into_vec(), m);
    }

    #[test]
    fn test_bipartite_matcher_solve_end_to_end() {
        let hunk_lens = [5, 5];
        let grid = HunkGridNeighborhood::new(&hunk_lens);

        let mut matcher = BipartiteMatcher::new();
        matcher.add_source("pub fn compute() {", (0, 0));
        matcher.add_source("    let x = 42;", (0, 1));
        // line (0, 2) is blank ""

        matcher.add_target("pub fn compute() {", (1, 0));
        matcher.add_target("    let x = 42;", (1, 1));
        // line (1, 2) is blank ""

        let candidate_sources = [(0, 2)];
        let runs = [vec![(0, 0), (0, 1), (0, 2)]];

        let result = matcher.solve(
            &candidate_sources,
            &grid,
            &grid,
            |p| p.1 == 2,
            |p| p.1 == 2,
            runs,
            |_| false,
        );

        let matches: Vec<_> = result.collect();
        assert_eq!(matches.len(), 3);
        assert_eq!(matches[0], RelocationMatch::new((0, 0), (1, 0)));
        assert_eq!(matches[1], RelocationMatch::new((0, 1), (1, 1)));
        assert_eq!(matches[2], RelocationMatch::new((0, 2), (1, 2)));
    }
}
