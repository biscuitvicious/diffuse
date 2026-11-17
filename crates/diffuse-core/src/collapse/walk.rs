//! Keyed anchor sorting and visual row stream reconstruction pass (Pass 4).

use std::cmp::Ordering;

use rustc_hash::FxHashMap;

use crate::{
    merge::RunKind,
    parse::{Hunk, Row, RowGroup, Spans},
};

use super::{assemble::AssembledMods, reflow::ReflowFilter};

/// Pass 4: Keyed anchor sorting into the final visual row stream.
pub fn pass_sort_ordered_walk(
    h: &Hunk,
    assembled: AssembledMods,
    reflow: ReflowFilter,
) -> Vec<Row> {
    let AssembledMods {
        out,
        add_spans,
        add_groups,
        add_runs,
        del_spans,
        del_groups,
        del_runs,
        interior_anchor_news,
        ..
    } = assembled;
    // Stage reflow-synthesized Ctx rows alongside `out` rows before
    // del-anchoring: leftover dels must anchor to synthesized context
    // exactly as they do to hunk context. (Coincidental-context splitting
    // routes trivial pairs through reflow instead of hunk passthrough;
    // without this, dels anchor to earlier rows and sort before their
    // own context, violating old-line monotonicity.) Reflow Mod rows
    // keep their existing late placement.
    let (staged_ctx, staged_mods): (Vec<Row>, Vec<Row>) = reflow
        .reflow_ctx
        .into_iter()
        .partition(|r| matches!(r, Row::Ctx(_)));

    let mut out = out;
    out.extend(staged_ctx);

    let mut anchor_old_of: FxHashMap<u32, u32> = FxHashMap::default();
    for r in &out {
        if let (Some(nl), Some(ol)) = (r.new_line(), r.old_line()) {
            anchor_old_of.insert(nl, ol);
        }
    }

    let mut anchor_of: FxHashMap<usize, usize> = FxHashMap::default();
    for &di in &reflow.meaningful_dels {
        if del_groups.contains_key(&di) {
            continue;
        }
        let Some(dold) = h.rows[di].old_line() else {
            continue;
        };
        let mut anchor: Option<usize> = None;
        for (mi, m) in out.iter().enumerate() {
            let Some(mold) = m.old_line() else {
                continue;
            };
            if mold <= dold && (anchor.is_none() || mold > out[anchor.unwrap()].old_line().unwrap())
            {
                anchor = Some(mi);
            }
        }
        if let Some(a) = anchor {
            anchor_of.insert(di, a);
        }
    }

    let mut anchor_max_aux: FxHashMap<u32, u32> = FxHashMap::default();
    for &ai in &reflow.kept_adds {
        if let Some(RowGroup::Aux { anchor }) = add_groups.get(&ai) {
            let nl = h.rows[ai].new_line().unwrap_or(0);
            anchor_max_aux
                .entry(*anchor)
                .and_modify(|m| *m = (*m).max(nl))
                .or_insert(nl);
        }
    }

    let mut keyed: Vec<(Row, (u32, f64))> = Vec::new();
    for r in out.into_iter() {
        let key = (
            r.new_line().unwrap_or(0),
            if matches!(r, Row::Mod { .. }) || matches!(r, Row::Ctx(_)) {
                1.0
            } else {
                2.0
            },
        );
        keyed.push((r, key));
    }
    for &di in &reflow.meaningful_dels {
        let d = &h.rows[di];
        if let Some(&RowGroup::Aux { anchor }) = del_groups.get(&di) {
            let spans = del_spans.get(&di).cloned().unwrap_or_default();
            let cached_runs = del_runs
                .get(&di)
                .cloned()
                .unwrap_or_else(|| vec![(RunKind::Del, d.text().to_string())]);
            let row = match d {
                Row::Del(del_line) => Row::AuxDel {
                    anchor,
                    line: del_line.clone(),
                    spans,
                    cached_runs,
                },
                other => other.clone(),
            };
            let sub_key =
                if let (Some(d_old), Some(&a_old)) = (d.old_line(), anchor_old_of.get(&anchor)) {
                    if d_old < a_old { 0.9 } else { 1.1 }
                } else {
                    1.1
                };
            let key = (anchor, sub_key);
            keyed.push((row, key));
            continue;
        }

        let (key, anchor_group) = match anchor_of.get(&di) {
            None => ((0, d.old_line().unwrap_or(0) as f64), None),
            Some(&mi) => {
                let base = keyed[mi].1.0;
                let grp = if keyed[mi].0.group() == Some(RowGroup::Anchor)
                    && !interior_anchor_news.contains(&keyed[mi].0.new_line().unwrap_or(0))
                {
                    keyed[mi]
                        .0
                        .new_line()
                        .map(|nl| RowGroup::Aux { anchor: nl })
                } else {
                    None
                };
                let sort_base = if let Some(RowGroup::Aux { anchor }) = grp {
                    base.max(anchor_max_aux.get(&anchor).copied().unwrap_or(base))
                } else {
                    base
                };
                ((sort_base, 1.5), grp)
            }
        };
        let row = match d {
            Row::Del(del_line) => {
                if let Some(RowGroup::Aux { anchor }) = anchor_group {
                    Row::AuxDel {
                        anchor,
                        line: del_line.clone(),
                        spans: Spans::default(),
                        cached_runs: vec![(RunKind::Del, del_line.text.clone())],
                    }
                } else {
                    Row::Del(del_line.clone())
                }
            }
            other => other.clone(),
        };
        keyed.push((row, key));
    }
    for &ai in &reflow.kept_adds {
        let a = match &h.rows[ai] {
            Row::Add(add_line) => {
                if let Some(&RowGroup::Aux { anchor }) = add_groups.get(&ai) {
                    let spans = add_spans.get(&ai).cloned().unwrap_or_else(|| {
                        if add_line.text.is_empty() {
                            Spans {
                                del: Vec::new(),
                                add: vec![(0, 0)],
                            }
                        } else {
                            Spans::default()
                        }
                    });
                    let runs = add_runs
                        .get(&ai)
                        .cloned()
                        .unwrap_or_else(|| vec![(RunKind::Add, add_line.text.clone())]);
                    Row::AuxAdd {
                        anchor,
                        line: add_line.clone(),
                        spans,
                        cached_runs: runs,
                    }
                } else {
                    Row::Add(add_line.clone())
                }
            }
            _ => h.rows[ai].clone(),
        };
        let key = (h.rows[ai].new_line().unwrap_or(0), 1.0);
        keyed.push((a, key));
    }
    for m in staged_mods {
        let key = (m.new_line().unwrap_or(0), 1.0);
        keyed.push((m, key));
    }

    keyed.sort_by(|a, b| {
        let (num_a, sub_a) = a.1;
        let (num_b, sub_b) = b.1;
        if num_a != num_b {
            num_a.cmp(&num_b)
        } else if (sub_a - sub_b).abs() > 1e-6 {
            sub_a.partial_cmp(&sub_b).unwrap_or(Ordering::Equal)
        } else {
            a.0.old_line().cmp(&b.0.old_line())
        }
    });

    keyed.into_iter().map(|(r, _)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{AddLine, CtxLine, DelLine, Hunk, Row};

    fn hunk(rows: Vec<Row>) -> Hunk {
        Hunk {
            old_start: 1,
            old_count: 1,
            new_start: 1,
            new_count: 1,
            section: String::new(),
            rows,
            above_ctx: 0,
            below_ctx: 0,
        }
    }

    /// Reflow-synthesized Ctx must anchor leftover dels exactly like hunk
    /// context (theme-light-toggle `event/tests.rs`: Del 918/919/920 must
    /// sort after reflow Ctx 917↔923, not before it).
    #[test]
    fn test_reflow_ctx_anchors_leftover_dels() {
        let h = hunk(vec![
            Row::Del(DelLine::new(917, "    );")),
            Row::Add(AddLine::new(923, "    );")),
            Row::Del(DelLine::new(918, "    assert!(")),
            Row::Del(DelLine::new(919, "        x,")),
        ]);
        let reflow = ReflowFilter {
            meaningful_dels: vec![2, 3],
            kept_adds: vec![],
            reflow_ctx: vec![Row::Ctx(CtxLine::new(917, 923, "    );"))],
        };
        let out = pass_sort_ordered_walk(&h, AssembledMods::default(), reflow);
        let pos = |old: u32| out.iter().position(|r| r.old_line() == Some(old)).unwrap();
        let ctx_pos = out
            .iter()
            .position(|r| matches!(r, Row::Ctx(_)) && r.old_line() == Some(917))
            .unwrap();
        assert!(
            pos(918) > ctx_pos,
            "Del 918 must follow Ctx 917, got {out:?}"
        );
        assert!(
            pos(919) > ctx_pos,
            "Del 919 must follow Ctx 917, got {out:?}"
        );
        crate::collapse::invariants::check_context_old_line_monotonicity(&out)
            .expect("normalized stream must satisfy the monotonicity invariant");
    }
}
