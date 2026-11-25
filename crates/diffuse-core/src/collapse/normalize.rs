//! Post-walk normalization: converts identical Mod rows to Ctx when invariants
//! hold.

use rustc_hash::FxHashSet;

use crate::{
    merge::RunKind,
    parse::{CtxLine, GroupId, Hunk, PeerGroup, Row},
};

/// Swap an identical Mod row to Ctx in place, returning the original for
/// potential rollback. Returns `None` for non-Mod rows (never a
/// candidate, or already converted by the fast path below).
pub(crate) fn swap_mod_to_ctx(slot: &mut Row) -> Option<Row> {
    let (dold, anew, text, group) = match slot {
        Row::Mod {
            old_line,
            new_line,
            group,
            ..
        } => (old_line.num, new_line.num, new_line.text.clone(), *group),
        _ => return None,
    };
    Some(std::mem::replace(
        slot,
        Row::ctx(CtxLine::new(dold, anew, text)).with_group(group),
    ))
}

/// Post-walk normalization: convert no-visible-change Mod rows (byte-identical
/// old/new text, no relocated runs) back to Ctx. A converted row keeps its peer
/// group: the formation's slot is unchanged, only its kind is; the context line
/// is the anchor that holds the changes around it together.
///
/// Split trivial Ctx rows re-pair in the DP as identical Mods; without
/// this they render as Mod noise. Each conversion is kept only when the
/// converted stream still satisfies every collapsed-row invariant
/// (verified against the actual final order, so crossed pairs that
/// cannot legally be context stay Mod).
pub fn pass_normalize_identical_mods(
    h: &Hunk,
    res: Vec<Row>,
    pinned_new_lines: &FxHashSet<u32>,
) -> Vec<Row> {
    let candidates: Vec<usize> = res
        .iter()
        .enumerate()
        .filter(|(_, r)| match r {
            Row::Mod {
                old_line, new_line, ..
            } => {
                old_line.text == new_line.text
                    && !has_moved_runs(r)
                    && !pinned_new_lines.contains(&new_line.num)
            }
            _ => false,
        })
        .map(|(i, _)| i)
        .collect();
    if candidates.is_empty() {
        return res;
    }
    let mut out = res;
    let mut converted: Vec<(usize, Row)> = Vec::new();
    let mut originals: Vec<(usize, Row)> = Vec::with_capacity(candidates.len());
    for &i in &candidates {
        if let Some(original) = swap_mod_to_ctx(&mut out[i]) {
            originals.push((i, original));
        }
    }
    if normalized_invariants_hold(h, &out) {
        repair_peer_group_atomicity(&mut out);
        return out;
    }
    for (i, original) in originals {
        out[i] = original;
    }
    for i in candidates {
        let Some(original) = swap_mod_to_ctx(&mut out[i]) else {
            continue;
        };
        if normalized_invariants_hold(h, &out) {
            converted.push((i, original));
        } else {
            out[i] = original;
        }
    }
    while !converted.is_empty() && !normalized_invariants_hold(h, &out) {
        let (i, original) = converted.pop().expect("non-empty checked above");
        out[i] = original;
    }
    repair_peer_group_atomicity(&mut out);
    out
}

pub(crate) fn repair_peer_group_atomicity(rows: &mut [Row]) {
    use crate::parse::GroupId;
    let mut max_group_id = rows
        .iter()
        .filter_map(|r| r.group().map(|g| g.id.0))
        .max()
        .unwrap_or(0);

    let mut current_group: Option<GroupId> = None;
    let mut seen_groups: rustc_hash::FxHashSet<GroupId> = rustc_hash::FxHashSet::default();
    let mut group_remap: rustc_hash::FxHashMap<GroupId, GroupId> = rustc_hash::FxHashMap::default();

    for r in rows.iter_mut() {
        let gid = r.group().map(|g| g.id);
        if let Some(mut id) = gid {
            if let Some(mapped) = group_remap.get(&id) {
                id = *mapped;
            }
            if current_group == Some(id) {
                // Continuation of current group
            } else if !seen_groups.contains(&id) {
                // First time seeing this group
                seen_groups.insert(id);
                current_group = Some(id);
            } else {
                // Group was seen before, but was interrupted! Split into new group!
                max_group_id += 1;
                let new_id = GroupId(max_group_id);
                let orig_id = gid.unwrap();
                group_remap.insert(orig_id, new_id);
                seen_groups.insert(new_id);
                current_group = Some(new_id);
                id = new_id;
            }
            if let Some(mut g) = r.group()
                && let crate::parse::PeerGroup {
                    id: ref mut gid, ..
                } = g
            {
                *gid = id;
                r.set_group(Some(g));
            }
        } else {
            // Row has no group (e.g. Ctx) -> interrupts current group!
            current_group = None;
        }
    }
}

/// Re-derives every peer group from the rows that still carry it.
///
/// Slot alignment types groups before context normalization. Converting a
/// byte-identical `Mod` to `Ctx` removes that row from its group and can
/// interrupt the group, leaving fragments that quote a formation which no
/// longer exists: contiguous slots and `n`/`m` counts of rows that are no
/// longer members. This pass is the single reconstruction point. It splits
/// interrupted groups into contiguous fragments with fresh ids, renumbers
/// slots from 0, and recomputes `n`/`m` from the surviving members.
///
/// A group is justified by relocated text: `MovedSrc` and `MovedTgt` runs,
/// which need no `Mod` anchor because tokens can travel across `Del`/`Add`
/// boundaries. Everything else dissolves:
/// - a `1`-to-`1` formation inside one slot is merely a modification and
///   becomes a plain `Mod`;
/// - a `1`-to-`1` formation spread across two slots without relocated text has
///   nothing in common: with no shared token it can be neither a `Mod` nor a
///   `Ctx`, so it dissolves back into independent `Del` and `Add` rows;
/// - any other formation without relocated text loses its group.
///
/// Dissolved rows are then reset to the order the surrounding stream gives
/// them (`Del` rows before `Add` rows within each ungrouped run), so the
/// ungrouped stream is topologically legal and the normalizer can process it.
pub(crate) fn recalibrate_peer_groups(rows: &mut [Row]) {
    let mut next_id = rows
        .iter()
        .filter_map(|r| r.group().map(|g| g.id.0))
        .max()
        .unwrap_or(0);
    let mut seen: FxHashSet<GroupId> = FxHashSet::default();
    let mut dissolved = false;

    let mut i = 0;
    while i < rows.len() {
        let Some(gid) = rows[i].group().map(|g| g.id) else {
            i += 1;
            continue;
        };
        let start = i;
        while i < rows.len() && rows[i].group().map(|g| g.id) == Some(gid) {
            i += 1;
        }
        // The first fragment keeps the group's id; later fragments are new
        // formations and must not share it.
        let id = if seen.insert(gid) {
            gid
        } else {
            next_id += 1;
            GroupId(next_id)
        };
        let (n, m, has_moved) = fragment_formation(&rows[start..i]);
        // A one-to-one formation confined to a single row is just a `Mod`;
        // only the same formation spread over two slots can witness a
        // relocation.
        let single_row = i - start == 1;
        if has_moved && !(n == 1 && m == 1 && single_row) {
            for (slot, r) in rows[start..i].iter_mut().enumerate() {
                r.set_group(Some(PeerGroup { id, slot, n, m }));
            }
        } else {
            for r in &mut rows[start..i] {
                r.set_group(None);
            }
            dissolved = true;
        }
    }

    if dissolved {
        reorder_ungrouped_del_add_runs(rows);
    }
}

/// Reports whether a row carries relocated text (`MovedSrc`/`MovedTgt` runs) or
/// is itself a whole-row move (`MovSrc`/`MovTgt`). Such rows are what justify a
/// formation; they need no `Mod` anchor because tokens can travel across
/// `Del`/`Add` boundaries.
fn has_moved_runs(r: &Row) -> bool {
    r.is_moved()
        || r.cached_runs()
            .iter()
            .any(|(kind, _)| matches!(kind, RunKind::MovedSrc | RunKind::MovedTgt))
}

/// Counts the sides a contiguous group fragment currently represents and
/// reports whether it still carries relocated text.
fn fragment_formation(members: &[Row]) -> (usize, usize, bool) {
    let mut n = 0usize;
    let mut m = 0usize;
    let mut has_moved = false;
    for r in members {
        if r.old_line().is_some() {
            n += 1;
        }
        if r.new_line().is_some() {
            m += 1;
        }
        has_moved |= has_moved_runs(r);
    }
    (n, m, has_moved)
}

/// Restores the canonical `Del`-before-`Add` order across each run of
/// ungrouped `Del`/`Add` rows. Dissolution can leave an addition ahead of the
/// deletion it was slot-paired with; once the pair is no longer a formation,
/// the order of the surrounding stream applies again. The run is maximal, so a
/// dissolved fragment is folded into the ungrouped rows on either side of it
/// rather than reordered in isolation: the surviving deletion joins the
/// deletions before it and the surviving addition joins the additions after it,
/// yielding one contiguous deletion run followed by one contiguous addition
/// run. The sort is stable, so each side keeps its own line numbers ascending,
/// and runs end at `Mod`, `Ctx` and grouped rows, leaving live formations
/// untouched.
fn reorder_ungrouped_del_add_runs(rows: &mut [Row]) {
    let mut run_start = 0;
    for i in 0..=rows.len() {
        if rows.get(i).is_some_and(is_ungrouped_del_add) {
            continue;
        }
        rows[run_start..i].sort_by_key(|r| usize::from(matches!(r, Row::Add { .. })));
        run_start = i + 1;
    }
}

fn is_ungrouped_del_add(r: &Row) -> bool {
    matches!(
        r,
        Row::Del { group: None, .. } | Row::Add { group: None, .. }
    )
}

fn check_peer_token_conservation(rows: &[Row]) -> Result<(), String> {
    let mut group_src: rustc_hash::FxHashMap<GroupId, Vec<&str>> = rustc_hash::FxHashMap::default();
    let mut group_tgt: rustc_hash::FxHashMap<GroupId, Vec<&str>> = rustc_hash::FxHashMap::default();
    for r in rows {
        if let Some(gid) = r.group().map(|g| g.id) {
            for (kind, text) in r.cached_runs() {
                match kind {
                    RunKind::MovedSrc => {
                        group_src.entry(gid).or_default().push(text.as_str());
                    }
                    RunKind::MovedTgt => {
                        group_tgt.entry(gid).or_default().push(text.as_str());
                    }
                    _ => {}
                }
            }
        }
    }
    for (gid, mut src) in group_src {
        let mut tgt = group_tgt.remove(&gid).unwrap_or_default();
        src.sort_unstable();
        tgt.sort_unstable();
        if src != tgt {
            return Err(format!("Group {gid:?} token conservation mismatch"));
        }
    }
    for (gid, tgt) in group_tgt {
        if !tgt.is_empty() {
            return Err(format!("Group {gid:?} token conservation mismatch"));
        }
    }
    Ok(())
}

/// Every collapsed-row invariant a normalized stream must satisfy (the
/// same gates `collapse_hunk_rows` enforces before returning).
fn normalized_invariants_hold(h: &Hunk, out: &[Row]) -> bool {
    let mut out_copy = out.to_vec();
    repair_peer_group_atomicity(&mut out_copy);
    super::invariants::check_context_old_line_monotonicity(&out_copy).is_ok()
        && super::invariants::check_no_orphaned_continuation_dels(h, &out_copy).is_ok()
        && super::invariants::check_del_add_topological_order(h, &out_copy).is_ok()
        && check_peer_token_conservation(&out_copy).is_ok()
}
