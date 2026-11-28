//! Raw dynamic programming token alignment and Gotoh 3-state affine gap
//! scoring.

use std::{cell::RefCell, ops::Range};

use rustc_hash::FxHashMap;

use super::{
    arena::{LcsScratchArena, Weights},
    pairs::subword_neighbor_flags,
    tokenizer::{
        are_contiguous_subwords, char_len, is_subword_token, token_is_alnum, token_is_ws,
        token_ws_kind, tokens_can_match,
    },
};

/// Precomputed per-side 2D token stream, line/slot coordinates, and Dyck
/// container state for slot-aware Gotoh LCS.
pub(crate) struct Side2D<'a> {
    pub(crate) tokens: &'a [&'a str],
    pub(crate) lines: &'a [usize],
    pub(crate) line_ranges: &'a [Range<usize>],
    pub(crate) slot_of_line: &'a [usize],
    pub(crate) depths: &'a [u16],
    pub(crate) containers: &'a [u8],
}

/// Slot-aware scoring context for one anchor-bounded interval
/// `(offset_a.., offset_b..)` inside a 2D multi-line alignment.
pub(crate) struct SlotAffinity<'a> {
    pub(crate) old: &'a Side2D<'a>,
    pub(crate) new: &'a Side2D<'a>,
    pub(crate) offset_a: usize,
    pub(crate) offset_b: usize,
    pub(crate) max_slot_distance: usize,
    pub(crate) same_slot_alnum_bonus: i32,
    pub(crate) same_slot_punct_bonus: i32,
}

impl<'a> SlotAffinity<'a> {
    #[inline]
    fn slot_pair(&self, i: usize, j: usize) -> (usize, usize) {
        let la = self.old.lines[self.offset_a + i];
        let lb = self.new.lines[self.offset_b + j];
        (self.old.slot_of_line[la], self.new.slot_of_line[lb])
    }

    /// Whether `(i, j)` is a strictly interior whitespace token sandwiched
    /// between identical non-whitespace tokens on both its left and right
    /// within the same `(old_line, new_line)` pair, with at least one
    /// alphanumeric neighbor (so bare punctuation clusters such as `- [ ]` or
    /// `) => {` do not gain cross-slot bridging power).
    #[inline]
    fn is_interior_ws_bridge(&self, i: usize, j: usize) -> bool {
        let gi = self.offset_a + i;
        let gj = self.offset_b + j;
        let ra = &self.old.line_ranges[self.old.lines[gi]];
        let rb = &self.new.line_ranges[self.new.lines[gj]];
        if !(gi > ra.start && gi + 1 < ra.end && gj > rb.start && gj + 1 < rb.end) {
            return false;
        }
        let prev_a = self.old.tokens[gi - 1];
        let next_a = self.old.tokens[gi + 1];
        !token_is_ws(prev_a)
            && !token_is_ws(next_a)
            && (token_is_alnum(prev_a) || token_is_alnum(next_a))
            && prev_a == self.new.tokens[gj - 1]
            && next_a == self.new.tokens[gj + 1]
    }

    #[inline]
    fn pair_permitted(&self, i: usize, j: usize, is_ws: bool) -> bool {
        let (sa, sb) = self.slot_pair(i, j);
        if sa.abs_diff(sb) > self.max_slot_distance {
            return false;
        }
        sa == sb || !is_ws || self.is_interior_ws_bridge(i, j)
    }

    #[inline]
    fn tokens_can_match(&self, i: usize, j: usize) -> bool {
        let gi = self.offset_a + i;
        let gj = self.offset_b + j;
        let ra = &self.old.line_ranges[self.old.lines[gi]];
        let rb = &self.new.line_ranges[self.new.lines[gj]];
        tokens_can_match(
            &self.old.tokens[ra.clone()],
            &self.new.tokens[rb.clone()],
            &[],
            &[],
            None,
            None,
            gi - ra.start,
            gj - rb.start,
        )
    }

    #[inline]
    fn same_slot_bonus(&self, i: usize, j: usize, is_ws: bool, token: &str) -> i32 {
        let (sa, sb) = self.slot_pair(i, j);
        if sa != sb || is_ws {
            return 0;
        }
        if token_is_alnum(token) {
            self.same_slot_alnum_bonus
        } else {
            self.same_slot_punct_bonus
        }
    }

    #[inline]
    fn same_line_neighbors(
        &self,
        i: usize,
        j: usize,
        n: usize,
        m: usize,
    ) -> (bool, bool, bool, bool) {
        let gi = self.offset_a + i;
        let gj = self.offset_b + j;
        let la = self.old.lines[gi];
        let lb = self.new.lines[gj];
        (
            i > 0 && self.old.lines[gi - 1] == la,
            i + 1 < n && self.old.lines[gi + 1] == la,
            j > 0 && self.new.lines[gj - 1] == lb,
            j + 1 < m && self.new.lines[gj + 1] == lb,
        )
    }

    #[inline]
    fn ws_match_reward(&self, i: usize) -> i32 {
        let gi = self.offset_a + i;
        let range = &self.old.line_ranges[self.old.lines[gi]];
        token_ws_kind(&self.old.tokens[range.clone()], &[], None, gi - range.start).match_reward()
    }

    #[inline]
    fn depths_and_containers(
        &self,
        n: usize,
        m: usize,
    ) -> (&'a [u16], &'a [u16], &'a [u8], &'a [u8]) {
        (
            &self.old.depths[self.offset_a..self.offset_a + n],
            &self.new.depths[self.offset_b..self.offset_b + m],
            &self.old.containers[self.offset_a..self.offset_a + n],
            &self.new.containers[self.offset_b..self.offset_b + m],
        )
    }
}

thread_local! {
    static SCRATCH_ARENA: RefCell<LcsScratchArena> = RefCell::new(LcsScratchArena::default());
}

/// Core LCS backtrace over string slices.
pub fn raw_lcs_pairs(a: &[&str], b: &[&str]) -> FxHashMap<usize, usize> {
    raw_lcs_pairs_with_context(a, b, &[], &[], None, None)
}

/// Core LCS backtrace with optional boundary lookahead tokens from parent line.
pub fn raw_lcs_pairs_with_lookahead(
    a: &[&str],
    b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
) -> FxHashMap<usize, usize> {
    raw_lcs_pairs_with_context(a, b, &[], &[], next_a, next_b)
}

/// Whether `t` is a paired syntactic container delimiter.
#[inline]
pub fn is_container_delim(t: &str) -> bool {
    matches!(
        t,
        "(" | ")" | "[" | "]" | "{" | "}" | "<" | ">" | "`" | "\"" | "'"
    )
}

/// Token match reward for Gotoh affine gap sequence alignment.
#[inline]
fn token_match_reward(t: &str) -> i32 {
    if token_is_ws(t) {
        2
    } else if is_container_delim(t) {
        20
    } else if token_is_alnum(t) {
        let len = char_len(t) as i32;
        if len <= 2 { 10 } else { 14 + len.min(10) * 2 }
    } else if char_len(t) <= 1 {
        // Low-entropy isolated punctuation and single-character operators
        // are weighted strictly below affine gap opening + extension (20),
        // preventing isolated punctuation matches across disjoint semantic
        // units.
        4
    } else {
        10
    }
}

/// Core sequence alignment backtrace with Gotoh 3-state affine gap dynamic
/// programming using a reusable scratch arena and default weights.
pub fn raw_lcs_pairs_with_arena(
    arena: &mut LcsScratchArena,
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
) -> FxHashMap<usize, usize> {
    raw_lcs_pairs_with_arena_and_weights(
        arena,
        a,
        b,
        prev_a,
        prev_b,
        next_a,
        next_b,
        &Weights::DEFAULT,
    )
}

/// Core sequence alignment backtrace with Gotoh 3-state affine gap dynamic
/// programming using a reusable scratch arena and custom weights.
#[allow(clippy::too_many_arguments)]
pub fn raw_lcs_pairs_with_arena_and_weights(
    arena: &mut LcsScratchArena,
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
    weights: &Weights,
) -> FxHashMap<usize, usize> {
    raw_lcs_dp(arena, a, b, prev_a, prev_b, next_a, next_b, weights, None)
}

/// 2D slot-aware Gotoh alignment over an interval `(a, b)` using thread-local
/// scratch memory, enforcing cross-slot whitespace prohibition, slot distance
/// bounds, line-bounded subword contiguity, and same-slot affinity.
pub(crate) fn raw_lcs_pairs_with_slots(
    a: &[&str],
    b: &[&str],
    slots: &SlotAffinity<'_>,
) -> FxHashMap<usize, usize> {
    SCRATCH_ARENA.with_borrow_mut(|arena| {
        raw_lcs_dp(
            arena,
            a,
            b,
            &[],
            &[],
            None,
            None,
            &Weights::DEFAULT,
            Some(slots),
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn raw_lcs_dp(
    arena: &mut LcsScratchArena,
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
    weights: &Weights,
    slots: Option<&SlotAffinity<'_>>,
) -> FxHashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return FxHashMap::default();
    }

    let stride = m + 1;
    let idx = |i: usize, j: usize| i * stride + j;
    let total_cells = (n + 1) * (m + 1);

    arena.prepare(total_cells, n, m);
    let (depth_a, depth_b, container_a, container_b) = if let Some(s) = slots {
        s.depths_and_containers(n, m)
    } else {
        diffuse_algo::track_nesting_depths_into(
            prev_a,
            a,
            &diffuse_algo::UniversalDelimiterClassifier,
            &mut arena.depth_a[..n],
        );
        diffuse_algo::track_nesting_depths_into(
            prev_b,
            b,
            &diffuse_algo::UniversalDelimiterClassifier,
            &mut arena.depth_b[..m],
        );
        diffuse_algo::track_enclosing_containers_into(
            prev_a,
            a,
            &diffuse_algo::UniversalDelimiterClassifier,
            &mut arena.container_a[..n],
        );
        diffuse_algo::track_enclosing_containers_into(
            prev_b,
            b,
            &diffuse_algo::UniversalDelimiterClassifier,
            &mut arena.container_b[..m],
        );
        (
            &arena.depth_a[..n],
            &arena.depth_b[..m],
            &arena.container_a[..n],
            &arena.container_b[..m],
        )
    };

    let m_matrix = &mut arena.m_matrix[..total_cells];
    let x_matrix = &mut arena.x_matrix[..total_cells];
    let y_matrix = &mut arena.y_matrix[..total_cells];
    let run_len = &mut arena.run_len[..total_cells];
    let tb_m = &mut arena.tb_m[..total_cells];
    let tb_x = &mut arena.tb_x[..total_cells];
    let tb_y = &mut arena.tb_y[..total_cells];

    // Backward Gotoh DP over suffixes: cell (i, j) aligns a[i..n] and b[j..m].
    m_matrix[idx(n, m)] = 0;
    for i in (0..n).rev() {
        x_matrix[idx(i, m)] = -weights.gap_open - ((n - i) as i32) * weights.gap_extend;
        tb_x[idx(i, m)] = 1;
    }
    for j in (0..m).rev() {
        y_matrix[idx(n, j)] = -weights.gap_open - ((m - j) as i32) * weights.gap_extend;
        tb_y[idx(n, j)] = 2;
    }

    for i in (0..n).rev() {
        for j in (0..m).rev() {
            // State X: deletion of a[i]
            let cx = x_matrix[idx(i + 1, j)].saturating_sub(weights.gap_extend);
            let cm_x =
                m_matrix[idx(i + 1, j)].saturating_sub(weights.gap_open + weights.gap_extend);
            let cy = y_matrix[idx(i + 1, j)].saturating_sub(weights.gap_extend);
            let (best_x, to_x) = if cx >= cm_x && cx >= cy {
                (cx, 1)
            } else if cy >= cm_x {
                (cy, 2)
            } else {
                (cm_x, 0)
            };
            x_matrix[idx(i, j)] = best_x;
            tb_x[idx(i, j)] = to_x;

            // State Y: insertion of b[j]
            let cy = y_matrix[idx(i, j + 1)].saturating_sub(weights.gap_extend);
            let cm_y =
                m_matrix[idx(i, j + 1)].saturating_sub(weights.gap_open + weights.gap_extend);
            let cx = x_matrix[idx(i, j + 1)].saturating_sub(weights.gap_extend);
            let (best_y, to_y) = if cy >= cm_y && cy >= cx {
                (cy, 2)
            } else if cx >= cm_y {
                (cx, 1)
            } else {
                (cm_y, 0)
            };
            y_matrix[idx(i, j)] = best_y;
            tb_y[idx(i, j)] = to_y;

            // State M: match a[i] with b[j]
            let is_ws = token_is_ws(a[i]);
            let (
                can_match,
                same_slot_bonus,
                same_line_prev_a,
                same_line_next_a,
                same_line_prev_b,
                same_line_next_b,
            ) = if let Some(s) = slots {
                if !s.pair_permitted(i, j, is_ws) {
                    continue;
                }
                let (prev_a_ok, next_a_ok, prev_b_ok, next_b_ok) =
                    s.same_line_neighbors(i, j, n, m);
                (
                    s.tokens_can_match(i, j),
                    s.same_slot_bonus(i, j, is_ws, a[i]),
                    prev_a_ok,
                    next_a_ok,
                    prev_b_ok,
                    next_b_ok,
                )
            } else {
                (
                    tokens_can_match(a, b, prev_a, prev_b, next_a, next_b, i, j),
                    0,
                    i > 0,
                    i + 1 < n,
                    j > 0,
                    j + 1 < m,
                )
            };

            if can_match {
                let ca = crate::token::ContainerKind::from_id(container_a[i]);
                let cb = crate::token::ContainerKind::from_id(container_b[j]);
                if !crate::token::ContainerKind::is_compatible_with(ca, cb) {
                    continue;
                }

                let is_subword = is_subword_token(a[i]);
                let (la, ra, lb, rb) = if is_subword {
                    let (la, ra) = subword_neighbor_flags(a, i, same_line_prev_a, same_line_next_a);
                    let (lb, rb) = subword_neighbor_flags(b, j, same_line_prev_b, same_line_next_b);
                    (la, ra, lb, rb)
                } else {
                    (false, false, false, false)
                };

                if is_subword && (la != lb || ra != rb) && (depth_a[i] != depth_b[j] || ca != cb) {
                    continue;
                }

                let depth_matched = depth_a[i] == depth_b[j];
                let next_rl = if is_ws { 0 } else { run_len[idx(i + 1, j + 1)] };
                let is_subword_pair = same_line_next_a
                    && same_line_next_b
                    && are_contiguous_subwords(a[i], a[i + 1])
                    && are_contiguous_subwords(b[j], b[j + 1]);
                let subword_bonus = if is_subword_pair {
                    weights.subword_bonus
                } else {
                    0
                };
                let same_line_continuation =
                    slots.is_none() || (same_line_next_a && same_line_next_b);
                let same_container = depth_matched
                    && same_line_continuation
                    && (i + 1 == n || j + 1 == m || depth_a[i + 1] == depth_b[j + 1]);
                let run_bonus = if is_ws || !same_container {
                    0
                } else {
                    weights.contig_bonus + (next_rl.min(8) as i32) * 2 + subword_bonus
                };
                let next_m_val = m_matrix[idx(i + 1, j + 1)].saturating_add(run_bonus);
                let next_x = x_matrix[idx(i + 1, j + 1)];
                let next_y = y_matrix[idx(i + 1, j + 1)];
                let (best_next, to_m, rl) = if next_m_val >= next_x && next_m_val >= next_y {
                    (next_m_val, 0, if same_container { next_rl + 1 } else { 1 })
                } else if next_x >= next_y {
                    (next_x, 1, 1)
                } else {
                    (next_y, 2, 1)
                };
                let base_r = if is_ws {
                    if let Some(s) = slots {
                        s.ws_match_reward(i)
                    } else {
                        token_ws_kind(a, prev_a, next_a, i).match_reward()
                    }
                } else {
                    token_match_reward(a[i])
                };
                let depth_diff = (depth_a[i] as i32 - depth_b[j] as i32).abs();
                let depth_adj = if depth_diff == 0 {
                    weights.depth_match_bonus
                } else {
                    -depth_diff * weights.depth_mismatch_penalty
                };
                let boundary_adj = if is_subword {
                    if la == lb && ra == rb {
                        weights.boundary_match_bonus
                    } else {
                        -weights.boundary_mismatch_penalty
                    }
                } else {
                    0
                };
                let r = (base_r + depth_adj + boundary_adj + same_slot_bonus).max(0);
                m_matrix[idx(i, j)] = r.saturating_add(best_next);
                tb_m[idx(i, j)] = to_m;
                run_len[idx(i, j)] = if is_ws { 0 } else { rl };
            }
        }
    }

    let start_m = m_matrix[idx(0, 0)];
    let start_x = x_matrix[idx(0, 0)];
    let start_y = y_matrix[idx(0, 0)];
    let mut cur_state = if start_m >= start_x && start_m >= start_y {
        0
    } else if start_x >= start_y {
        1
    } else {
        2
    };

    let mut i = 0;
    let mut j = 0;
    let mut pairs = FxHashMap::default();

    while i < n || j < m {
        match cur_state {
            0 => {
                if i < n && j < m {
                    pairs.insert(i, j);
                    cur_state = tb_m[idx(i, j)];
                    i += 1;
                    j += 1;
                } else {
                    break;
                }
            }
            1 => {
                if i < n {
                    cur_state = tb_x[idx(i, j)];
                    i += 1;
                } else {
                    break;
                }
            }
            2 => {
                if j < m {
                    cur_state = tb_y[idx(i, j)];
                    j += 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }

    pairs
}

/// Core sequence alignment backtrace with Gotoh 3-state affine gap dynamic
/// programming using thread-local scratch memory.
pub fn raw_lcs_pairs_with_context(
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
) -> FxHashMap<usize, usize> {
    SCRATCH_ARENA.with_borrow_mut(|arena| {
        raw_lcs_pairs_with_arena(arena, a, b, prev_a, prev_b, next_a, next_b)
    })
}
