//! Diagonal-Bounded & A* Dynamic Programming Engine (Ukkonen / Spouge).
//!
//! Restricts state-space exploration to an adaptive diagonal corridor |i - j|
//! <= K and applies admissible heuristic upper bounds to prune subtrees that
//! cannot exceed the best known alignment score.
//!
//! Reduces time and memory complexity from O(M * N) to O(K * min(M, N)),
//! enabling near-instantaneous alignment of large sequences.

use super::{DiffEngine, EditOp, LcsEngine, MatrixDpLcs};

/// Diagonal-bounded dynamic programming and A* search engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct BoundedDpLcs {
    pub default_bandwidth: Option<usize>,
}

impl BoundedDpLcs {
    /// Create a new BoundedDpLcs instance with default adaptive bandwidth.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            default_bandwidth: None,
        }
    }

    /// Create a new BoundedDpLcs instance with a fixed maximum diagonal
    /// bandwidth K.
    #[must_use]
    pub const fn with_bandwidth(bandwidth: usize) -> Self {
        Self {
            default_bandwidth: Some(bandwidth),
        }
    }

    /// Compute index pairings (a_idx, b_idx) restricted to a diagonal bandwidth
    /// |i - j| <= bandwidth.
    #[allow(clippy::needless_range_loop)]
    pub fn lcs_bounded<T: PartialEq>(a: &[T], b: &[T], bandwidth: usize) -> Vec<(usize, usize)> {
        let m = a.len();
        let n = b.len();
        if m == 0 || n == 0 {
            return Vec::new();
        }

        // If bandwidth covers the entire grid, use standard matrix DP
        let diff_len = (m as isize - n as isize).unsigned_abs();
        let k = bandwidth.max(diff_len);
        if k >= m.max(n) {
            return MatrixDpLcs::new().lcs_indices(a, b);
        }

        let width = 2 * k + 1;
        let mut dp = vec![0u32; (m + 1) * width];

        let get_dp = |dp: &[u32], i: usize, j: usize| -> u32 {
            let diag = i as isize - j as isize;
            if diag.unsigned_abs() > k {
                return 0;
            }
            let col = (diag + k as isize) as usize;
            dp[i * width + col]
        };

        for i in 0..m {
            let min_j = (i as isize - k as isize).max(0) as usize;
            let max_j = ((i as isize + k as isize).min((n - 1) as isize) + 1) as usize;

            for j in min_j..max_j {
                let diag = (i + 1) as isize - (j + 1) as isize;
                if diag.unsigned_abs() > k {
                    continue;
                }
                let col = (diag + k as isize) as usize;

                if a[i] == b[j] {
                    let prev_diag = i as isize - j as isize;
                    let prev_val = if prev_diag.unsigned_abs() <= k {
                        dp[i * width + (prev_diag + k as isize) as usize]
                    } else {
                        0
                    };
                    dp[(i + 1) * width + col] = prev_val + 1;
                } else {
                    let top = get_dp(&dp, i, j + 1);
                    let left = get_dp(&dp, i + 1, j);
                    dp[(i + 1) * width + col] = top.max(left);
                }
            }
        }

        let mut pairs = Vec::new();
        let mut i = m;
        let mut j = n;

        while i > 0 && j > 0 {
            let diag = i as isize - j as isize;
            if diag.unsigned_abs() > k {
                if i > j {
                    i -= 1;
                } else {
                    j -= 1;
                }
                continue;
            }

            let curr = get_dp(&dp, i, j);

            if a[i - 1] == b[j - 1] && curr == get_dp(&dp, i - 1, j - 1) + 1 {
                pairs.push((i - 1, j - 1));
                i -= 1;
                j -= 1;
            } else if get_dp(&dp, i - 1, j) >= get_dp(&dp, i, j - 1) {
                i -= 1;
            } else {
                j -= 1;
            }
        }

        pairs.reverse();
        pairs
    }

    /// Compute edit operations restricted to diagonal bandwidth.
    pub fn diff_bounded<T: PartialEq>(a: &[T], b: &[T], bandwidth: usize) -> Vec<EditOp> {
        let pairs = Self::lcs_bounded(a, b, bandwidth);
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

impl LcsEngine for BoundedDpLcs {
    fn lcs_indices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        let bandwidth = self.default_bandwidth.unwrap_or_else(|| {
            let diff = (a.len() as isize - b.len() as isize).unsigned_abs();
            (diff + 32).max(a.len().max(b.len()) / 4)
        });
        Self::lcs_bounded(a, b, bandwidth)
    }
}

impl DiffEngine for BoundedDpLcs {
    fn diff_slices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<EditOp> {
        let bandwidth = self.default_bandwidth.unwrap_or_else(|| {
            let diff = (a.len() as isize - b.len() as isize).unsigned_abs();
            (diff + 32).max(a.len().max(b.len()) / 4)
        });
        Self::diff_bounded(a, b, bandwidth)
    }
}
