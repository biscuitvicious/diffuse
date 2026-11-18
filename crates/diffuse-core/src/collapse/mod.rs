//! One-for-one port of `review.ts`'s `collapseHunkRows` (PLAN.md §3),
//! refactored in Phase 6 into composable passes over a hunk's diff AST:
//!
//! 1. `pass_pairing`: Similarity and brace-anchor pairing (1-to-1).
//! 2. `pass_assemble_mods`: Assemble 1-to-1 `Mod` rows, multiline subword
//!    distribution, and reflow suppression.
//! 3. `pass_filter_reflows`: Filtering verbatim reflow dels and adds.
//! 4. `pass_sort_ordered_walk`: Keyed anchor sorting into the final visual row
//!    stream.

use std::{sync::atomic::AtomicU64, time::Instant};

use rustc_hash::{FxHashMap, FxHashSet};

use diffuse_syntax::profile::LangProfile;

use crate::{
    parse::{Hunk, Row},
    shift::pass_shift_hunk,
};

pub mod assemble;
pub mod aux;
pub mod invariants;
pub mod peer;
pub mod reflow;
pub mod residual;
pub mod slot;
pub mod walk;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use crate::parse::RowGroup;
pub use assemble::{AssembledMods, pass_assemble_mods, pass_normalize_identical_mods};
pub(crate) use aux::line_delim_delta;
#[cfg(test)]
pub(crate) use aux::relabel_moved_runs;
pub(crate) use invariants::assert_collapsed_reflow_invariants;
pub use invariants::{
    check_context_old_line_monotonicity, check_del_add_topological_order,
    check_no_orphaned_continuation_dels, check_reflow_cluster_confinement,
};
pub use peer::{assert_peer_group_invariants, peer_reflow_pipeline};
pub use reflow::{ReflowFilter, pass_filter_reflows};
pub use residual::pass_residual_pairing;
pub use slot::{AlignedSlotRow, PeerGroupRows, align_and_type_peer_group};
pub use walk::pass_sort_ordered_walk;

/// Results of the initial pairing pass.
pub struct PairingResults {
    pub pair_of: FxHashMap<usize, usize>, // add row id -> del row id
    pub del_runs: Vec<Vec<usize>>,
    pub run_of: FxHashMap<usize, Vec<usize>>,
    pub del_ids: Vec<usize>,
    pub collapse_blocks: FxHashMap<usize, Vec<usize>>,
    pub expand_blocks: FxHashMap<usize, Vec<usize>>,
}

/// Context holding all pairing maps and sets for assembly.
pub struct PairingContext<'a> {
    pub pair_of: &'a FxHashMap<usize, usize>,
    pub add_of: &'a FxHashMap<usize, usize>,
    pub run_of: &'a FxHashMap<usize, Vec<usize>>,
    pub paired_dels: &'a FxHashSet<usize>,
    pub collapse_blocks: &'a FxHashMap<usize, Vec<usize>>,
    pub expand_blocks: &'a FxHashMap<usize, Vec<usize>>,
}

/// Collapse each FIFO del→add pair into a single "mod" row: the new line
/// with its word-diff spans baked in.
///
/// Composed of 5 pure passes over the diff AST.
pub static T_COLLAPSE: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];

/// New-side line numbers owned by block-transition anchors: their joined
/// del_text may equal the add text while absorbed member lines still need
/// the anchor to render, so normalization must leave them Mod.
fn block_anchor_new_lines(h: &Hunk, p1: &PairingResults) -> FxHashSet<u32> {
    let mut news = FxHashSet::default();
    for ai in p1.collapse_blocks.keys().chain(p1.expand_blocks.keys()) {
        if let Some(n) = h.rows.get(*ai).and_then(|r| r.new_line()) {
            news.insert(n);
        }
    }
    news
}

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
    if std::env::var("DIFFUSE_USE_PEER_REFLOW").as_deref() == Ok("1") {
        return peer_reflow_pipeline(h, old_content, new_content, profile);
    }
    let t0 = Instant::now();
    let mut h_norm = pass_shift_hunk(h, profile);
    // Split coincidental context rows (lone `}`, `break;`, blanks) into
    // Del+Add in place so flanking single-sided clusters merge before any
    // pairing decision. Runs after the move pre-pass (raw-hunk clustering
    // preserved) but before every pairing stage below.
    let (split_rows, split_homes) = crate::ctx_split::split_trivial_ctx_rows(h_norm.rows, profile);
    h_norm.rows = split_rows;
    let h = &h_norm;
    let t1 = Instant::now();
    T_COLLAPSE[0].fetch_add(
        t1.duration_since(t0).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 1: DP cluster pairing engine
    let mut p1 = crate::cluster::pairing::dp_pairing_with_splits(
        h,
        old_content,
        new_content,
        profile,
        &split_homes,
    );
    let t2 = Instant::now();
    T_COLLAPSE[1].fetch_add(
        t2.duration_since(t1).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 1.5: Tier 2 residual evidence matching engine
    pass_residual_pairing(h, &mut p1, old_content, new_content, profile);

    let paired_dels: FxHashSet<usize> = p1.pair_of.values().copied().collect();
    let add_of: FxHashMap<usize, usize> =
        p1.pair_of.iter().map(|(&add, &del)| (del, add)).collect();

    // Pass 2: Assemble Mod rows and subword spans
    let pairing_ctx = PairingContext {
        pair_of: &p1.pair_of,
        add_of: &add_of,
        run_of: &p1.run_of,
        paired_dels: &paired_dels,
        collapse_blocks: &p1.collapse_blocks,
        expand_blocks: &p1.expand_blocks,
    };
    let p2 = pass_assemble_mods(h, &pairing_ctx, profile);
    let t3 = Instant::now();
    T_COLLAPSE[2].fetch_add(
        t3.duration_since(t2).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 3: Filter verbatim reflow dels and adds
    let p3 = pass_filter_reflows(
        h,
        &p2.out,
        &p2.kept_dels,
        &p2.pure_adds,
        &p2.del_groups,
        profile,
    );
    let t4 = Instant::now();
    T_COLLAPSE[3].fetch_add(
        t4.duration_since(t3).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 4: Keyed anchor sorting into the final visual row stream
    let res = pass_sort_ordered_walk(h, p2, p3);
    // Pass 5: Normalize no-visible-change Mod rows back to Ctx, keeping
    // each conversion only when the converted stream still satisfies
    // every collapsed-row invariant (crossed pairs stay Mod).
    let block_anchor_news = block_anchor_new_lines(h, &p1);
    let res = pass_normalize_identical_mods(h, res, profile, &block_anchor_news);
    let t5 = Instant::now();
    T_COLLAPSE[4].fetch_add(
        t5.duration_since(t4).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    crate::ensure!(
        res.iter().all(|r| match r {
            Row::Mod {
                old_line, new_line, ..
            } => old_line.num > 0 && new_line.num > 0,
            Row::Ctx(c) => c.old_pos > 0 && c.new_pos > 0,
            Row::Add(a) => a.num > 0,
            Row::Del(d) => d.num > 0,
            Row::MovSrc { line, .. } => line.num > 0,
            Row::MovTgt { line, .. } => line.num > 0,
            Row::AuxDel { line, .. } => line.num > 0,
            Row::AuxAdd { line, .. } => line.num > 0,
            Row::Sep(_) => true,
        }),
        "All collapsed rows must have valid, non-zero line coordinates"
    );

    crate::invariant!(
        res.iter()
            .all(|r| !matches!(r, Row::Mod { .. }) || !crate::merge::is_wholesale_replacement(r)),
        "Collapsed rows must never contain wholesale replacement Mod rows"
    );

    crate::invariant!(
        assert_collapsed_reflow_invariants(&res, profile).is_ok(),
        "Collapsed rows must satisfy reflow group invariants: {:?}",
        assert_collapsed_reflow_invariants(&res, profile).err()
    );

    crate::invariant!(
        check_no_orphaned_continuation_dels(h, &res).is_ok(),
        "{}",
        check_no_orphaned_continuation_dels(h, &res)
            .err()
            .unwrap_or_default()
    );

    crate::invariant!(
        check_context_old_line_monotonicity(&res).is_ok(),
        "{}",
        check_context_old_line_monotonicity(&res)
            .err()
            .unwrap_or_default()
    );

    crate::invariant!(
        check_del_add_topological_order(h, &res).is_ok(),
        "{}",
        check_del_add_topological_order(h, &res)
            .err()
            .unwrap_or_default()
    );

    res
}
