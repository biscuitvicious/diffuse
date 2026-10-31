//! Dynamic programming scratch arena, scoring weights, and token product caps.

use std::cell::RefCell;

pub const NEG_INF: i32 = -1_000_000;

/// Scoring weights and alignment parameters for Gotoh subline sequence
/// alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weights {
    /// Cost to open a gap in Gotoh affine DP (18).
    pub gap_open: i32,
    /// Incremental cost to extend a gap (2).
    pub gap_extend: i32,
    /// Bonus for consecutive matched tokens in the same container run (6).
    pub contig_bonus: i32,
    /// Subword concatenation bonus when adjacent subwords match (16).
    pub subword_bonus: i32,
    /// Bonus for matching tokens at identical delimiter nesting depth (2).
    pub depth_match_bonus: i32,
    /// Penalty per depth level for matching tokens at differing delimiter
    /// nesting depths (2).
    pub depth_mismatch_penalty: i32,
    /// Bonus for matching tokens with concordant subword boundary context (2).
    pub boundary_match_bonus: i32,
    /// Penalty for matching tokens with discordant subword boundary context
    /// (0).
    pub boundary_mismatch_penalty: i32,
}

impl Default for Weights {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Weights {
    pub const DEFAULT: Self = Self {
        gap_open: 18,
        gap_extend: 2,
        contig_bonus: 6,
        subword_bonus: 16,
        depth_match_bonus: 2,
        depth_mismatch_penalty: 2,
        boundary_match_bonus: 2,
        boundary_mismatch_penalty: 0,
    };
}

/// Reusable scratch memory arena for Gotoh 3-state affine gap dynamic
/// programming matrices, eliminating repeated per-invocation heap allocations.
#[derive(Default)]
pub struct LcsScratchArena {
    pub(crate) m_matrix: Vec<i32>,
    pub(crate) x_matrix: Vec<i32>,
    pub(crate) y_matrix: Vec<i32>,
    pub(crate) run_len: Vec<u16>,
    pub(crate) tb_m: Vec<u8>,
    pub(crate) tb_x: Vec<u8>,
    pub(crate) tb_y: Vec<u8>,
    pub(crate) depth_a: Vec<u16>,
    pub(crate) depth_b: Vec<u16>,
    pub(crate) container_a: Vec<u8>,
    pub(crate) container_b: Vec<u8>,
}

impl LcsScratchArena {
    /// Create a new scratch arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Prepares backing matrices for a DP run of size `total_cells`.
    /// Reallocates only if the existing capacity is insufficient, and
    /// resets initial values without releasing allocated memory.
    pub fn prepare(&mut self, total_cells: usize, n: usize, m: usize) {
        if self.m_matrix.len() < total_cells {
            self.m_matrix.resize(total_cells, NEG_INF);
            self.x_matrix.resize(total_cells, NEG_INF);
            self.y_matrix.resize(total_cells, NEG_INF);
            self.run_len.resize(total_cells, 0);
            self.tb_m.resize(total_cells, 0);
            self.tb_x.resize(total_cells, 0);
            self.tb_y.resize(total_cells, 0);
        }
        if self.depth_a.len() < n {
            self.depth_a.resize(n, 0);
        }
        if self.depth_b.len() < m {
            self.depth_b.resize(m, 0);
        }
        if self.container_a.len() < n {
            self.container_a.resize(n, 0);
        }
        if self.container_b.len() < m {
            self.container_b.resize(m, 0);
        }
        self.m_matrix[..total_cells].fill(NEG_INF);
        self.x_matrix[..total_cells].fill(NEG_INF);
        self.y_matrix[..total_cells].fill(NEG_INF);
        self.run_len[..total_cells].fill(0);
        self.tb_m[..total_cells].fill(0);
        self.tb_x[..total_cells].fill(0);
        self.tb_y[..total_cells].fill(0);
    }
}

thread_local! {
    static SCRATCH_ARENA: RefCell<LcsScratchArena> = RefCell::new(LcsScratchArena::default());
}

/// Default maximum token product cap (old tokens × new tokens) allowed for
/// word-level diffing before falling back.
pub const DEFAULT_TOKEN_PRODUCT_CAP: usize = 250_000;

thread_local! {
    static TOKEN_PRODUCT_CAP: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_TOKEN_PRODUCT_CAP) };
}

/// Set the maximum token product cap (old tokens × new tokens) allowed for
/// word-level diffing before falling back.
pub fn set_token_product_cap(cap: usize) {
    TOKEN_PRODUCT_CAP.with(|c| c.set(cap));
}

/// Get the current token product cap.
pub fn token_product_cap() -> usize {
    TOKEN_PRODUCT_CAP.with(|c| c.get())
}
