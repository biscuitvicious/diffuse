//! The DP pairing bridge: `solve_cluster` as a drop-in replacement for the
//! legacy `pass_pairing`'s greedy scoring — the switchover's fidelity-first
//! step. Legacy passes 2–4 (span assembly, reflow filtering, ordered walk)
//! run UNCHANGED on top.
//!
//! Fidelity rules:
//! - The legacy guard set (moved rows, was-moved dels, dels whose content
//!   survives elsewhere in the file, tail-before-closing candidates, …) filters
//!   the DP's pairs exactly as it filtered the greedy candidates — a guarded
//!   del renders as a pure del under both engines.
//! - A DP block transition (expand/collapse) contributes its ANCHOR pairing to
//!   `pair_of` (the legacy 1-to-1 shape passes 2–4 expect); its absorbed rows
//!   stay unpaired adds/dels, which passes 2–4 already handle (the
//!   expansion-tail logic in `pass_assemble_mods`).
//!
//! The engine override is thread-local (`set_dp_engine`) so the frozen
//! golden legs keep running legacy in the same process; the binary reads
//! `DIFFUSE_ENGINE=dp`.

pub mod filter;
pub mod legacy;
pub mod seed;
pub mod survival;

use std::{
    sync::atomic::{self, AtomicU64},
    time::Instant,
};

use rustc_hash::{FxHashMap, FxHashSet};

use diffuse_syntax::profile::LangProfile;

use crate::{
    collapse::PairingResults,
    parse::{Hunk, Row},
};

use super::{ChangeCluster, HunkSegment, dp, partition_hunk_segments};

use filter::is_fragment_pair_rejected;
use legacy::legacy_index_maps;
pub use seed::{VerbatimSeedInput, verbatim_seeds};
pub use survival::{DelGuards, FnScopeCache, classify_dels};

/// DP pairing: partition the hunk's change runs into clusters, solve each
/// with the DP, and convert the transitions into the legacy `pair_of` map
/// (global add row index -> global del row index). 1-to-1 `Mod`/`Mov`
/// transitions pair directly; block transitions contribute their anchor
/// pair; hard-guarded (relocated) dels never pair.
pub static T_DP: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];

pub fn dp_pairing(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> PairingResults {
    let t0 = Instant::now();
    let (del_runs, run_of, del_ids) = legacy_index_maps(h);
    let t1 = Instant::now();
    T_DP[0].fetch_add(
        t1.duration_since(t0).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    let mut fn_cache: FnScopeCache = FxHashMap::default();
    let guards = classify_dels(h, old_content, new_content, profile, &mut fn_cache);
    let t2 = Instant::now();
    T_DP[1].fetch_add(
        t2.duration_since(t1).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    // Global row indices by line number (unique per hunk side). Pre-sized:
    // at most one entry per row, so these maps never rehash.
    let del_by_num: FxHashMap<u32, usize> = {
        let mut m = FxHashMap::with_capacity_and_hasher(h.rows.len(), Default::default());
        m.extend(h.rows.iter().enumerate().filter_map(|(i, r)| match r {
            Row::Del(d) | Row::MovSrc { line: d, .. } => Some((d.num, i)),
            _ => None,
        }));
        m
    };
    let add_by_num: FxHashMap<u32, usize> = {
        let mut m = FxHashMap::with_capacity_and_hasher(h.rows.len(), Default::default());
        m.extend(h.rows.iter().enumerate().filter_map(|(i, r)| match r {
            Row::Add(a) | Row::MovTgt { line: a, .. } => Some((a.num, i)),
            _ => None,
        }));
        m
    };
    let t3 = Instant::now();
    T_DP[2].fetch_add(
        t3.duration_since(t2).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    let mut pair_of: FxHashMap<usize, usize> = FxHashMap::default();
    let mut collapse_blocks: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    let mut expand_blocks: FxHashMap<usize, Vec<usize>> = FxHashMap::default();

    let segments = partition_hunk_segments(h, old_content, new_content, profile);
    let t4 = Instant::now();
    T_DP[3].fetch_add(
        t4.duration_since(t3).as_micros() as u64,
        atomic::Ordering::Relaxed,
    );

    // Verbatim pre-seeding: unambiguous trimmed-equal pairs join `pair_of`
    // before the DP walk, so corridor exclusion and global rerouting cannot
    // strand true moves. The walk below keeps exclusivity (seeded endpoints
    // never re-pair, blocks never absorb seeded rows except a seeded anchor
    // with clean aux); assembly builds Mods from `pair_of` regardless of
    // provenance, so no downstream changes are needed.
    // Seeded endpoints (walk exclusivity + block-member guards below).
    // Seeded rows are pre-inserted in `pair_of`, so the walk must never
    // re-pair them or absorb them into blocks (double-rendering).
    let mut seeded_dels: FxHashSet<usize> = FxHashSet::default();
    let mut seeded_adds: FxHashSet<usize> = FxHashSet::default();
    for seg in &segments {
        let HunkSegment::Cluster(cluster) = seg else {
            continue;
        };
        for (ga, gd) in verbatim_seeds(VerbatimSeedInput {
            cluster,
            h,
            del_by_num: &del_by_num,
            add_by_num: &add_by_num,
            old_content,
            new_content,
            profile,
            fn_cache: &mut fn_cache,
            hard: &guards.hard,
        }) {
            pair_of.insert(ga, gd);
            seeded_dels.insert(gd);
            seeded_adds.insert(ga);
        }
    }

    // Reuse the thread-local scratch arena across hunks: every Vec (tokens,
    // metas, flattened token streams, both skyline matrices) keeps its
    // capacity instead of regrowing from zero per hunk.
    super::dp::with_tls_arena(|arena| {
        for seg in segments {
            let cluster = match seg {
                HunkSegment::Cluster(mut c) => {
                    c.del_survival = guards.soft.clone();
                    c
                }
                HunkSegment::Context(_) => continue,
            };
            let t_s0 = Instant::now();
            let sol = super::dp::solve_cluster_with_arena(
                &cluster,
                profile,
                &super::dp::Weights::default(),
                arena,
            );
            let t_s1 = Instant::now();
            T_DP[4].fetch_add(
                t_s1.duration_since(t_s0).as_micros() as u64,
                atomic::Ordering::Relaxed,
            );

            // Walk the DP's consumption of cluster sides, mapping transitions
            // to global indices via line numbers.
            let mut oi = 0usize;
            let mut ni = 0usize;
            let global_del = |oi: usize| -> Option<usize> {
                cluster
                    .old_rows
                    .get(oi)
                    .and_then(Row::old_line)
                    .and_then(|n| del_by_num.get(&n).copied())
            };
            let global_add = |ni: usize| -> Option<usize> {
                cluster
                    .new_rows
                    .get(ni)
                    .and_then(Row::new_line)
                    .and_then(|n| add_by_num.get(&n).copied())
            };
            for t in &sol.transitions {
                match t {
                    dp::DpOp::Del => {
                        oi += 1;
                    }
                    dp::DpOp::Add => {
                        ni += 1;
                    }
                    dp::DpOp::Mod { .. } | dp::DpOp::Mov => {
                        if let (Some(gd), Some(ga)) = (global_del(oi), global_add(ni))
                            && !guards.hard.contains(&gd)
                            && !h.rows[gd].is_moved()
                            && !h.rows[ga].is_moved()
                            // A seeded del never re-pairs elsewhere: verbatim
                            // evidence outranks DP convenience. (Seeded adds
                            // need no check — pre-inserted, the
                            // contains_key guard below already skips them;
                            // the seeded pair itself is skipped there too,
                            // mapping preserved.)
                            && !seeded_dels.contains(&gd)
                            && !pair_of.contains_key(&ga)
                            && !is_fragment_pair_rejected(
                                gd,
                                ga,
                                h,
                                old_content,
                                new_content,
                                profile,
                                &mut fn_cache,
                            )
                        {
                            pair_of.insert(ga, gd);
                        }
                        oi += 1;
                        ni += 1;
                    }
                    dp::DpOp::Expand { anchor, k, .. } => {
                        if let (Some(gd), Some(ga)) = (global_del(oi), global_add(ni + anchor))
                            && !guards.hard.contains(&gd)
                            && !h.rows[gd].is_moved()
                            && !h.rows[ga].is_moved()
                            && !pair_of.contains_key(&ga)
                            && !seeded_dels.contains(&gd)
                            && !is_fragment_pair_rejected(
                                gd,
                                ga,
                                h,
                                old_content,
                                new_content,
                                profile,
                                &mut fn_cache,
                            )
                        {
                            pair_of.insert(ga, gd);
                            let mut block_adds = Vec::with_capacity(*k);
                            let mut all_valid = true;
                            for offset in 0..*k {
                                if let Some(a) = global_add(ni + offset) {
                                    if h.rows[a].is_moved() {
                                        all_valid = false;
                                        break;
                                    }
                                    block_adds.push(a);
                                } else {
                                    all_valid = false;
                                    break;
                                }
                            }
                            // Seeded aux members would double-render (aux
                            // context plus their own seed Mod): fall back to
                            // 1-to-1/standalone rows instead of forming.
                            let aux_clean = block_adds.iter().all(|a| !seeded_adds.contains(a));
                            if all_valid && block_adds.len() == *k && aux_clean {
                                expand_blocks.insert(ga, block_adds);
                            }
                        }
                        oi += 1;
                        ni += k;
                    }
                    dp::DpOp::Collapse { anchor, k, .. } => {
                        if let (Some(gd), Some(ga)) = (global_del(oi + anchor), global_add(ni))
                            && !guards.hard.contains(&gd)
                            && !h.rows[gd].is_moved()
                            && !h.rows[ga].is_moved()
                            && !pair_of.contains_key(&ga)
                            && !seeded_dels.contains(&gd)
                            && !is_fragment_pair_rejected(
                                gd,
                                ga,
                                h,
                                old_content,
                                new_content,
                                profile,
                                &mut fn_cache,
                            )
                        {
                            pair_of.insert(ga, gd);
                            let mut block_dels = Vec::with_capacity(*k);
                            let mut all_valid = true;
                            for offset in 0..*k {
                                if let Some(d) = global_del(oi + offset) {
                                    if guards.hard.contains(&d) || h.rows[d].is_moved() {
                                        all_valid = false;
                                        break;
                                    }
                                    block_dels.push(d);
                                } else {
                                    all_valid = false;
                                    break;
                                }
                            }
                            // Seeded block members would double-render (see
                            // Expand arm): fall back instead of forming.
                            let dels_clean = block_dels.iter().all(|d| !seeded_dels.contains(d));
                            if all_valid && block_dels.len() == *k && dels_clean {
                                collapse_blocks.insert(ga, block_dels);
                            }
                        }
                        oi += k;
                        ni += 1;
                    }
                }
            }
        }
    });

    PairingResults {
        pair_of,
        del_runs,
        run_of,
        del_ids,
        collapse_blocks,
        expand_blocks,
    }
}
