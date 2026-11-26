//! Diff row collapse pipeline via unified reflow lattice.

use diffuse_syntax::profile::LangProfile;

use crate::parse::{Hunk, Row};

pub mod evidence;
pub mod formation;
pub mod frame;
pub mod invariants;
pub mod lattice;
pub mod normalize;
pub mod peer;
pub mod reflow;
pub mod residual;
pub mod slot;
pub mod tokens;

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
pub use slot::{AlignedSlotRow, PeerGroupRows};

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
