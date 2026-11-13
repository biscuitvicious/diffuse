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
    let hunk_lens = [10, 10];
    let grid = HunkGridNeighborhood::new(&hunk_lens);

    // Run 1: First line did not move -> all unmarked
    let mut matches1 = vec![
        RelocationMatch::new((0, 1), (1, 1)),
        RelocationMatch::new((0, 2), (1, 2)),
    ];
    let run1 = [(0, 0), (0, 1), (0, 2)];
    filter_cohesion_runs(&mut matches1, [run1], |_| false, &grid);
    assert!(matches1.is_empty());

    // Run 2: First line is trivial -> all unmarked
    let mut matches2 = vec![
        RelocationMatch::new((0, 0), (1, 0)),
        RelocationMatch::new((0, 1), (1, 1)),
    ];
    let run2 = [(0, 0), (0, 1)];
    filter_cohesion_runs(&mut matches2, [run2], |p| p.1 == 0, &grid);
    assert!(matches2.is_empty());

    // Run 3: Entire run moved -> all retained
    let mut matches3 = vec![
        RelocationMatch::new((0, 0), (1, 0)),
        RelocationMatch::new((0, 1), (1, 1)),
        RelocationMatch::new((0, 2), (1, 2)),
    ];
    let run3 = [(0, 0), (0, 1), (0, 2)];
    filter_cohesion_runs(&mut matches3, [run3], |_| false, &grid);
    assert_eq!(matches3.len(), 3);

    // Run 4: Prefix >= 2 moved -> prefix retained, rest unmarked
    let mut matches4 = vec![
        RelocationMatch::new((0, 0), (1, 0)),
        RelocationMatch::new((0, 1), (1, 1)),
    ];
    let run4 = [(0, 0), (0, 1), (0, 2), (0, 3)];
    filter_cohesion_runs(&mut matches4, [run4], |_| false, &grid);
    assert_eq!(matches4.len(), 2);

    // Run 5: Prefix < 2 moved -> all unmarked
    let mut matches5 = vec![RelocationMatch::new((0, 0), (1, 0))];
    let run5 = [(0, 0), (0, 1), (0, 2)];
    filter_cohesion_runs(&mut matches5, [run5], |_| false, &grid);
    assert!(matches5.is_empty());

    // Run 6: Isolated moved lines after unmoved line -> all unmarked
    let mut matches6 = vec![
        RelocationMatch::new((0, 0), (1, 0)),
        RelocationMatch::new((0, 2), (1, 2)),
    ];
    let run6 = [(0, 0), (0, 1), (0, 2)];
    filter_cohesion_runs(&mut matches6, [run6], |_| false, &grid);
    assert!(matches6.is_empty());

    // Run 7: Empty run
    let mut matches7 = vec![RelocationMatch::new((0, 0), (1, 0))];
    let empty_run: [(usize, usize); 0] = [];
    filter_cohesion_runs(&mut matches7, [empty_run], |_| false, &grid);
    assert_eq!(matches7.len(), 1);

    // Run 8: Isolated moved line after unmoved line causes entire run to be
    // unmarked
    let mut matches8 = vec![
        RelocationMatch::new((0, 0), (1, 0)),
        RelocationMatch::new((0, 1), (1, 1)),
        RelocationMatch::new((0, 3), (1, 3)),
    ];
    let run8 = [(0, 0), (0, 1), (0, 2), (0, 3)];
    filter_cohesion_runs(&mut matches8, [run8], |_| false, &grid);
    assert!(matches8.is_empty());

    // Run 9: Severed target contiguity on trivial line — source is contiguous
    // (0,0)..(0,2), but target for trivial line (0,2) jumps to (1,8) instead of
    // (1,2). The severed trivial line (0,2) must be unmarked; prefix
    // (0,0)..(0,1) retained.
    let mut matches9 = vec![
        RelocationMatch::new((0, 0), (1, 0)),
        RelocationMatch::new((0, 1), (1, 1)),
        RelocationMatch::new((0, 2), (1, 8)),
    ];
    let run9 = [(0, 0), (0, 1), (0, 2)];
    filter_cohesion_runs(&mut matches9, [run9], |p| p.1 == 2, &grid);
    assert_eq!(matches9.len(), 2);
    assert_eq!(matches9[0], RelocationMatch::new((0, 0), (1, 0)));
    assert_eq!(matches9[1], RelocationMatch::new((0, 1), (1, 1)));
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

#[test]
fn test_bipartite_matcher_solve_filtered() {
    let hunk_lens = [5, 5];
    let grid = HunkGridNeighborhood::new(&hunk_lens);

    let mut matcher = BipartiteMatcher::new();
    matcher.add_source("fn a()", (0, 0));
    matcher.add_source("fn b()", (0, 1));

    matcher.add_target("fn a()", (1, 0));
    matcher.add_target("fn b()", (1, 1));

    let runs = [vec![(0, 1)]];

    // Filter disallowing (0, 0) -> (1, 0)
    let result = matcher.solve_filtered(
        &[],
        &grid,
        &grid,
        |_| false,
        |_| false,
        runs,
        |_| false,
        |src, tgt| !(src.1 == 0 && tgt.1 == 0),
    );

    let matches: Vec<_> = result.collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0], RelocationMatch::new((0, 1), (1, 1)));
}
