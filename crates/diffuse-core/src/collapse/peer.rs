//! Unified M-to-N peer reflow pipeline.
//!
//! Replaces legacy asymmetric anchor/aux reflow hierarchy with symmetric macro
//! cluster factorization cuts and micro slot-alignment DP.

use diffuse_syntax::profile::LangProfile;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    cluster::factor::factor_cluster,
    collapse::{
        assemble::pass_normalize_identical_mods,
        invariants::{
            assert_collapsed_reflow_invariants, check_context_old_line_monotonicity,
            check_del_add_topological_order, check_no_orphaned_continuation_dels,
        },
        slot::align_and_type_peer_group,
    },
    merge::RunKind,
    parse::{AddLine, DelLine, GroupId, Hunk, Row, RowGroup},
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
        if let Some(RowGroup::Peer { id, .. }) = r.group() {
            group_slots.entry(id).or_default().push(r);
        }
    }

    for (&id, members) in &group_slots {
        let mut expected_slot = 0;
        let mut last_old: Option<u32> = None;
        let mut last_new: Option<u32> = None;
        let mut moved_src_chars: Vec<char> = Vec::new();
        let mut moved_tgt_chars: Vec<char> = Vec::new();

        for r in members {
            if let Some(RowGroup::Peer { slot, .. }) = r.group() {
                assert_eq!(
                    slot, expected_slot,
                    "Group {id} slot out of order: expected {expected_slot}, got {slot}"
                );
                expected_slot += 1;
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
                    RunKind::MovedSrc => moved_src_chars.extend(text.chars()),
                    RunKind::MovedTgt => moved_tgt_chars.extend(text.chars()),
                    _ => {}
                }
            }
        }

        moved_src_chars.sort_unstable();
        moved_tgt_chars.sort_unstable();
        assert_eq!(
            moved_src_chars, moved_tgt_chars,
            "Group {id} character conservation mismatch: MovedSrc={:?} vs MovedTgt={:?}",
            moved_src_chars, moved_tgt_chars
        );
    }
}

/// Executes the unified peer reflow pipeline over a diff hunk.
pub fn peer_reflow_pipeline(
    h: &Hunk,
    _old_content: Option<&[&str]>,
    _new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Vec<Row> {
    let mut h_norm = pass_shift_hunk(h, profile);
    let (split_rows, _split_homes) = crate::ctx_split::split_trivial_ctx_rows(h_norm.rows, profile);
    h_norm.rows = split_rows;
    let h = &h_norm;

    let mut out: Vec<Row> = Vec::new();
    let mut i = 0;
    let mut group_id_counter = 1u32;

    while i < h.rows.len() {
        match &h.rows[i] {
            Row::Ctx(_) | Row::Sep(_) => {
                out.push(h.rows[i].clone());
                i += 1;
            }
            _ => {
                let cluster_start = i;
                while i < h.rows.len() && !matches!(&h.rows[i], Row::Ctx(_) | Row::Sep(_)) {
                    i += 1;
                }
                let cluster_rows = &h.rows[cluster_start..i];

                let mut old_rows = Vec::new();
                let mut new_rows = Vec::new();
                for r in cluster_rows {
                    if matches!(r, Row::Del(_) | Row::MovSrc { .. } | Row::AuxDel { .. }) {
                        old_rows.push(r.clone());
                    } else if matches!(r, Row::Add(_) | Row::MovTgt { .. } | Row::AuxAdd { .. }) {
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
                } else {
                    let factored = factor_cluster(&old_rows, &new_rows, profile);
                    for group in factored {
                        let mut peer_group = group;
                        peer_group.id = GroupId(group_id_counter);
                        group_id_counter += 1;

                        let group_rows =
                            align_and_type_peer_group(&peer_group, &old_rows, &new_rows, profile);
                        group_rows.assert_character_conservation();

                        let n = peer_group.old_range.len();
                        let m = peer_group.new_range.len();

                        for slot in group_rows.slots {
                            let group_meta = if n == 1 && m == 1 {
                                None
                            } else {
                                Some(slot.group)
                            };

                            match (slot.old_line, slot.new_line) {
                                (Some(u), Some(v)) => {
                                    let old_text = slot.old_text.unwrap_or_default();
                                    let new_text = slot.new_text.unwrap_or_default();
                                    out.push(Row::Mod {
                                        old_line: DelLine::new(u, old_text),
                                        new_line: AddLine::new(v, new_text),
                                        spans: slot.spans,
                                        cached_runs: slot.runs,
                                        group: group_meta,
                                    });
                                }
                                (None, Some(v)) => {
                                    let new_text = slot.new_text.unwrap_or_default();
                                    if n == 0 {
                                        out.push(Row::Add(AddLine::new(v, new_text)));
                                    } else {
                                        out.push(Row::AuxAdd {
                                            anchor: peer_group.id.0,
                                            line: AddLine::new(v, new_text),
                                            spans: slot.spans,
                                            cached_runs: slot.runs,
                                            group: group_meta,
                                        });
                                    }
                                }
                                (Some(u), None) => {
                                    let old_text = slot.old_text.unwrap_or_default();
                                    if m == 0 {
                                        out.push(Row::Del(DelLine::new(u, old_text)));
                                    } else {
                                        out.push(Row::AuxDel {
                                            anchor: peer_group.id.0,
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
                }
            }
        }
    }

    let block_anchor_news = FxHashSet::default();
    let res = pass_normalize_identical_mods(h, out, profile, &block_anchor_news);

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

    assert_peer_group_invariants(&res);

    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        collapse::collapse_hunk_rows,
        parse::{CtxLine, RowKind},
    };

    fn make_ctx(old: u32, new: u32, text: &str) -> Row {
        Row::Ctx(CtxLine::new(old, new, text))
    }

    fn make_del(num: u32, text: &str) -> Row {
        Row::Del(DelLine::new(num, text))
    }

    fn make_add(num: u32, text: &str) -> Row {
        Row::Add(AddLine::new(num, text))
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

        let legacy = collapse_hunk_rows(&hunk, None, None, None);
        let peer = peer_reflow_pipeline(&hunk, None, None, None);

        assert_eq!(peer.len(), legacy.len());
        for (p, l) in peer.iter().zip(&legacy) {
            assert_eq!(p.kind(), l.kind());
            assert_eq!(p.old_line(), l.old_line());
            assert_eq!(p.new_line(), l.new_line());
            assert_eq!(p.text(), l.text());
        }
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

        let legacy = collapse_hunk_rows(&hunk, None, None, None);
        let peer = peer_reflow_pipeline(&hunk, None, None, None);

        assert_eq!(peer.len(), legacy.len());
        for (p, l) in peer.iter().zip(&legacy) {
            assert_eq!(p.kind(), l.kind());
            assert_eq!(p.old_line(), l.old_line());
            assert_eq!(p.new_line(), l.new_line());
            assert_eq!(p.text(), l.text());
        }
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

        let legacy = collapse_hunk_rows(&hunk, None, None, None);
        let peer = peer_reflow_pipeline(&hunk, None, None, None);

        assert_eq!(peer.len(), legacy.len());
        assert_eq!(peer.len(), 3);
        assert_eq!(peer[1].kind(), RowKind::Mod);
        assert_eq!(peer[1].old_line(), Some(11));
        assert_eq!(peer[1].new_line(), Some(11));
        assert_eq!(peer[1].spans(), legacy[1].spans());
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

        // Check peer group metadata
        assert!(matches!(
            peer[1].group(),
            Some(RowGroup::Peer {
                n: 2,
                m: 3,
                slot: 0,
                ..
            })
        ));
        assert!(matches!(
            peer[2].group(),
            Some(RowGroup::Peer {
                n: 2,
                m: 3,
                slot: 1,
                ..
            })
        ));
        assert!(matches!(
            peer[3].group(),
            Some(RowGroup::Peer {
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
        assert_eq!(peer.len(), 4);
        assert_eq!(peer[0].kind(), RowKind::Ctx);
        assert_eq!(peer[1].kind(), RowKind::Add);
        assert_eq!(peer[2].kind(), RowKind::Mod);
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
        assert_eq!(peer.len(), 4);
        assert_eq!(peer[0].kind(), RowKind::Ctx);
        assert_eq!(peer[1].kind(), RowKind::Del);
        assert_eq!(peer[2].kind(), RowKind::Mod);
        assert_eq!(peer[3].kind(), RowKind::Ctx);
        assert_peer_group_invariants(&peer);
    }
}
