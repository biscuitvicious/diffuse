//! Unit tests for generic algorithm traits and reference implementations.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Token {
    id: u32,
    name: &'static str,
}

#[test]
fn test_matrix_dp_lcs_integers() {
    let lcs = MatrixDpLcs::new();
    let a = [10, 20, 30, 40, 50];
    let b = [20, 35, 40, 60];
    let pairs = lcs.lcs_indices(&a, &b);
    assert_eq!(pairs, vec![(1, 0), (3, 2)]);
}

#[test]
fn test_matrix_dp_lcs_strings() {
    let lcs = MatrixDpLcs::new();
    let a = ["the", "quick", "brown", "fox"];
    let b = ["the", "fast", "brown", "dog"];
    let pairs = lcs.lcs_indices(&a, &b);
    assert_eq!(pairs, vec![(0, 0), (2, 2)]);
}

#[test]
fn test_matrix_dp_lcs_custom_structs() {
    let lcs = MatrixDpLcs::new();
    let a = [
        Token { id: 1, name: "fn" },
        Token {
            id: 2,
            name: "main",
        },
        Token { id: 3, name: "()" },
    ];
    let b = [
        Token { id: 1, name: "fn" },
        Token {
            id: 4,
            name: "test",
        },
        Token { id: 3, name: "()" },
    ];
    let pairs = lcs.lcs_indices(&a, &b);
    assert_eq!(pairs, vec![(0, 0), (2, 2)]);
}

#[test]
fn test_matrix_dp_lcs_empty_and_disjoint() {
    let lcs = MatrixDpLcs::new();
    let empty: [i32; 0] = [];
    let full = [1, 2, 3];
    assert!(lcs.lcs_indices(&empty, &full).is_empty());
    assert!(lcs.lcs_indices(&full, &empty).is_empty());

    let a = [1, 2, 3];
    let b = [4, 5, 6];
    assert!(lcs.lcs_indices(&a, &b).is_empty());
}

#[test]
fn test_matrix_dp_diff_slices() {
    let engine = MatrixDpLcs::new();
    let a = ["a", "b", "c", "d"];
    let b = ["a", "c", "e", "d"];
    let ops = engine.diff_slices(&a, &b);

    assert_eq!(
        ops,
        vec![
            EditOp::Keep { a_idx: 0, b_idx: 0 },
            EditOp::Delete { a_idx: 1 },
            EditOp::Keep { a_idx: 2, b_idx: 1 },
            EditOp::Insert { b_idx: 2 },
            EditOp::Keep { a_idx: 3, b_idx: 3 },
        ]
    );
}

#[test]
fn test_trie_prefix_matcher() {
    let matcher =
        TriePrefixMatcher::from_patterns(["//", "/*", "///", "/**", "fn", "let mut", "let"]);

    assert_eq!(
        matcher.longest_matching_prefix("/// doc comment"),
        Some("///")
    );
    assert_eq!(
        matcher.longest_matching_prefix("// regular comment"),
        Some("//")
    );
    assert_eq!(
        matcher.longest_matching_prefix("let mut x = 1;"),
        Some("let mut")
    );
    assert_eq!(matcher.longest_matching_prefix("let x = 1;"), Some("let"));
    assert_eq!(matcher.longest_matching_prefix("fn test()"), Some("fn"));
    assert_eq!(matcher.longest_matching_prefix("struct Foo"), None);
    assert!(matcher.has_matching_prefix("/** block */"));
    assert!(!matcher.has_matching_prefix("unmatched text"));
}

#[test]
fn test_bit_parallel_lcs_matches_matrix_dp() {
    let bp = BitParallelLcs::new();
    let matrix = MatrixDpLcs::new();

    let cases: Vec<(&[u32], &[u32])> = vec![
        (&[], &[]),
        (&[1, 2, 3], &[]),
        (&[], &[1, 2, 3]),
        (&[1, 2, 3, 4, 5], &[1, 2, 3, 4, 5]),
        (&[1, 2, 3, 4, 5], &[2, 4, 6]),
        (&[10, 20, 30, 40, 50, 60], &[5, 10, 25, 30, 45, 60, 70]),
        (&[1, 1, 1, 1], &[1, 1]),
        (&[1, 2, 1, 2, 1], &[2, 1, 2]),
    ];

    for (a, b) in cases {
        let pairs_bp = bp.lcs_indices(a, b);
        let pairs_matrix = matrix.lcs_indices(a, b);
        assert_eq!(
            pairs_bp.len(),
            pairs_matrix.len(),
            "LCS lengths must match exactly: bp={pairs_bp:?}, matrix={pairs_matrix:?}"
        );
        // Verify valid monotonic matching
        for &(ai, bi) in &pairs_bp {
            assert_eq!(a[ai], b[bi]);
        }
        let diff_bp = bp.diff_slices(a, b);
        let diff_matrix = matrix.diff_slices(a, b);
        let kept_bp = diff_bp
            .iter()
            .filter(|op| matches!(op, EditOp::Keep { .. }))
            .count();
        let kept_matrix = diff_matrix
            .iter()
            .filter(|op| matches!(op, EditOp::Keep { .. }))
            .count();
        assert_eq!(kept_bp, kept_matrix);
    }
}

#[test]
fn test_bit_parallel_pseudo_random_fuzz_equivalence() {
    let bp = BitParallelLcs::new();
    let matrix = MatrixDpLcs::new();

    // Deterministic linear congruential generator for reproducibility
    let mut state = 0x1234_5678_u64;
    let mut rand = || -> u32 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (state >> 32) as u32
    };

    for _ in 0..100 {
        let m = (rand() % 64) as usize;
        let n = (rand() % 64) as usize;
        let a: Vec<u32> = (0..m).map(|_| rand() % 16).collect();
        let b: Vec<u32> = (0..n).map(|_| rand() % 16).collect();

        let pairs_bp = bp.lcs_indices(&a, &b);
        let pairs_matrix = matrix.lcs_indices(&a, &b);

        assert_eq!(
            pairs_bp.len(),
            pairs_matrix.len(),
            "Fuzz match failure on lengths m={m}, n={n}: a={a:?}, b={b:?}"
        );

        // Verify valid subsequence properties
        let mut prev_a = None;
        let mut prev_b = None;
        for &(ai, bi) in &pairs_bp {
            assert_eq!(a[ai], b[bi]);
            if let Some(pa) = prev_a {
                assert!(ai > pa, "strictly monotonic in A");
            }
            if let Some(pb) = prev_b {
                assert!(bi > pb, "strictly monotonic in B");
            }
            prev_a = Some(ai);
            prev_b = Some(bi);
        }
    }
}

#[test]
fn test_myers_ond_matches_matrix_dp() {
    let myers = MyersOnD::new();
    let matrix = MatrixDpLcs::new();

    let cases: Vec<(&[u32], &[u32])> = vec![
        (&[], &[]),
        (&[1, 2, 3], &[]),
        (&[], &[1, 2, 3]),
        (&[1, 2, 3, 4, 5], &[1, 2, 3, 4, 5]),
        (&[1, 2, 3, 4, 5], &[2, 4, 6]),
        (&[10, 20, 30, 40, 50, 60], &[5, 10, 25, 30, 45, 60, 70]),
        (&[1, 1, 1, 1], &[1, 1]),
        (&[1, 2, 1, 2, 1], &[2, 1, 2]),
        (&[1, 2, 3, 4, 5, 6, 7, 8], &[1, 2, 9, 4, 5, 10, 7, 8]),
    ];

    for (a, b) in cases {
        let pairs_myers = myers.lcs_indices(a, b);
        let pairs_matrix = matrix.lcs_indices(a, b);
        assert_eq!(
            pairs_myers.len(),
            pairs_matrix.len(),
            "Myers O(ND) LCS length mismatch: myers={pairs_myers:?}, matrix={pairs_matrix:?}"
        );
        for &(ai, bi) in &pairs_myers {
            assert_eq!(a[ai], b[bi]);
        }
        let diff_myers = myers.diff_slices(a, b);
        let diff_matrix = matrix.diff_slices(a, b);
        let kept_myers = diff_myers
            .iter()
            .filter(|op| matches!(op, EditOp::Keep { .. }))
            .count();
        let kept_matrix = diff_matrix
            .iter()
            .filter(|op| matches!(op, EditOp::Keep { .. }))
            .count();
        assert_eq!(kept_myers, kept_matrix);
    }
}

#[test]
fn test_myers_ond_pseudo_random_fuzz_equivalence() {
    let myers = MyersOnD::new();
    let matrix = MatrixDpLcs::new();

    let mut state = 0x8765_4321_u64;
    let mut rand = || -> u32 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (state >> 32) as u32
    };

    for _ in 0..100 {
        let m = (rand() % 40) as usize;
        let n = (rand() % 40) as usize;
        let a: Vec<u32> = (0..m).map(|_| rand() % 10).collect();
        let b: Vec<u32> = (0..n).map(|_| rand() % 10).collect();

        let pairs_myers = myers.lcs_indices(&a, &b);
        let pairs_matrix = matrix.lcs_indices(&a, &b);

        assert_eq!(
            pairs_myers.len(),
            pairs_matrix.len(),
            "Myers O(ND) fuzz length mismatch for m={m}, n={n}: a={a:?}, b={b:?}"
        );

        let mut prev_a = None;
        let mut prev_b = None;
        for &(ai, bi) in &pairs_myers {
            assert_eq!(a[ai], b[bi]);
            if let Some(pa) = prev_a {
                assert!(ai > pa, "strictly monotonic in A");
            }
            if let Some(pb) = prev_b {
                assert!(bi > pb, "strictly monotonic in B");
            }
            prev_a = Some(ai);
            prev_b = Some(bi);
        }
    }
}

#[test]
fn test_hunt_szymanski_matches_matrix_dp() {
    let hs = HuntSzymanskiLcs::new();
    let matrix = MatrixDpLcs::new();

    let cases: Vec<(&[u32], &[u32])> = vec![
        (&[], &[]),
        (&[1, 2, 3], &[]),
        (&[], &[1, 2, 3]),
        (&[1, 2, 3, 4, 5], &[1, 2, 3, 4, 5]),
        (&[1, 2, 3, 4, 5], &[2, 4, 6]),
        (&[10, 20, 30, 40, 50, 60], &[5, 10, 25, 30, 45, 60, 70]),
        (&[1, 1, 1, 1], &[1, 1]),
        (&[1, 2, 1, 2, 1], &[2, 1, 2]),
        (&[1, 2, 3, 4, 5, 6, 7, 8], &[1, 2, 9, 4, 5, 10, 7, 8]),
    ];

    for (a, b) in cases {
        let pairs_hs = hs.lcs_indices(a, b);
        let pairs_matrix = matrix.lcs_indices(a, b);
        assert_eq!(
            pairs_hs.len(),
            pairs_matrix.len(),
            "Hunt-Szymanski LCS length mismatch: hs={pairs_hs:?}, matrix={pairs_matrix:?}"
        );
        for &(ai, bi) in &pairs_hs {
            assert_eq!(a[ai], b[bi]);
        }
        let diff_hs = hs.diff_slices(a, b);
        let diff_matrix = matrix.diff_slices(a, b);
        let kept_hs = diff_hs
            .iter()
            .filter(|op| matches!(op, EditOp::Keep { .. }))
            .count();
        let kept_matrix = diff_matrix
            .iter()
            .filter(|op| matches!(op, EditOp::Keep { .. }))
            .count();
        assert_eq!(kept_hs, kept_matrix);
    }
}

#[test]
fn test_hunt_szymanski_pseudo_random_fuzz_equivalence() {
    let hs = HuntSzymanskiLcs::new();
    let matrix = MatrixDpLcs::new();

    let mut state = 0x9999_8888_u64;
    let mut rand = || -> u32 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        (state >> 32) as u32
    };

    for _ in 0..100 {
        let m = (rand() % 50) as usize;
        let n = (rand() % 50) as usize;
        let a: Vec<u32> = (0..m).map(|_| rand() % 15).collect();
        let b: Vec<u32> = (0..n).map(|_| rand() % 15).collect();

        let pairs_hs = hs.lcs_indices(&a, &b);
        let pairs_matrix = matrix.lcs_indices(&a, &b);

        assert_eq!(
            pairs_hs.len(),
            pairs_matrix.len(),
            "Hunt-Szymanski fuzz length mismatch for m={m}, n={n}: a={a:?}, b={b:?}"
        );

        let mut prev_a = None;
        let mut prev_b = None;
        for &(ai, bi) in &pairs_hs {
            assert_eq!(a[ai], b[bi]);
            if let Some(pa) = prev_a {
                assert!(ai > pa, "strictly monotonic in A");
            }
            if let Some(pb) = prev_b {
                assert!(bi > pb, "strictly monotonic in B");
            }
            prev_a = Some(ai);
            prev_b = Some(bi);
        }
    }
}
