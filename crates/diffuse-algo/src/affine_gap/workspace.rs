use super::{AffineAlignmentSteps, AffineGapPenalties};
use crate::EditOp;

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
