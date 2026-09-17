//! Augmented Interval Tree for Coordinate & Span Queries.
//!
//! A flat, cache-conscious array-backed augmented interval tree (Cormen et al.
//! §14.3).
//!
//! # Mathematical Foundation
//! An interval $[low, high)$ associated with payload `value: T` is indexed in
//! a balanced binary search tree ordered by `low`. Each node is augmented with
//! `max`, storing the maximum upper bound ($high$) in the node's entire
//! subtree: $$node.max = \max(node.high, left.max, right.max)$$
//!
//! This enables stabbing queries ($p \in [low, high)$) and range overlap
//! queries ($[start, end) \cap [low, high) \neq \emptyset$) to execute in
//! $O(\log N + M)$ time, where $N$ is the number of intervals in the tree and
//! $M$ is the number of reporting matches.
//!
//! # Memory Layout
//! All nodes are stored contiguously in a single flat `Vec<IntervalNode<T>>`.
//! Child pointers are compact `u32` array indices (`u32::MAX` represents
//! `None`), ensuring cache locality with zero pointer chasing or dynamic heap
//! allocations during traversal.

/// Sentinel constant representing an empty child pointer in the flat node
/// array.
pub const NULL_NODE: u32 = u32::MAX;

/// A node in the flat array-backed augmented interval tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntervalNode<T> {
    /// Left bound of the interval (inclusive).
    pub low: usize,
    /// Right bound of the interval (exclusive).
    pub high: usize,
    /// Maximum `high` value across this node and all its descendants.
    pub max: usize,
    /// Index of the left child in the flat `nodes` array, or [`NULL_NODE`].
    pub left: u32,
    /// Index of the right child in the flat `nodes` array, or [`NULL_NODE`].
    pub right: u32,
    /// User payload associated with this interval.
    pub value: T,
}

impl<T> IntervalNode<T> {
    /// Checks if a point falls within this half-open interval $[low, high)$.
    #[inline(always)]
    pub fn contains_point(&self, point: usize) -> bool {
        self.low <= point && point < self.high
    }

    /// Checks if this interval $[low, high)$ overlaps query range $[start,
    /// end)$.
    #[inline(always)]
    pub fn overlaps_range(&self, start: usize, end: usize) -> bool {
        self.low < self.high && start < end && self.low < end && start < self.high
    }
}

/// Cache-conscious, flat array-backed augmented interval tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AugmentedIntervalTree<T> {
    root: u32,
    nodes: Vec<IntervalNode<T>>,
}

impl<T> Default for AugmentedIntervalTree<T> {
    fn default() -> Self {
        Self {
            root: NULL_NODE,
            nodes: Vec::new(),
        }
    }
}

impl<T> AugmentedIntervalTree<T> {
    /// Creates an empty interval tree with no pre-allocated storage.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            root: NULL_NODE,
            nodes: Vec::new(),
        }
    }

    /// Number of intervals stored in the tree.
    #[must_use]
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the tree contains zero intervals.
    #[must_use]
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Clears the tree, removing all intervals while preserving vector
    /// capacity.
    pub fn clear(&mut self) {
        self.root = NULL_NODE;
        self.nodes.clear();
    }

    /// Direct slice access to the underlying flat nodes.
    #[must_use]
    #[inline(always)]
    pub fn nodes(&self) -> &[IntervalNode<T>] {
        &self.nodes
    }

    /// Traverse all intervals stabbing `point` ($low \le point < high$),
    /// invoking `visitor` on each matching node with zero heap allocations.
    pub fn visit_stabbing<'a, F>(&'a self, point: usize, mut visitor: F)
    where
        F: FnMut(&'a IntervalNode<T>),
    {
        if self.root == NULL_NODE {
            return;
        }

        let mut stack = [0u32; 64];
        let mut depth = 0;
        stack[0] = self.root;
        depth += 1;

        while depth > 0 {
            depth -= 1;
            let curr_idx = stack[depth];
            let node = &self.nodes[curr_idx as usize];

            // If subtree max <= point, no interval in this entire subtree can
            // contain point.
            if node.max <= point {
                continue;
            }

            // Check current node [low, high)
            if node.contains_point(point) {
                visitor(node);
            }

            // Right subtree can only match if point >= node.low
            if node.right != NULL_NODE && point >= node.low {
                let right_max = self.nodes[node.right as usize].max;
                if right_max > point && depth < stack.len() {
                    stack[depth] = node.right;
                    depth += 1;
                }
            }

            // Left subtree can only match if left.max > point
            if node.left != NULL_NODE {
                let left_max = self.nodes[node.left as usize].max;
                if left_max > point && depth < stack.len() {
                    stack[depth] = node.left;
                    depth += 1;
                }
            }
        }
    }

    /// Traverse all intervals overlapping query range $[start, end)$ ($low <
    /// end \land start < high$), invoking `visitor` on each matching node
    /// with zero heap allocations.
    pub fn visit_overlapping<'a, F>(&'a self, start: usize, end: usize, mut visitor: F)
    where
        F: FnMut(&'a IntervalNode<T>),
    {
        if self.root == NULL_NODE || start >= end {
            return;
        }

        let mut stack = [0u32; 64];
        let mut depth = 0;
        stack[0] = self.root;
        depth += 1;

        while depth > 0 {
            depth -= 1;
            let curr_idx = stack[depth];
            let node = &self.nodes[curr_idx as usize];

            // If subtree max <= start, no interval in this entire subtree can
            // overlap.
            if node.max <= start {
                continue;
            }

            // Check current node overlap
            if node.overlaps_range(start, end) {
                visitor(node);
            }

            // Right subtree: all nodes have low >= node.low.
            // If node.low >= end, no node in right subtree can have low < end.
            if node.right != NULL_NODE && node.low < end {
                let right_max = self.nodes[node.right as usize].max;
                if right_max > start && depth < stack.len() {
                    stack[depth] = node.right;
                    depth += 1;
                }
            }

            // Left subtree: can match if left.max > start.
            if node.left != NULL_NODE {
                let left_max = self.nodes[node.left as usize].max;
                if left_max > start && depth < stack.len() {
                    stack[depth] = node.left;
                    depth += 1;
                }
            }
        }
    }

    /// Checks if any interval contains `point` ($O(\log N)$ early exit on first
    /// hit).
    #[must_use]
    pub fn has_stabbing(&self, point: usize) -> bool {
        let mut found = false;
        self.visit_stabbing(point, |_| {
            found = true;
        });
        found
    }

    /// Checks if any interval overlaps query range $[start, end)$ ($O(\log N)$
    /// early exit on first hit).
    #[must_use]
    pub fn has_overlapping(&self, start: usize, end: usize) -> bool {
        let mut found = false;
        self.visit_overlapping(start, end, |_| {
            found = true;
        });
        found
    }

    /// Collects references to the payloads of all intervals containing `point`.
    pub fn stabbing_query<'a>(&'a self, point: usize, matches: &mut Vec<&'a T>) {
        self.visit_stabbing(point, |node| {
            matches.push(&node.value);
        });
    }

    /// Collects references to the payloads of all intervals overlapping query
    /// range $[start, end)$.
    pub fn overlapping_query<'a>(&'a self, start: usize, end: usize, matches: &mut Vec<&'a T>) {
        self.visit_overlapping(start, end, |node| {
            matches.push(&node.value);
        });
    }

    /// Collects references to matching nodes containing `point`.
    pub fn stabbing_nodes<'a>(&'a self, point: usize, matches: &mut Vec<&'a IntervalNode<T>>) {
        self.visit_stabbing(point, |node| {
            matches.push(node);
        });
    }

    /// Collects references to matching nodes overlapping query range $[start,
    /// end)$.
    pub fn overlapping_nodes<'a>(
        &'a self,
        start: usize,
        end: usize,
        matches: &mut Vec<&'a IntervalNode<T>>,
    ) {
        self.visit_overlapping(start, end, |node| {
            matches.push(node);
        });
    }
}

impl<T: Clone> AugmentedIntervalTree<T> {
    /// Builds the tree from a pre-sorted slice of intervals `(low, high,
    /// value)`.
    ///
    /// The slice must be sorted primarily by `low` ascending, and secondarily
    /// by `high`. Executes in $O(N)$ linear time.
    #[must_use]
    pub fn from_sorted_slice(intervals: &[(usize, usize, T)]) -> Self {
        let mut tree = Self::new();
        tree.build_from_sorted(intervals);
        tree
    }

    /// Builds the tree from an unsorted slice of intervals `(low, high,
    /// value)`.
    ///
    /// Executes in $O(N \log N)$ time.
    #[must_use]
    pub fn from_slice(intervals: &[(usize, usize, T)]) -> Self {
        let mut tree = Self::new();
        tree.build_from_slice(intervals);
        tree
    }

    /// Re-builds the tree from a pre-sorted slice, reusing existing vector
    /// allocation.
    pub fn build_from_sorted(&mut self, intervals: &[(usize, usize, T)]) {
        self.nodes.clear();
        if intervals.is_empty() {
            self.root = NULL_NODE;
            return;
        }

        self.nodes.reserve(intervals.len());
        // Populate node templates with dummy pointers
        for &(low, high, ref value) in intervals {
            let clamped_high = high.max(low);
            self.nodes.push(IntervalNode {
                low,
                high: clamped_high,
                max: clamped_high,
                left: NULL_NODE,
                right: NULL_NODE,
                value: value.clone(),
            });
        }

        self.root = Self::build_balanced_subtree(&mut self.nodes, 0, intervals.len());
    }

    /// Re-builds the tree from an unsorted slice, sorting and reusing vector
    /// allocation.
    pub fn build_from_slice(&mut self, intervals: &[(usize, usize, T)]) {
        if intervals.is_empty() {
            self.clear();
            return;
        }
        let mut sorted = intervals.to_vec();
        sorted.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        self.build_from_sorted(&sorted);
    }
}

impl<T> AugmentedIntervalTree<T> {
    fn build_balanced_subtree(nodes: &mut [IntervalNode<T>], start: usize, end: usize) -> u32 {
        if start >= end {
            return NULL_NODE;
        }
        let mid = start + (end - start) / 2;

        let left = if start < mid {
            Self::build_balanced_subtree(nodes, start, mid)
        } else {
            NULL_NODE
        };

        let right = if mid + 1 < end {
            Self::build_balanced_subtree(nodes, mid + 1, end)
        } else {
            NULL_NODE
        };

        let mut max = nodes[mid].high;
        if left != NULL_NODE {
            max = max.max(nodes[left as usize].max);
        }
        if right != NULL_NODE {
            max = max.max(nodes[right as usize].max);
        }

        nodes[mid].left = left;
        nodes[mid].right = right;
        nodes[mid].max = max;

        mid as u32
    }

    /// Builds the tree taking ownership of an unsorted vector of `(low, high,
    /// value)`.
    ///
    /// Sorts in-place and constructs the tree in $O(N \log N)$ with zero extra
    /// vector allocations.
    #[must_use]
    pub fn from_vec(mut intervals: Vec<(usize, usize, T)>) -> Self {
        if intervals.is_empty() {
            return Self::new();
        }
        intervals.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let mut nodes = Vec::with_capacity(intervals.len());
        for (low, high, value) in intervals {
            let clamped_high = high.max(low);
            nodes.push(IntervalNode {
                low,
                high: clamped_high,
                max: clamped_high,
                left: NULL_NODE,
                right: NULL_NODE,
                value,
            });
        }
        let len = nodes.len();
        let root = Self::build_balanced_subtree(&mut nodes, 0, len);
        Self { root, nodes }
    }
}

#[cfg(test)]
mod tests;
