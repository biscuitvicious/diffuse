//! Diff row collapse pipeline via irreducible peer-group reflow.

use std::sync::atomic::AtomicU64;

use rustc_hash::FxHashMap;

use diffuse_syntax::profile::LangProfile;

use crate::parse::{Hunk, Row};

pub mod invariants;
pub mod normalize;
pub mod peer;
pub mod residual;
pub mod slot;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use crate::parse::PeerGroup;
pub use invariants::{
    check_context_old_line_monotonicity, check_del_add_topological_order,
    check_no_orphaned_continuation_dels,
};
pub use normalize::pass_normalize_identical_mods;
pub use peer::{assert_peer_group_invariants, peer_reflow_pipeline};
pub use residual::pass_residual_pairing;
pub use slot::{AlignedSlotRow, PeerGroupRows, align_and_type_peer_group};

pub(crate) fn line_delim_delta(text: &str) -> i32 {
    let mut delta = 0i32;
    for &b in text.as_bytes() {
        match b {
            b'{' | b'(' | b'[' => delta += 1,
            b'}' | b')' | b']' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// Results of the initial pairing pass.
pub struct PairingResults {
    pub pair_of: FxHashMap<usize, usize>, // add row id -> del row id
    pub del_runs: Vec<Vec<usize>>,
    pub run_of: FxHashMap<usize, Vec<usize>>,
    pub del_ids: Vec<usize>,
    pub collapse_blocks: FxHashMap<usize, Vec<usize>>,
    pub expand_blocks: FxHashMap<usize, Vec<usize>>,
}

/// Collapse each FIFO del→add pair into a single "mod" row: the new line
/// with its word-diff spans baked in.
pub static T_COLLAPSE: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];

#[tracing::instrument(
    level = "debug",
    skip(h, old_content, new_content, profile),
    fields(old_start = h.old_start, new_start = h.new_start, row_count = h.rows.len())
)]
pub fn collapse_hunk_rows(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Vec<Row> {
    peer_reflow_pipeline(h, old_content, new_content, profile)
}
