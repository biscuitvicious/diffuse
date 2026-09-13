//! Shannon Information Content & Empirical Frequency Filter.
//!
//! Academic foundation:
//! - Claude E. Shannon (1948). "A Mathematical Theory of Communication". Bell
//!   System Technical Journal, 27(3), 379–423.
//!
//! # Strict Architectural Separation
//! Operates over generic types `T: Hash + Eq + Clone`.
//! Zero diffuse-domain dependencies.

use std::{hash::Hash, marker::PhantomData};

use rustc_hash::FxHashMap;

/// Empirical frequency and Shannon self-information ranker for generic
/// elements.
#[derive(Clone, Debug, Default)]
pub struct FrequencyRanker<T> {
    counts: FxHashMap<T, usize>,
    total: usize,
}

impl<T: Hash + Eq + Clone> FromIterator<T> for FrequencyRanker<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut counts = FxHashMap::default();
        let mut total = 0;
        for item in iter {
            *counts.entry(item).or_insert(0) += 1;
            total += 1;
        }
        Self { counts, total }
    }
}

impl<T: Hash + Eq + Clone> FrequencyRanker<T> {
    /// Build a frequency ranker from a sample sequence.
    pub fn from_sample<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_iter(iter)
    }

    /// Return total number of observed elements.
    #[inline]
    pub fn total(&self) -> usize {
        self.total
    }

    /// Return the occurrence count of a specific item.
    #[inline]
    pub fn count(&self, item: &T) -> usize {
        self.counts.get(item).copied().unwrap_or(0)
    }

    /// Compute the Shannon self-information of an item in bits:
    /// $$I(x) = -\log_2 P(x) = \log_2 \frac{N}{\text{count}(x)}$$
    ///
    /// If an item has never been observed or total is 0, returns 0.0.
    pub fn self_information_bits(&self, item: &T) -> f64 {
        let count = self.count(item);
        if count == 0 || self.total == 0 {
            return 0.0;
        }
        ((self.total as f64) / (count as f64)).log2()
    }
}

/// Iterator adapter filtering elements whose Shannon self-information meets or
/// exceeds a threshold.
pub struct EntropyFiltered<I, T> {
    iter: I,
    ranker: FrequencyRanker<T>,
    min_bits: f64,
    _marker: PhantomData<T>,
}

impl<I, T> EntropyFiltered<I, T>
where
    I: Iterator<Item = T>,
    T: Hash + Eq + Clone,
{
    pub fn new(iter: I, ranker: FrequencyRanker<T>, min_bits: f64) -> Self {
        Self {
            iter,
            ranker,
            min_bits,
            _marker: PhantomData,
        }
    }
}

impl<I, T> Iterator for EntropyFiltered<I, T>
where
    I: Iterator<Item = T>,
    T: Hash + Eq + Clone,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .by_ref()
            .find(|item| self.ranker.self_information_bits(item) >= self.min_bits)
    }
}

/// Extension trait providing entropy filtering over generic iterators.
pub trait DiffuseEntropyExt: Iterator + Sized {
    fn filter_entropy(
        self,
        ranker: FrequencyRanker<Self::Item>,
        min_bits: f64,
    ) -> EntropyFiltered<Self, Self::Item>
    where
        Self::Item: Hash + Eq + Clone;
}

impl<I: Iterator + Sized> DiffuseEntropyExt for I {
    fn filter_entropy(
        self,
        ranker: FrequencyRanker<Self::Item>,
        min_bits: f64,
    ) -> EntropyFiltered<Self, Self::Item>
    where
        Self::Item: Hash + Eq + Clone,
    {
        EntropyFiltered::new(self, ranker, min_bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frequency_ranker_self_information() {
        // Sample with:
        // 1 occurs 4 times
        // 2 occurs 2 times
        // 3 occurs 1 time
        // 4 occurs 1 time
        // Total = 8
        let sample = vec![1, 1, 1, 1, 2, 2, 3, 4];
        let ranker = FrequencyRanker::from_iter(sample);

        assert_eq!(ranker.total(), 8);
        assert_eq!(ranker.count(&1), 4);
        assert_eq!(ranker.count(&2), 2);
        assert_eq!(ranker.count(&3), 1);
        assert_eq!(ranker.count(&99), 0);

        // I(1) = log2(8 / 4) = log2(2) = 1.0 bit
        assert!((ranker.self_information_bits(&1) - 1.0).abs() < 1e-6);
        // I(2) = log2(8 / 2) = log2(4) = 2.0 bits
        assert!((ranker.self_information_bits(&2) - 2.0).abs() < 1e-6);
        // I(3) = log2(8 / 1) = log2(8) = 3.0 bits
        assert!((ranker.self_information_bits(&3) - 3.0).abs() < 1e-6);
        // Unobserved item returns 0.0
        assert_eq!(ranker.self_information_bits(&99), 0.0);
    }

    #[test]
    fn test_entropy_filter_iterator() {
        let sample = vec![1, 1, 1, 1, 2, 2, 3, 4];
        let ranker = FrequencyRanker::from_iter(sample.clone());

        // Filter items with entropy >= 2.0 bits (only 2, 3, 4 qualify; 1 has
        // only 1.0 bit)
        let filtered: Vec<i32> = sample.into_iter().filter_entropy(ranker, 2.0).collect();
        assert_eq!(filtered, vec![2, 2, 3, 4]);
    }

    #[test]
    fn test_entropy_filter_empty_and_zero_threshold() {
        let empty: Vec<i32> = vec![];
        let ranker = FrequencyRanker::from_iter(empty.clone());
        let filtered: Vec<i32> = empty.into_iter().filter_entropy(ranker, 0.0).collect();
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_frequency_ranker_empty_and_defaults() {
        let ranker: FrequencyRanker<String> = FrequencyRanker::default();
        assert_eq!(ranker.total(), 0);
        assert_eq!(ranker.count(&"hello".to_string()), 0);
        assert_eq!(ranker.self_information_bits(&"hello".to_string()), 0.0);
    }
}
