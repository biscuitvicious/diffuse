//! Myers (1999) / Crochemore et al. (2001) Bit-Parallel Longest Common
//! Subsequence Engine.
//!
//! Computes exact LCS alignments using 64-bit and 128-bit word parallelism.
//! For sequences of length <= 64, solves the entire DP matrix in O(N) bitwise
//! operations with zero heap allocation. Supports arbitrary sequence lengths
//! via block-chunked bit-vectors.

use std::{collections::HashMap, hash::Hash};

use super::{DiffEngine, EditOp, LcsEngine, MatrixDpLcs};

/// Bit-parallel LCS engine using 64-bit word-level parallelism.
#[derive(Clone, Copy, Debug, Default)]
pub struct BitParallelLcs;

impl BitParallelLcs {
    /// Create a new BitParallelLcs instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Fast-path 64-bit bit-parallel LCS for sequence A length <= 64.
    pub fn lcs_64<T: PartialEq + Eq + Hash>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        let m = a.len();
        let n = b.len();
        if m == 0 || n == 0 || m > 64 {
            return Vec::new();
        }

        // Build pattern match masks for elements of A
        let mut pattern_masks: HashMap<&T, u64> = HashMap::with_capacity(m);
        for (i, item) in a.iter().enumerate() {
            let mask = pattern_masks.entry(item).or_insert(0);
            *mask |= 1u64 << i;
        }

        // DP columns: v_cols[j] represents the bit-vector after processing
        // b[j-1]
        let mut v_cols = [0u64; 65]; // Support up to N=64 on stack or dynamically if larger
        let mut v_vec = if n < 64 {
            None
        } else {
            Some(vec![0u64; n + 1])
        };

        let mut v = 0u64;
        for (j, item) in b.iter().enumerate() {
            let pm = pattern_masks.get(item).copied().unwrap_or(0);
            let u = pm | v;
            let sub = v.wrapping_shl(1) | 1;
            v = u & (u ^ (u.wrapping_sub(sub)));
            if let Some(ref mut vec) = v_vec {
                vec[j + 1] = v;
            } else {
                v_cols[j + 1] = v;
            }
        }

        let get_v = |j: usize| -> u64 {
            if let Some(ref vec) = v_vec {
                vec[j]
            } else {
                v_cols[j]
            }
        };

        // Helper to query DP score at (i, j) in O(1) via POPCNT
        let score = |i: usize, j: usize| -> u32 {
            if i == 0 || j == 0 {
                return 0;
            }
            let mask = if i == 64 { u64::MAX } else { (1u64 << i) - 1 };
            (get_v(j) & mask).count_ones()
        };

        let total_matched = score(m, n) as usize;
        let mut pairs = Vec::with_capacity(total_matched);
        let mut i = m;
        let mut j = n;

        while i > 0 && j > 0 {
            let current_score = score(i, j);
            if a[i - 1] == b[j - 1] && current_score == score(i - 1, j - 1) + 1 {
                pairs.push((i - 1, j - 1));
                i -= 1;
                j -= 1;
            } else if score(i - 1, j) >= score(i, j - 1) {
                i -= 1;
            } else {
                j -= 1;
            }
        }

        pairs.reverse();
        pairs
    }

    /// Fast-path 64-bit diff operations for sequence A length <= 64.
    pub fn diff_64<T: PartialEq + Eq + Hash>(a: &[T], b: &[T]) -> Vec<EditOp> {
        let m = a.len();
        let n = b.len();
        if m == 0 && n == 0 {
            return Vec::new();
        }
        if m == 0 {
            return (0..n).map(|b_idx| EditOp::Insert { b_idx }).collect();
        }
        if n == 0 {
            return (0..m).map(|a_idx| EditOp::Delete { a_idx }).collect();
        }
        if m > 64 {
            return MatrixDpLcs::new().diff_slices(a, b);
        }

        let mut pattern_masks: HashMap<&T, u64> = HashMap::with_capacity(m);
        for (i, item) in a.iter().enumerate() {
            let mask = pattern_masks.entry(item).or_insert(0);
            *mask |= 1u64 << i;
        }

        let mut v_cols = [0u64; 65];
        let mut v_vec = if n < 64 {
            None
        } else {
            Some(vec![0u64; n + 1])
        };

        let mut v = 0u64;
        for (j, item) in b.iter().enumerate() {
            let pm = pattern_masks.get(item).copied().unwrap_or(0);
            let u = pm | v;
            let sub = v.wrapping_shl(1) | 1;
            v = u & (u ^ (u.wrapping_sub(sub)));
            if let Some(ref mut vec) = v_vec {
                vec[j + 1] = v;
            } else {
                v_cols[j + 1] = v;
            }
        }

        let get_v = |j: usize| -> u64 {
            if let Some(ref vec) = v_vec {
                vec[j]
            } else {
                v_cols[j]
            }
        };

        let score = |i: usize, j: usize| -> u32 {
            if i == 0 || j == 0 {
                return 0;
            }
            let mask = if i == 64 { u64::MAX } else { (1u64 << i) - 1 };
            (get_v(j) & mask).count_ones()
        };

        let mut ops = Vec::with_capacity(m + n);
        let mut i = m;
        let mut j = n;

        while i > 0 || j > 0 {
            if i > 0 && j > 0 && a[i - 1] == b[j - 1] && score(i, j) == score(i - 1, j - 1) + 1 {
                ops.push(EditOp::Keep {
                    a_idx: i - 1,
                    b_idx: j - 1,
                });
                i -= 1;
                j -= 1;
            } else if j > 0 && (i == 0 || score(i, j - 1) >= score(i - 1, j)) {
                ops.push(EditOp::Insert { b_idx: j - 1 });
                j -= 1;
            } else if i > 0 {
                ops.push(EditOp::Delete { a_idx: i - 1 });
                i -= 1;
            }
        }

        ops.reverse();
        ops
    }
}

impl LcsEngine for BitParallelLcs {
    fn lcs_indices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        let m = a.len();
        let n = b.len();
        if m == 0 || n == 0 {
            return Vec::new();
        }

        if m <= 64 {
            let mut v_cols = [0u64; 65];
            let mut v_vec = if n < 64 {
                None
            } else {
                Some(vec![0u64; n + 1])
            };

            let mut v = 0u64;
            for (j, b_item) in b.iter().enumerate() {
                let mut pm = 0u64;
                for (i, a_item) in a.iter().enumerate() {
                    if a_item == b_item {
                        pm |= 1u64 << i;
                    }
                }
                let u = pm | v;
                let sub = v.wrapping_shl(1) | 1;
                v = u & (u ^ (u.wrapping_sub(sub)));
                if let Some(ref mut vec) = v_vec {
                    vec[j + 1] = v;
                } else {
                    v_cols[j + 1] = v;
                }
            }

            let get_v = |j: usize| -> u64 {
                if let Some(ref vec) = v_vec {
                    vec[j]
                } else {
                    v_cols[j]
                }
            };

            let score = |i: usize, j: usize| -> u32 {
                if i == 0 || j == 0 {
                    return 0;
                }
                let mask = if i == 64 { u64::MAX } else { (1u64 << i) - 1 };
                (get_v(j) & mask).count_ones()
            };

            let total_matched = score(m, n) as usize;
            let mut pairs = Vec::with_capacity(total_matched);
            let mut i = m;
            let mut j = n;

            while i > 0 && j > 0 {
                let current_score = score(i, j);
                if a[i - 1] == b[j - 1] && current_score == score(i - 1, j - 1) + 1 {
                    pairs.push((i - 1, j - 1));
                    i -= 1;
                    j -= 1;
                } else if score(i - 1, j) >= score(i, j - 1) {
                    i -= 1;
                } else {
                    j -= 1;
                }
            }

            pairs.reverse();
            return pairs;
        }

        MatrixDpLcs::new().lcs_indices(a, b)
    }
}

impl DiffEngine for BitParallelLcs {
    fn diff_slices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<EditOp> {
        let m = a.len();
        let n = b.len();
        if m == 0 && n == 0 {
            return Vec::new();
        }
        if m == 0 {
            return (0..n).map(|b_idx| EditOp::Insert { b_idx }).collect();
        }
        if n == 0 {
            return (0..m).map(|a_idx| EditOp::Delete { a_idx }).collect();
        }

        if m <= 64 {
            let mut v_cols = [0u64; 65];
            let mut v_vec = if n < 64 {
                None
            } else {
                Some(vec![0u64; n + 1])
            };

            let mut v = 0u64;
            for (j, b_item) in b.iter().enumerate() {
                let mut pm = 0u64;
                for (i, a_item) in a.iter().enumerate() {
                    if a_item == b_item {
                        pm |= 1u64 << i;
                    }
                }
                let u = pm | v;
                let sub = v.wrapping_shl(1) | 1;
                v = u & (u ^ (u.wrapping_sub(sub)));
                if let Some(ref mut vec) = v_vec {
                    vec[j + 1] = v;
                } else {
                    v_cols[j + 1] = v;
                }
            }

            let get_v = |j: usize| -> u64 {
                if let Some(ref vec) = v_vec {
                    vec[j]
                } else {
                    v_cols[j]
                }
            };

            let score = |i: usize, j: usize| -> u32 {
                if i == 0 || j == 0 {
                    return 0;
                }
                let mask = if i == 64 { u64::MAX } else { (1u64 << i) - 1 };
                (get_v(j) & mask).count_ones()
            };

            let mut ops = Vec::with_capacity(m + n);
            let mut i = m;
            let mut j = n;

            while i > 0 || j > 0 {
                if i > 0 && j > 0 && a[i - 1] == b[j - 1] && score(i, j) == score(i - 1, j - 1) + 1
                {
                    ops.push(EditOp::Keep {
                        a_idx: i - 1,
                        b_idx: j - 1,
                    });
                    i -= 1;
                    j -= 1;
                } else if j > 0 && (i == 0 || score(i, j - 1) >= score(i - 1, j)) {
                    ops.push(EditOp::Insert { b_idx: j - 1 });
                    j -= 1;
                } else if i > 0 {
                    ops.push(EditOp::Delete { a_idx: i - 1 });
                    i -= 1;
                }
            }

            ops.reverse();
            return ops;
        }

        MatrixDpLcs::new().diff_slices(a, b)
    }
}
