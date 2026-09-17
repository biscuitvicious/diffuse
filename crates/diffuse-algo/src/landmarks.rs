//! Maximal Unique Matches (MUMs) and $N$-Gram Landmark Indexing.
//!
//! Academic foundation:
//! - Arthur L. Delcher, Simon Kasif, et al. (1999). "Alignment of Whole
//!   Genomes". Nucleic Acids Research, 27(11), 2369–2376.
//! - Michael Burrows and David J. Wheeler (1994). "A Block-sorting Lossless
//!   Data Compression Algorithm".
//!
//! # Strict Architectural Separation
//! Operates over generic streams `Iterator<Item = T>` and slices `&[T]` where
//! `T: Hash + Eq + Clone`. Has zero diffuse-domain dependencies.

use std::{hash::Hash, marker::PhantomData};

/// An aligned match interval between sequence A and sequence B.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchInterval {
    /// Start index in sequence A.
    pub a_start: usize,
    /// Start index in sequence B.
    pub b_start: usize,
    /// Number of matching elements.
    pub len: usize,
}

impl MatchInterval {
    #[inline]
    pub const fn new(a_start: usize, b_start: usize, len: usize) -> Self {
        Self {
            a_start,
            b_start,
            len,
        }
    }

    #[inline]
    pub const fn a_end(&self) -> usize {
        self.a_start + self.len
    }

    #[inline]
    pub const fn b_end(&self) -> usize {
        self.b_start + self.len
    }
}

/// Finds all Maximal Unique Matches (MUMs) between two generic slices.
pub fn find_mums<T: Hash + Eq>(a: &[T], b: &[T], min_len: usize) -> Vec<MatchInterval> {
    if a.len() < min_len || b.len() < min_len || min_len == 0 {
        return Vec::new();
    }

    let mut mums = Vec::new();

    for i in 0..a.len() {
        for j in 0..b.len() {
            // Only inspect left-maximal matches
            if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
                continue;
            }
            if a[i] != b[j] {
                continue;
            }

            let mut len = 0;
            while i + len < a.len() && j + len < b.len() && a[i + len] == b[j + len] {
                len += 1;
            }

            if len < min_len {
                continue;
            }

            let match_slice = &a[i..i + len];

            // Verify unique occurrence in sequence A
            let count_a = a.windows(len).filter(|w| *w == match_slice).count();
            if count_a != 1 {
                continue;
            }

            // Verify unique occurrence in sequence B
            let count_b = b.windows(len).filter(|w| *w == match_slice).count();
            if count_b != 1 {
                continue;
            }

            mums.push(MatchInterval::new(i, j, len));
        }
    }

    mums
}

/// Finds a strictly monotonic, non-overlapping chain of items
/// maximizing a custom objective score function.
pub fn find_monotonic_chain<T, I, S>(items: &[T], interval_fn: I, score_fn: S) -> Vec<T>
where
    T: Clone,
    I: Fn(&T) -> MatchInterval,
    S: Fn(&T) -> f64,
{
    if items.is_empty() {
        return Vec::new();
    }

    let mut indexed: Vec<(usize, MatchInterval, f64)> = items
        .iter()
        .enumerate()
        .map(|(i, item)| (i, interval_fn(item), score_fn(item)))
        .collect();
    indexed.sort_by_key(|(_, m, _)| m.a_start);

    let n = indexed.len();
    let mut dp: Vec<(f64, Option<usize>)> = Vec::with_capacity(n);

    for k in 0..n {
        let (_, ref k_int, k_score) = indexed[k];
        let mut best_score = k_score;
        let mut best_prev = None;
        for prev in 0..k {
            let (_, ref prev_int, _) = indexed[prev];
            if prev_int.a_end() <= k_int.a_start && prev_int.b_end() <= k_int.b_start {
                let score = dp[prev].0 + k_score;
                if score > best_score {
                    best_score = score;
                    best_prev = Some(prev);
                }
            }
        }
        dp.push((best_score, best_prev));
    }

    let mut best_idx = 0;
    let mut max_score = 0.0;
    for (k, &(score, _)) in dp.iter().enumerate() {
        if score > max_score {
            max_score = score;
            best_idx = k;
        }
    }

    let mut chain = Vec::new();
    let mut curr = Some(best_idx);
    while let Some(idx) = curr {
        chain.push(items[indexed[idx].0].clone());
        curr = dp[idx].1;
    }
    chain.reverse();
    chain
}

/// Finds a strictly monotonic, non-overlapping chain of Maximal Unique Matches
/// maximizing cumulative coverage.
pub fn find_monotonic_mums<T: Hash + Eq>(a: &[T], b: &[T], min_len: usize) -> Vec<MatchInterval> {
    let mums = find_mums(a, b, min_len);
    find_monotonic_chain(&mums, |m| *m, |m| m.len as f64)
}

/// Finds a strictly monotonic chain of unique matching elements between
/// sequence A and sequence B using hash-indexed frequency counting and $O(K
/// \log K)$ Longest Increasing Subsequence (LIS).
///
/// Elements that occur more than once in either sequence, or that do not
/// satisfy `predicate`, are ignored.
pub fn find_unique_anchors<T: Hash + Eq>(
    a: &[T],
    b: &[T],
    predicate: impl Fn(&T) -> bool,
) -> Vec<(usize, usize)> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }

    use rustc_hash::FxHashMap;

    let mut map_a: FxHashMap<&T, (usize, usize)> = FxHashMap::default();
    for (i, item) in a.iter().enumerate() {
        if predicate(item) {
            let entry = map_a.entry(item).or_insert((0, i));
            entry.0 += 1;
        }
    }

    let mut map_b: FxHashMap<&T, (usize, usize)> = FxHashMap::default();
    for (j, item) in b.iter().enumerate() {
        if predicate(item) {
            let entry = map_b.entry(item).or_insert((0, j));
            entry.0 += 1;
        }
    }

    let mut matches = Vec::new();
    for (&item, &(count_a, idx_a)) in &map_a {
        if count_a == 1
            && let Some(&(count_b, idx_b)) = map_b.get(item)
            && count_b == 1
        {
            matches.push((idx_a, idx_b));
        }
    }

    if matches.is_empty() {
        return Vec::new();
    }

    matches.sort_unstable_by_key(|&(idx_a, _)| idx_a);

    // Compute Longest Increasing Subsequence (LIS) on idx_b in O(K log K)
    let mut tails: Vec<usize> = Vec::new();
    let mut tail_prev: Vec<usize> = Vec::new();
    let mut prev_idx: Vec<Option<usize>> = vec![None; matches.len()];

    for (k, &(_, b_val)) in matches.iter().enumerate() {
        let pos = tails.partition_point(|&val| val < b_val);
        if pos == tails.len() {
            tails.push(b_val);
            tail_prev.push(k);
        } else {
            tails[pos] = b_val;
            tail_prev[pos] = k;
        }
        if pos > 0 {
            prev_idx[k] = Some(tail_prev[pos - 1]);
        }
    }

    let mut chain = Vec::new();
    if let Some(&last_match_idx) = tail_prev.last() {
        let mut curr = Some(last_match_idx);
        while let Some(idx) = curr {
            chain.push(matches[idx]);
            curr = prev_idx[idx];
        }
        chain.reverse();
    }
    chain
}

/// Dedicated iterator struct for streaming maximal unique match landmarks.
pub struct Landmarks<I1, I2, T> {
    stream_a: Option<I1>,
    stream_b: Option<I2>,
    min_len: usize,
    buffer: Vec<MatchInterval>,
    idx: usize,
    _marker: PhantomData<T>,
}

impl<I1, I2, T> Landmarks<I1, I2, T>
where
    I1: Iterator<Item = T>,
    I2: Iterator<Item = T>,
    T: Hash + Eq,
{
    pub fn new(stream_a: I1, stream_b: I2, min_len: usize) -> Self {
        Self {
            stream_a: Some(stream_a),
            stream_b: Some(stream_b),
            min_len,
            buffer: Vec::new(),
            idx: 0,
            _marker: PhantomData,
        }
    }
}

impl<I1, I2, T> Iterator for Landmarks<I1, I2, T>
where
    I1: Iterator<Item = T>,
    I2: Iterator<Item = T>,
    T: Hash + Eq,
{
    type Item = MatchInterval;

    fn next(&mut self) -> Option<Self::Item> {
        if let (Some(sa), Some(sb)) = (self.stream_a.take(), self.stream_b.take()) {
            let a_items: Vec<T> = sa.collect();
            let b_items: Vec<T> = sb.collect();
            self.buffer = find_monotonic_mums(&a_items, &b_items, self.min_len);
        }
        if self.idx < self.buffer.len() {
            let item = self.buffer[self.idx];
            self.idx += 1;
            Some(item)
        } else {
            None
        }
    }
}

/// Extension trait for sequences and iterators providing MUM landmark
/// detection.
pub trait DiffuseSequenceExt: Iterator + Sized {
    fn find_landmarks<I2: Iterator<Item = Self::Item>>(
        self,
        other: I2,
        min_len: usize,
    ) -> Landmarks<Self, I2, Self::Item>
    where
        Self::Item: Hash + Eq;
}

impl<I: Iterator + Sized> DiffuseSequenceExt for I {
    fn find_landmarks<I2: Iterator<Item = Self::Item>>(
        self,
        other: I2,
        min_len: usize,
    ) -> Landmarks<Self, I2, Self::Item>
    where
        Self::Item: Hash + Eq,
    {
        Landmarks::new(self, other, min_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_landmarks_empty_sequences() {
        let a: Vec<u32> = vec![];
        let b: Vec<u32> = vec![];
        assert_eq!(find_mums(&a, &b, 2), vec![]);
        assert_eq!(find_monotonic_mums(&a, &b, 2), vec![]);
    }

    #[test]
    fn test_landmarks_single_exact_mums() {
        // Shared unique 3-gram [10, 20, 30]
        let a = vec![1, 2, 10, 20, 30, 3];
        let b = vec![9, 10, 20, 30, 8, 7];
        let mums = find_mums(&a, &b, 2);
        assert_eq!(mums, vec![MatchInterval::new(2, 1, 3)]);
    }

    #[test]
    fn test_landmarks_rejects_duplicates() {
        // [10, 20] occurs twice in sequence A -> not unique in A!
        let a = vec![1, 10, 20, 2, 10, 20, 3];
        let b = vec![9, 10, 20, 8];
        let mums = find_mums(&a, &b, 2);
        assert!(mums.is_empty(), "Duplicate in A must be rejected as MUM");
    }

    #[test]
    fn test_landmarks_monotonic_chaining() {
        // Sequence with two disjoint unique landmarks:
        // [10, 20] at a=(1..3), b=(2..4)
        // [50, 60] at a=(5..7), b=(6..8)
        let a = vec![0, 10, 20, 30, 40, 50, 60, 70];
        let b = vec![99, 98, 10, 20, 97, 96, 50, 60, 95];
        let chain = find_monotonic_mums(&a, &b, 2);
        assert_eq!(
            chain,
            vec![MatchInterval::new(1, 2, 2), MatchInterval::new(5, 6, 2)]
        );
    }

    #[test]
    fn test_landmarks_iterator_adapter() {
        let a = vec![1, 2, 10, 20, 30, 3];
        let b = vec![9, 10, 20, 30, 8];
        let mut iter = a.into_iter().find_landmarks(b.into_iter(), 2);
        assert_eq!(iter.next(), Some(MatchInterval::new(2, 1, 3)));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_landmarks_edge_cases() {
        let a = vec![1, 2, 3];
        let b = vec![1, 2, 3];

        // min_len == 0 returns empty
        assert_eq!(find_mums(&a, &b, 0), vec![]);

        // short sequences below min_len return empty
        assert_eq!(find_mums(&a, &b, 5), vec![]);
        assert_eq!(find_mums(&[1], &b, 2), vec![]);

        // Completely disjoint sequences
        let c = vec![7, 8, 9];
        assert_eq!(find_mums(&a, &c, 2), vec![]);
        assert_eq!(find_monotonic_mums(&a, &c, 2), vec![]);

        // Duplicate in sequence B must be rejected
        let b_dup = vec![9, 1, 2, 8, 1, 2, 7];
        assert_eq!(find_mums(&a, &b_dup, 2), vec![]);

        // MatchInterval accessor methods
        let interval = MatchInterval::new(10, 20, 5);
        assert_eq!(interval.a_end(), 15);
        assert_eq!(interval.b_end(), 25);
    }

    #[test]
    fn test_find_monotonic_chain_custom_items() {
        #[derive(Clone, Debug, PartialEq)]
        struct CustomItem {
            a: usize,
            b: usize,
            len: usize,
            weight: f64,
        }

        // Empty items
        let empty: Vec<CustomItem> = vec![];
        assert_eq!(
            find_monotonic_chain(
                &empty,
                |i| MatchInterval::new(i.a, i.b, i.len),
                |i| i.weight
            ),
            vec![]
        );

        // Multiple candidates where one path has higher total weight:
        // Item 1: a=0..5, b=0..5, weight=10.0
        // Item 2: a=1..4, b=1..4, weight=50.0 (conflicts with 1)
        // Item 3: a=6..10, b=6..10, weight=20.0 (chains with 1 or 2)
        let items = vec![
            CustomItem {
                a: 0,
                b: 0,
                len: 5,
                weight: 10.0,
            },
            CustomItem {
                a: 1,
                b: 1,
                len: 3,
                weight: 50.0,
            },
            CustomItem {
                a: 6,
                b: 6,
                len: 4,
                weight: 20.0,
            },
        ];

        let chain = find_monotonic_chain(
            &items,
            |i| MatchInterval::new(i.a, i.b, i.len),
            |i| i.weight,
        );
        // Item 2 (50.0) + Item 3 (20.0) = 70.0 should win over Item 1 (10.0) +
        // Item 3 (20.0) = 30.0
        assert_eq!(
            chain,
            vec![
                CustomItem {
                    a: 1,
                    b: 1,
                    len: 3,
                    weight: 50.0,
                },
                CustomItem {
                    a: 6,
                    b: 6,
                    len: 4,
                    weight: 20.0,
                },
            ]
        );
    }

    #[test]
    fn test_find_unique_anchors_lis() {
        let a = ["apple", "banana", "cherry", "date", "fig", "grape"];
        let b = ["banana", "apple", "cherry", "kiwi", "date", "grape"];
        let anchors = find_unique_anchors(&a, &b, |_| true);
        assert_eq!(anchors, vec![(1, 0), (2, 2), (3, 4), (5, 5)]);

        let anchors_no_cherry = find_unique_anchors(&a, &b, |&s| s != "cherry");
        assert_eq!(anchors_no_cherry, vec![(1, 0), (3, 4), (5, 5)]);

        let a_dup = ["apple", "apple", "banana", "cherry"];
        let b_dup = ["apple", "banana", "cherry"];
        let anchors_dup = find_unique_anchors(&a_dup, &b_dup, |_| true);
        assert_eq!(anchors_dup, vec![(2, 1), (3, 2)]);
    }
}
