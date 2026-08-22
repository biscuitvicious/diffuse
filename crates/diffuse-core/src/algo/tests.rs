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
