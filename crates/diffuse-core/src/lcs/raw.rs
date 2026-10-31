//! Raw dynamic programming token alignment and Gotoh 3-state affine gap
//! scoring.

use rustc_hash::FxHashMap;
use std::cell::RefCell;

use super::{
    arena::{LcsScratchArena, Weights},
    tokenizer::{
        are_contiguous_subwords, char_len, is_subword_token, token_is_alnum, token_is_ws,
        token_ws_kind, tokens_can_match,
    },
};

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
        // preventing isolated punctuation matches across disjoint semantic units.
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
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return FxHashMap::default();
    }

    let stride = m + 1;
    let idx = |i: usize, j: usize| i * stride + j;
    let total_cells = (n + 1) * (m + 1);

    arena.prepare(total_cells, n, m);
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
    let depth_a = &arena.depth_a[..n];
    let depth_b = &arena.depth_b[..m];

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
    let container_a = &arena.container_a[..n];
    let container_b = &arena.container_b[..m];

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
            if tokens_can_match(a, b, prev_a, prev_b, next_a, next_b, i, j) {
                let ca = crate::token::ContainerKind::from_id(container_a[i]);
                let cb = crate::token::ContainerKind::from_id(container_b[j]);
                if !crate::token::ContainerKind::is_compatible_with(ca, cb) {
                    continue;
                }

                if is_subword_token(a[i]) {
                    let la = i > 0
                        && is_subword_token(a[i - 1])
                        && are_contiguous_subwords(a[i - 1], a[i]);
                    let ra = i + 1 < n
                        && is_subword_token(a[i + 1])
                        && are_contiguous_subwords(a[i], a[i + 1]);
                    let lb = j > 0
                        && is_subword_token(b[j - 1])
                        && are_contiguous_subwords(b[j - 1], b[j]);
                    let rb = j + 1 < m
                        && is_subword_token(b[j + 1])
                        && are_contiguous_subwords(b[j], b[j + 1]);
                    if (la != lb || ra != rb) && (depth_a[i] != depth_b[j] || ca != cb) {
                        continue;
                    }
                }
                let is_ws = token_is_ws(a[i]);
                let depth_matched = depth_a[i] == depth_b[j];
                let next_rl = if is_ws { 0 } else { run_len[idx(i + 1, j + 1)] };
                let is_subword_pair = i + 1 < n
                    && j + 1 < m
                    && are_contiguous_subwords(a[i], a[i + 1])
                    && are_contiguous_subwords(b[j], b[j + 1]);
                let subword_bonus = if is_subword_pair {
                    weights.subword_bonus
                } else {
                    0
                };
                let same_container =
                    depth_matched && (i + 1 == n || j + 1 == m || depth_a[i + 1] == depth_b[j + 1]);
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
                    token_ws_kind(a, prev_a, next_a, i).match_reward()
                } else {
                    token_match_reward(a[i])
                };
                let depth_diff = (depth_a[i] as i32 - depth_b[j] as i32).abs();
                let depth_adj = if depth_diff == 0 {
                    weights.depth_match_bonus
                } else {
                    -depth_diff * weights.depth_mismatch_penalty
                };
                let boundary_adj = if is_subword_token(a[i]) {
                    let la = i > 0
                        && is_subword_token(a[i - 1])
                        && are_contiguous_subwords(a[i - 1], a[i]);
                    let ra = i + 1 < n
                        && is_subword_token(a[i + 1])
                        && are_contiguous_subwords(a[i], a[i + 1]);
                    let lb = j > 0
                        && is_subword_token(b[j - 1])
                        && are_contiguous_subwords(b[j - 1], b[j]);
                    let rb = j + 1 < m
                        && is_subword_token(b[j + 1])
                        && are_contiguous_subwords(b[j], b[j + 1]);
                    if la == lb && ra == rb {
                        weights.boundary_match_bonus
                    } else {
                        -weights.boundary_mismatch_penalty
                    }
                } else {
                    0
                };
                let r = (base_r + depth_adj + boundary_adj).max(0);
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
