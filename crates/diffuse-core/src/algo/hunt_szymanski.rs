//! Hunt-Szymanski (1977) / Robinson-Schensted Patience LCS Engine.
//!
//! Reduces the Longest Common Subsequence problem to finding the Longest
//! Increasing Subsequence (LIS) over candidate match positions via binary
//! search (Patience sorting).
//!
//! Runs in O((R + N) * log M) time, where R is the total count of matching
//! pairs. Particularly optimal for sequences with distinct tokens, large
//! structural blocks, and sparse differences.

use std::{collections::HashMap, hash::Hash};

use super::{DiffEngine, EditOp, LcsEngine};

/// Hunt-Szymanski / Patience LCS engine.
#[derive(Clone, Copy, Debug, Default)]
pub struct HuntSzymanskiLcs;

impl HuntSzymanskiLcs {
    /// Create a new HuntSzymanskiLcs instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Compute index pairings (a_idx, b_idx) using the Hunt-Szymanski LIS
    /// reduction.
    pub fn lcs<T: PartialEq + Eq + Hash>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        let n = a.len();
        let m = b.len();
        if n == 0 || m == 0 {
            return Vec::new();
        }

        // Map elements of B to their sorted occurrence indices in B
        let mut b_pos: HashMap<&T, Vec<usize>> = HashMap::with_capacity(m);
        for (j, item) in b.iter().enumerate() {
            b_pos.entry(item).or_default().push(j);
        }

        // Nodes for LIS backtracing: (a_idx, b_idx, parent_node_index)
        let mut nodes: Vec<(usize, usize, Option<usize>)> = Vec::new();
        // tails[k] stores the node index representing the tail of an increasing
        // subsequence of length k+1
        let mut tails: Vec<usize> = Vec::new();

        for (i, item) in a.iter().enumerate() {
            if let Some(positions) = b_pos.get(item) {
                // Process B positions in descending order to avoid multiple matches from the
                // same A element
                for &j in positions.iter().rev() {
                    let k = tails.partition_point(|&node_idx| nodes[node_idx].1 < j);
                    let parent = if k > 0 { Some(tails[k - 1]) } else { None };
                    let node_idx = nodes.len();
                    nodes.push((i, j, parent));

                    if k == tails.len() {
                        tails.push(node_idx);
                    } else {
                        tails[k] = node_idx;
                    }
                }
            }
        }

        if tails.is_empty() {
            return Vec::new();
        }

        // Reconstruct the LIS chain
        let mut pairs = Vec::with_capacity(tails.len());
        let mut curr = Some(*tails.last().unwrap());
        while let Some(node_idx) = curr {
            let (ai, bi, parent) = nodes[node_idx];
            pairs.push((ai, bi));
            curr = parent;
        }

        pairs.reverse();
        pairs
    }

    /// Compute edit script from Hunt-Szymanski matches.
    pub fn diff<T: PartialEq + Eq + Hash>(a: &[T], b: &[T]) -> Vec<EditOp> {
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

impl LcsEngine for HuntSzymanskiLcs {
    fn lcs_indices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        let n = a.len();
        let m = b.len();
        if n == 0 || m == 0 {
            return Vec::new();
        }

        let mut unique: Vec<&T> = Vec::with_capacity(n + m);
        let mut a_ids = Vec::with_capacity(n);
        for item in a {
            if let Some(pos) = unique.iter().position(|&x| x == item) {
                a_ids.push(pos as u32);
            } else {
                let id = unique.len() as u32;
                unique.push(item);
                a_ids.push(id);
            }
        }
        let mut b_ids = Vec::with_capacity(m);
        for item in b {
            if let Some(pos) = unique.iter().position(|&x| x == item) {
                b_ids.push(pos as u32);
            } else {
                let id = unique.len() as u32;
                unique.push(item);
                b_ids.push(id);
            }
        }

        Self::lcs(&a_ids, &b_ids)
    }
}

impl DiffEngine for HuntSzymanskiLcs {
    fn diff_slices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<EditOp> {
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

        let pairs = self.lcs_indices(a, b);
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
