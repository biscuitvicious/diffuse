//! Property and differential equivalence tests for diffuse-algo.
//!
//! Verifies that optimized algorithms (`BitParallelLcs`, `HirschbergLcs`,
//! `HuntSzymanskiLcs`, `MyersOnD`, `BoundedDpLcs`) match canonical
//! `MatrixDpLcs` reference behavior across randomized input spaces.

use diffuse_algo::{
    AdaptiveEnvelopeBuilder, AugmentedIntervalTree, BitParallelLcs, BoundedDpLcs, HirschbergLcs,
    HuntSzymanskiLcs, LcsEngine, MatrixDpLcs, MyersOnD, SkylineMatrix,
};

/// Deterministic pseudo-random number generator (Linear Congruential
/// Generator).
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }

    fn next_usize(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next_u32() as usize) % bound
        }
    }

    fn random_string(&mut self, len: usize, alphabet: &[u8]) -> Vec<u8> {
        (0..len)
            .map(|_| alphabet[self.next_usize(alphabet.len())])
            .collect()
    }
}

#[test]
fn test_lcs_differential_equivalence_across_engines() {
    let mut rng = SimpleRng::new(0xDEADBEEFCAFE);
    let alphabet = b"ACGT0123";

    let matrix_dp = MatrixDpLcs::new();
    let myers = MyersOnD::new();
    let hirsch = HirschbergLcs::new();
    let hunt = HuntSzymanskiLcs::new();
    let bit_par = BitParallelLcs::new();

    for trial in 0..100 {
        let len_a = rng.next_usize(50);
        let len_b = rng.next_usize(50);
        let a = rng.random_string(len_a, alphabet);
        let b = rng.random_string(len_b, alphabet);

        let pairs_matrix = matrix_dp.lcs_indices(&a, &b);
        let pairs_myers = myers.lcs_indices(&a, &b);
        let pairs_hirsch = hirsch.lcs_indices(&a, &b);
        let pairs_hunt = hunt.lcs_indices(&a, &b);
        let pairs_bit = bit_par.lcs_indices(&a, &b);

        let expected_len = pairs_matrix.len();
        assert_eq!(
            pairs_myers.len(),
            expected_len,
            "trial {trial}: Myers LCS length mismatch"
        );
        assert_eq!(
            pairs_hirsch.len(),
            expected_len,
            "trial {trial}: Hirschberg LCS length mismatch"
        );
        assert_eq!(
            pairs_hunt.len(),
            expected_len,
            "trial {trial}: Hunt-Szymanski LCS length mismatch"
        );
        assert_eq!(
            pairs_bit.len(),
            expected_len,
            "trial {trial}: Bit-Parallel LCS length mismatch"
        );

        // Verify valid monotonic match pairs for all engines
        for (name, pairs) in [
            ("Myers", &pairs_myers),
            ("Hirschberg", &pairs_hirsch),
            ("Hunt-Szymanski", &pairs_hunt),
            ("Bit-Parallel", &pairs_bit),
        ] {
            for &(ai, bi) in pairs {
                assert_eq!(
                    a[ai], b[bi],
                    "trial {trial}: {name} pairing ({ai}, {bi}) does not match"
                );
            }
            if pairs.len() > 1 {
                for w in pairs.windows(2) {
                    assert!(
                        w[0].0 < w[1].0 && w[0].1 < w[1].1,
                        "trial {trial}: {name} pairings are not strictly monotonic"
                    );
                }
            }
        }
    }
}

#[test]
fn test_bounded_dp_equivalence_within_bandwidth() {
    let mut rng = SimpleRng::new(0x42424242);
    let alphabet = b"abcdefgh";

    let matrix_dp = MatrixDpLcs::new();
    let bounded = BoundedDpLcs::new();

    // Sequences with high similarity (low edit distance), well within bandwidth
    for _ in 0..50 {
        let base_len = 10 + rng.next_usize(30);
        let a = rng.random_string(base_len, alphabet);
        let mut b = a.clone();

        // Introduce 1-2 point mutations
        for _ in 0..2 {
            if !b.is_empty() {
                let idx = rng.next_usize(b.len());
                b[idx] = alphabet[rng.next_usize(alphabet.len())];
            }
        }

        let matrix_pairs = matrix_dp.lcs_indices(&a, &b);
        let bounded_pairs = bounded.lcs_indices(&a, &b);
        assert_eq!(
            matrix_pairs.len(),
            bounded_pairs.len(),
            "Bounded DP must find maximal LCS within bandwidth"
        );
    }
}

#[test]
fn test_interval_tree_differential_equivalence() {
    let mut rng = SimpleRng::new(0x987654321);
    let mut intervals = Vec::new();

    for id in 0..200 {
        let low = rng.next_usize(1000);
        let len = 1 + rng.next_usize(150);
        let high = low + len;
        intervals.push((low, high, id));
    }

    let tree = AugmentedIntervalTree::from_slice(&intervals);

    // Test 100 random stabbing queries
    for _ in 0..100 {
        let point = rng.next_usize(1200);
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

        assert_eq!(actual, expected, "Stabbing query at {point} mismatch");
        assert_eq!(tree.has_stabbing(point), !expected.is_empty());
    }

    // Test 100 random range overlap queries
    for _ in 0..100 {
        let start = rng.next_usize(1000);
        let len = 1 + rng.next_usize(200);
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
            "Overlap query at [{start}, {end}) mismatch"
        );
        assert_eq!(tree.has_overlapping(start, end), !expected.is_empty());
    }
}

#[test]
#[allow(clippy::needless_range_loop)]
fn test_skyline_matrix_dense_oracle_verification() {
    let m = 30;
    let n = 40;

    let envelope = AdaptiveEnvelopeBuilder::new(m, n)
        .with_bandwidth_min(8)
        .with_bandwidth_slack(4)
        .with_budget(500)
        .build();

    let mut matrix = SkylineMatrix::<u32>::new_empty();
    matrix.init_envelope(m, n, &envelope.bounds, 0);

    let mut dense_oracle = vec![vec![0u32; n + 1]; m + 1];

    for i in 0..=m {
        for j in matrix.row_range(i) {
            let val = (i * 100 + j) as u32;
            if let Some(cell) = matrix.get_mut(i, j) {
                *cell = val;
            }
            dense_oracle[i][j] = val;
        }
    }

    for i in 0..=m {
        for j in 0..=n {
            if matrix.is_in_envelope(i, j) {
                assert_eq!(
                    matrix.get(i, j).copied(),
                    Some(dense_oracle[i][j]),
                    "SkylineMatrix ({i}, {j}) must match dense oracle"
                );
            } else {
                assert_eq!(matrix.get(i, j), None);
            }
        }
    }
}
