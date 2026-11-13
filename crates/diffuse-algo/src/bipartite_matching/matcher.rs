//! Greedy multiset bipartite matching algorithms and neighborhood expansion.

use std::hash::Hash;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    matches::{RelocationMatch, RelocationMatches, filter_cohesion_runs},
    neighborhood::{PositionedItem, SpatialNeighborhood},
};

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
    greedy_multiset_match_filtered(sources, targets, |_, _| true)
}

/// Greedily matches source and target items by key equality into relocation
/// matches, subject to an admission filter.
pub fn greedy_multiset_match_filtered<K, P, F>(
    sources: &[PositionedItem<K, P>],
    targets: &[PositionedItem<K, P>],
    can_match: F,
) -> Vec<RelocationMatch<P>>
where
    K: Eq + Hash + Clone,
    P: Copy + Eq + Hash,
    F: Fn(&P, &P) -> bool,
{
    let mut targets_by_key: FxHashMap<&K, Vec<P>> = FxHashMap::default();

    for target in targets {
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
        if let Some(positions) = targets_by_key.get(&source.key) {
            let cursor = target_cursor.entry(&source.key).or_insert(0);
            while *cursor < positions.len() {
                let pos = positions[*cursor];
                *cursor += 1;
                if !used_targets.contains(&pos) && can_match(&source.position, &pos) {
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
    expand_neighborhood_matches_filtered(
        matches,
        candidate_sources,
        neighborhood_source,
        neighborhood_target,
        can_expand_source,
        can_expand_target,
        |_, _| true,
    );
}

/// Expands existing matches to adjacent items satisfying predicate conditions
/// subject to an admission filter.
pub fn expand_neighborhood_matches_filtered<P, N, Fs, Ft, Ffilt>(
    matches: &mut Vec<RelocationMatch<P>>,
    candidate_sources: &[P],
    neighborhood_source: &N,
    neighborhood_target: &N,
    can_expand_source: Fs,
    can_expand_target: Ft,
    can_match: Ffilt,
) where
    P: Copy + Eq + Hash,
    N: SpatialNeighborhood<P>,
    Fs: Fn(&P) -> bool,
    Ft: Fn(&P) -> bool,
    Ffilt: Fn(&P, &P) -> bool,
{
    let mut pair_of: FxHashMap<P, P> = FxHashMap::default();
    let mut used_targets: FxHashSet<P> = FxHashSet::default();

    for m in matches.iter() {
        pair_of.insert(m.source_pos, m.target_pos);
        used_targets.insert(m.target_pos);
    }

    let mut remaining: Vec<P> = candidate_sources
        .iter()
        .copied()
        .filter(|src| can_expand_source(src))
        .collect();

    loop {
        let mut matched_any = false;
        remaining.retain(|&src| {
            let mut target_candidate: Option<P> = None;

            // Check predecessor
            if let Some(pred_src) = neighborhood_source.predecessor(&src)
                && let Some(&pred_tgt) = pair_of.get(&pred_src)
                && let Some(cand_tgt) = neighborhood_target.successor(&pred_tgt)
                && !used_targets.contains(&cand_tgt)
                && can_expand_target(&cand_tgt)
                && can_match(&src, &cand_tgt)
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
                && can_match(&src, &cand_tgt)
            {
                target_candidate = Some(cand_tgt);
            }

            if let Some(cand_tgt) = target_candidate {
                used_targets.insert(cand_tgt);
                pair_of.insert(src, cand_tgt);
                matches.push(RelocationMatch::new(src, cand_tgt));
                matched_any = true;
                false
            } else {
                true
            }
        });

        if !matched_any {
            break;
        }
    }
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
        self.solve_filtered(
            candidate_sources,
            neighborhood_source,
            neighborhood_target,
            can_expand_source,
            can_expand_target,
            runs,
            is_trivial,
            |_, _| true,
        )
    }

    /// Solves the bipartite matching problem with neighborhood expansion and
    /// spatial cohesion filtering, subject to a source-target admission filter.
    #[allow(clippy::too_many_arguments)]
    pub fn solve_filtered<N, Fs, Ft, I, Ftriv, Ffilt>(
        &self,
        candidate_sources: &[P],
        neighborhood_source: &N,
        neighborhood_target: &N,
        can_expand_source: Fs,
        can_expand_target: Ft,
        runs: impl IntoIterator<Item = I>,
        is_trivial: Ftriv,
        can_match: Ffilt,
    ) -> RelocationMatches<P>
    where
        N: SpatialNeighborhood<P>,
        Fs: Fn(&P) -> bool,
        Ft: Fn(&P) -> bool,
        I: IntoIterator<Item = P>,
        Ftriv: Fn(&P) -> bool,
        Ffilt: Fn(&P, &P) -> bool,
    {
        let mut matches = greedy_multiset_match_filtered(&self.sources, &self.targets, &can_match);
        expand_neighborhood_matches_filtered(
            &mut matches,
            candidate_sources,
            neighborhood_source,
            neighborhood_target,
            can_expand_source,
            can_expand_target,
            &can_match,
        );
        filter_cohesion_runs(&mut matches, runs, is_trivial, neighborhood_target);
        RelocationMatches::new(matches)
    }
}
