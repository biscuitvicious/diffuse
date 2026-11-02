//! Iterator adapters and extension traits for Dyck balanced container
//! extraction.

use std::marker::PhantomData;

use super::{
    classifier::{DelimiterCategory, DelimiterClassifier},
    stack::SmallStack,
};

/// Dedicated iterator adapter yielding balanced container ranges `(start, end)`
/// in a sequence.
pub struct DyckBalanced<I, C, T> {
    iter: I,
    classifier: C,
    idx: usize,
    _marker: PhantomData<T>,
}

impl<I, C, T> DyckBalanced<I, C, T> {
    pub fn new(iter: I, classifier: C) -> Self {
        Self {
            iter,
            classifier,
            idx: 0,
            _marker: PhantomData,
        }
    }
}

impl<I, C, T> Iterator for DyckBalanced<I, C, T>
where
    I: Iterator<Item = T>,
    C: DelimiterClassifier<T>,
{
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let mut stack: SmallStack<u8, 32> = SmallStack::new();
        let mut container_start = None;

        for item in self.iter.by_ref() {
            let curr_idx = self.idx;
            self.idx += 1;
            let cat = self.classifier.classify(&item);

            match cat {
                Some(DelimiterCategory::Open(id)) => {
                    if stack.is_empty() && container_start.is_none() {
                        container_start = Some(curr_idx);
                    }
                    stack.push(id);
                }
                Some(DelimiterCategory::Close(id)) => {
                    if stack.last() == Some(&id) {
                        stack.pop();
                        if stack.is_empty() && container_start.is_some() {
                            return Some((container_start.take().unwrap(), curr_idx + 1));
                        }
                    } else {
                        // Unmatched close resets state
                        stack.clear();
                        container_start = None;
                    }
                }
                Some(DelimiterCategory::Symmetric(id)) => {
                    if stack.last() == Some(&id) {
                        stack.pop();
                        if stack.is_empty() && container_start.is_some() {
                            return Some((container_start.take().unwrap(), curr_idx + 1));
                        }
                    } else {
                        if stack.is_empty() && container_start.is_none() {
                            container_start = Some(curr_idx);
                        }
                        stack.push(id);
                    }
                }
                _ => {}
            }
        }

        None
    }
}

/// Extension trait providing Dyck delimiter balancing over generic iterators.
pub trait DiffuseDyckExt: Iterator + Sized {
    fn dyck_balanced<C: DelimiterClassifier<Self::Item>>(
        self,
        classifier: C,
    ) -> DyckBalanced<Self, C, Self::Item>;
}

impl<I: Iterator + Sized> DiffuseDyckExt for I {
    fn dyck_balanced<C: DelimiterClassifier<Self::Item>>(
        self,
        classifier: C,
    ) -> DyckBalanced<Self, C, Self::Item> {
        DyckBalanced::new(self, classifier)
    }
}
