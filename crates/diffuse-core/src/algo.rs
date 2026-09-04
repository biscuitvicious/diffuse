//! Modular, algorithm-agnostic diffing, LCS, and pattern matching interfaces.
//!
//! # Strict Architectural Separation
//! All algorithms in this module operate strictly on generic sequences (`&[T]
//! where T: PartialEq`, `&[u8]`, or string slices). They have **zero
//! dependency** on diffuse ASTs, hunks, rows, terminal colors, notes, or wire
//! representations.
//!
//! Higher-level diffuse components interact with algorithms exclusively through
//! these generic trait boundaries (`LcsEngine`, `DiffEngine`, `PrefixMatcher`).

use std::collections::HashMap;

pub mod bit_parallel;
pub mod bounded_dp;
pub mod hirschberg;
pub mod hunt_szymanski;
pub mod myers_ond;
pub mod prefix_dfa;

pub use bit_parallel::BitParallelLcs;
pub use bounded_dp::BoundedDpLcs;
pub use hirschberg::HirschbergLcs;
pub use hunt_szymanski::HuntSzymanskiLcs;
pub use myers_ond::MyersOnD;
pub use prefix_dfa::PrefixDfa;

/// An elementary edit operation between two generic sequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditOp {
    /// An item present in both sequences at `(a_idx, b_idx)`.
    Keep { a_idx: usize, b_idx: usize },
    /// An item inserted from sequence B at `b_idx`.
    Insert { b_idx: usize },
    /// An item deleted from sequence A at `a_idx`.
    Delete { a_idx: usize },
}

/// Generic interface for Longest Common Subsequence (LCS) algorithms.
pub trait LcsEngine {
    /// Compute index pairings `(a_idx, b_idx)` forming the longest common
    /// subsequence.
    fn lcs_indices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<(usize, usize)>;
}

/// Generic interface for Sequence Differencing engines.
pub trait DiffEngine {
    /// Compute an optimal sequence of edit operations transforming sequence A
    /// into sequence B.
    fn diff_slices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<EditOp>;
}

/// Generic interface for fast prefix and pattern matching automata.
pub trait PrefixMatcher {
    /// Return the longest matching prefix slice from the dictionary, if any.
    fn longest_matching_prefix<'a>(&self, text: &'a str) -> Option<&'a str>;

    /// Return true if the text begins with any prefix from the dictionary.
    fn has_matching_prefix(&self, text: &str) -> bool {
        self.longest_matching_prefix(text).is_some()
    }
}

/// Standard 2D matrix dynamic programming LCS and Diff engine.
///
/// Serves as the canonical reference implementation and verification oracle.
#[derive(Clone, Copy, Debug, Default)]
pub struct MatrixDpLcs;

impl MatrixDpLcs {
    /// Create a new Matrix DP LCS solver instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl LcsEngine for MatrixDpLcs {
    fn lcs_indices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<(usize, usize)> {
        let m = a.len();
        let n = b.len();
        if m == 0 || n == 0 {
            return Vec::new();
        }

        let stride = n + 1;
        let mut dp = vec![0u32; (m + 1) * stride];

        for i in 0..m {
            for j in 0..n {
                if a[i] == b[j] {
                    dp[(i + 1) * stride + (j + 1)] = dp[i * stride + j] + 1;
                } else {
                    let top = dp[i * stride + (j + 1)];
                    let left = dp[(i + 1) * stride + j];
                    dp[(i + 1) * stride + (j + 1)] = top.max(left);
                }
            }
        }

        let mut i = m;
        let mut j = n;
        let mut pairs = Vec::with_capacity(dp[m * stride + n] as usize);

        while i > 0 && j > 0 {
            if a[i - 1] == b[j - 1] {
                pairs.push((i - 1, j - 1));
                i -= 1;
                j -= 1;
            } else if dp[(i - 1) * stride + j] >= dp[i * stride + (j - 1)] {
                i -= 1;
            } else {
                j -= 1;
            }
        }

        pairs.reverse();
        pairs
    }
}

impl DiffEngine for MatrixDpLcs {
    fn diff_slices<T: PartialEq>(&self, a: &[T], b: &[T]) -> Vec<EditOp> {
        let m = a.len();
        let n = b.len();
        let stride = n + 1;
        let mut dp = vec![0u32; (m + 1) * stride];

        for i in 0..m {
            for j in 0..n {
                if a[i] == b[j] {
                    dp[(i + 1) * stride + (j + 1)] = dp[i * stride + j] + 1;
                } else {
                    let top = dp[i * stride + (j + 1)];
                    let left = dp[(i + 1) * stride + j];
                    dp[(i + 1) * stride + (j + 1)] = top.max(left);
                }
            }
        }

        let mut i = m;
        let mut j = n;
        let mut ops = Vec::with_capacity(m + n);

        while i > 0 || j > 0 {
            if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
                ops.push(EditOp::Keep {
                    a_idx: i - 1,
                    b_idx: j - 1,
                });
                i -= 1;
                j -= 1;
            } else if j > 0 && (i == 0 || dp[i * stride + (j - 1)] >= dp[(i - 1) * stride + j]) {
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

/// Compact Trie-based dictionary prefix matcher.
#[derive(Clone, Debug, Default)]
pub struct TriePrefixMatcher {
    root: TrieNode,
}

#[derive(Clone, Debug, Default)]
struct TrieNode {
    children: HashMap<u8, TrieNode>,
    is_terminal: bool,
    prefix_len: usize,
}

impl TriePrefixMatcher {
    /// Build a prefix matcher from a collection of pattern strings.
    #[must_use]
    pub fn from_patterns<I, S>(patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut root = TrieNode::default();
        for p in patterns {
            let s = p.as_ref();
            if s.is_empty() {
                continue;
            }
            let mut curr = &mut root;
            for &b in s.as_bytes() {
                curr = curr.children.entry(b).or_default();
            }
            curr.is_terminal = true;
            curr.prefix_len = s.len();
        }
        Self { root }
    }
}

impl PrefixMatcher for TriePrefixMatcher {
    fn longest_matching_prefix<'a>(&self, text: &'a str) -> Option<&'a str> {
        let bytes = text.as_bytes();
        let mut curr = &self.root;
        let mut longest = None;

        for (idx, &b) in bytes.iter().enumerate() {
            if let Some(next) = curr.children.get(&b) {
                curr = next;
                if curr.is_terminal {
                    longest = Some(idx + 1);
                }
            } else {
                break;
            }
        }

        longest.map(|len| &text[..len])
    }
}

#[cfg(test)]
mod tests;
