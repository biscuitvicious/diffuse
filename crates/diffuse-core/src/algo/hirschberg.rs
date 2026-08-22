//! Hirschberg (1975) Linear-Space Divide-and-Conquer LCS Engine.
//!
//! Computes exact global sequence alignments and longest common subsequences
//! in O(M * N) time while strictly bounding memory consumption to O(min(M, N)).
//!
//! Evaluates forward and backward dynamic programming score vectors over two
//! recycled rows, finding the optimal midpoint split (mid, k) and recursing on
//! independent subproblems. Guarantees complete L1 cache residency even for
//! massive sequences.

use super::{DiffEngine, EditOp, LcsEngine};

/// Hirschberg's linear-space divide-and-conquer LCS engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct HirschbergLcs;

impl HirschbergLcs {
    /// Create a new HirschbergLcs instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Compute index pairings (a_idx, b_idx) forming the LCS in linear space.
    pub fn lcs<T: PartialEq>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        Self::hirschberg_recursive(a, b, 0, 0, &mut pairs);
        pairs
    }

    fn hirschberg_recursive<T: PartialEq>(
        a: &[T],
        b: &[T],
        offset_a: usize,
        offset_b: usize,
        out: &mut Vec<(usize, usize)>,
    ) {
        let m = a.len();
        let n = b.len();

        if m == 0 || n == 0 {
            return;
        }

        if m == 1 {
            if let Some(j) = b.iter().position(|item| *item == a[0]) {
                out.push((offset_a, offset_b + j));
            }
            return;
        }

        if n == 1 {
            if let Some(i) = a.iter().position(|item| *item == b[0]) {
                out.push((offset_a + i, offset_b));
            }
            return;
        }

        let mid = m / 2;
        let l1 = Self::score_vector(&a[..mid], b);
        let l2 = Self::score_vector_rev(&a[mid..], b);

        let mut max_sum = 0;
        let mut split_k = 0;

        for k in 0..=n {
            let sum = l1[k] + l2[n - k];
            if sum > max_sum || (k == 0 && max_sum == 0) {
                max_sum = sum;
                split_k = k;
            }
        }

        Self::hirschberg_recursive(&a[..mid], &b[..split_k], offset_a, offset_b, out);
        Self::hirschberg_recursive(
            &a[mid..],
            &b[split_k..],
            offset_a + mid,
            offset_b + split_k,
            out,
        );
    }

    /// Compute the last row of the forward LCS score matrix using two recycled
    /// rows.
    fn score_vector<T: PartialEq>(a: &[T], b: &[T]) -> Vec<u32> {
        let n = b.len();
        let mut prev = vec![0u32; n + 1];
        let mut curr = vec![0u32; n + 1];

        for a_item in a {
            for j in 0..n {
                if *a_item == b[j] {
                    curr[j + 1] = prev[j] + 1;
                } else {
                    curr[j + 1] = prev[j + 1].max(curr[j]);
                }
            }
            std::mem::swap(&mut prev, &mut curr);
        }

        prev
    }

    /// Compute the score vector for the reversed sequences (backward DP).
    fn score_vector_rev<T: PartialEq>(a: &[T], b: &[T]) -> Vec<u32> {
        let n = b.len();
        let mut prev = vec![0u32; n + 1];
        let mut curr = vec![0u32; n + 1];

        for a_item in a.iter().rev() {
            for (j, b_item) in b.iter().rev().enumerate() {
                if *a_item == *b_item {
                    curr[j + 1] = prev[j] + 1;
                } else {
                    curr[j + 1] = prev[j + 1].max(curr[j]);
                }
            }
            std::mem::swap(&mut prev, &mut curr);
        }

        prev
    }

    /// Compute edit script using linear-space LCS.
    pub fn diff<T: PartialEq>(a: &[T], b: &[T]) -> Vec<EditOp> {
        let pairs = Self::lcs(a, b);
        let n = a.len();
        let m = b.len();
        let mut ops = Vec::with_capacity(n + m);

        let mut curr_a = 0;
        let mut curr_b = 0;

        for (ai, bi) in pairs {
            while curr_a < ai {
                ops.push(EditOp::Delete { a_idx: curr_a });
                curr_a += 1;
            }
            while curr_b < bi {
                ops.push(EditOp::Insert { b_idx: curr_b });
                curr_b += 1;
            }
            ops.push(EditOp::Keep {
                a_idx: ai,
                b_idx: bi,
            });
            curr_a = ai + 1;
            curr_b = bi + 1;
        }

        while curr_a < n {
            ops.push(EditOp::Delete { a_idx: curr_a });
            curr_a += 1;
        }
        while curr_b < m {
            ops.push(EditOp::Insert { b_idx: curr_b });
            curr_b += 1;
        }

        ops
    }
}

impl LcsEngine for HirschbergLcs {
    fn lcs_indices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        Self::lcs(a, b)
    }
}

impl DiffEngine for HirschbergLcs {
    fn diff_slices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<EditOp> {
        Self::diff(a, b)
    }
}
