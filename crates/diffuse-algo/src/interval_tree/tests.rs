use super::*;

#[test]
fn test_empty_tree_properties_and_queries() {
    let tree: AugmentedIntervalTree<&str> = AugmentedIntervalTree::new();
    assert_eq!(tree.len(), 0);
    assert!(tree.is_empty());
    assert!(tree.nodes().is_empty());
    assert!(!tree.has_stabbing(10));
    assert!(!tree.has_overlapping(0, 100));

    let mut matches = Vec::new();
    tree.stabbing_query(10, &mut matches);
    assert!(matches.is_empty());

    tree.overlapping_query(0, 100, &mut matches);
    assert!(matches.is_empty());

    let mut count = 0;
    tree.visit_stabbing(5, |_| count += 1);
    assert_eq!(count, 0);

    tree.visit_overlapping(5, 10, |_| count += 1);
    assert_eq!(count, 0);
}

#[test]
fn test_single_interval_boundary_conditions() {
    let intervals = [(10, 20, "payload")];
    let tree = AugmentedIntervalTree::from_slice(&intervals);
    assert_eq!(tree.len(), 1);
    assert!(!tree.is_empty());

    // Stabbing tests on half-open [10, 20)
    assert!(!tree.has_stabbing(9), "point before start must not stab");
    assert!(tree.has_stabbing(10), "point on inclusive start must stab");
    assert!(tree.has_stabbing(15), "point strictly inside must stab");
    assert!(
        tree.has_stabbing(19),
        "point immediately before end must stab"
    );
    assert!(
        !tree.has_stabbing(20),
        "point on exclusive end must not stab"
    );
    assert!(!tree.has_stabbing(21), "point after end must not stab");

    let mut stabbed = Vec::new();
    tree.stabbing_query(15, &mut stabbed);
    assert_eq!(stabbed, vec![&"payload"]);

    // Overlapping tests against query range [start, end)
    assert!(
        !tree.has_overlapping(0, 10),
        "adjacent left range [0, 10) must not overlap"
    );
    assert!(
        tree.has_overlapping(5, 12),
        "left overlap [5, 12) must overlap"
    );
    assert!(
        tree.has_overlapping(12, 18),
        "strictly contained range [12, 18) must overlap"
    );
    assert!(
        tree.has_overlapping(18, 25),
        "right overlap [18, 25) must overlap"
    );
    assert!(
        !tree.has_overlapping(20, 30),
        "adjacent right range [20, 30) must not overlap"
    );
    assert!(
        tree.has_overlapping(0, 30),
        "strictly enclosing range [0, 30) must overlap"
    );
    assert!(
        !tree.has_overlapping(15, 15),
        "zero-length query range [15, 15) must not overlap"
    );
    assert!(
        !tree.has_overlapping(20, 10),
        "inverted query range [20, 10) must not overlap"
    );
}

#[test]
fn test_nested_and_overlapping_intervals() {
    let intervals = vec![
        (0, 100, "root"),
        (10, 30, "left_a"),
        (20, 40, "left_b"),
        (50, 60, "mid"),
        (70, 80, "right"),
    ];
    let tree = AugmentedIntervalTree::from_slice(&intervals);
    assert_eq!(tree.len(), 5);

    // Stabbing at point 25 hits [0, 100), [10, 30), and [20, 40)
    let mut matches = Vec::new();
    tree.stabbing_query(25, &mut matches);
    matches.sort();
    assert_eq!(matches, vec![&"left_a", &"left_b", &"root"]);

    // Stabbing at point 65 hits only [0, 100)
    matches.clear();
    tree.stabbing_query(65, &mut matches);
    assert_eq!(matches, vec![&"root"]);

    // Overlapping range [25, 55) hits [0, 100), [10, 30), [20, 40), and [50,
    // 60)
    matches.clear();
    tree.overlapping_query(25, 55, &mut matches);
    matches.sort();
    assert_eq!(matches, vec![&"left_a", &"left_b", &"mid", &"root"]);

    // Overlapping range [85, 95) hits only [0, 100)
    matches.clear();
    tree.overlapping_query(85, 95, &mut matches);
    assert_eq!(matches, vec![&"root"]);

    // Overlapping range [100, 150) hits nothing
    matches.clear();
    tree.overlapping_query(100, 150, &mut matches);
    assert!(matches.is_empty());
}

#[test]
fn test_from_vec_ownership_and_sort_in_place() {
    let intervals = vec![(50, 60, 3), (10, 20, 1), (30, 40, 2), (0, 100, 0)];
    let tree = AugmentedIntervalTree::from_vec(intervals);
    assert_eq!(tree.len(), 4);

    let mut matches = Vec::new();
    tree.stabbing_query(35, &mut matches);
    matches.sort();
    assert_eq!(matches, vec![&0, &2]);
}

#[test]
fn test_tree_rebuild_reuses_buffer_allocation() {
    let mut tree = AugmentedIntervalTree::new();
    let initial = [(0, 10, 1), (10, 20, 2), (20, 30, 3)];
    tree.build_from_slice(&initial);
    assert_eq!(tree.len(), 3);
    let cap = tree.nodes.capacity();

    let next = [(5, 15, 10), (15, 25, 20)];
    tree.build_from_slice(&next);
    assert_eq!(tree.len(), 2);
    assert_eq!(
        tree.nodes.capacity(),
        cap,
        "rebuilding must preserve vector capacity"
    );

    let mut matches = Vec::new();
    tree.stabbing_query(10, &mut matches);
    assert_eq!(matches, vec![&10]);
}

#[test]
fn test_differential_brute_force_equivalence() {
    // Deterministic pseudo-random interval generator
    let mut intervals: Vec<(usize, usize, usize)> = Vec::new();
    let mut state = 0x12345678u64;
    let mut next_rand = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 32) as usize
    };

    for id in 0..150 {
        let low = next_rand() % 500;
        let len = 1 + (next_rand() % 80);
        let high = low + len;
        intervals.push((low, high, id));
    }

    let tree = AugmentedIntervalTree::from_slice(&intervals);

    // Stabbing queries
    for point in 0..600 {
        let mut expected: Vec<usize> = intervals
            .iter()
            .filter(|(l, h, _)| *l <= point && point < *h)
            .map(|(_, _, id)| *id)
            .collect();
        expected.sort();

        let mut actual: Vec<usize> = Vec::new();
        tree.visit_stabbing(point, |node| {
            actual.push(node.value);
        });
        actual.sort();

        assert_eq!(
            actual, expected,
            "stabbing query at point {point} must match brute force"
        );
        assert_eq!(
            tree.has_stabbing(point),
            !expected.is_empty(),
            "has_stabbing at point {point} must match expected presence"
        );
    }

    // Overlapping queries
    for start in (0..550).step_by(25) {
        for len in [5, 20, 50, 100] {
            let end = start + len;
            let mut expected: Vec<usize> = intervals
                .iter()
                .filter(|(l, h, _)| *l < end && start < *h)
                .map(|(_, _, id)| *id)
                .collect();
            expected.sort();

            let mut actual: Vec<usize> = Vec::new();
            tree.visit_overlapping(start, end, |node| {
                actual.push(node.value);
            });
            actual.sort();

            assert_eq!(
                actual, expected,
                "overlapping query for [{start}, {end}) must match brute force"
            );
            assert_eq!(
                tree.has_overlapping(start, end),
                !expected.is_empty(),
                "has_overlapping for [{start}, {end}) must match expected presence"
            );
        }
    }
}

#[test]
fn test_node_query_helpers_and_zero_length_intervals() {
    let intervals = [(10, 10, "empty_span"), (20, 30, "valid_span")];
    let tree = AugmentedIntervalTree::from_slice(&intervals);

    // A zero-length half-open interval [10, 10) cannot contain any integer
    // points
    assert!(!tree.has_stabbing(10));
    assert!(!tree.has_overlapping(5, 15));

    let mut nodes = Vec::new();
    tree.stabbing_nodes(25, &mut nodes);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].low, 20);
    assert_eq!(nodes[0].high, 30);
    assert_eq!(nodes[0].value, "valid_span");

    nodes.clear();
    tree.overlapping_nodes(25, 35, &mut nodes);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].value, "valid_span");
}

#[test]
fn test_default_clear_and_empty_construction_variants() {
    let mut tree: AugmentedIntervalTree<u32> = AugmentedIntervalTree::default();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);

    // Empty slice / vec variations
    let empty_slice: &[(usize, usize, u32)] = &[];
    let tree_slice = AugmentedIntervalTree::from_sorted_slice(empty_slice);
    assert!(tree_slice.is_empty());

    let tree_vec: AugmentedIntervalTree<u32> = AugmentedIntervalTree::from_vec(Vec::new());
    assert!(tree_vec.is_empty());

    // Populate and clear
    tree.build_from_slice(&[(10, 20, 1), (30, 40, 2)]);
    assert_eq!(tree.len(), 2);
    tree.clear();
    assert!(tree.is_empty());
    assert_eq!(tree.len(), 0);

    // build_from_slice with empty slice triggers clear
    tree.build_from_slice(&[(10, 20, 1)]);
    assert_eq!(tree.len(), 1);
    tree.build_from_slice(&[]);
    assert!(tree.is_empty());
}

#[test]
fn test_from_sorted_slice_and_inverted_interval_clamping() {
    let sorted_intervals = vec![(5, 15, "first"), (20, 30, "second"), (35, 25, "inverted")];
    let tree = AugmentedIntervalTree::from_sorted_slice(&sorted_intervals);
    assert_eq!(tree.len(), 3);

    // The inverted interval (35, 25) must be clamped to (35, 35)
    let node = &tree.nodes()[2];
    assert_eq!(node.low, 35);
    assert_eq!(node.high, 35);
    assert_eq!(node.value, "inverted");
    assert!(!node.contains_point(35));
    assert!(!node.overlaps_range(30, 40));

    let mut matches = Vec::new();
    tree.stabbing_query(10, &mut matches);
    assert_eq!(matches, vec![&"first"]);

    matches.clear();
    tree.stabbing_query(25, &mut matches);
    assert_eq!(matches, vec![&"second"]);

    matches.clear();
    tree.stabbing_query(35, &mut matches);
    assert!(matches.is_empty());
}
