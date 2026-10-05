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

use std::{
    sync::atomic::{self, AtomicU64},
    time::Instant,
};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    collapse::PairingResults,
    parse::{Hunk, Row},
    scope::{fn_of_exists, is_tail_before_closing},
    syntax::profile::LangProfile,
};

use super::{HunkSegment, dp, partition_hunk_segments};

type LegacyIndexMaps = (Vec<Vec<usize>>, FxHashMap<usize, Vec<usize>>, Vec<usize>);

/// The legacy run/del-index construction (identical for both engines —
/// passes 2–4 consume it).
fn legacy_index_maps(h: &Hunk) -> LegacyIndexMaps {
    let mut del_runs: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < h.rows.len() {
        if !matches!(h.rows[i], Row::Del(_) | Row::MovSrc(_)) {
            i += 1;
            continue;
        }
        let mut run = vec![i];
        while i + 1 < h.rows.len() && matches!(h.rows[i + 1], Row::Del(_) | Row::MovSrc(_)) {
            i += 1;
            run.push(i);
        }
        del_runs.push(run);
        i += 1;
    }
    let mut run_of: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for run in &del_runs {
        for &ri in run {
            run_of.insert(ri, run.clone());
        }
    }
    let del_ids: Vec<usize> = h
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r, Row::Del(_) | Row::MovSrc(_)))
        .map(|(i, _)| i)
        .collect();
    (del_runs, run_of, del_ids)
}

/// Per-hunk memo of [`crate::scope::fn_of_str`] keyed by `(old_side, line)`:
/// the guarded-dels pass and the transition-walk pair checks query the same
/// rows' enclosing scopes repeatedly; each is resolved once per hunk.
type FnScopeCache<'a> = FxHashMap<(bool, u32), Option<&'a str>>;

fn memo_fn_of_str<'a>(
    cache: &mut FnScopeCache<'a>,
    old_side: bool,
    content: Option<&'a [&'a str]>,
    line: Option<u32>,
    profile: Option<&LangProfile>,
) -> Option<&'a str> {
    let line = line?;
    if let Some(cached) = cache.get(&(old_side, line)) {
        return *cached;
    }
    let computed = crate::scope::fn_of_str(content, Some(line), profile);
    cache.insert((old_side, line), computed);
    computed
}

/// The dels the legacy `pass_pairing` refuses to pair (its top-of-loop
/// guards), computed identically — the DP's pairs over these dels are
/// dropped so both engines leave the same rows standalone.
pub(crate) fn guarded_dels<'a>(
    h: &Hunk,
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&LangProfile>,
    fn_cache: &mut FnScopeCache<'a>,
) -> FxHashSet<usize> {
    if !h
        .rows
        .iter()
        .any(|r| matches!(r, Row::Del(_) | Row::MovSrc(_)))
    {
        return FxHashSet::default();
    }

    let mut skip = FxHashSet::default();
    let mut new_trimmed_set: Option<FxHashSet<&str>> = None;
    let has_any_moved = h.rows.iter().any(|a| a.is_moved());

    let same_hunk_add_trimmed: FxHashSet<&str> = h
        .rows
        .iter()
        .filter_map(|a| match a {
            Row::Add(_) | Row::MovTgt(_) => Some(a.text().trim()),
            _ => None,
        })
        .collect();

    let same_hunk_property_keys: FxHashSet<&str> = if let Some(p) = profile {
        h.rows
            .iter()
            .filter_map(|a| match a {
                Row::Add(_) | Row::MovTgt(_) => p.extract_property_key(a.text()),
                _ => None,
            })
            .collect()
    } else {
        FxHashSet::default()
    };

    let moved_old_lines: FxHashSet<u32> = if has_any_moved {
        h.rows
            .iter()
            .filter_map(|a| if a.is_moved() { a.old_line() } else { None })
            .collect()
    } else {
        FxHashSet::default()
    };
    let moved_trimmed: FxHashSet<&str> = if has_any_moved {
        h.rows
            .iter()
            .filter_map(|a| {
                if a.is_moved() {
                    Some(a.text().trim())
                } else {
                    None
                }
            })
            .collect()
    } else {
        FxHashSet::default()
    };

    // Precompute add-tail candidates to eliminate O(D * A) scans in the
    // deletion loop.
    let mut has_any_add_tail = false;
    let mut add_tail_fns: FxHashSet<&str> = FxHashSet::default();
    for a in &h.rows {
        if matches!(a, Row::Add(_) | Row::MovTgt(_))
            && is_tail_before_closing(a.new_line(), false, old_content, new_content, profile)
        {
            has_any_add_tail = true;
            if let Some(afn) = memo_fn_of_str(fn_cache, false, new_content, a.new_line(), profile) {
                add_tail_fns.insert(afn);
            }
        }
    }

    for (ri, r) in h.rows.iter().enumerate() {
        if !matches!(r, Row::Del(_) | Row::MovSrc(_)) {
            continue;
        }
        if r.is_moved() {
            skip.insert(ri);
            continue;
        }
        let r_trim = r.text().trim();
        let was_moved = has_any_moved
            && !r_trim.is_empty()
            && (r
                .old_line()
                .is_some_and(|num| moved_old_lines.contains(&num))
                || moved_trimmed.contains(r_trim));
        if was_moved {
            skip.insert(ri);
            continue;
        }
        let has_same_hunk_add = same_hunk_add_trimmed.contains(r_trim);
        let has_same_hunk_property_add = profile.is_some_and(|p| {
            p.extract_property_key(r.text())
                .is_some_and(|key| same_hunk_property_keys.contains(key))
        });
        let del_is_tail =
            is_tail_before_closing(r.old_line(), true, old_content, new_content, profile);
        let del_fn = if del_is_tail || (!has_same_hunk_add && !has_same_hunk_property_add) {
            memo_fn_of_str(fn_cache, true, old_content, r.old_line(), profile)
        } else {
            None
        };
        let tail_candidate = del_is_tail
            && has_any_add_tail
            && match del_fn {
                None => true,
                Some(dfn) => add_tail_fns.contains(dfn),
            };

        if !r_trim.is_empty()
            && !tail_candidate
            && !has_same_hunk_add
            && !has_same_hunk_property_add
        {
            let in_new = if let Some(dfn) = del_fn {
                // If function scope is known, check whether the line survives
                // within the SAME function in the new file,
                // rather than an unrelated scope.
                if let Some(body) = crate::scope::fn_body_lines(new_content, dfn, profile) {
                    body.iter().any(|l| l.trim() == r_trim)
                } else {
                    false
                }
            } else if let Some(nc) = new_content {
                let set =
                    new_trimmed_set.get_or_insert_with(|| nc.iter().map(|l| l.trim()).collect());
                set.contains(r_trim)
            } else {
                false
            };
            if in_new {
                skip.insert(ri);
            }
        }
    }
    skip
}

fn is_fragment_pair_rejected<'a>(
    gd: usize,
    ga: usize,
    h: &Hunk,
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&LangProfile>,
    fn_cache: &mut FnScopeCache<'a>,
) -> bool {
    let r = &h.rows[gd];
    let a = &h.rows[ga];

    let del_fn = memo_fn_of_str(fn_cache, true, old_content, r.old_line(), profile);
    let add_fn = memo_fn_of_str(fn_cache, false, new_content, a.new_line(), profile);
    if let (Some(dfn), Some(afn)) = (del_fn, add_fn)
        && dfn != afn
        && (fn_of_exists(new_content, dfn, profile) || fn_of_exists(old_content, afn, profile))
    {
        return true;
    }

    false
}

/// DP pairing: partition the hunk's change runs into clusters, solve each
/// with the DP, and convert the transitions into the legacy `pair_of` map
/// (global add row index -> global del row index). 1-to-1 `Mod`/`Mov`
/// transitions pair directly; block transitions contribute their anchor
/// pair; guarded dels never pair.
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
    let guards = guarded_dels(h, old_content, new_content, profile, &mut fn_cache);
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
            Row::Del(d) | Row::MovSrc(d) => Some((d.num.get(), i)),
            _ => None,
        }));
        m
    };
    let add_by_num: FxHashMap<u32, usize> = {
        let mut m = FxHashMap::with_capacity_and_hasher(h.rows.len(), Default::default());
        m.extend(h.rows.iter().enumerate().filter_map(|(i, r)| match r {
            Row::Add(a) | Row::MovTgt(a) => Some((a.num.get(), i)),
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

    // Reuse the thread-local scratch arena across hunks: every Vec (tokens,
    // metas, flattened token streams, both skyline matrices) keeps its
    // capacity instead of regrowing from zero per hunk.
    super::dp::with_tls_arena(|arena| {
        for seg in segments {
            let cluster = match seg {
                HunkSegment::Cluster(c) => c,
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
                            && !guards.contains(&gd)
                            && !h.rows[gd].is_moved()
                            && !h.rows[ga].is_moved()
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
                            && !guards.contains(&gd)
                            && !h.rows[gd].is_moved()
                            && !h.rows[ga].is_moved()
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
                            if all_valid && block_adds.len() == *k {
                                expand_blocks.insert(ga, block_adds);
                            }
                        }
                        oi += 1;
                        ni += k;
                    }
                    dp::DpOp::Collapse { anchor, k, .. } => {
                        if let (Some(gd), Some(ga)) = (global_del(oi + anchor), global_add(ni))
                            && !guards.contains(&gd)
                            && !h.rows[gd].is_moved()
                            && !h.rows[ga].is_moved()
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
                            let mut block_dels = Vec::with_capacity(*k);
                            let mut all_valid = true;
                            for offset in 0..*k {
                                if let Some(d) = global_del(oi + offset) {
                                    if guards.contains(&d) || h.rows[d].is_moved() {
                                        all_valid = false;
                                        break;
                                    }
                                    block_dels.push(d);
                                } else {
                                    all_valid = false;
                                    break;
                                }
                            }
                            if all_valid && block_dels.len() == *k {
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
