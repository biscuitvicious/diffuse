//! Unified M-to-N peer reflow pipeline.
//!
//! Single-path cluster alignment (`lattice`), formation segmentation
//! (`reflow`), closed-economy window typing (`slot`), and normalization.

use diffuse_syntax::profile::LangProfile;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    cluster::dp::cluster_budget,
    collapse::{
        formation::Formation,
        invariants::{
            check_context_old_line_monotonicity, check_del_add_topological_order,
            check_no_orphaned_continuation_dels,
        },
        lattice::solve_cluster_path,
        normalize::{pass_normalize_identical_mods, recalibrate_peer_groups},
        reflow::{Segment, segment_path},
        slot::{PeerGroupRows, compute_slot_spans, has_salient_moved_link, type_window},
    },
    lcs::word_spans_and_runs_with_profile,
    merge::{RunKind, is_wholesale_replacement},
    parse::{AddLine, DelLine, GroupId, Hunk, PeerGroup, Row, Spans},
    shift::pass_shift_hunk,
};

/// Validates mathematical invariants across peer groups in a collapsed row
/// sequence:
/// 1. Closed-economy conservation: sum(MovedSrc chars) == sum(MovedTgt chars).
/// 2. Slot indexing: consecutive slots 0..L within each peer group.
/// 3. Monotonic line coordinates: old lines strictly increasing, new lines
///    strictly increasing.
pub fn assert_peer_group_invariants(rows: &[Row]) {
    let mut group_slots: FxHashMap<GroupId, Vec<&Row>> = FxHashMap::default();

    for r in rows {
        if let Some(PeerGroup { id, .. }) = r.group() {
            group_slots.entry(id).or_default().push(r);
        }
    }

    for (&id, members) in &group_slots {
        let mut expected_slot = 0;
        let mut last_old: Option<u32> = None;
        let mut last_new: Option<u32> = None;
        let mut moved_src_tokens: Vec<&str> = Vec::new();
        let mut moved_tgt_tokens: Vec<&str> = Vec::new();

        for r in members {
            if let Some(PeerGroup { slot, .. }) = r.group() {
                assert!(
                    slot >= expected_slot,
                    "Group {id} slot out of order: expected >= {expected_slot}, got {slot}"
                );
                expected_slot = slot + 1;
            }

            if let Some(old) = r.old_line() {
                if let Some(prev) = last_old {
                    assert!(
                        old > prev,
                        "Group {id} old line not strictly monotonic: {prev} -> {old}"
                    );
                }
                last_old = Some(old);
            }

            if let Some(new) = r.new_line() {
                if let Some(prev) = last_new {
                    assert!(
                        new > prev,
                        "Group {id} new line not strictly monotonic: {prev} -> {new}"
                    );
                }
                last_new = Some(new);
            }

            for (kind, text) in r.cached_runs() {
                match kind {
                    RunKind::MovedSrc => moved_src_tokens.push(text.as_str()),
                    RunKind::MovedTgt => moved_tgt_tokens.push(text.as_str()),
                    _ => {}
                }
            }
        }

        moved_src_tokens.sort_unstable();
        moved_tgt_tokens.sort_unstable();

        assert_eq!(
            moved_src_tokens, moved_tgt_tokens,
            "Group {id} token conservation mismatch: MovedSrc={:?} vs MovedTgt={:?}",
            moved_src_tokens, moved_tgt_tokens
        );
    }
}

/// Fail-safe: every slot's runs must project back to the verbatim slot
/// texts. Compound slots can mux words across lines (reordered garbage);
/// never synthesize text — the caller dissolves a failing group to the
/// original Del/Add rows instead (partial fallback would orphan moved
/// partners and break conservation).
fn slots_project_to_text(group_rows: &PeerGroupRows) -> bool {
    group_rows.slots.iter().all(|s| {
        let old_proj: String = s
            .runs
            .iter()
            .filter(|(k, _)| !k.is_add_like())
            .map(|(_, t)| t.as_str())
            .collect();
        let new_proj: String = s
            .runs
            .iter()
            .filter(|(k, _)| !k.is_del_like())
            .map(|(_, t)| t.as_str())
            .collect();
        match (&s.old_text, &s.new_text) {
            (Some(o), Some(n)) => old_proj == *o && new_proj == *n,
            (Some(o), None) => old_proj == *o,
            (None, Some(n)) => new_proj == *n,
            (None, None) => true,
        }
    })
}

/// Decides the group metadata for slots of a factored peer group. `None`
/// strips grouping: 1×1 pairs and uniform 1-to-1 Mod runs render as plain
/// rows, and groups with no strong token link (neither a `Mod` seed nor a
/// salient moved-token relationship) dissolve to plain Del/Add rather than
/// implying a reflow that slot alignment found no evidence for.
fn peer_group_meta(
    n: usize,
    m: usize,
    has_strong_link: bool,
    is_all_1to1_mods: bool,
    group: PeerGroup,
) -> Option<PeerGroup> {
    if (n == 1 && m == 1) || is_all_1to1_mods || !has_strong_link {
        None
    } else {
        Some(group)
    }
}

fn topological_sort_cluster(rows: Vec<Row>) -> Vec<Row> {
    enum Item {
        PeerGroup(Vec<Row>),
        Ungrouped(Row),
    }

    let mut rows_iter = rows.into_iter().peekable();
    let mut items: Vec<Item> = Vec::new();
    while let Some(row) = rows_iter.next() {
        if let Some(gid) = row.group().map(|g| g.id) {
            let mut peer_rows = vec![row];
            while rows_iter.peek().and_then(|r| r.group().map(|g| g.id)) == Some(gid) {
                peer_rows.push(rows_iter.next().unwrap());
            }
            items.push(Item::PeerGroup(peer_rows));
        } else {
            items.push(Item::Ungrouped(row));
        }
    }

    let mut out = Vec::new();
    let mut pending_dels = Vec::new();
    let mut pending_adds = Vec::new();
    let mut pending_others = Vec::new();

    for it in items {
        match it {
            Item::PeerGroup(group_rows) => {
                out.append(&mut pending_dels);
                out.append(&mut pending_adds);
                out.append(&mut pending_others);
                out.extend(group_rows);
            }
            Item::Ungrouped(row) => match row {
                Row::Mod { .. } => {
                    out.append(&mut pending_dels);
                    out.append(&mut pending_adds);
                    out.append(&mut pending_others);
                    out.push(row);
                }
                Row::Del { .. } | Row::MovSrc { .. } => {
                    pending_dels.push(row);
                }
                Row::Add { .. } | Row::MovTgt { .. } => {
                    pending_adds.push(row);
                }
                _ => {
                    pending_others.push(row);
                }
            },
        }
    }
    out.append(&mut pending_dels);
    out.append(&mut pending_adds);
    out.append(&mut pending_others);
    out
}

/// Executes the unified peer reflow pipeline over a diff hunk.
pub fn peer_reflow_pipeline(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Vec<Row> {
    let has_dels = h
        .rows
        .iter()
        .any(|r| matches!(r, Row::Del { .. } | Row::MovSrc { .. }));
    let has_adds = h
        .rows
        .iter()
        .any(|r| matches!(r, Row::Add { .. } | Row::MovTgt { .. }));

    let h_split = if has_dels && has_adds {
        let rows = &h.rows;
        let first_change = rows
            .iter()
            .position(|r| !matches!(r, Row::Ctx { .. } | Row::Sep(_)));
        let last_change = rows
            .iter()
            .rposition(|r| !matches!(r, Row::Ctx { .. } | Row::Sep(_)));
        if let (Some(first), Some(last)) = (first_change, last_change) {
            let mut out = Vec::with_capacity(rows.len());
            for (i, r) in rows.iter().enumerate() {
                if i > first && i < last {
                    match r {
                        Row::Ctx { line: c, .. }
                            if crate::ctx_split::should_split_ctx(&c.text, &c.text, profile) =>
                        {
                            out.push(Row::del(DelLine::new(c.old_pos, c.text.clone())));
                            out.push(Row::add(AddLine::new(c.new_pos, c.text.clone())));
                        }
                        other => out.push(other.clone()),
                    }
                } else {
                    out.push(r.clone());
                }
            }
            let mut h_s = h.clone();
            h_s.rows = out;
            h_s
        } else {
            h.clone()
        }
    } else {
        h.clone()
    };

    let h_norm = pass_shift_hunk(&h_split, profile);
    let h = &h_norm;

    let mut out: Vec<Row> = Vec::new();
    let mut i = 0;
    let mut group_id_counter = 1u32;

    while i < h.rows.len() {
        match &h.rows[i] {
            Row::Ctx { .. } | Row::Sep(_) => {
                out.push(h.rows[i].clone());
                i += 1;
            }
            _ => {
                let cluster_start = i;
                while i < h.rows.len() && !matches!(&h.rows[i], Row::Ctx { .. } | Row::Sep(_)) {
                    i += 1;
                }
                let cluster_rows = &h.rows[cluster_start..i];

                let mut old_rows = Vec::new();
                let mut new_rows = Vec::new();
                for r in cluster_rows {
                    if matches!(r, Row::Del { .. } | Row::MovSrc { .. }) {
                        old_rows.push(r.clone());
                    } else if matches!(r, Row::Add { .. } | Row::MovTgt { .. }) {
                        new_rows.push(r.clone());
                    }
                }

                if old_rows.is_empty() {
                    for r in new_rows {
                        out.push(r);
                    }
                } else if new_rows.is_empty() {
                    for r in old_rows {
                        out.push(r);
                    }
                } else if old_rows.len() * new_rows.len() > cluster_budget() {
                    out.extend(old_rows);
                    out.extend(new_rows);
                } else {
                    let mut cluster_out = Vec::new();
                    let path =
                        solve_cluster_path(&old_rows, &new_rows, old_content, new_content, profile);
                    let segments = segment_path(&path, &old_rows, &new_rows, profile);
                    let ctx = WindowTypingCtx {
                        old_content,
                        new_content,
                        profile,
                    };

                    // Each segment is emitted or typed on its own, with the
                    // correspondences the path already chose. Plain single rows
                    // bypass window typing completely; multi-row windows go
                    // through `type_window`. Old and new row indices advance
                    // monotonically and independently of 1D `DropOld`/`DropNew`
                    // tie-break order in mixed inter-`Diag` gaps.
                    let diag_pairs = path.diag_pairs();
                    let mut diag_idx = 0usize;
                    let (mut oi, mut ni) = (0usize, 0usize);
                    for segment in segments {
                        match segment {
                            Segment::Del { old } => {
                                debug_assert_eq!(oi, old);
                                oi += 1;
                                cluster_out.push(old_rows[old].clone());
                            }
                            Segment::Add { new } => {
                                debug_assert_eq!(ni, new);
                                ni += 1;
                                cluster_out.push(new_rows[new].clone());
                            }
                            Segment::Pair { old, new } => {
                                debug_assert_eq!(diag_pairs[diag_idx], (old, new));
                                debug_assert_eq!((oi, ni), (old, new));
                                diag_idx += 1;
                                oi += 1;
                                ni += 1;
                                emit_plain_pair(
                                    &old_rows[old],
                                    &new_rows[new],
                                    profile,
                                    &mut cluster_out,
                                );
                            }
                            Segment::Window { old, new } => {
                                debug_assert_eq!(oi, old.start);
                                debug_assert_eq!(ni, new.start);
                                let mut pairs = Vec::new();
                                while diag_idx < diag_pairs.len() {
                                    let (o, n) = diag_pairs[diag_idx];
                                    if o < old.end && n < new.end {
                                        debug_assert!(old.contains(&o) && new.contains(&n));
                                        pairs.push((o - old.start, n - new.start));
                                        diag_idx += 1;
                                    } else {
                                        break;
                                    }
                                }
                                oi = old.end;
                                ni = new.end;
                                let formation = Formation {
                                    id: GroupId(group_id_counter),
                                    old_range: old,
                                    new_range: new,
                                };
                                group_id_counter += 1;
                                emit_window_segment(
                                    &formation,
                                    &old_rows,
                                    &new_rows,
                                    &pairs,
                                    &ctx,
                                    &mut cluster_out,
                                );
                            }
                        }
                    }
                    debug_assert_eq!(
                        (oi, ni, diag_idx),
                        (old_rows.len(), new_rows.len(), diag_pairs.len())
                    );
                    out.extend(topological_sort_cluster(cluster_out));
                }
            }
        }
    }

    // Group metadata is typed before normalization, which both removes members
    // and dissolves formations that no longer witness a relocation. Rebuild
    // first so normalization is only offered whole formations, then rebuild
    // again to re-derive whatever its conversions left behind.
    recalibrate_peer_groups(&mut out);
    let mut res = pass_normalize_identical_mods(h, out, &FxHashSet::default());
    recalibrate_peer_groups(&mut res);

    crate::ensure!(
        res.iter().all(|r| match r {
            Row::Mod {
                old_line, new_line, ..
            } => old_line.num > 0 && new_line.num > 0,
            Row::Ctx { line: c, .. } => c.old_pos > 0 && c.new_pos > 0,
            Row::Add { line, .. } => line.num > 0,
            Row::Del { line, .. } => line.num > 0,
            Row::MovSrc { line, .. } => line.num > 0,
            Row::MovTgt { line, .. } => line.num > 0,
            Row::Sep(_) => true,
        }),
        "All collapsed rows must have valid, non-zero line coordinates"
    );

    crate::invariant!(
        check_no_orphaned_continuation_dels(h, &res).is_ok(),
        "{}",
        check_no_orphaned_continuation_dels(h, &res)
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

    crate::invariant!(
        check_context_old_line_monotonicity(&res).is_ok(),
        "{}",
        check_context_old_line_monotonicity(&res)
            .err()
            .unwrap_or_default()
    );

    crate::invariant!(
        res.iter()
            .all(|r| !matches!(r, Row::Mod { .. }) || !is_wholesale_replacement(r)),
        "Collapsed rows must never contain wholesale replacement Mod rows: {:?}",
        res.iter()
            .find(|r| matches!(r, Row::Mod { .. }) && is_wholesale_replacement(r))
    );

    assert_peer_group_invariants(&res);

    res
}

struct WindowTypingCtx<'a> {
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&'a LangProfile>,
}

fn emit_plain_pair(u: &Row, v: &Row, profile: Option<&LangProfile>, cluster_out: &mut Vec<Row>) {
    let (Some(u_num), Some(v_num)) = (u.old_line(), v.new_line()) else {
        cluster_out.push(u.clone());
        cluster_out.push(v.clone());
        return;
    };
    if u.text() == v.text() {
        cluster_out.push(Row::Mod {
            old_line: DelLine::new(u_num, u.text()),
            new_line: AddLine::new(v_num, v.text()),
            spans: Spans::default(),
            cached_runs: vec![(RunKind::Ctx, u.text().to_string())],
            group: None,
        });
        return;
    }
    let (_, runs) = word_spans_and_runs_with_profile(u.text(), v.text(), profile);
    let spans = compute_slot_spans(&runs);
    cluster_out.push(Row::Mod {
        old_line: DelLine::new(u_num, u.text()),
        new_line: AddLine::new(v_num, v.text()),
        spans,
        cached_runs: runs,
        group: None,
    });
}

fn emit_window_segment(
    formation: &Formation,
    old_rows: &[Row],
    new_rows: &[Row],
    pairs: &[(usize, usize)],
    ctx: &WindowTypingCtx<'_>,
    cluster_out: &mut Vec<Row>,
) {
    let old_slice = &old_rows[formation.old_range.clone()];
    let new_slice = &new_rows[formation.new_range.clone()];
    let mut group_rows = type_window(
        formation.id,
        old_slice,
        new_slice,
        pairs,
        ctx.old_content,
        ctx.new_content,
        ctx.profile,
    );
    group_rows.repair_unmatched_moves();
    group_rows.assert_token_conservation();

    if !slots_project_to_text(&group_rows) {
        cluster_out.extend(old_slice.iter().cloned());
        cluster_out.extend(new_slice.iter().cloned());
        return;
    }

    let n = formation.old_count();
    let m = formation.new_count();
    let has_mod = group_rows
        .slots
        .iter()
        .any(|s| s.old_line.is_some() && s.new_line.is_some());
    let is_all_1to1_mods = group_rows.slots.len() == n
        && group_rows.slots.len() == m
        && group_rows
            .slots
            .iter()
            .all(|s| s.old_line.is_some() && s.new_line.is_some())
        && group_rows.slots.iter().all(|s| {
            s.runs
                .iter()
                .all(|(k, _)| *k != RunKind::MovedSrc && *k != RunKind::MovedTgt)
        });
    let has_strong_link = has_mod || has_salient_moved_link(&group_rows, ctx.profile);

    for slot in group_rows.slots {
        let group_meta = peer_group_meta(n, m, has_strong_link, is_all_1to1_mods, slot.group);
        match (slot.old_line, slot.new_line) {
            (Some(u), Some(v)) => {
                let old_text = slot.old_text.unwrap_or_default();
                let new_text = slot.new_text.unwrap_or_default();
                cluster_out.push(Row::Mod {
                    old_line: DelLine::new(u, old_text),
                    new_line: AddLine::new(v, new_text),
                    spans: slot.spans,
                    cached_runs: slot.runs,
                    group: group_meta,
                });
            }
            (None, Some(v)) => {
                let new_text = slot.new_text.unwrap_or_default();
                if slot.is_moved_tgt {
                    cluster_out.push(Row::MovTgt {
                        line: AddLine::new(v, new_text),
                        spans: slot.spans,
                        cached_runs: slot.runs,
                        group: group_meta,
                    });
                } else if n == 0 || group_meta.is_none() {
                    cluster_out.push(Row::add(AddLine::new(v, new_text)));
                } else {
                    cluster_out.push(Row::Add {
                        line: AddLine::new(v, new_text),
                        spans: slot.spans,
                        cached_runs: slot.runs,
                        group: group_meta,
                    });
                }
            }
            (Some(u), None) => {
                let old_text = slot.old_text.unwrap_or_default();
                if slot.is_moved_src {
                    cluster_out.push(Row::MovSrc {
                        line: DelLine::new(u, old_text),
                        spans: slot.spans,
                        cached_runs: slot.runs,
                        group: group_meta,
                    });
                } else if m == 0 || group_meta.is_none() {
                    cluster_out.push(Row::del(DelLine::new(u, old_text)));
                } else {
                    cluster_out.push(Row::Del {
                        line: DelLine::new(u, old_text),
                        spans: slot.spans,
                        cached_runs: slot.runs,
                        group: group_meta,
                    });
                }
            }
            (None, None) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{CtxLine, RowKind};

    fn make_ctx(old: u32, new: u32, text: &str) -> Row {
        Row::ctx(CtxLine::new(old, new, text))
    }

    fn make_del(num: u32, text: &str) -> Row {
        Row::del(DelLine::new(num, text))
    }

    fn make_add(num: u32, text: &str) -> Row {
        Row::add(AddLine::new(num, text))
    }

    #[test]
    fn test_peer_pipeline_pure_adds() {
        let hunk = Hunk {
            old_start: 1,
            old_count: 2,
            new_start: 1,
            new_count: 4,
            rows: vec![
                make_ctx(1, 1, "fn hello() {"),
                make_add(2, "    println!(\"added 1\");"),
                make_add(3, "    println!(\"added 2\");"),
                make_ctx(2, 4, "}"),
            ],
            ..Default::default()
        };

        let peer = peer_reflow_pipeline(&hunk, None, None, None);

        assert_eq!(peer.len(), 4);
        assert!(matches!(peer[0], Row::Ctx { .. }));
        assert_eq!(peer[1].kind(), RowKind::Add);
        assert_eq!(peer[1].new_line(), Some(2));
        assert_eq!(peer[2].kind(), RowKind::Add);
        assert_eq!(peer[2].new_line(), Some(3));
        assert!(matches!(peer[3], Row::Ctx { .. }));
    }

    #[test]
    fn test_peer_pipeline_token_hunk() {
        let hunk = Hunk {
            old_start: 680,
            old_count: 31,
            new_start: 719,
            new_count: 6,
            rows: vec![
                make_ctx(
                    680,
                    719,
                    "pub fn compute_token_containers(tokens: &[&str]) -> Vec<Option<ContainerKind>> {",
                ),
                make_del(
                    681,
                    "    let mut result = Vec::with_capacity(tokens.len());",
                ),
                make_del(
                    682,
                    "    let mut stack: diffuse_algo::SmallStack<ContainerKind, 8> = diffuse_algo::SmallStack::new();",
                ),
                make_del(683, ""),
                make_del(684, "    for &t in tokens {"),
                make_del(685, "        match t {"),
                make_del(686, "            \"`\" => {"),
                make_del(
                    687,
                    "                if stack.last() == Some(&ContainerKind::Backtick) {",
                ),
                make_del(688, "                    let top = stack.pop();"),
                make_del(689, "                    result.push(top);"),
                make_del(690, "                } else {"),
                make_del(
                    691,
                    "                    stack.push(ContainerKind::Backtick);",
                ),
                make_del(
                    692,
                    "                    result.push(stack.last().copied());",
                ),
                make_del(693, "                }"),
                make_del(694, "            }"),
                make_del(695, "            \"\\\"\" => {"),
                make_del(
                    696,
                    "                if stack.last() == Some(&ContainerKind::DoubleQuote) {",
                ),
                make_del(697, "                    let top = stack.pop();"),
                make_del(698, "                    result.push(top);"),
                make_del(699, "                } else {"),
                make_del(
                    700,
                    "                    stack.push(ContainerKind::DoubleQuote);",
                ),
                make_del(
                    701,
                    "                    result.push(stack.last().copied());",
                ),
                make_del(702, "                }"),
                make_del(703, "            }"),
                make_del(704, "            _ => {"),
                make_del(705, "                result.push(stack.last().copied());"),
                make_del(706, "            }"),
                make_del(707, "        }"),
                make_del(708, "    }"),
                make_del(709, "    result"),
                make_add(
                    720,
                    "    diffuse_algo::track_enclosing_containers(tokens, &diffuse_algo::AsciiDelimiterClassifier)",
                ),
                make_add(721, "        .into_iter()"),
                make_add(
                    722,
                    "        .map(|opt| opt.and_then(ContainerKind::from_id))",
                ),
                make_add(723, "        .collect()"),
                make_ctx(710, 724, "}"),
            ],
            ..Default::default()
        };
        let res = peer_reflow_pipeline(&hunk, None, None, None);
        assert_eq!(res.len(), 34);
        assert!(matches!(res[0], Row::Ctx { .. }));
        for r in &res[1..33] {
            assert!(matches!(
                r,
                Row::Del { .. } | Row::Mod { .. } | Row::Add { .. }
            ));
        }
        assert!(matches!(res[33], Row::Ctx { .. }));
    }

    #[test]
    fn test_peer_pipeline_pure_dels() {
        let hunk = Hunk {
            old_start: 1,
            old_count: 4,
            new_start: 1,
            new_count: 2,
            rows: vec![
                make_ctx(1, 1, "fn hello() {"),
                make_del(2, "    println!(\"removed 1\");"),
                make_del(3, "    println!(\"removed 2\");"),
                make_ctx(4, 2, "}"),
            ],
            ..Default::default()
        };

        let peer = peer_reflow_pipeline(&hunk, None, None, None);

        assert_eq!(peer.len(), 4);
        assert!(matches!(peer[0], Row::Ctx { .. }));
        assert_eq!(peer[1].kind(), RowKind::Del);
        assert_eq!(peer[1].old_line(), Some(2));
        assert_eq!(peer[2].kind(), RowKind::Del);
        assert_eq!(peer[2].old_line(), Some(3));
        assert!(matches!(peer[3], Row::Ctx { .. }));
    }

    #[test]
    fn test_peer_pipeline_single_1to1_mod() {
        let hunk = Hunk {
            old_start: 10,
            old_count: 3,
            new_start: 10,
            new_count: 3,
            rows: vec![
                make_ctx(10, 10, "fn compute() -> i32 {"),
                make_del(11, "    let val = 100;"),
                make_add(11, "    let val = 101;"),
                make_ctx(12, 12, "    val"),
            ],
            ..Default::default()
        };

        let peer = peer_reflow_pipeline(&hunk, None, None, None);

        assert_eq!(peer.len(), 3);
        assert_eq!(peer[1].kind(), RowKind::Mod);
        assert_eq!(peer[1].old_line(), Some(11));
        assert_eq!(peer[1].new_line(), Some(11));
        assert!(
            peer[1].spans().is_some(),
            "1-to-1 mod must carry word-diff spans"
        );
    }

    #[test]
    fn test_peer_pipeline_compound_scrollbar_reflow() {
        let hunk = Hunk {
            old_start: 290,
            old_count: 4,
            new_start: 285,
            new_count: 5,
            rows: vec![
                make_ctx(290, 285, "impl Scope {"),
                make_del(293, "find_entry(pat: &str) -> Option<usize>"),
                make_del(
                    294,
                    "|pat: &str| self.entries.iter().position(|e| e.name == pat);",
                ),
                make_add(
                    288,
                    "find_entry<'a>(&'a self, pat: &str) -> Option<usize> {",
                ),
                make_add(289, "    self.entries.iter().position(|e| e.name == pat)"),
                make_add(290, "};"),
                make_ctx(295, 291, "}"),
            ],
            ..Default::default()
        };

        let peer = peer_reflow_pipeline(&hunk, None, None, None);
        assert_eq!(peer.len(), 5);
        assert_eq!(peer[0].kind(), RowKind::Ctx);
        assert_eq!(peer[1].kind(), RowKind::Mod); // 293 -> 288
        assert_eq!(peer[2].kind(), RowKind::Mod); // 294 -> 289
        assert_eq!(peer[3].kind(), RowKind::Add); // None -> 290
        assert_eq!(peer[4].kind(), RowKind::Ctx);

        assert!(matches!(
            peer[1].group(),
            Some(PeerGroup {
                n: 2,
                m: 3,
                slot: 0,
                ..
            })
        ));
        assert!(matches!(
            peer[2].group(),
            Some(PeerGroup {
                n: 2,
                m: 3,
                slot: 1,
                ..
            })
        ));
        assert!(matches!(
            peer[3].group(),
            Some(PeerGroup {
                n: 2,
                m: 3,
                slot: 2,
                ..
            })
        ));
        assert_peer_group_invariants(&peer);
    }

    #[test]
    fn test_peer_pipeline_expansion_block() {
        let hunk = Hunk {
            old_start: 20,
            old_count: 3,
            new_start: 20,
            new_count: 5,
            rows: vec![
                make_ctx(20, 20, "fn setup() {"),
                make_del(21, "    init(); finalize();"),
                make_add(21, "    init();"),
                make_add(22, "    finalize();"),
                make_ctx(22, 23, "}"),
            ],
            ..Default::default()
        };

        let peer = peer_reflow_pipeline(&hunk, None, None, None);
        // The old line pairs with the continuation `finalize();`; the head
        // `init();` is a grouped addition of the same 1-to-2 formation.
        assert_eq!(peer.len(), 4);
        assert_eq!(peer[0].kind(), RowKind::Ctx);
        assert_eq!(peer[1].kind(), RowKind::Add);
        assert_eq!(peer[1].new_line(), Some(21));
        assert!(matches!(
            peer[1].group(),
            Some(PeerGroup {
                n: 1,
                m: 2,
                slot: 0,
                ..
            })
        ));
        assert_eq!(peer[2].kind(), RowKind::Mod);
        assert_eq!(peer[2].old_line(), Some(21));
        assert_eq!(peer[2].new_line(), Some(22));
        assert!(matches!(
            peer[2].group(),
            Some(PeerGroup {
                n: 1,
                m: 2,
                slot: 1,
                ..
            })
        ));
        assert_eq!(peer[3].kind(), RowKind::Ctx);
        assert_peer_group_invariants(&peer);
    }

    #[test]
    fn test_peer_pipeline_collapse_block() {
        let hunk = Hunk {
            old_start: 30,
            old_count: 5,
            new_start: 30,
            new_count: 3,
            rows: vec![
                make_ctx(30, 30, "fn teardown() {"),
                make_del(31, "    step1();"),
                make_del(32, "    step2();"),
                make_add(31, "    step1(); step2();"),
                make_ctx(33, 32, "}"),
            ],
            ..Default::default()
        };

        let peer = peer_reflow_pipeline(&hunk, None, None, None);
        // The path pairs the head deletion with the merged line and keeps the
        // continuation as a grouped deletion: a 2-to-1 collapse formation.
        assert_eq!(peer.len(), 4);
        assert_eq!(peer[0].kind(), RowKind::Ctx);
        assert_eq!(peer[1].kind(), RowKind::Mod);
        assert_eq!(peer[1].old_line(), Some(31));
        assert_eq!(peer[1].new_line(), Some(31));
        assert!(matches!(
            peer[1].group(),
            Some(PeerGroup {
                n: 2,
                m: 1,
                slot: 0,
                ..
            })
        ));
        assert_eq!(peer[2].kind(), RowKind::Del);
        assert_eq!(peer[2].old_line(), Some(32));
        assert!(matches!(
            peer[2].group(),
            Some(PeerGroup {
                n: 2,
                m: 1,
                slot: 1,
                ..
            })
        ));
        assert_eq!(peer[3].kind(), RowKind::Ctx);
        assert_peer_group_invariants(&peer);
    }

    #[test]
    fn test_peer_pipeline_no_mod_salient_link_admitted() {
        let dels = vec![
            Row::del(DelLine::new(11, "aaa row bbb")),
            Row::del(DelLine::new(12, "ccc row ddd")),
        ];
        let adds = vec![
            Row::add(AddLine::new(11, "eee row fff")),
            Row::add(AddLine::new(12, "ggg row hhh")),
        ];
        let mut aligned = type_window(GroupId(1), &dels, &adds, &[], None, None, None);
        aligned.repair_unmatched_moves();
        aligned.assert_token_conservation();
        // No pair was admissible, so every slot stays single-sided …
        assert!(
            aligned
                .slots
                .iter()
                .all(|s| s.old_line.is_none() || s.new_line.is_none()),
            "expected no Mod slots, got {:?}",
            aligned.slots
        );
        // … yet the shared identifier survives as a balanced moved link.
        assert!(has_salient_moved_link(&aligned, None));
        // The gate therefore keeps grouping instead of dissolving.
        let meta = peer_group_meta(2, 2, true, false, aligned.slots[0].group);
        assert_eq!(meta, Some(aligned.slots[0].group));
    }

    #[test]
    fn test_peer_group_meta_truth_table() {
        let group = PeerGroup {
            id: GroupId(7),
            slot: 0,
            n: 2,
            m: 3,
        };
        assert_eq!(peer_group_meta(1, 1, true, false, group), None);
        assert_eq!(peer_group_meta(2, 2, true, true, group), None);
        assert_eq!(peer_group_meta(2, 3, false, false, group), None);
        assert_eq!(peer_group_meta(2, 3, true, false, group), Some(group));
    }

    #[test]
    fn test_peer_pipeline_no_mod_delimiter_only_stripped() {
        let hunk = Hunk {
            old_start: 10,
            old_count: 4,
            new_start: 10,
            new_count: 4,
            rows: vec![
                make_ctx(10, 10, "fn setup() {"),
                make_del(11, "aaa;bbb"),
                make_del(12, "ccc;ddd"),
                make_add(11, "eee;fff"),
                make_add(12, "ggg;hhh"),
                make_ctx(13, 13, "}"),
            ],
            ..Default::default()
        };

        let res = peer_reflow_pipeline(&hunk, None, None, None);
        assert_eq!(res.len(), 6);
        assert!(matches!(res[0], Row::Ctx { .. }));
        assert!(matches!(res[5], Row::Ctx { .. }));
        for r in &res[1..5] {
            assert!(
                matches!(r, Row::Del { .. } | Row::Add { .. }),
                "expected plain row, got {:?}",
                r
            );
            assert_eq!(r.group(), None);
        }
        assert_peer_group_invariants(&res);
    }

    #[test]
    fn test_peer_pipeline_single_pair_rival_stays_plain() {
        // A verbatim rival elsewhere in the old snapshot blocks Mod pairing,
        // and the shared identifier still relocates — but a lone pair is not
        // a group: it must dissolve to plain Del/Add, never to grouped rows
        // (the peer invariants require atomic groups).
        let hunk = Hunk {
            old_start: 10,
            old_count: 3,
            new_start: 10,
            new_count: 3,
            rows: vec![
                make_ctx(10, 10, "fn setup() {"),
                make_del(11, "pub enum Alpha {"),
                make_add(11, "pub struct Alpha {"),
                make_ctx(12, 12, "}"),
            ],
            ..Default::default()
        };
        let old_content = [
            "fn setup() {",
            "pub struct Alpha {",
            "pub enum Alpha {",
            "}",
        ];
        let new_content = ["fn setup() {", "pub struct Alpha {", "}"];

        let res = peer_reflow_pipeline(&hunk, Some(&old_content), Some(&new_content), None);
        assert_eq!(res.len(), 4);
        assert!(matches!(res[1], Row::Del { .. }), "got {:?}", res[1]);
        assert!(matches!(res[2], Row::Add { .. }), "got {:?}", res[2]);
        assert_eq!(res[1].group(), None);
        assert_eq!(res[2].group(), None);
        assert_peer_group_invariants(&res);
    }

    #[test]
    fn test_build_bin_topological_inversion() {
        let hunk = Hunk {
            old_start: 6,
            old_count: 12,
            new_start: 6,
            new_count: 18,
            rows: vec![
                make_ctx(6, 6, "#   PROFILE=debug ./scripts/build-bin.sh"),
                make_ctx(7, 7, "set -e"),
                make_ctx(
                    8,
                    8,
                    "cd \"$(dirname \"$(readlink -f \"$0\")\")/../../..\" # the workspace root",
                ),
                make_ctx(9, 9, "PROFILE=\"${PROFILE:-dev}\""),
                make_del(10, "if [ \"$PROFILE\" = \"release\" ]; then"),
                make_del(11, "\tcargo build --release --bin diffuse"),
                make_del(12, "\tcp target/release/diffuse extension/pi/bin/diffuse"),
                make_add(
                    10,
                    "TOOLCHAIN=\"$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p' rust-toolchain.toml)\"",
                ),
                make_add(11, "if [ -n \"$TOOLCHAIN\" ]; then"),
                make_add(12, "  CARGO=\"cargo +$TOOLCHAIN\""),
                make_ctx(13, 13, "else"),
                make_del(14, "\tcargo build --bin diffuse"),
                make_del(15, "\tcp target/debug/diffuse extension/pi/bin/diffuse"),
                make_add(14, "  CARGO=cargo"),
                make_add(15, "fi"),
                make_add(16, "if [ \"$PROFILE\" = \"release\" ]; then"),
                make_add(17, "\t$CARGO build --release --bin diffuse"),
                make_add(
                    18,
                    "\tcp --remove-destination target/release/diffuse extension/pi/bin/diffuse",
                ),
                make_ctx(16, 19, "else"),
            ],
            ..Default::default()
        };

        let res = peer_reflow_pipeline(&hunk, None, None, None);
        assert_peer_group_invariants(&res);
    }

    #[test]
    fn test_peer_pipeline_identical_anchors_leave_no_group() {
        // `scrollbar-grab-drag` `geometry.rs`: a 2 -> 8 reflow whose only two
        // anchors are byte-identical lines. Normalizing those Mods to Ctx
        // removes every relocated token from the group, so the surviving
        // additions must not stay annotated as peers.
        let hunk = Hunk {
            old_start: 299,
            old_count: 3,
            new_start: 373,
            new_count: 11,
            rows: vec![
                make_ctx(299, 373, "        if max_scroll == 1 {"),
                make_del(300, "            return 0;"),
                make_del(301, "        }"),
                make_add(
                    374,
                    "            return if row >= track_h / 2 { 1 } else { 0 };",
                ),
                make_add(375, "        }"),
                make_add(
                    376,
                    "        let Some(thumb_size) = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines) else {",
                ),
                make_add(377, "            return 0;"),
                make_add(378, "        };"),
                make_add(
                    379,
                    "        let span = track_h.saturating_sub(thumb_size);",
                ),
                make_add(380, "        if span == 0 {"),
                make_add(381, "            return 0;"),
                make_add(382, "        }"),
                make_ctx(302, 383, "        let r = row.min(track_h - 1);"),
            ],
            ..Default::default()
        };

        let res = peer_reflow_pipeline(&hunk, None, None, None);

        assert!(
            res.iter().all(|r| r.group().is_none()),
            "identical anchors relocate nothing, so no peer group may remain: {:?}",
            res.iter().filter_map(|r| r.group()).collect::<Vec<_>>()
        );
    }
}
