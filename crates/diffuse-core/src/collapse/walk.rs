//! Keyed anchor sorting and visual row stream reconstruction pass (Pass 4).

use std::cmp::Ordering;

use rustc_hash::FxHashMap;

use crate::{
    merge::RunKind,
    parse::{Hunk, Row, RowGroup, Spans},
};

use super::reflow::ReflowFilter;

/// Pass 4: Keyed anchor sorting into the final visual row stream.
pub fn pass_sort_ordered_walk(
    h: &Hunk,
    out: Vec<Row>,
    reflow: ReflowFilter,
    add_spans: &FxHashMap<usize, Spans>,
    add_groups: &FxHashMap<usize, RowGroup>,
    add_runs: &FxHashMap<usize, Vec<(RunKind, String)>>,
) -> Vec<Row> {
    let mut anchor_of: FxHashMap<usize, usize> = FxHashMap::default();
    for &di in &reflow.meaningful_dels {
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
        let (key, anchor_group) = match anchor_of.get(&di) {
            None => ((0, d.old_line().unwrap_or(0) as f64), None),
            Some(&mi) => {
                let base = keyed[mi].1.0;
                let grp = if keyed[mi].0.group() == Some(RowGroup::Anchor) {
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
    for ctx in reflow.reflow_ctx {
        let key = (ctx.new_line().unwrap_or(0), 1.0);
        keyed.push((ctx, key));
    }

    keyed.sort_by(|a, b| {
        let (num_a, sub_a) = a.1;
        let (num_b, sub_b) = b.1;
        if num_a != num_b {
            num_a.cmp(&num_b)
        } else {
            sub_a.partial_cmp(&sub_b).unwrap_or(Ordering::Equal)
        }
    });

    keyed.into_iter().map(|(r, _)| r).collect()
}
