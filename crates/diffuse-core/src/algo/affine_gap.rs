//! Gotoh Affine Gap Sequence Aligner with Zero-Allocation Workspace.
//!
//! Academic foundation:
//! - Osamu Gotoh (1982). "An Improved Algorithm for Matching Biological
//!   Sequences". Journal of Molecular Biology, 162(3), 705–708.
//! - Saul B. Needleman and Christian D. Wunsch (1970). "A General Method
//!   Applicable to the Search for Similarities in the Amino Acid Sequence of
//!   Two Proteins". Journal of Molecular Biology, 48(3), 443–453.
//!
//! # Strict Architectural Separation
//! Operates over generic slices `&[T]` where `T: PartialEq`.
//! Zero diffuse-domain dependencies.
//! Reusable [`AffineGapWorkspace`] avoids heap allocations across repeated
//! runs.

use crate::algo::EditOp;

/// Scoring parameters for Gotoh's affine gap penalty model.
///
/// Under affine gap costs, opening a gap of length $k$ incurs:
/// $$\text{Cost}(k) = \text{gap\_open} + k \cdot \text{gap\_extend}$$
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AffineGapPenalties {
    /// Reward for matching identical elements ($s > 0$).
    pub match_score: i32,
    /// Optional penalty subtracted for mismatched aligned elements ($p_{mis}
    /// \ge 0$), or `None` to disallow substitutions (standard diff engine
    /// behavior where only identical items are Kept).
    pub mismatch_penalty: Option<i32>,
    /// High initial cost to open a gap ($g_{open} > 0$).
    pub gap_open_penalty: i32,
    /// Incremental cost to extend an existing gap ($g_{extend} > 0$).
    pub gap_extend_penalty: i32,
}

impl Default for AffineGapPenalties {
    /// Default penalties tuned to suppress micro-token fragmentation:
    /// High gap opening penalty (5) prevents isolated coincidental punctuation
    /// matches from splitting cohesive replacement spans, and substitutions
    /// are disabled (`None`) for diff LCS correctness.
    fn default() -> Self {
        Self {
            match_score: 2,
            mismatch_penalty: None,
            gap_open_penalty: 5,
            gap_extend_penalty: 1,
        }
    }
}

impl AffineGapPenalties {
    pub const fn new(
        match_score: i32,
        mismatch_penalty: Option<i32>,
        gap_open_penalty: i32,
        gap_extend_penalty: i32,
    ) -> Self {
        Self {
            match_score,
            mismatch_penalty,
            gap_open_penalty,
            gap_extend_penalty,
        }
    }
}

/// Dynamic programming states in Gotoh's three-matrix recurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum DpState {
    Match = 0,
    GapA = 1, // Gap in sequence A (Insertion of B)
    GapB = 2, // Gap in sequence B (Deletion from A)
}

/// Reusable workspace holding dynamic programming matrices and traceback
/// buffers.
///
/// Prevents heap allocations when performing thousands of sequence alignments.
#[derive(Clone, Debug, Default)]
pub struct AffineGapWorkspace {
    m_matrix: Vec<i32>,
    ix_matrix: Vec<i32>,
    iy_matrix: Vec<i32>,
    traceback: Vec<u8>,
    steps: Vec<EditOp>,
}

impl AffineGapWorkspace {
    /// Creates a new empty workspace.
    pub fn new() -> Self {
        Self::default()
    }

    /// Aligns sequences `a` and `b` and returns a streaming iterator over the
    /// edit operations.
    pub fn align_steps<'a, T: PartialEq>(
        &'a mut self,
        a: &[T],
        b: &[T],
        penalties: &AffineGapPenalties,
    ) -> AffineAlignmentSteps<'a> {
        let steps = self.align(a, b, penalties);
        AffineAlignmentSteps::new(steps)
    }

    /// Aligns sequences `a` and `b` under Gotoh's affine gap penalty model,
    /// reusing internal capacity.
    pub fn align<T: PartialEq>(
        &mut self,
        a: &[T],
        b: &[T],
        penalties: &AffineGapPenalties,
    ) -> &[EditOp] {
        let m = a.len();
        let n = b.len();

        if m == 0 && n == 0 {
            self.steps.clear();
            return &self.steps;
        }

        if m == 0 {
            self.steps.clear();
            self.steps.reserve(n);
            for j in 0..n {
                self.steps.push(EditOp::Insert { b_idx: j });
            }
            return &self.steps;
        }

        if n == 0 {
            self.steps.clear();
            self.steps.reserve(m);
            for i in 0..m {
                self.steps.push(EditOp::Delete { a_idx: i });
            }
            return &self.steps;
        }

        let stride = n + 1;
        let total_cells = (m + 1) * stride;

        const NEG_INF: i32 = -1_000_000_000;

        self.m_matrix.clear();
        self.m_matrix.resize(total_cells, NEG_INF);

        self.ix_matrix.clear();
        self.ix_matrix.resize(total_cells, NEG_INF);

        self.iy_matrix.clear();
        self.iy_matrix.resize(total_cells, NEG_INF);

        self.traceback.clear();
        self.traceback.resize(total_cells, 0);

        // Base cases at (0, 0)
        self.m_matrix[0] = 0;

        let gap_cost = penalties.gap_open_penalty + penalties.gap_extend_penalty;

        // Base cases along boundaries:
        // ix (deletions from A)
        for i in 1..=m {
            let idx = i * stride;
            let cost = -(penalties.gap_open_penalty + (i as i32) * penalties.gap_extend_penalty);
            self.ix_matrix[idx] = cost;
            // Traceback: i == 1 opens from M (1), i > 1 extends (0)
            let ix_src = if i == 1 { 1u8 } else { 0u8 };
            self.traceback[idx] = ix_src << 2;
        }

        // iy (insertions from B)
        for j in 1..=n {
            let cost = -(penalties.gap_open_penalty + (j as i32) * penalties.gap_extend_penalty);
            self.iy_matrix[j] = cost;
            // Traceback: j == 1 opens from M (1), j > 1 extends (0)
            let iy_src = if j == 1 { 1u8 } else { 0u8 };
            self.traceback[j] = iy_src << 4;
        }

        // Fill Gotoh recurrence grid
        for i in 1..=m {
            let prev_row = (i - 1) * stride;
            let curr_row = i * stride;

            let a_elem = &a[i - 1];

            for j in 1..=n {
                let curr_idx = curr_row + j;
                let b_elem = &b[j - 1];

                // 1. Compute Ix(i, j) - Deletion from A (gap in B)
                // Can extend from Ix, or open from M or Iy
                let ext_ix = self.ix_matrix[prev_row + j] - penalties.gap_extend_penalty;
                let open_m_ix = self.m_matrix[prev_row + j] - gap_cost;
                let open_iy_ix = self.iy_matrix[prev_row + j] - gap_cost;

                let (best_ix, ix_src) = if ext_ix >= open_m_ix && ext_ix >= open_iy_ix {
                    (ext_ix, 0u8)
                } else if open_m_ix >= open_iy_ix {
                    (open_m_ix, 1u8)
                } else {
                    (open_iy_ix, 2u8)
                };
                self.ix_matrix[curr_idx] = best_ix;

                // 2. Compute Iy(i, j) - Insertion from B (gap in A)
                // Can extend from Iy, or open from M or Ix
                let ext_iy = self.iy_matrix[curr_row + (j - 1)] - penalties.gap_extend_penalty;
                let open_m_iy = self.m_matrix[curr_row + (j - 1)] - gap_cost;
                let open_ix_iy = self.ix_matrix[curr_row + (j - 1)] - gap_cost;

                let (best_iy, iy_src) = if ext_iy >= open_m_iy && ext_iy >= open_ix_iy {
                    (ext_iy, 0u8)
                } else if open_m_iy >= open_ix_iy {
                    (open_m_iy, 1u8)
                } else {
                    (open_ix_iy, 2u8)
                };
                self.iy_matrix[curr_idx] = best_iy;

                // 3. Compute M(i, j) - Match/Mismatch
                let score_opt = if a_elem == b_elem {
                    Some(penalties.match_score)
                } else {
                    penalties.mismatch_penalty.map(|p| -p)
                };

                let (m_score, m_src) = if let Some(score) = score_opt {
                    let diag_m = self.m_matrix[prev_row + (j - 1)];
                    let diag_iy = self.iy_matrix[prev_row + (j - 1)];
                    let diag_ix = self.ix_matrix[prev_row + (j - 1)];

                    let (best_diag, src) = if diag_m >= diag_iy && diag_m >= diag_ix {
                        (diag_m, 0u8)
                    } else if diag_iy >= diag_ix {
                        (diag_iy, 1u8)
                    } else {
                        (diag_ix, 2u8)
                    };

                    (score + best_diag, src)
                } else {
                    (NEG_INF, 0u8)
                };
                self.m_matrix[curr_idx] = m_score;

                // Pack traceback sources into a single byte:
                // bits 0..1: M source (0=M, 1=Iy/GapA, 2=Ix/GapB)
                // bits 2..3: Ix source (0=Ix ext, 1=from M, 2=from Iy)
                // bits 4..5: Iy source (0=Iy ext, 1=from M, 2=from Ix)
                self.traceback[curr_idx] =
                    (m_src & 0x03) | ((ix_src & 0x03) << 2) | ((iy_src & 0x03) << 4);
            }
        }

        // Traceback from (m, n)
        self.steps.clear();
        let end_idx = m * stride + n;
        let score_m = self.m_matrix[end_idx];
        let score_ix = self.ix_matrix[end_idx];
        let score_iy = self.iy_matrix[end_idx];

        let mut curr_state = if score_m >= score_ix && score_m >= score_iy {
            DpState::Match
        } else if score_ix >= score_iy {
            DpState::GapB
        } else {
            DpState::GapA
        };

        let mut curr_i = m;
        let mut curr_j = n;

        while curr_i > 0 || curr_j > 0 {
            if curr_i == 0 {
                self.steps.push(EditOp::Insert { b_idx: curr_j - 1 });
                curr_j -= 1;
                continue;
            }
            if curr_j == 0 {
                self.steps.push(EditOp::Delete { a_idx: curr_i - 1 });
                curr_i -= 1;
                continue;
            }

            let idx = curr_i * stride + curr_j;
            let tb = self.traceback[idx];

            match curr_state {
                DpState::Match => {
                    let a_idx = curr_i - 1;
                    let b_idx = curr_j - 1;

                    if a[a_idx] == b[b_idx] {
                        self.steps.push(EditOp::Keep { a_idx, b_idx });
                    } else {
                        self.steps.push(EditOp::Insert { b_idx });
                        self.steps.push(EditOp::Delete { a_idx });
                    }

                    let src = tb & 0x03;
                    curr_state = match src {
                        1 => DpState::GapA,
                        2 => DpState::GapB,
                        _ => DpState::Match,
                    };
                    curr_i -= 1;
                    curr_j -= 1;
                }
                DpState::GapB => {
                    self.steps.push(EditOp::Delete { a_idx: curr_i - 1 });
                    let src = (tb >> 2) & 0x03;
                    curr_state = match src {
                        0 => DpState::GapB,
                        1 => DpState::Match,
                        _ => DpState::GapA,
                    };
                    curr_i -= 1;
                }
                DpState::GapA => {
                    self.steps.push(EditOp::Insert { b_idx: curr_j - 1 });
                    let src = (tb >> 4) & 0x03;
                    curr_state = match src {
                        0 => DpState::GapA,
                        1 => DpState::Match,
                        _ => DpState::GapB,
                    };
                    curr_j -= 1;
                }
            }
        }

        self.steps.reverse();
        &self.steps
    }
}

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

impl<'a> std::iter::FusedIterator for AffineAlignmentSteps<'a> {}

/// Computes Gotoh affine gap alignment over generic sequences.
pub fn align_affine<T: PartialEq>(a: &[T], b: &[T], penalties: &AffineGapPenalties) -> Vec<EditOp> {
    let mut ws = AffineGapWorkspace::new();
    ws.align(a, b, penalties).to_vec()
}

/// Extension trait providing Gotoh sequence alignment over generic slices.
pub trait DiffuseAffineExt<T> {
    fn align_affine(&self, other: &[T], penalties: &AffineGapPenalties) -> Vec<EditOp>;
}

impl<T: PartialEq> DiffuseAffineExt<T> for [T] {
    fn align_affine(&self, other: &[T], penalties: &AffineGapPenalties) -> Vec<EditOp> {
        align_affine(self, other, penalties)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_affine_empty_sequences() {
        let mut ws = AffineGapWorkspace::new();
        let penalties = AffineGapPenalties::default();

        let a: Vec<u32> = vec![];
        let b: Vec<u32> = vec![];
        assert_eq!(ws.align(&a, &b, &penalties), &[]);

        let non_empty = vec![1, 2, 3];
        let empty: Vec<u32> = vec![];
        assert_eq!(
            ws.align(&empty, &non_empty, &penalties),
            &[
                EditOp::Insert { b_idx: 0 },
                EditOp::Insert { b_idx: 1 },
                EditOp::Insert { b_idx: 2 },
            ]
        );
        assert_eq!(
            ws.align(&non_empty, &empty, &penalties),
            &[
                EditOp::Delete { a_idx: 0 },
                EditOp::Delete { a_idx: 1 },
                EditOp::Delete { a_idx: 2 },
            ]
        );
    }

    #[test]
    fn test_affine_identical_sequences() {
        let penalties = AffineGapPenalties::default();
        let a = [10, 20, 30];
        let b = vec![10, 20, 30];
        let ops = a.align_affine(&b, &penalties);
        assert_eq!(
            ops,
            vec![
                EditOp::Keep { a_idx: 0, b_idx: 0 },
                EditOp::Keep { a_idx: 1, b_idx: 1 },
                EditOp::Keep { a_idx: 2, b_idx: 2 },
            ]
        );
    }

    #[test]
    fn test_affine_anti_fragmentation_prefers_block_replacement() {
        // Anti-fragmentation test:
        // A: ["get", "_", "foo"]
        // B: ["set", "_", "bar"]
        //
        // In unweighted LCS, the coincidental "_" in the middle is preserved,
        // fragmenting the identifier.
        // With Gotoh's gap opening penalty (5) > match score (2), the affine
        // model mathematically prefers a single cohesive block deletion
        // & insertion.
        let penalties = AffineGapPenalties::new(2, None, 5, 1);
        let a = vec!["get", "_", "foo"];
        let b = vec!["set", "_", "bar"];

        let ops = align_affine(&a, &b, &penalties);

        // Verify that "_" was NOT kept as an isolated common token
        let kept_underscore = ops.iter().any(|op| match op {
            EditOp::Keep { a_idx, b_idx } => a[*a_idx] == "_" && b[*b_idx] == "_",
            _ => false,
        });
        assert!(
            !kept_underscore,
            "Gotoh affine gap model must prevent micro-token fragmentation on coincidental '_'"
        );
    }

    #[test]
    fn test_affine_substitution_when_configured() {
        // When mismatch penalty is explicitly provided and gap open penalty is
        // very high, substitutions along the diagonal are permitted.
        let penalties = AffineGapPenalties::new(2, Some(1), 10, 5);
        let a = vec![1, 2, 3];
        let b = vec![1, 9, 3];

        let ops = align_affine(&a, &b, &penalties);
        assert_eq!(
            ops,
            vec![
                EditOp::Keep { a_idx: 0, b_idx: 0 },
                EditOp::Delete { a_idx: 1 },
                EditOp::Insert { b_idx: 1 },
                EditOp::Keep { a_idx: 2, b_idx: 2 },
            ]
        );
    }

    #[test]
    fn test_affine_zero_allocation_workspace_benchmark() {
        let mut ws = AffineGapWorkspace::new();
        let penalties = AffineGapPenalties::default();

        let a: Vec<u32> = (0..200).collect();
        let mut b: Vec<u32> = (0..200).collect();
        // Introduce small mutation
        b[50] = 999;
        b.remove(100);
        b.insert(150, 888);

        // Run 100 iterations using the same recycled workspace
        for _ in 0..100 {
            let steps = ws.align(&a, &b, &penalties);
            assert!(!steps.is_empty());
        }
    }

    #[test]
    fn test_affine_alignment_steps_iterator() {
        let steps = vec![
            EditOp::Keep { a_idx: 0, b_idx: 0 },
            EditOp::Delete { a_idx: 1 },
            EditOp::Insert { b_idx: 1 },
        ];

        let mut iter = AffineAlignmentSteps::new(&steps);
        assert_eq!(iter.len(), 3);
        assert_eq!(iter.next(), Some(EditOp::Keep { a_idx: 0, b_idx: 0 }));
        assert_eq!(iter.next_back(), Some(EditOp::Insert { b_idx: 1 }));
        assert_eq!(iter.len(), 1);
        assert_eq!(iter.next(), Some(EditOp::Delete { a_idx: 1 }));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next_back(), None);
    }

    #[test]
    fn test_affine_align_steps_workspace() {
        let mut ws = AffineGapWorkspace::new();
        let penalties = AffineGapPenalties::default();
        let a = vec![1, 2];
        let b = vec![1, 2];
        let steps_iter = ws.align_steps(&a, &b, &penalties);
        let collected: Vec<EditOp> = steps_iter.collect();
        assert_eq!(
            collected,
            vec![
                EditOp::Keep { a_idx: 0, b_idx: 0 },
                EditOp::Keep { a_idx: 1, b_idx: 1 },
            ]
        );
    }
}
