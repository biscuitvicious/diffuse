//! Generic 2D Multi-Objective Block Transformation Lattice Aligner.
//!
//! Academic foundation:
//! - Richard Bellman (1957). "Dynamic Programming". Princeton University Press.
//! - Robert A. Wagner and Michael J. Fischer (1974). "The String-to-String
//!   Correction Problem". Journal of the ACM, 21(1), 168–173.
//!
//! # Strict Architectural Separation
//! Solves a 2D dynamic programming grid over generic items `ItemA` and `ItemB`
//! parameterized by a [`BlockTransitionEvaluator`].
//! Zero diffuse-domain dependencies (no ASTs, rows, colors, or hunks).

/// One transition step in the 2D block transformation lattice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockTransition {
    /// 1-to-1 in-place modification.
    Mod,
    /// Verbatim item relocation.
    Mov,
    /// 1-to-K expansion where 1 item in sequence A expands across K items in
    /// sequence B.
    Expand { k: usize, anchor_offset: usize },
    /// K-to-1 collapse where K items in sequence A fold into 1 item in sequence
    /// B.
    Collapse { k: usize, anchor_offset: usize },
    /// Standalone deletion of an item in sequence A.
    Del,
    /// Standalone addition of an item in sequence B.
    Add,
}

/// Evaluator trait defining objective scores for transitions in the 2D lattice.
pub trait BlockTransitionEvaluator {
    type ItemA;
    type ItemB;

    /// Evaluates 1-to-1 modification between item A and item B.
    /// Returns `None` if the pairing is inadmissible.
    fn score_mod(&mut self, a: &Self::ItemA, b: &Self::ItemB) -> Option<f64>;

    /// Evaluates verbatim relocation between item A and item B.
    /// Returns `None` if the items do not match verbatim.
    fn score_mov(&mut self, a: &Self::ItemA, b: &Self::ItemB) -> Option<f64>;

    /// Evaluates 1-to-K expansion where item `a` is distributed across
    /// `b_slice`. Returns `Some((score, anchor_offset))` if valid, where
    /// `anchor_offset` designates which item in `b_slice` serves as the
    /// primary anchor.
    fn score_expand(&mut self, a: &Self::ItemA, b_slice: &[Self::ItemB]) -> Option<(f64, usize)>;

    /// Evaluates K-to-1 collapse where `a_slice` is folded into item `b`.
    /// Returns `Some((score, anchor_offset))` if valid, where `anchor_offset`
    /// designates which item in `a_slice` serves as the primary anchor.
    fn score_collapse(&mut self, a_slice: &[Self::ItemA], b: &Self::ItemB) -> Option<(f64, usize)>;

    /// Penalty/score for deleting item `a`.
    fn score_del(&mut self, a: &Self::ItemA) -> f64;

    /// Penalty/score for adding item `b`.
    fn score_add(&mut self, b: &Self::ItemB) -> f64;

    /// Maximum block window size $K$ for expansions and collapses.
    #[inline]
    fn max_block_k(&self) -> usize {
        10
    }
}

#[derive(Clone, Copy, Debug)]
struct DpCell {
    score: f64,
    prev_i: u32,
    prev_j: u32,
    transition: BlockTransition,
}

/// Reusable workspace holding DP grid cells and traceback buffers.
#[derive(Clone, Debug, Default)]
pub struct BlockDpWorkspace {
    cells: Vec<DpCell>,
    transitions: Vec<BlockTransition>,
}

impl BlockDpWorkspace {
    /// Creates a new empty workspace.
    pub fn new() -> Self {
        Self::default()
    }

    /// Solves the 2D transformation lattice and returns `(optimal_score,
    /// transitions_slice)`.
    pub fn solve<E: BlockTransitionEvaluator>(
        &mut self,
        a: &[E::ItemA],
        b: &[E::ItemB],
        evaluator: &mut E,
    ) -> (f64, &[BlockTransition]) {
        let m = a.len();
        let n = b.len();

        if m == 0 && n == 0 {
            self.transitions.clear();
            return (0.0, &self.transitions);
        }

        let stride = n + 1;
        let total_cells = (m + 1) * stride;
        let neg = f64::NEG_INFINITY;

        self.cells.clear();
        self.cells.resize(
            total_cells,
            DpCell {
                score: neg,
                prev_i: 0,
                prev_j: 0,
                transition: BlockTransition::Mod,
            },
        );

        self.cells[0].score = 0.0;
        let max_k = evaluator.max_block_k().max(2);

        for i in 0..=m {
            let row_curr = i * stride;
            let row_next = (i + 1) * stride;

            let has_old = i < m;

            for j in 0..=n {
                let curr_idx = row_curr + j;
                let base = self.cells[curr_idx].score;
                if base == neg {
                    continue;
                }

                // 1. Standalone Del
                if has_old {
                    let del_score = base + evaluator.score_del(&a[i]);
                    let next_i = row_next + j;
                    if del_score > self.cells[next_i].score {
                        self.cells[next_i] = DpCell {
                            score: del_score,
                            prev_i: i as u32,
                            prev_j: j as u32,
                            transition: BlockTransition::Del,
                        };
                    }
                }

                // 2. Standalone Add
                if j < n {
                    let add_score = base + evaluator.score_add(&b[j]);
                    let next_j = curr_idx + 1;
                    // On tie, prefer Add so Dels precede Adds
                    if add_score >= self.cells[next_j].score {
                        self.cells[next_j] = DpCell {
                            score: add_score,
                            prev_i: i as u32,
                            prev_j: j as u32,
                            transition: BlockTransition::Add,
                        };
                    }
                }

                // 3. 1-to-1: Mod and Mov
                if has_old && j < n {
                    let next_diag = row_next + (j + 1);

                    if let Some(mov_score) = evaluator.score_mov(&a[i], &b[j]) {
                        let s = base + mov_score;
                        if s > self.cells[next_diag].score {
                            self.cells[next_diag] = DpCell {
                                score: s,
                                prev_i: i as u32,
                                prev_j: j as u32,
                                transition: BlockTransition::Mov,
                            };
                        }
                    }

                    if let Some(mod_score) = evaluator.score_mod(&a[i], &b[j]) {
                        let s = base + mod_score;
                        if s > self.cells[next_diag].score {
                            self.cells[next_diag] = DpCell {
                                score: s,
                                prev_i: i as u32,
                                prev_j: j as u32,
                                transition: BlockTransition::Mod,
                            };
                        }
                    }
                }

                // 4. 1-to-K Expansion
                if has_old {
                    for k in 2..=max_k {
                        if j + k <= n
                            && let Some((exp_score, anchor)) =
                                evaluator.score_expand(&a[i], &b[j..j + k])
                            {
                                let s = base + exp_score;
                                let target = row_next + (j + k);
                                if s > self.cells[target].score {
                                    self.cells[target] = DpCell {
                                        score: s,
                                        prev_i: i as u32,
                                        prev_j: j as u32,
                                        transition: BlockTransition::Expand {
                                            k,
                                            anchor_offset: anchor,
                                        },
                                    };
                                }
                            }
                    }
                }

                // 5. K-to-1 Collapse
                if j < n {
                    for k in 2..=max_k {
                        if i + k <= m
                            && let Some((col_score, anchor)) =
                                evaluator.score_collapse(&a[i..i + k], &b[j])
                            {
                                let s = base + col_score;
                                let target = (i + k) * stride + (j + 1);
                                if s > self.cells[target].score {
                                    self.cells[target] = DpCell {
                                        score: s,
                                        prev_i: i as u32,
                                        prev_j: j as u32,
                                        transition: BlockTransition::Collapse {
                                            k,
                                            anchor_offset: anchor,
                                        },
                                    };
                                }
                            }
                    }
                }
            }
        }

        // Traceback from (m, n)
        self.transitions.clear();
        let end_idx = m * stride + n;
        let final_score = self.cells[end_idx].score;

        let mut curr_i = m as u32;
        let mut curr_j = n as u32;

        while curr_i > 0 || curr_j > 0 {
            let idx = (curr_i as usize) * stride + (curr_j as usize);
            let cell = self.cells[idx];
            self.transitions.push(cell.transition);
            curr_i = cell.prev_i;
            curr_j = cell.prev_j;
        }

        self.transitions.reverse();
        (final_score, &self.transitions)
    }

    /// Solves the lattice and yields an owned streaming iterator over
    /// transitions.
    pub fn solve_iter<E: BlockTransitionEvaluator>(
        &mut self,
        a: &[E::ItemA],
        b: &[E::ItemB],
        evaluator: &mut E,
    ) -> (f64, BlockTransitionsIter) {
        let (score, transitions) = self.solve(a, b, evaluator);
        (score, BlockTransitionsIter::new(transitions.to_vec()))
    }
}

/// Streaming iterator over block transitions.
#[derive(Clone, Debug)]
pub struct BlockTransitionsIter {
    transitions: Vec<BlockTransition>,
    cursor: usize,
}

impl BlockTransitionsIter {
    pub fn new(transitions: Vec<BlockTransition>) -> Self {
        Self {
            transitions,
            cursor: 0,
        }
    }
}

impl Iterator for BlockTransitionsIter {
    type Item = BlockTransition;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor < self.transitions.len() {
            let item = self.transitions[self.cursor];
            self.cursor += 1;
            Some(item)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.transitions.len() - self.cursor;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for BlockTransitionsIter {}
impl std::iter::FusedIterator for BlockTransitionsIter {}

#[cfg(test)]
mod tests {
    use super::*;

    struct SimpleEvaluator;

    impl BlockTransitionEvaluator for SimpleEvaluator {
        type ItemA = &'static str;
        type ItemB = &'static str;

        fn score_mod(&mut self, a: &Self::ItemA, b: &Self::ItemB) -> Option<f64> {
            if a == b {
                Some(10.0) // Exact match via Mod
            } else {
                Some(5.0) // Inexact substitution
            }
        }

        fn score_mov(&mut self, a: &Self::ItemA, b: &Self::ItemB) -> Option<f64> {
            if a == b {
                Some(12.0) // Verbatim Mov preferred over Mod
            } else {
                None
            }
        }

        fn score_expand(
            &mut self,
            a: &Self::ItemA,
            b_slice: &[Self::ItemB],
        ) -> Option<(f64, usize)> {
            // Expansion if b_slice concatenated equals a
            let combined = b_slice.join("");
            if *a == combined {
                Some((20.0, 0))
            } else {
                None
            }
        }

        fn score_collapse(
            &mut self,
            a_slice: &[Self::ItemA],
            b: &Self::ItemB,
        ) -> Option<(f64, usize)> {
            // Collapse if a_slice concatenated equals b
            let combined = a_slice.join("");
            if combined == *b {
                Some((20.0, 0))
            } else {
                None
            }
        }

        fn score_del(&mut self, _a: &Self::ItemA) -> f64 {
            -2.0
        }

        fn score_add(&mut self, _b: &Self::ItemB) -> f64 {
            -2.0
        }
    }

    #[test]
    fn test_block_dp_empty_sequences() {
        let mut ws = BlockDpWorkspace::new();
        let mut eval = SimpleEvaluator;
        let (score, transitions) = ws.solve(&[], &[], &mut eval);
        assert_eq!(score, 0.0);
        assert_eq!(transitions, &[]);
    }

    #[test]
    fn test_block_dp_verbatim_mov_preference() {
        let mut ws = BlockDpWorkspace::new();
        let mut eval = SimpleEvaluator;
        let a = ["foo", "bar"];
        let b = ["foo", "bar"];
        let (score, transitions) = ws.solve(&a, &b, &mut eval);
        assert_eq!(score, 24.0); // 12.0 * 2
        assert_eq!(transitions, &[BlockTransition::Mov, BlockTransition::Mov]);
    }

    #[test]
    fn test_block_dp_expand_transition() {
        let mut ws = BlockDpWorkspace::new();
        let mut eval = SimpleEvaluator;
        // "foobar" expands into "foo" and "bar"
        let a = ["foobar"];
        let b = ["foo", "bar"];
        let (score, transitions) = ws.solve(&a, &b, &mut eval);
        assert_eq!(score, 20.0);
        assert_eq!(
            transitions,
            &[BlockTransition::Expand {
                k: 2,
                anchor_offset: 0
            }]
        );
    }

    #[test]
    fn test_block_dp_collapse_transition() {
        let mut ws = BlockDpWorkspace::new();
        let mut eval = SimpleEvaluator;
        // "foo" and "bar" collapse into "foobar"
        let a = ["foo", "bar"];
        let b = ["foobar"];
        let (score, transitions) = ws.solve(&a, &b, &mut eval);
        assert_eq!(score, 20.0);
        assert_eq!(
            transitions,
            &[BlockTransition::Collapse {
                k: 2,
                anchor_offset: 0
            }]
        );
    }

    #[test]
    fn test_block_dp_pure_del_and_add() {
        struct NoMatchEvaluator;
        impl BlockTransitionEvaluator for NoMatchEvaluator {
            type ItemA = u32;
            type ItemB = u32;

            fn score_mod(&mut self, _a: &u32, _b: &u32) -> Option<f64> {
                None
            }
            fn score_mov(&mut self, _a: &u32, _b: &u32) -> Option<f64> {
                None
            }
            fn score_expand(&mut self, _a: &u32, _b: &[u32]) -> Option<(f64, usize)> {
                None
            }
            fn score_collapse(&mut self, _a: &[u32], _b: &u32) -> Option<(f64, usize)> {
                None
            }
            fn score_del(&mut self, _a: &u32) -> f64 {
                -5.0
            }
            fn score_add(&mut self, _b: &u32) -> f64 {
                -3.0
            }
        }

        let mut ws = BlockDpWorkspace::new();
        let mut eval = NoMatchEvaluator;
        let a = [1];
        let b = [2];
        let (score, transitions) = ws.solve(&a, &b, &mut eval);
        assert_eq!(score, -8.0);
        assert_eq!(transitions, &[BlockTransition::Del, BlockTransition::Add]);
    }

    #[test]
    fn test_block_transitions_iter() {
        let transitions = vec![BlockTransition::Del, BlockTransition::Add];
        let mut iter = BlockTransitionsIter::new(transitions);
        assert_eq!(iter.len(), 2);
        assert_eq!(iter.next(), Some(BlockTransition::Del));
        assert_eq!(iter.len(), 1);
        assert_eq!(iter.next(), Some(BlockTransition::Add));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_block_dp_solve_iter() {
        let mut ws = BlockDpWorkspace::new();
        let mut eval = SimpleEvaluator;
        let a = ["foo"];
        let b = ["foo"];
        let (score, iter) = ws.solve_iter(&a, &b, &mut eval);
        assert_eq!(score, 12.0);
        let transitions: Vec<BlockTransition> = iter.collect();
        assert_eq!(transitions, vec![BlockTransition::Mov]);
    }
}
