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
pub mod reflow;
pub mod walk;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use crate::parse::RowGroup;
pub use assemble::{AssembledMods, pass_assemble_mods};
pub(crate) use aux::line_delim_delta;
#[cfg(test)]
pub(crate) use aux::relabel_moved_runs;
pub(crate) use invariants::assert_collapsed_reflow_invariants;
pub use invariants::{
    check_context_old_line_monotonicity, check_del_add_topological_order,
    check_no_orphaned_continuation_dels, check_reflow_cluster_confinement,
};
pub use reflow::{ReflowFilter, pass_filter_reflows};
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
/// Composed of 4 pure passes over the diff AST.
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
    let t0 = Instant::now();
    let h_norm = pass_shift_hunk(h, profile);
    let h = &h_norm;
    let t1 = Instant::now();
    T_COLLAPSE[0].fetch_add(
        t1.duration_since(t0).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 1: DP cluster pairing engine
    let p1 = crate::cluster::pairing::dp_pairing(h, old_content, new_content, profile);
    let t2 = Instant::now();
    T_COLLAPSE[1].fetch_add(
        t2.duration_since(t1).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

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
    let p3 = pass_filter_reflows(h, &p2.out, &p2.kept_dels, &p2.pure_adds, profile);
    let t4 = Instant::now();
    T_COLLAPSE[3].fetch_add(
        t4.duration_since(t3).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 4: Keyed anchor sorting into the final visual row stream
    let res = pass_sort_ordered_walk(h, p2.out, p3, &p2.add_spans, &p2.add_groups, &p2.add_runs);
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
