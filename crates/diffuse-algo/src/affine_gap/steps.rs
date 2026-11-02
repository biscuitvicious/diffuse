use std::iter::FusedIterator;

use crate::EditOp;

/// Streaming iterator over affine gap alignment operations.
#[derive(Clone, Debug)]
pub struct AffineAlignmentSteps<'a> {
    steps: &'a [EditOp],
    idx: usize,
}

impl<'a> AffineAlignmentSteps<'a> {
    pub const fn new(steps: &'a [EditOp]) -> Self {
        Self { steps, idx: 0 }
    }
}

impl<'a> Iterator for AffineAlignmentSteps<'a> {
    type Item = EditOp;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.steps.len() {
            let item = self.steps[self.idx];
            self.idx += 1;
            Some(item)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.steps.len() - self.idx;
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for AffineAlignmentSteps<'a> {}

impl<'a> DoubleEndedIterator for AffineAlignmentSteps<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.idx < self.steps.len() {
            let last_idx = self.steps.len() - 1;
            let item = self.steps[last_idx];
            self.steps = &self.steps[..last_idx];
            Some(item)
        } else {
            None
        }
    }
}

impl<'a> FusedIterator for AffineAlignmentSteps<'a> {}
