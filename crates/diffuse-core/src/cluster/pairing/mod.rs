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
//! - A DP block transition (expand/collapse) contributes its representative
//!   pairing to `pair_of` (the legacy 1-to-1 shape passes 2–4 expect); its
//!   absorbed rows stay unpaired adds/dels, which passes 2–4 already handle
//!   (the expansion-tail logic in `pass_assemble_mods`).
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

/// Returns true if candidate pair `(add, del)` crosses an existing pair
/// `(ex_add, ex_del)`. Out-of-order pairings invert line monotonicity and are
/// rejected.
#[inline]
pub(crate) fn pair_crosses(add: usize, del: usize, ex_add: usize, ex_del: usize) -> bool {
    (add < ex_add && del > ex_del) || (add > ex_add && del < ex_del)
}

/// Returns true if candidate pair `(add, del)` crosses any pair in `existing`.
#[inline]
pub(crate) fn pair_crosses_any<'a, I>(add: usize, del: usize, existing: I) -> bool
where
    I: IntoIterator<Item = (&'a usize, &'a usize)>,
{
    existing
        .into_iter()
        .any(|(&ex_a, &ex_d)| pair_crosses(add, del, ex_a, ex_d))
}

/// DP pairing: partition the hunk's change runs into clusters, solve each
/// with the DP, and convert the transitions into the legacy `pair_of` map
/// (global add row index -> global del row index). 1-to-1 `Mod`/`Mov`
/// transitions pair directly; block transitions contribute their representative
/// pair; hard-guarded (relocated) dels never pair.
pub static T_DP: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];

pub fn dp_pairing(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> PairingResults {
    dp_pairing_with_splits(h, old_content, new_content, profile, &[])
}

/// [`dp_pairing`] with split-home restoration: `split_homes` carries the
/// `(del_row_idx, add_row_idx)` home pairs recorded by
/// [`crate::ctx_split::split_trivial_ctx_rows`] in this hunk's row
/// indexing. After the walk, homes neither half improved are restored
/// (see [`restore_split_homes`]); callers without splits pass `&[]`.
pub fn dp_pairing_with_splits(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
    split_homes: &[(usize, usize)],
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
            Row::Del { line: d, .. } | Row::MovSrc { line: d, .. } => Some((d.num, i)),
            _ => None,
        }));
        m
    };
    let add_by_num: FxHashMap<u32, usize> = {
        let mut m = FxHashMap::with_capacity_and_hasher(h.rows.len(), Default::default());
        m.extend(h.rows.iter().enumerate().filter_map(|(i, r)| match r {
            Row::Add { line: a, .. } | Row::MovTgt { line: a, .. } => Some((a.num, i)),
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
    // strand true moves. Seeded endpoints keep walk exclusivity below
    // (never re-pair; blocks never absorb them except a seeded pair with
    // clean members); assembly builds Mods from `pair_of` regardless of
    // provenance, so no downstream changes are needed.
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
            if !pair_crosses_any(ga, gd, &pair_of) {
                pair_of.insert(ga, gd);
                seeded_dels.insert(gd);
                seeded_adds.insert(ga);
            }
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
                            && !pair_crosses_any(ga, gd, &pair_of)
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
                            && !pair_crosses_any(ga, gd, &pair_of)
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
                            // Seeded members would double-render (member
                            // context plus their own seed Mod): fall back to
                            // 1-to-1/standalone rows instead of forming.
                            let member_clean = block_adds.iter().all(|a| !seeded_adds.contains(a));
                            if all_valid && block_adds.len() == *k && member_clean {
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
                            && !pair_crosses_any(ga, gd, &pair_of)
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
                                pair_of.insert(ga, block_dels[0]);
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

    restore_split_homes(SplitHomeInput {
        h,
        pair_of: &mut pair_of,
        split_homes,
        seeded_dels: &seeded_dels,
        seeded_adds: &seeded_adds,
        hard: &guards.hard,
        collapse_blocks: &collapse_blocks,
        expand_blocks: &expand_blocks,
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

/// Restore no-gain split homes.
///
/// Splitting a coincidental Ctx row frees both halves to pair anywhere in
/// the merged cluster. When the DP's global optimum is an *identical*
/// reshuffle — a half left unpaired, or paired with a full-text-identical
/// twin elsewhere — the split bought nothing: git's positional alignment
/// carries at least as much evidence (same text, nearer position), and
/// the identical twin it stranded was quietly paired before. Such homes
/// are restored by force-pairing the originals (assembly renders the
/// identical Mod and the normalize pass folds it back to Ctx), unseating
/// the identical theft links so the stranded twins fall back to leftover
/// or reflow handling.
///
/// A home is kept split (never restored) when:
/// - either half paired non-identically (a Mod with real differences — the
///   split enabled genuine evidence, e.g. the dispatch reindent tails whose
///   halves pair across with indent deltas); full-text equality is the bar, so
///   indent-only Mods count as improvement;
/// - either half is seeded, hard-guarded, moved, or claimed by a block
///   transition (stronger evidence or structural ownership wins);
/// - restoring would cross a surviving pair (positional monotonicity).
///
/// Input bundle for [`restore_split_homes`]: eight fields would trip
/// `clippy::too_many_arguments` as fn args.
struct SplitHomeInput<'a> {
    h: &'a Hunk,
    pair_of: &'a mut FxHashMap<usize, usize>,
    split_homes: &'a [(usize, usize)],
    seeded_dels: &'a FxHashSet<usize>,
    seeded_adds: &'a FxHashSet<usize>,
    hard: &'a FxHashSet<usize>,
    collapse_blocks: &'a FxHashMap<usize, Vec<usize>>,
    expand_blocks: &'a FxHashMap<usize, Vec<usize>>,
}

fn restore_split_homes(input: SplitHomeInput<'_>) {
    let SplitHomeInput {
        h,
        pair_of,
        split_homes,
        seeded_dels,
        seeded_adds,
        hard,
        collapse_blocks,
        expand_blocks,
    } = input;
    if split_homes.is_empty() {
        return;
    }
    let mut blocked: FxHashSet<usize> = FxHashSet::default();
    for (ga, members) in collapse_blocks.iter().chain(expand_blocks.iter()) {
        blocked.insert(*ga);
        blocked.extend(members.iter().copied());
    }
    // Inverse map (del -> add) rebuilt as homes are restored.
    let mut add_of: FxHashMap<usize, usize> =
        pair_of.iter().map(|(&add, &del)| (del, add)).collect();
    let full_text = |idx: usize| -> Option<&str> {
        match h.rows.get(idx) {
            Some(Row::Del { line: d, .. }) | Some(Row::MovSrc { line: d, .. }) => {
                Some(d.text.as_str())
            }
            Some(Row::Add { line: a, .. }) | Some(Row::MovTgt { line: a, .. }) => {
                Some(a.text.as_str())
            }
            _ => None,
        }
    };
    for &(gd, ga) in split_homes {
        // Split homes are Del+Add by construction; anything else (or
        // out-of-range after upstream surgery) keeps the DP solution.
        if !matches!(h.rows.get(gd), Some(Row::Del { .. }))
            || !matches!(h.rows.get(ga), Some(Row::Add { .. }))
            || hard.contains(&gd)
            || h.rows[gd].is_moved()
            || h.rows[ga].is_moved()
            || seeded_dels.contains(&gd)
            || seeded_adds.contains(&ga)
            || blocked.contains(&gd)
            || blocked.contains(&ga)
        {
            continue;
        }
        let home_text = match full_text(gd) {
            Some(t) => t,
            None => continue,
        };
        // Each half is at most identically re-paired: unpaired, or paired
        // with a full-text-identical twin. (Trimmed-only equality is
        // improvement — indent deltas are the dispatch reindent signal.)
        let del_ok = match add_of.get(&gd) {
            None => true,
            Some(&other_ga) => full_text(other_ga) == Some(home_text),
        };
        let add_ok = match pair_of.get(&ga) {
            None => true,
            Some(&other_gd) => full_text(other_gd) == Some(home_text),
        };
        if !del_ok || !add_ok {
            continue;
        }
        // Never cross a surviving pair: shared endpoints cannot cross,
        // so only pairs disjoint from both halves can veto.
        let crosses = pair_of
            .iter()
            .any(|(&a2, &d2)| a2 != ga && d2 != gd && (d2 < gd) != (a2 < ga));
        if crosses {
            continue;
        }
        // Unseat the identical theft links, then force-pair the originals.
        if let Some(&thief_ga) = add_of.get(&gd) {
            pair_of.remove(&thief_ga);
        }
        pair_of.remove(&ga);
        pair_of.insert(ga, gd);
        add_of.insert(gd, ga);
    }
}

#[cfg(test)]
mod restore_tests {
    use super::*;
    use crate::parse::{AddLine, DelLine};

    fn hunk_of(rows: Vec<Row>) -> Hunk {
        Hunk {
            rows,
            ..Hunk::default()
        }
    }

    fn del(num: u32, text: &str) -> Row {
        Row::del(DelLine::new(num, text.to_string()))
    }

    fn add(num: u32, text: &str) -> Row {
        Row::add(AddLine::new(num, text.to_string()))
    }

    /// Restore helper: seeds, guards, and block membership default to
    /// empty; the seeded/guarded test passes its overrides.
    fn restore(
        h: &Hunk,
        pair_of: &mut FxHashMap<usize, usize>,
        homes: &[(usize, usize)],
        seeded_adds: &FxHashSet<usize>,
        hard: &FxHashSet<usize>,
    ) {
        let empty_set: FxHashSet<usize> = FxHashSet::default();
        let empty_blocks: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
        restore_split_homes(SplitHomeInput {
            h,
            pair_of,
            split_homes: homes,
            seeded_dels: &empty_set,
            seeded_adds,
            hard,
            collapse_blocks: &empty_blocks,
            expand_blocks: &empty_blocks,
        });
    }

    /// Twin-theft geometry (theme-light-toggle diff_grid hunk, Ctx 303/304
    /// vs del 329): two split homes up front, a mass of trailing dels, and
    /// far identical twins. The DP pairs at the end; the revert must
    /// restore both homes and strand the thieves.
    fn twin_theft_hunk() -> (Hunk, Vec<(usize, usize)>) {
        let mut rows = vec![del(303, "}"), add(304, "}"), del(304, ""), add(305, "")];
        for (i, name) in ["alpha", "beta", "gamma", "delta"].iter().enumerate() {
            rows.push(del(
                306 + i as u32,
                &format!("let filler_{name} = compute();"),
            ));
            rows.push(del(
                310 + i as u32,
                &format!("let other_{name} = render();"),
            ));
        }
        rows.push(del(329, "}"));
        rows.push(del(330, ""));
        (hunk_of(rows), vec![(0, 1), (2, 3)])
    }

    #[test]
    fn test_dp_steals_identical_twins_at_end() {
        // Characterizes the DP's tiebreak (no revert): the far twins win.
        let (h, _) = twin_theft_hunk();
        let profile = diffuse_syntax::registry::lang_profile("rs");
        let res = dp_pairing(&h, None, None, profile);
        assert_eq!(
            res.pair_of.get(&1),
            Some(&12),
            "DP must steal Add(304) for far twin Del(329), got {:?}",
            res.pair_of
        );
    }

    #[test]
    fn test_revert_restores_no_gain_split_homes() {
        let (h, homes) = twin_theft_hunk();
        let profile = diffuse_syntax::registry::lang_profile("rs");
        let res = dp_pairing_with_splits(&h, None, None, profile, &homes);
        assert_eq!(
            res.pair_of.get(&1),
            Some(&0),
            "split home (303,304) must be restored, got {:?}",
            res.pair_of
        );
        assert_eq!(
            res.pair_of.get(&3),
            Some(&2),
            "split home (304,305) must be restored, got {:?}",
            res.pair_of
        );
        assert!(
            !res.pair_of.values().any(|&d| d == 12 || d == 13),
            "thieves Del(329)/Del(330) must strand, got {:?}",
            res.pair_of
        );
    }

    #[test]
    fn test_revert_keeps_non_identical_repairing() {
        // Dispatch-pattern guard: a half re-paired with real differences
        // (indent delta) justifies the split — no restore.
        let h = hunk_of(vec![del(309, "    }"), add(345, "    }"), add(305, "}")]);
        let mut pair_of: FxHashMap<usize, usize> = FxHashMap::default();
        pair_of.insert(2, 0);
        let before = pair_of.clone();
        restore(
            &h,
            &mut pair_of,
            &[(0, 1)],
            &FxHashSet::default(),
            &FxHashSet::default(),
        );
        assert_eq!(
            pair_of, before,
            "non-identical re-pairing must keep the split"
        );
    }

    #[test]
    fn test_revert_skips_crossing_survivors() {
        // Restoring (2,3) would cross the surviving pair (4,1).
        let h = hunk_of(vec![
            del(100, "let a = 1;"),
            add(100, "let a = 1;"),
            del(101, "}"),
            add(102, "}"),
            del(103, "let b = 2;"),
        ]);
        let mut pair_of: FxHashMap<usize, usize> = FxHashMap::default();
        pair_of.insert(1, 4);
        let before = pair_of.clone();
        restore(
            &h,
            &mut pair_of,
            &[(2, 3)],
            &FxHashSet::default(),
            &FxHashSet::default(),
        );
        assert_eq!(pair_of, before, "crossing restore must be skipped");
    }

    #[test]
    fn test_revert_skips_seeded_and_guarded_halves() {
        let h = hunk_of(vec![del(303, "}"), add(304, "}"), del(329, "}")]);
        // Seeded add: theft stands.
        let mut pair_of: FxHashMap<usize, usize> = FxHashMap::default();
        pair_of.insert(1, 2);
        let mut seeded_adds: FxHashSet<usize> = FxHashSet::default();
        seeded_adds.insert(1);
        let before = pair_of.clone();
        restore(
            &h,
            &mut pair_of,
            &[(0, 1)],
            &seeded_adds,
            &FxHashSet::default(),
        );
        assert_eq!(pair_of, before, "seeded halves must never be unseated");
        // Hard-guarded del: no restore.
        let mut hard: FxHashSet<usize> = FxHashSet::default();
        hard.insert(0);
        let mut pair_of2: FxHashMap<usize, usize> = FxHashMap::default();
        let before2 = pair_of2.clone();
        restore(&h, &mut pair_of2, &[(0, 1)], &FxHashSet::default(), &hard);
        assert_eq!(
            pair_of2, before2,
            "guarded halves must keep the DP solution"
        );
    }
}
