//! Thread-local scratch arena management and metadata structures for
//! zero-allocation DP solving.

use diffuse_algo::skyline::SkylineMatrix;
use diffuse_syntax::prelude::CloseDelimiter;
use rustc_hash::FxHashSet;

use crate::{cluster::SyntaxSectionKind, similarity::TokenInterner};

use super::{
    block::RowValidBlocks,
    solver::{Cell, DpSolution, solve_cluster_with_arena},
    tokens::DpTokens,
};

#[derive(Clone, Copy, Default)]
pub struct DpRowMeta {
    pub trimmed_id: u32,
    pub comment: bool,
    pub kind: SyntaxSectionKind,
    pub first_word: Option<u32>,
    pub indent: usize,
    pub stripped_len: usize,
    pub prox_close: f64,
    pub prox_open: f64,
    pub is_reflow: bool,
    pub ends_with_semi: bool,
    pub is_stmt_start: bool,
    pub is_arm_terminal: bool,
    pub property_key: Option<u32>,
    pub close_tail: CloseDelimiter,
    pub exact_close: CloseDelimiter,
    /// First word is a language keyword (`use`, `let`, …): the line has a
    /// genuine statement head (prose and macro/identifier heads do not).
    /// Gates the same-indent head bonus.
    pub head_is_keyword: bool,
    /// Return-slot tail (`-> T {` opening a function body): pairing two of
    /// these at equal indent is a structural slot claim, scored with the
    /// slot bonus. Set from `LangProfile::is_return_tail_position`; silent
    /// without a profile or for languages that opt out.
    pub is_return_tail: bool,
    /// The current function in context, interned: the line's own function
    /// name capture when the signature head sits on the line, else the
    /// nearest capture in the preceding context (a wrapped signature). The
    /// slot claim requires both sides to agree on it.
    pub slot_owner: Option<u32>,
    /// Verbatim twin survives elsewhere with multiplicity intact: pairing
    /// this del as `Mod` pays `Weights::survives_elsewhere_penalty`.
    pub survives_in_new: bool,
    /// Delimiter depth net delta for the row.
    pub delim_delta: i16,
}

/// Scratch arena for DP cluster solving to reuse memory across clusters.
#[derive(Default)]
pub struct DpScratchArena {
    pub(crate) interner: TokenInterner,
    pub(crate) old_tokens: Vec<DpTokens>,
    pub(crate) new_tokens: Vec<DpTokens>,
    pub(crate) old_meta: Vec<DpRowMeta>,
    pub(crate) new_meta: Vec<DpRowMeta>,
    pub(crate) old_close_dist: Vec<usize>,
    pub(crate) new_close_dist: Vec<usize>,
    pub(crate) old_open_dist: Vec<usize>,
    pub(crate) new_open_dist: Vec<usize>,
    pub(crate) all_old_ids: Vec<u32>,
    pub(crate) all_old_coords: Vec<(u8, u8)>,
    pub(crate) all_old_token_chars: Vec<usize>,
    pub(crate) old_row_token_starts: Vec<usize>,
    pub(crate) all_new_ids: Vec<u32>,
    pub(crate) all_new_coords: Vec<(u8, u8)>,
    pub(crate) all_new_token_chars: Vec<usize>,
    pub(crate) new_row_token_starts: Vec<usize>,
    pub(crate) old_valid_blocks: Vec<RowValidBlocks>,
    pub(crate) new_valid_blocks: Vec<RowValidBlocks>,
    pub(crate) lcs_scratch: Vec<u64>,
    pub(crate) grid: SkylineMatrix<Cell>,
    pub(crate) precomputed_lcs: SkylineMatrix<u32>,
    pub(crate) precomputed_salient_lcs: SkylineMatrix<u32>,
    pub(crate) lcs_candidates: Vec<(usize, usize)>,
    pub(crate) lcs_scores: Vec<u32>,
    pub(crate) lcs_salient_scores: Vec<u32>,
    pub(crate) anchor_token_ids: Vec<u32>,
    pub(crate) old_anchor_ranges: Vec<(usize, usize, usize)>,
    pub(crate) new_anchor_ranges: Vec<(usize, usize, usize)>,
    pub(crate) forbidden_pairs: FxHashSet<(usize, usize)>,
}

impl DpScratchArena {
    pub fn clear(&mut self) {
        self.interner.clear();
        self.old_tokens.clear();
        self.new_tokens.clear();
        self.old_meta.clear();
        self.new_meta.clear();
        self.old_close_dist.clear();
        self.new_close_dist.clear();
        self.old_open_dist.clear();
        self.new_open_dist.clear();
        self.all_old_ids.clear();
        self.all_old_coords.clear();
        self.all_old_token_chars.clear();
        self.old_row_token_starts.clear();
        self.all_new_ids.clear();
        self.all_new_coords.clear();
        self.all_new_token_chars.clear();
        self.new_row_token_starts.clear();
        self.lcs_candidates.clear();
        self.lcs_scores.clear();
        self.lcs_salient_scores.clear();
        self.anchor_token_ids.clear();
        self.old_anchor_ranges.clear();
        self.new_anchor_ranges.clear();
    }
}

thread_local! {
    static ARENA: std::cell::RefCell<DpScratchArena> = std::cell::RefCell::new(DpScratchArena::default());
}

/// Solves a cluster using thread-local scratch arena to eliminate allocations
/// in multithreaded runs.
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(
        old_rows = cluster.m(),
        new_rows = cluster.n(),
        section = ?cluster.section
    )
)]
pub fn solve_cluster_with_tls(
    cluster: &crate::cluster::ChangeCluster,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
    w: &super::config::Weights,
) -> DpSolution {
    let sol = ARENA.with(|arena_cell| {
        let mut arena = arena_cell.borrow_mut();
        solve_cluster_with_arena(cluster, profile, w, &mut arena)
    });
    tracing::debug!(
        score = sol.score,
        transition_count = sol.transitions.len(),
        "Cluster solved"
    );
    sol
}

/// Run `f` with this thread's reusable DP scratch arena: every Vec keeps its
/// capacity across calls instead of regrowing from zero per cluster.
pub fn with_tls_arena<R>(f: impl FnOnce(&mut DpScratchArena) -> R) -> R {
    ARENA.with_borrow_mut(f)
}
