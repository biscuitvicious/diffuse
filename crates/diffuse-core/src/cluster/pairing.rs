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

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    collapse::PairingResults,
    parse::{Hunk, Row},
    scope::{fn_of, fn_of_exists, is_tail_before_closing},
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

/// The dels the legacy `pass_pairing` refuses to pair (its top-of-loop
/// guards), computed identically — the DP's pairs over these dels are
/// dropped so both engines leave the same rows standalone.
fn guarded_dels(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> FxHashSet<usize> {
    let mut skip = FxHashSet::default();
    let mut new_trimmed_set: Option<FxHashSet<&str>> = None;
    let has_any_moved = h.rows.iter().any(|a| a.is_moved());

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
            && h.rows.iter().any(|a| {
                a.is_moved() && (a.old_line() == r.old_line() || a.text().trim() == r_trim)
            });
        if was_moved {
            skip.insert(ri);
            continue;
        }
        let del_is_tail =
            is_tail_before_closing(r.old_line(), true, old_content, new_content, profile);
        let tail_candidate = del_is_tail
            && h.rows.iter().any(|a| {
                matches!(a, Row::Add(_) | Row::MovTgt(_))
                    && is_tail_before_closing(
                        a.new_line(),
                        false,
                        old_content,
                        new_content,
                        profile,
                    )
                    && (fn_of(old_content, r.old_line(), profile).is_none()
                        || fn_of(old_content, r.old_line(), profile)
                            == fn_of(new_content, a.new_line(), profile))
            });
        let has_same_hunk_add = h
            .rows
            .iter()
            .any(|a| matches!(a, Row::Add(_) | Row::MovTgt(_)) && a.text().trim() == r_trim);
        if !r_trim.is_empty() && !tail_candidate && !has_same_hunk_add {
            let in_new = if let Some(nc) = new_content {
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

fn is_fragment_pair_rejected(
    gd: usize,
    ga: usize,
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> bool {
    let r = &h.rows[gd];
    let a = &h.rows[ga];

    let del_fn = fn_of(old_content, r.old_line(), profile);
    let add_fn = fn_of(new_content, a.new_line(), profile);
    if let (Some(dfn), Some(afn)) = (&del_fn, &add_fn)
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
pub fn dp_pairing(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> PairingResults {
    let (del_runs, run_of, del_ids) = legacy_index_maps(h);
    let guards = guarded_dels(h, old_content, new_content, profile);

    // Global row indices by line number (unique per hunk side).
    let del_by_num: FxHashMap<u32, usize> = h
        .rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| match r {
            Row::Del(d) | Row::MovSrc(d) => Some((d.num.get(), i)),
            _ => None,
        })
        .collect();
    let add_by_num: FxHashMap<u32, usize> = h
        .rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| match r {
            Row::Add(a) | Row::MovTgt(a) => Some((a.num.get(), i)),
            _ => None,
        })
        .collect();

    let mut pair_of: FxHashMap<usize, usize> = FxHashMap::default();
    let mut arena = super::dp::DpScratchArena::default();

    for seg in partition_hunk_segments(h, old_content, new_content, profile) {
        let cluster = match seg {
            HunkSegment::Cluster(c) => c,
            HunkSegment::Context(_) => continue,
        };
        let sol = super::dp::solve_cluster_with_arena(
            &cluster,
            profile,
            &super::dp::Weights::default(),
            &mut arena,
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
                        && !is_fragment_pair_rejected(gd, ga, h, old_content, new_content, profile)
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
                        && !is_fragment_pair_rejected(gd, ga, h, old_content, new_content, profile)
                    {
                        pair_of.insert(ga, gd);
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
                        && !is_fragment_pair_rejected(gd, ga, h, old_content, new_content, profile)
                    {
                        pair_of.insert(ga, gd);
                    }
                    oi += k;
                    ni += 1;
                }
            }
        }
    }

    PairingResults {
        pair_of,
        del_runs,
        run_of,
        del_ids,
    }
}
