use super::*;

#[test]
fn test_skyline_matrix_rectangular_full_coverage() {
    let m = 3;
    let n = 4;
    let bounds = vec![(0, n); m + 1];
    let mut matrix = SkylineMatrix::new_with_envelope(m, n, &bounds, -1i32);

    assert_eq!(matrix.m(), m);
    assert_eq!(matrix.n(), n);
    assert_eq!(matrix.cell_count(), (m + 1) * (n + 1));

    // Initially all default values
    for i in 0..=m {
        assert_eq!(matrix.row_range(i), 0..=n);
        for j in 0..=n {
            assert_eq!(matrix.get(i, j), Some(&-1));
        }
    }

    // Out-of-bounds coordinates return None
    assert_eq!(matrix.get(m + 1, 0), None);
    assert_eq!(matrix.get(0, n + 1), None);

    // Mutation works as expected
    *matrix.get_mut(1, 2).unwrap() = 42;
    assert_eq!(matrix.get(1, 2), Some(&42));
    assert_eq!(matrix.get(1, 1), Some(&-1));
}

#[test]
fn test_skyline_matrix_banded_diagonal() {
    // 5x5 matrix with bandwidth K=1 (diagonal i-1..=i+1)
    let m = 4;
    let n = 4;
    let bounds = vec![
        (0, 1), // row 0: j in 0..=1
        (0, 2), // row 1: j in 0..=2
        (1, 3), // row 2: j in 1..=3
        (2, 4), // row 3: j in 2..=4
        (3, 4), // row 4: j in 3..=4
    ];
    let mut matrix = SkylineMatrix::new_with_envelope(m, n, &bounds, 0u32);

    assert_eq!(matrix.cell_count(), 2 + 3 + 3 + 3 + 2); // 13 cells instead of 25

    // In-corridor cells are accessible
    assert!(matrix.is_in_envelope(0, 0));
    assert!(matrix.is_in_envelope(0, 1));
    assert!(matrix.is_in_envelope(2, 2));
    assert!(matrix.is_in_envelope(4, 4));

    // Outside-corridor cells return None
    assert_eq!(matrix.get(0, 2), None);
    assert_eq!(matrix.get(2, 0), None);
    assert_eq!(matrix.get(4, 0), None);
    assert!(!matrix.is_in_envelope(4, 2));

    // Can mutate inside corridor
    *matrix.get_mut(2, 3).unwrap() = 100;
    assert_eq!(matrix.get(2, 3), Some(&100));
}

#[test]
fn test_skyline_matrix_reuse_with_init_envelope() {
    let mut matrix = SkylineMatrix::new_empty();
    assert_eq!(matrix.cell_count(), 0);

    let bounds1 = vec![(0, 2), (0, 2)];
    matrix.init_envelope(1, 2, &bounds1, 10);
    assert_eq!(matrix.cell_count(), 6);
    assert_eq!(matrix.get(0, 1), Some(&10));

    // Reuse with a different envelope
    let bounds2 = vec![(0, 1), (1, 2), (2, 2)];
    matrix.init_envelope(2, 2, &bounds2, 20);
    assert_eq!(matrix.cell_count(), 5);
    assert_eq!(matrix.get(0, 0), Some(&20));
    assert_eq!(matrix.get(0, 2), None);
    assert_eq!(matrix.get(1, 1), Some(&20));
    assert_eq!(matrix.get(2, 2), Some(&20));
}

#[test]
fn test_adaptive_envelope_builder_small_cluster_full_coverage() {
    let m = 10;
    let n = 10;
    let envelope = AdaptiveEnvelopeBuilder::new(m, n)
        .with_bandwidth_min(64)
        .with_bandwidth_slack(32)
        .with_budget(10_000)
        .build();

    // Since m, n <= bandwidth_min, it covers the entire (m+1)*(n+1) grid
    assert_eq!(envelope.total_cells, (m + 1) * (n + 1));
    for (start, end) in envelope.bounds {
        assert_eq!(start, 0);
        assert_eq!(end, n);
    }
}

#[test]
fn test_adaptive_envelope_builder_large_equal_replacement() {
    let m = 1000;
    let n = 1000;
    let envelope = AdaptiveEnvelopeBuilder::new(m, n)
        .with_bandwidth_min(32)
        .with_bandwidth_slack(16)
        .with_budget(1_000_000)
        .build();

    // Corridor of K=32 around diagonal i == j
    // Start of row 0 must be 0, end of row m must be n
    assert_eq!(envelope.bounds[0].0, 0);
    assert_eq!(envelope.bounds[0].1, 32);
    assert_eq!(envelope.bounds[m].1, n);

    // Total cells must be O(K * m), far below m * n (1,000,000)
    assert!(envelope.total_cells < (m + 1) * 66);
    assert!(envelope.total_cells > (m + 1) * 32);
}

#[test]
fn test_adaptive_envelope_builder_asymmetric_burst() {
    let m = 20;
    let n = 100;
    let envelope = AdaptiveEnvelopeBuilder::new(m, n)
        .with_bandwidth_min(32)
        .with_bandwidth_slack(16)
        .with_budget(100_000)
        .build();

    // Delta = 80, so K >= 80 + 16 = 96
    // Corridor start at (0, 0) and end at (20, 100)
    assert_eq!(envelope.bounds[0].0, 0);
    assert_eq!(envelope.bounds[m].1, n);
    assert!(envelope.total_cells <= 100_000);
}

#[test]
fn test_adaptive_envelope_builder_budget_clamping() {
    let m = 500;
    let n = 500;
    // Tight budget of 5000 cells for 501 rows (clamps from K=64 to K=4)
    let envelope = AdaptiveEnvelopeBuilder::new(m, n)
        .with_bandwidth_min(64)
        .with_bandwidth_slack(32)
        .with_budget(5000)
        .build();

    assert!(envelope.total_cells <= 5000);
    assert_eq!(envelope.bounds[0].0, 0);
    assert_eq!(envelope.bounds[m].1, n);
}

#[test]
fn test_skyline_matrix_zero_dimension_and_boundaries() {
    // M = 0, N = 0
    let bounds = vec![(0, 0)];
    let mut matrix = SkylineMatrix::new_with_envelope(0, 0, &bounds, 7);
    assert_eq!(matrix.cell_count(), 1);
    assert_eq!(matrix.get(0, 0), Some(&7));
    assert_eq!(matrix.get(0, 1), None);
    assert_eq!(matrix.get(1, 0), None);
    assert!(!matrix.is_in_envelope(1, 0));
    assert_eq!(matrix.get_mut(1, 0), None);
    *matrix.get_mut(0, 0).unwrap() = 8;
    assert_eq!(matrix.get(0, 0), Some(&8));

    // M = 0, N = 5 (pure addition cluster)
    let env_add = AdaptiveEnvelopeBuilder::new(0, 5).build();
    assert_eq!(env_add.bounds.len(), 1);
    assert_eq!(env_add.bounds[0], (0, 5));

    // M = 5, N = 0 (pure deletion cluster)
    let env_del = AdaptiveEnvelopeBuilder::new(5, 0).build();
    assert_eq!(env_del.bounds.len(), 6);
    for b in env_del.bounds {
        assert_eq!(b, (0, 0));
    }
}
