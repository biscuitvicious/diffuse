use super::*;
use crate::EditOp;

#[test]
fn test_affine_empty_sequences() {
    let mut ws = AffineGapWorkspace::new();
    let penalties = AffineGapPenalties::default();

    let a: Vec<u32> = vec![];
    let b: Vec<u32> = vec![];
    assert_eq!(ws.align(&a, &b, &penalties), &[]);

    let non_empty = vec![1, 2, 3];
    let empty: Vec<u32> = vec![];
    assert_eq!(
        ws.align(&empty, &non_empty, &penalties),
        &[
            EditOp::Insert { b_idx: 0 },
            EditOp::Insert { b_idx: 1 },
            EditOp::Insert { b_idx: 2 },
        ]
    );
    assert_eq!(
        ws.align(&non_empty, &empty, &penalties),
        &[
            EditOp::Delete { a_idx: 0 },
            EditOp::Delete { a_idx: 1 },
            EditOp::Delete { a_idx: 2 },
        ]
    );
}

#[test]
fn test_affine_identical_sequences() {
    let penalties = AffineGapPenalties::default();
    let a = [10, 20, 30];
    let b = vec![10, 20, 30];
    let ops = a.align_affine(&b, &penalties);
    assert_eq!(
        ops,
        vec![
            EditOp::Keep { a_idx: 0, b_idx: 0 },
            EditOp::Keep { a_idx: 1, b_idx: 1 },
            EditOp::Keep { a_idx: 2, b_idx: 2 },
        ]
    );
}

#[test]
fn test_affine_anti_fragmentation_prefers_block_replacement() {
    // Anti-fragmentation test:
    // A: ["get", "_", "foo"]
    // B: ["set", "_", "bar"]
    //
    // In unweighted LCS, the coincidental "_" in the middle is preserved,
    // fragmenting the identifier.
    // With Gotoh's gap opening penalty (5) > match score (2), the affine
    // model mathematically prefers a single cohesive block deletion
    // & insertion.
    let penalties = AffineGapPenalties::new(2, None, 5, 1);
    let a = vec!["get", "_", "foo"];
    let b = vec!["set", "_", "bar"];

    let ops = align_affine(&a, &b, &penalties);

    // Verify that "_" was NOT kept as an isolated common token
    let kept_underscore = ops.iter().any(|op| match op {
        EditOp::Keep { a_idx, b_idx } => a[*a_idx] == "_" && b[*b_idx] == "_",
        _ => false,
    });
    assert!(
        !kept_underscore,
        "Gotoh affine gap model must prevent micro-token fragmentation on coincidental '_'"
    );
}

#[test]
fn test_affine_substitution_when_configured() {
    // When mismatch penalty is explicitly provided and gap open penalty is
    // very high, substitutions along the diagonal are permitted.
    let penalties = AffineGapPenalties::new(2, Some(1), 10, 5);
    let a = vec![1, 2, 3];
    let b = vec![1, 9, 3];

    let ops = align_affine(&a, &b, &penalties);
    assert_eq!(
        ops,
        vec![
            EditOp::Keep { a_idx: 0, b_idx: 0 },
            EditOp::Delete { a_idx: 1 },
            EditOp::Insert { b_idx: 1 },
            EditOp::Keep { a_idx: 2, b_idx: 2 },
        ]
    );
}

#[test]
fn test_affine_zero_allocation_workspace_benchmark() {
    let mut ws = AffineGapWorkspace::new();
    let penalties = AffineGapPenalties::default();

    let a: Vec<u32> = (0..200).collect();
    let mut b: Vec<u32> = (0..200).collect();
    // Introduce small mutation
    b[50] = 999;
    b.remove(100);
    b.insert(150, 888);

    // Run 100 iterations using the same recycled workspace
    for _ in 0..100 {
        let steps = ws.align(&a, &b, &penalties);
        assert!(!steps.is_empty());
    }
}

#[test]
fn test_affine_alignment_steps_iterator() {
    let steps = vec![
        EditOp::Keep { a_idx: 0, b_idx: 0 },
        EditOp::Delete { a_idx: 1 },
        EditOp::Insert { b_idx: 1 },
    ];

    let mut iter = AffineAlignmentSteps::new(&steps);
    assert_eq!(iter.len(), 3);
    assert_eq!(iter.next(), Some(EditOp::Keep { a_idx: 0, b_idx: 0 }));
    assert_eq!(iter.next_back(), Some(EditOp::Insert { b_idx: 1 }));
    assert_eq!(iter.len(), 1);
    assert_eq!(iter.next(), Some(EditOp::Delete { a_idx: 1 }));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}

#[test]
fn test_affine_align_steps_workspace() {
    let mut ws = AffineGapWorkspace::new();
    let penalties = AffineGapPenalties::default();
    let a = vec![1, 2];
    let b = vec![1, 2];
    let steps_iter = ws.align_steps(&a, &b, &penalties);
    let collected: Vec<EditOp> = steps_iter.collect();
    assert_eq!(
        collected,
        vec![
            EditOp::Keep { a_idx: 0, b_idx: 0 },
            EditOp::Keep { a_idx: 1, b_idx: 1 },
        ]
    );
}
