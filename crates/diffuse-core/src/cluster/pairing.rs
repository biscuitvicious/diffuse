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

use diffuse_syntax::{prelude::first_word, profile::LangProfile};

use crate::{
    collapse::PairingResults,
    parse::{Hunk, Row},
    scope::{fn_of_exists, is_tail_before_closing},
};

use super::{ChangeCluster, HunkSegment, dp, partition_hunk_segments};

type LegacyIndexMaps = (Vec<Vec<usize>>, FxHashMap<usize, Vec<usize>>, Vec<usize>);

/// The legacy run/del-index construction (identical for both engines —
/// passes 2–4 consume it).
fn legacy_index_maps(h: &Hunk) -> LegacyIndexMaps {
    let mut del_runs: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < h.rows.len() {
        if !matches!(h.rows[i], Row::Del(_) | Row::MovSrc { .. }) {
            i += 1;
            continue;
        }
        let mut run = vec![i];
        while i + 1 < h.rows.len() && matches!(h.rows[i + 1], Row::Del(_) | Row::MovSrc { .. }) {
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
        .filter(|(_, r)| matches!(r, Row::Del(_) | Row::MovSrc { .. }))
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

/// Per-del pairing guards for one hunk: hard vetoes plus soft survival
/// signals. Relocated lines (`is_moved`) stay hard-guarded — moves.rs owns
/// relocations. Verbatim twins surviving elsewhere are *not* vetoed: they
/// are reported as soft signals carrying multiplicity accounting, and the
/// DP penalizes (rather than forbids) pairing them, so high-similarity
/// modifications still form while slot-coincidence pairings fall back to
/// `Del` + `Add`.
#[derive(Default)]
pub(crate) struct DelGuards {
    /// Global row indices that must never pair.
    pub hard: FxHashSet<usize>,
    /// Old line numbers with surplus verbatim survival elsewhere
    /// (`count_new >= count_old` in the resolved scope): pairing one as
    /// `Mod` pays `Weights::survives_elsewhere_penalty`.
    pub soft: FxHashSet<u32>,
}

/// True when `trim` survives with multiplicity intact in the resolved
/// scope: the new count covers a non-zero old count, so this del instance
/// is redundant (a surviving twin) rather than transformed. A modified
/// instance always drops the new count below the old one and is exempt.
/// The non-zero old count also keeps synthetic contents (which may omit
/// the del text entirely) from ever penalizing. Scope mirrors the old
/// veto: the enclosing function when both bodies resolve, otherwise the
/// whole file; unavailable content never penalizes.
fn surplus_survival<'a>(
    trim: &str,
    del_fn: Option<&str>,
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&LangProfile>,
    old_counts: &mut Option<FxHashMap<&'a str, usize>>,
    new_counts: &mut Option<FxHashMap<&'a str, usize>>,
) -> bool {
    // Count occurrences of one trimmed line.
    fn count_in(lines: &[&str], trim: &str) -> usize {
        lines.iter().filter(|l| l.trim() == trim).count()
    }
    if let Some(dfn) = del_fn
        && let (Some(old_body), Some(new_body)) = (
            crate::scope::fn_body_lines(old_content, dfn, profile),
            crate::scope::fn_body_lines(new_content, dfn, profile),
        )
    {
        // Surplus needs a non-zero baseline: a twin can only remain
        // where one existed.
        return count_in(new_body, trim) >= count_in(old_body, trim).max(1);
    }
    match (old_content, new_content) {
        (Some(old), Some(new)) => {
            let old_map = old_counts.get_or_insert_with(|| {
                let mut m = FxHashMap::default();
                for l in old.iter() {
                    let s: &'a str = l;
                    *m.entry(s.trim()).or_insert(0) += 1;
                }
                m
            });
            let new_map = new_counts.get_or_insert_with(|| {
                let mut m = FxHashMap::default();
                for l in new.iter() {
                    let s: &'a str = l;
                    *m.entry(s.trim()).or_insert(0) += 1;
                }
                m
            });
            new_map.get(trim).copied().unwrap_or(0)
                >= old_map.get(trim).copied().unwrap_or(0).max(1)
        }
        _ => false,
    }
}

/// Verbatim pre-seeding: map unambiguous trimmed-equal del/add pairs in
/// one cluster before the DP walk, so corridor exclusion and global
/// traceback rerouting cannot strand true moves (pairing content-similar
/// rivals over verbatim counterparts rows away).
///
/// A pair seeds only when ALL hold:
/// - trimmed text non-blank, occurring exactly once among the cluster's del
///   candidates and exactly once among its add candidates (ambiguity —
///   duplicated lines — stays with the DP, which alone can weigh position
///   against content);
/// - different indents (same-spot verbatim pairs already resolve through the
///   Mov path or trivial-Mod; seeding targets relocations);
/// - neither endpoint pre-marked moved, hard-guarded, or fragment rejected
///   (same policy as the walk arms — moves.rs owns relocations);
/// - no same-indent head-matching rival add in the cluster: where same-slot
///   competition exists (e.g. an in-place edit beside a moved twin), the DP's
///   positional and content evidence decides, not a verbatim fiat.
///
/// Returns global add-index → global del-index pairs for direct insertion
/// into `pair_of`. Callers keep DP-walk exclusivity: Mod/Mov arms skip
/// seeded dels, and block formation skips seeded members (rows fall back
/// to 1-to-1/standalone rendering — never double-rendered). Assembly
/// needs no changes — it builds Mods from `pair_of` regardless of
/// provenance (heuristic aux may still attach unseeded neighbors around
/// seeded anchors).
/// Input bundle for [`verbatim_seeds`] (kept as a struct: nine fields
/// would trip `clippy::too_many_arguments`, and the call sites —
/// one production, five tests — read better named than positional).
pub(crate) struct VerbatimSeedInput<'a, 'm> {
    pub cluster: &'a ChangeCluster<'m>,
    pub h: &'a Hunk,
    pub del_by_num: &'a FxHashMap<u32, usize>,
    pub add_by_num: &'a FxHashMap<u32, usize>,
    pub old_content: Option<&'m [&'m str]>,
    pub new_content: Option<&'m [&'m str]>,
    pub profile: Option<&'m LangProfile>,
    pub fn_cache: &'a mut FnScopeCache<'m>,
    pub hard: &'a FxHashSet<usize>,
}

pub(crate) fn verbatim_seeds(input: VerbatimSeedInput<'_, '_>) -> FxHashMap<usize, usize> {
    let VerbatimSeedInput {
        cluster,
        h,
        del_by_num,
        add_by_num,
        old_content,
        new_content,
        profile,
        fn_cache,
        hard,
    } = input;
    // Head word mirroring the DP's first-word classification (comment
    // bodies classify by remainder), for the same-slot-rival rule below.
    fn head_of<'t>(text: &'t str, profile: Option<&LangProfile>) -> Option<&'t str> {
        let comment = profile.is_some_and(|p| p.is_comment_line(text));
        if comment {
            let body = text
                .trim()
                .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
                .trim_start();
            first_word(body)
        } else {
            first_word(text)
        }
    }
    fn indent_of(text: &str) -> usize {
        text.len() - text.trim_start().len()
    }
    // Candidate pools: trimmed text -> (global row index, indent, head).
    // Keys borrow cluster rows; only indices escape.
    struct Cand<'x> {
        gi: usize,
        indent: usize,
        head: Option<&'x str>,
    }
    let mut dels: FxHashMap<&str, Vec<Cand<'_>>> = FxHashMap::default();
    for r in &cluster.old_rows {
        if !matches!(r, Row::Del(_)) {
            continue;
        }
        let text = r.text();
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        let Some(num) = r.old_line() else {
            continue;
        };
        let Some(&gi) = del_by_num.get(&num) else {
            continue;
        };
        dels.entry(t).or_default().push(Cand {
            gi,
            indent: indent_of(text),
            head: head_of(text, profile),
        });
    }
    let mut adds: FxHashMap<&str, Vec<Cand<'_>>> = FxHashMap::default();
    for r in &cluster.new_rows {
        if !matches!(r, Row::Add(_)) {
            continue;
        }
        let text = r.text();
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        let Some(num) = r.new_line() else {
            continue;
        };
        let Some(&gi) = add_by_num.get(&num) else {
            continue;
        };
        adds.entry(t).or_default().push(Cand {
            gi,
            indent: indent_of(text),
            head: head_of(text, profile),
        });
    }
    let mut seeds = FxHashMap::default();
    for (t, ds) in &dels {
        if ds.len() != 1 {
            continue;
        }
        let Some(as_) = adds.get(t) else {
            continue;
        };
        if as_.len() != 1 {
            continue;
        }
        let d = &ds[0];
        let a = &as_[0];
        // Relocations seed at their own level: same-spot verbatim pairs
        // already resolve through Mov/trivial-Mod.
        if d.indent == a.indent {
            continue;
        }
        // Same-slot competition belongs to the DP: a same-indent
        // head-matching rival add means positional and content evidence
        // coincide there (e.g. an in-place edit beside a moved twin), and
        // only the DP weighs both. (The twin itself can never falsely
        // block: same indent as the del would already have excluded it
        // above.)
        let rivaled = adds
            .values()
            .flatten()
            .any(|o| o.indent == d.indent && o.head.is_some() && o.head == d.head);
        if rivaled {
            continue;
        }
        if hard.contains(&d.gi) || hard.contains(&a.gi) {
            continue;
        }
        // (Moved rows never reach here: the pools above admit only
        // `Row::Del`/`Row::Add`, so moves.rs ownership is structural.)
        if is_fragment_pair_rejected(d.gi, a.gi, h, old_content, new_content, profile, fn_cache) {
            continue;
        }
        seeds.insert(a.gi, d.gi);
    }
    seeds
}

pub(crate) fn classify_dels<'a>(
    h: &Hunk,
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&LangProfile>,
    fn_cache: &mut FnScopeCache<'a>,
) -> DelGuards {
    if !h
        .rows
        .iter()
        .any(|r| matches!(r, Row::Del(_) | Row::MovSrc { .. }))
    {
        return DelGuards::default();
    }

    let mut hard = FxHashSet::default();
    let mut soft = FxHashSet::default();
    let mut new_counts: Option<FxHashMap<&str, usize>> = None;
    let mut old_counts: Option<FxHashMap<&str, usize>> = None;
    let has_any_moved = h.rows.iter().any(|a| a.is_moved());

    let same_hunk_add_trimmed: FxHashSet<&str> = h
        .rows
        .iter()
        .filter_map(|a| match a {
            Row::Add(_) | Row::MovTgt { .. } => Some(a.text().trim()),
            _ => None,
        })
        .collect();

    let same_hunk_property_keys: FxHashSet<&str> = if let Some(p) = profile {
        h.rows
            .iter()
            .filter_map(|a| match a {
                Row::Add(_) | Row::MovTgt { .. } => p.extract_property_key(a.text()),
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
        if matches!(a, Row::Add(_) | Row::MovTgt { .. })
            && is_tail_before_closing(a.new_line(), false, old_content, new_content, profile)
        {
            has_any_add_tail = true;
            if let Some(afn) = memo_fn_of_str(fn_cache, false, new_content, a.new_line(), profile) {
                add_tail_fns.insert(afn);
            }
        }
    }

    for (ri, r) in h.rows.iter().enumerate() {
        // Del and moved-source rows carry old line numbers by construction;
        // everything else skips classification entirely.
        let Some(line_num) = (match r {
            Row::Del(d) | Row::MovSrc { line: d, .. } => Some(d.num),
            _ => None,
        }) else {
            continue;
        };
        if r.is_moved() {
            hard.insert(ri);
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
            hard.insert(ri);
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
            && surplus_survival(
                r_trim,
                del_fn,
                old_content,
                new_content,
                profile,
                &mut old_counts,
                &mut new_counts,
            )
        {
            soft.insert(line_num);
        }
    }
    DelGuards { hard, soft }
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
