//! Configuration parameters and path gap cost for cluster alignment.

use std::cell::Cell;

/// Cost of leaving one row unpaired on the cluster path (`γ`).
///
/// This is the only price of displacement: the path has no diagonal threshold
/// and no explanation gate, so a correspondence survives a gap exactly when
/// its evidence outbids the gap. Units are matched content characters —
/// `pair_evidence` awards `MATCHED_CHAR_REWARD` (10) per explained character —
/// so `γ = 10` reads as "a pair worth `k` characters may bridge `k` rows".
pub const PATH_GAP_COST: i64 = 10;

/// Default maximum cluster cell budget allowed before falling back to
/// wholesale deletion/addition.
pub const DEFAULT_CLUSTER_BUDGET: usize = 10_000_000;
pub const DEFAULT_CLUSTER_PRODUCT_CAP: usize = DEFAULT_CLUSTER_BUDGET;

thread_local! {
    static CLUSTER_BUDGET: Cell<usize> = const { Cell::new(DEFAULT_CLUSTER_BUDGET) };
}

/// Set the maximum cluster cell budget allowed before falling back.
pub fn set_cluster_budget(budget: usize) {
    CLUSTER_BUDGET.with(|c| c.set(budget));
}

/// Get the current cluster cell budget.
pub fn cluster_budget() -> usize {
    CLUSTER_BUDGET.with(Cell::get)
}

/// Backwards-compatible alias for [`set_cluster_budget`].
pub fn set_cluster_product_cap(cap: usize) {
    set_cluster_budget(cap);
}

/// Backwards-compatible alias for [`cluster_budget`].
pub fn cluster_product_cap() -> usize {
    cluster_budget()
}
