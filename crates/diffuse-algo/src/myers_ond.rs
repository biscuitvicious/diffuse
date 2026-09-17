//! Myers (1986) O(ND) Difference and LCS Algorithm.
//!
//! An optimal greedy search across edit graph diagonals `k = x - y`.
//! Runs in `O((N + M) * D)` time and `O(D^2)` space with trace reconstruction,
//! where `D` is the size of the minimum edit script. Highly efficient for
//! sequences with few modifications (`D << N + M`).

use super::{DiffEngine, EditOp, LcsEngine};

/// Myers' O(ND) diff and LCS engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct MyersOnD;

impl MyersOnD {
    /// Create a new MyersOnD instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Compute the shortest edit script transforming sequence A into sequence
    /// B.
    #[tracing::instrument(level = "trace", ret, skip_all, fields(n = a.len(), m = b.len()))]
    pub fn diff<T: PartialEq>(a: &[T], b: &[T]) -> Vec<EditOp> {
        let n = a.len();
        let m = b.len();

        if n == 0 && m == 0 {
            return Vec::new();
        }
        if n == 0 {
            return (0..m).map(|b_idx| EditOp::Insert { b_idx }).collect();
        }
        if m == 0 {
            return (0..n).map(|a_idx| EditOp::Delete { a_idx }).collect();
        }

        let max = n + m;
        let offset = max as isize;
        let mut v = vec![0usize; 2 * max + 1];
        let mut trace = Vec::new();

        'outer: for d in 0..=max {
            let mut v_snap = v.clone();
            let d_isize = d as isize;

            for k in (-d_isize..=d_isize).step_by(2) {
                let k_idx = (k + offset) as usize;
                let mut x = if k == -d_isize
                    || (k != d_isize && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize])
                {
                    v[(k + 1 + offset) as usize]
                } else {
                    v[(k - 1 + offset) as usize] + 1
                };

                let mut y = (x as isize - k) as usize;

                // Follow the diagonal snake (identical elements)
                while x < n && y < m && a[x] == b[y] {
                    x += 1;
                    y += 1;
                }

                v_snap[k_idx] = x;

                if x >= n && y >= m {
                    trace.push(v_snap);
                    break 'outer;
                }
            }

            trace.push(v_snap);
            v.copy_from_slice(&trace[trace.len() - 1]);
        }

        // Backtrack trace to reconstruct the optimal edit script
        let mut ops = Vec::new();
        let mut x = n;
        let mut y = m;

        for d in (1..trace.len()).rev() {
            let d_isize = d as isize;
            let k = x as isize - y as isize;
            let prev_v = &trace[d - 1];

            let prev_k = if k == -d_isize
                || (k != d_isize
                    && prev_v[(k - 1 + offset) as usize] < prev_v[(k + 1 + offset) as usize])
            {
                k + 1
            } else {
                k - 1
            };

            let prev_x = prev_v[(prev_k + offset) as usize];
            let prev_y = (prev_x as isize - prev_k) as usize;

            // Common items in the snake
            while x > prev_x && y > prev_y && x > 0 && y > 0 && a[x - 1] == b[y - 1] {
                ops.push(EditOp::Keep {
                    a_idx: x - 1,
                    b_idx: y - 1,
                });
                x -= 1;
                y -= 1;
            }

            if d > 0 && (x > prev_x || y > prev_y) {
                if x == prev_x {
                    // Vertical step -> Insert
                    if y > 0 {
                        ops.push(EditOp::Insert { b_idx: y - 1 });
                        y -= 1;
                    }
                } else if y == prev_y {
                    // Horizontal step -> Delete
                    if x > 0 {
                        ops.push(EditOp::Delete { a_idx: x - 1 });
                        x -= 1;
                    }
                }
            }
        }

        // Initial snake from (0, 0)
        while x > 0 && y > 0 && a[x - 1] == b[y - 1] {
            ops.push(EditOp::Keep {
                a_idx: x - 1,
                b_idx: y - 1,
            });
            x -= 1;
            y -= 1;
        }

        ops.reverse();

        crate::ensure!(
            ops.iter()
                .filter(|op| matches!(op, EditOp::Keep { .. } | EditOp::Delete { .. }))
                .count()
                == n,
            "MyersOnD edit script must account for all elements in slice A"
        );
        crate::ensure!(
            ops.iter()
                .filter(|op| matches!(op, EditOp::Keep { .. } | EditOp::Insert { .. }))
                .count()
                == m,
            "MyersOnD edit script must account for all elements in slice B"
        );

        ops
    }
}

impl LcsEngine for MyersOnD {
    fn lcs_indices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        let diff_ops = Self::diff(a, b);
        let mut pairs = Vec::with_capacity(diff_ops.len());
        for op in diff_ops {
            if let EditOp::Keep { a_idx, b_idx } = op {
                pairs.push((a_idx, b_idx));
            }
        }
        pairs
    }
}

impl DiffEngine for MyersOnD {
    fn diff_slices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<EditOp> {
        Self::diff(a, b)
    }
}
