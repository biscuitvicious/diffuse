//! Post-walk normalization: converts identical Mod rows to Ctx when invariants
//! hold.

use diffuse_syntax::profile::LangProfile;
use rustc_hash::FxHashSet;

use crate::parse::{CtxLine, Hunk, Row, RowGroup};

/// Swap an identical Mod row to Ctx in place, returning the original for
/// potential rollback. Returns `None` for non-Mod rows (never a
/// candidate, or already converted by the fast path below).
pub(crate) fn swap_mod_to_ctx(slot: &mut Row) -> Option<Row> {
    let (dold, anew, text) = match slot {
        Row::Mod {
            old_line, new_line, ..
        } => (old_line.num, new_line.num, new_line.text.clone()),
        _ => return None,
    };
    Some(std::mem::replace(
        slot,
        Row::Ctx(CtxLine::new(dold, anew, text)),
    ))
}

/// Post-walk normalization: convert no-visible-change Mod rows
/// (byte-identical old/new text, no group attachments) back to Ctx.
///
/// Split trivial Ctx rows re-pair in the DP as identical Mods; without
/// this they render as Mod noise. Each conversion is kept only when the
/// converted stream still satisfies every collapsed-row invariant
/// (verified against the actual final order, so crossed pairs that
/// cannot legally be context stay Mod).
pub fn pass_normalize_identical_mods(
    h: &Hunk,
    res: Vec<Row>,
    profile: Option<&LangProfile>,
    block_anchor_news: &FxHashSet<u32>,
) -> Vec<Row> {
    let candidates: Vec<usize> = res
        .iter()
        .enumerate()
        .filter(|(_, r)| match r {
            Row::Mod {
                old_line, new_line, ..
            } => {
                let has_moves = r.cached_runs().iter().any(|(kind, _)| {
                    matches!(
                        kind,
                        crate::merge::RunKind::MovedSrc | crate::merge::RunKind::MovedTgt
                    )
                });
                old_line.text == new_line.text
                    && !has_moves
                    && !matches!(r.group(), Some(RowGroup::Anchor))
                    && !block_anchor_news.contains(&new_line.num)
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
    if normalized_invariants_hold(h, &out, profile) {
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
        if normalized_invariants_hold(h, &out, profile) {
            converted.push((i, original));
        } else {
            out[i] = original;
        }
    }
    while !converted.is_empty() && !normalized_invariants_hold(h, &out, profile) {
        let (i, original) = converted.pop().expect("non-empty checked above");
        out[i] = original;
    }
    repair_peer_group_atomicity(&mut out);
    out
}

pub(crate) fn repair_peer_group_atomicity(rows: &mut [Row]) {
    use crate::parse::GroupId;
    let mut max_group_id = 0u32;
    for r in rows.iter() {
        if let Some(g) = r.group().and_then(|g| g.peer_id())
            && *g > max_group_id
        {
            max_group_id = *g;
        }
    }

    let mut current_group: Option<GroupId> = None;
    let mut seen_groups: rustc_hash::FxHashSet<GroupId> = rustc_hash::FxHashSet::default();
    let mut group_remap: rustc_hash::FxHashMap<GroupId, GroupId> = rustc_hash::FxHashMap::default();

    for r in rows.iter_mut() {
        let gid = r.group().and_then(|g| g.peer_id());
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
                let orig_id = r.group().and_then(|g| g.peer_id()).unwrap();
                group_remap.insert(orig_id, new_id);
                seen_groups.insert(new_id);
                current_group = Some(new_id);
                id = new_id;
            }
            if let Some(mut g) = r.group()
                && let crate::parse::RowGroup::Peer {
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

fn check_peer_token_conservation(rows: &[Row]) -> Result<(), String> {
    let mut group_src: rustc_hash::FxHashMap<crate::parse::GroupId, Vec<&str>> =
        rustc_hash::FxHashMap::default();
    let mut group_tgt: rustc_hash::FxHashMap<crate::parse::GroupId, Vec<&str>> =
        rustc_hash::FxHashMap::default();
    for r in rows {
        if let Some(gid) = r.group().and_then(|g| g.peer_id()) {
            for (kind, text) in r.cached_runs() {
                match kind {
                    crate::merge::RunKind::MovedSrc => {
                        group_src.entry(gid).or_default().push(text.as_str());
                    }
                    crate::merge::RunKind::MovedTgt => {
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
fn normalized_invariants_hold(h: &Hunk, out: &[Row], profile: Option<&LangProfile>) -> bool {
    let mut out_copy = out.to_vec();
    repair_peer_group_atomicity(&mut out_copy);
    super::invariants::check_context_old_line_monotonicity(&out_copy).is_ok()
        && super::invariants::check_no_orphaned_continuation_dels(h, &out_copy).is_ok()
        && super::invariants::check_del_add_topological_order(h, &out_copy).is_ok()
        && super::invariants::assert_collapsed_reflow_invariants(&out_copy, profile).is_ok()
        && check_peer_token_conservation(&out_copy).is_ok()
}
