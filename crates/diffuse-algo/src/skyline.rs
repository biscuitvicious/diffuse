//! Generic Skyline Matrix (Variable-Band Profile Storage) and Adaptive
//! Wavefront Corridor Builder.
//!
//! # Mathematical Foundation
//! In sequence differencing and dynamic programming, standard rectangular
//! $O(M \times N)$ matrices require prohibitive memory when aligning large
//! changesets. This module implements **Profile / Skyline Storage**
//! (Jennings, 1966), where each row $i \in [0 \dots M]$ stores only its active
//! column corridor $[j_{\min}(i) \dots j_{\max}(i)]$ contiguously in a single
//! flat allocation.
//!
//! Paired with **Adaptive Banded DP** (Chao et al., 1992) and **Wavefront
//! Alignment** principles (Marco-Sola et al., 2021), the corridor dynamically
//! tracks the expected trajectory $j = \lfloor i \cdot N / M \rfloor$,
//! stretching locally where changes diverge and contracting along clean
//! matches.

use std::ops::RangeInclusive;

/// Metadata for a single row in the skyline envelope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkylineRow {
    /// Leftmost valid column index for this row ($j_{\min}$).
    pub start: usize,
    /// Rightmost valid column index for this row ($j_{\max}$, inclusive).
    pub end: usize,
    /// Starting offset in the contiguous cell storage buffer.
    pub offset: usize,
}

/// A cache-optimized, variable-bandwidth dynamic programming matrix.
///
/// Cells within the active envelope are stored contiguously in a single vector.
/// Accessing $(i, j)$ resolves in $O(1)$ arithmetic with zero heap or hashing
/// overhead. Any cell coordinates outside the active envelope return `None`.
#[derive(Clone, Debug, Default)]
pub struct SkylineMatrix<T> {
    rows: Vec<SkylineRow>,
    cells: Vec<T>,
    m: usize,
    n: usize,
}

impl<T> SkylineMatrix<T> {
    /// Creates an empty matrix with no pre-allocated envelope.
    #[must_use]
    pub const fn new_empty() -> Self {
        Self {
            rows: Vec::new(),
            cells: Vec::new(),
            m: 0,
            n: 0,
        }
    }

    /// Number of rows ($M$). Valid row indices are in `0..=M`.
    #[must_use]
    #[inline(always)]
    pub fn m(&self) -> usize {
        self.m
    }

    /// Number of columns ($N$). Valid column indices are in `0..=N`.
    #[must_use]
    #[inline(always)]
    pub fn n(&self) -> usize {
        self.n
    }

    /// Total number of cells allocated across all rows within the envelope.
    #[must_use]
    #[inline(always)]
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Checks if coordinate $(i, j)$ is within the active skyline envelope.
    #[must_use]
    #[inline(always)]
    pub fn is_in_envelope(&self, i: usize, j: usize) -> bool {
        if i > self.m {
            return false;
        }
        let row = self.rows[i];
        j >= row.start && j <= row.end
    }

    /// Returns the active column range $[j_{\min} \dots j_{\max}]$ for row $i$.
    ///
    /// # Panics
    /// Panics if $i > M$.
    #[must_use]
    #[inline(always)]
    pub fn row_range(&self, i: usize) -> RangeInclusive<usize> {
        let row = self.rows[i];
        row.start..=row.end
    }

    /// Direct $O(1)$ read of cell $(i, j)$.
    ///
    /// Returns `None` if $(i, j)$ is outside the active skyline envelope.
    #[must_use]
    #[inline(always)]
    pub fn get(&self, i: usize, j: usize) -> Option<&T> {
        if i > self.m {
            return None;
        }
        let row = self.rows[i];
        if j >= row.start && j <= row.end {
            Some(&self.cells[row.offset + (j - row.start)])
        } else {
            None
        }
    }

    /// Direct $O(1)$ mutable access to cell $(i, j)$.
    ///
    /// Returns `None` if $(i, j)$ is outside the active skyline envelope.
    #[must_use]
    #[inline(always)]
    pub fn get_mut(&mut self, i: usize, j: usize) -> Option<&mut T> {
        if i > self.m {
            return None;
        }
        let row = self.rows[i];
        if j >= row.start && j <= row.end {
            let idx = row.offset + (j - row.start);
            Some(&mut self.cells[idx])
        } else {
            None
        }
    }
}

impl<T: Clone> SkylineMatrix<T> {
    /// Constructs a new `SkylineMatrix` initialized with the given per-row
    /// bounds and default cell value.
    ///
    /// # Arguments
    /// * `m` - Sequence A length (rows $0 \dots M$).
    /// * `n` - Sequence B length (columns $0 \dots N$).
    /// * `bounds` - Slice of length $M + 1$, where `bounds[i] = (start_j,
    ///   end_j)`.
    /// * `default_val` - Value used to populate every cell in the envelope.
    #[must_use]
    pub fn new_with_envelope(
        m: usize,
        n: usize,
        bounds: &[(usize, usize)],
        default_val: T,
    ) -> Self {
        let mut matrix = Self::new_empty();
        matrix.init_envelope(m, n, bounds, default_val);
        matrix
    }

    /// Re-initializes this matrix with a new envelope, reusing existing vector
    /// capacities to avoid heap allocations.
    pub fn init_envelope(&mut self, m: usize, n: usize, bounds: &[(usize, usize)], default_val: T) {
        self.m = m;
        self.n = n;
        self.rows.clear();
        self.rows.reserve(m + 1);

        let mut offset = 0;
        for &(start, end) in bounds.iter().take(m + 1) {
            let clamped_start = start.min(n);
            let clamped_end = end.min(n).max(clamped_start);
            let len = clamped_end - clamped_start + 1;
            self.rows.push(SkylineRow {
                start: clamped_start,
                end: clamped_end,
                offset,
            });
            offset += len;
        }

        self.cells.clear();
        self.cells.resize(offset, default_val);
    }
}

/// Computed envelope bounds representing active column ranges for each row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdaptiveEnvelope {
    /// Active $(j_{\min}, j_{\max})$ column bounds for each row $i \in [0 \dots
    /// M]$.
    pub bounds: Vec<(usize, usize)>,
    /// Total number of active cells across all rows.
    pub total_cells: usize,
}

/// Builder for computing adaptive wavefront corridor envelopes.
#[derive(Clone, Copy, Debug)]
pub struct AdaptiveEnvelopeBuilder {
    m: usize,
    n: usize,
    bandwidth_min: usize,
    bandwidth_slack: usize,
    budget: usize,
}

impl AdaptiveEnvelopeBuilder {
    /// Create a new envelope builder for a grid of size $(M + 1) \times (N +
    /// 1)$.
    #[must_use]
    pub const fn new(m: usize, n: usize) -> Self {
        Self {
            m,
            n,
            bandwidth_min: 64,
            bandwidth_slack: 32,
            budget: 10_000_000,
        }
    }

    /// Set the minimum corridor half-bandwidth ($K_{\min}$). Default: 64.
    #[must_use]
    pub const fn with_bandwidth_min(mut self, min: usize) -> Self {
        self.bandwidth_min = min;
        self
    }

    /// Set the asymmetric bandwidth slack added to $|M - N|$. Default: 32.
    #[must_use]
    pub const fn with_bandwidth_slack(mut self, slack: usize) -> Self {
        self.bandwidth_slack = slack;
        self
    }

    /// Set the maximum cell budget ceiling. Default: 10,000,000.
    #[must_use]
    pub const fn with_budget(mut self, budget: usize) -> Self {
        self.budget = budget;
        self
    }

    /// Build the adaptive envelope corridor.
    #[must_use]
    pub fn build(self) -> AdaptiveEnvelope {
        let m = self.m;
        let n = self.n;

        // If the full rectangular matrix fits within budget and bandwidth
        // covers it:
        let full_cells = (m + 1).saturating_mul(n + 1);
        let delta = (m as isize - n as isize).unsigned_abs();
        let desired_k = self.bandwidth_min.max(delta + self.bandwidth_slack);

        if full_cells <= self.budget && desired_k >= m.max(n) {
            let bounds = vec![(0, n); m + 1];
            return AdaptiveEnvelope {
                bounds,
                total_cells: full_cells,
            };
        }

        // Otherwise, construct a diagonal corridor centered on expected slope j
        // = i * N / M.
        let mut k = desired_k;
        let mut envelope = Self::compute_corridor(m, n, k);

        // If total cells exceed the budget, adaptively clamp K down to a
        // minimum of 4.
        if envelope.total_cells > self.budget {
            let safe_k = self.budget.saturating_sub(m + 1) / (2 * (m + 1).max(1));
            if safe_k >= 4 {
                k = k.min(safe_k);
                envelope = Self::compute_corridor(m, n, k);
            }
        }

        envelope
    }

    fn compute_corridor(m: usize, n: usize, k: usize) -> AdaptiveEnvelope {
        let mut bounds = Vec::with_capacity(m + 1);
        let mut total_cells = 0;

        for i in 0..=m {
            let expected_j = if m == 0 {
                0
            } else {
                (i * n).checked_div(m).unwrap_or(0)
            };

            let start_j = expected_j.saturating_sub(k);
            let end_j = (expected_j + k).min(n);

            bounds.push((start_j, end_j));
            total_cells += end_j - start_j + 1;
        }

        AdaptiveEnvelope {
            bounds,
            total_cells,
        }
    }
}

#[cfg(test)]
mod tests;
