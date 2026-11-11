//! Cluster Dynamic Programming transformation engine (Phase 3).
//!
//! Each [`ChangeCluster`] — an isolated (M deleted, N added) run bounded by
//! context lines or syntax-section barriers (Phase 2) — is solved as a 2D
//! DP over the line matrix. `DP[i][j]` is the optimal decomposition of old
//! lines `o[0..i]` into new lines `n[0..j]`; the transitions (plan §3.2)
//! are:
//!
//! | transition         | step              | rows emitted            |
//! |--------------------|-------------------|-------------------------|
//! | `Mod`              | `(i+1, j+1)`      | 1 `Mod`                 |
//! | `Mov`              | `(i+1, j+1)`      | 1 `Mov` (verbatim pair) |
//! | `Expand` (1-to-K)  | `(i+1, j+K)`      | 1 `Mod` + K−1 `Add`     |
//! | `Collapse` (K-to-1)| `(i+K, j+1)`      | K−1 `Del` + 1 `Mod`     |
//! | `Del`              | `(i+1, j)`        | 1 `Del`                 |
//! | `Add`              | `(i, j+1)`        | 1 `Add`                 |
//!
//! The objective (plan §3.3) is multi-objective:
//!
//! ```text
//! score = match_reward·matched_chars − mismatch·unmatched_chars
//!       − row_cost·row_count − sprawl·fragmentation + syntax_alignment
//!       + slot_continuity + boundary_proximity
//! ```

pub mod arena;
pub mod block;
pub mod bounds;
pub mod config;
pub mod solver;
pub mod tokens;
pub mod verify;

#[cfg(test)]
pub(crate) use arena::DpRowMeta;
pub use arena::{DpScratchArena, solve_cluster_with_tls, with_tls_arena};
pub use block::{BlockMatch, block_anchor, block_lcs};
#[cfg(test)]
pub(crate) use bounds::{cell_touches_band_edge, is_statement_start, mod_pair_needs_scoring};
pub use config::{
    DEFAULT_CLUSTER_BANDWIDTH_MIN, DEFAULT_CLUSTER_BANDWIDTH_SLACK, DEFAULT_CLUSTER_BUDGET,
    DEFAULT_CLUSTER_PRODUCT_CAP, DEFAULT_MIN_CONTAINMENT_LEN, MAX_BLOCK_K,
    MAX_OFFSET_KEYWORD_ZERO_SALIENT, MIN_RATIO_CHAR_SCALE, MIN_RATIO_CODE, MIN_RATIO_COMMENT,
    MIN_RATIO_COMMENT_OFFSET, MIN_RATIO_KEYWORD_ZERO_SALIENT, Weights, cluster_bandwidth_min,
    cluster_bandwidth_slack, cluster_budget, cluster_product_cap, set_cluster_bandwidth_min,
    set_cluster_bandwidth_slack, set_cluster_budget, set_cluster_product_cap,
};
pub(crate) use solver::solve_cluster_with_arena;
pub use solver::{DpOp, DpSolution, solve_cluster, solve_cluster_with};
#[cfg(test)]
pub(crate) use solver::{
    score_lcs_candidates_par, score_lcs_candidates_seq, solve_cluster_with_anchors,
};
pub use tokens::{DpTokens, dp_tokens};
pub use verify::verify_projection;
