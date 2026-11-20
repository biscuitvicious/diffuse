//! Legacy run/del-index map construction for cluster pairing.

use rustc_hash::FxHashMap;

use crate::parse::{Hunk, Row};

pub(crate) type LegacyIndexMaps = (Vec<Vec<usize>>, FxHashMap<usize, Vec<usize>>, Vec<usize>);

/// The legacy run/del-index construction (identical for both engines —
/// passes 2–4 consume it).
pub(crate) fn legacy_index_maps(h: &Hunk) -> LegacyIndexMaps {
    let mut del_runs: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < h.rows.len() {
        if !matches!(h.rows[i], Row::Del { .. } | Row::MovSrc { .. }) {
            i += 1;
            continue;
        }
        let mut run = vec![i];
        while i + 1 < h.rows.len() && matches!(h.rows[i + 1], Row::Del { .. } | Row::MovSrc { .. })
        {
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
        .filter(|(_, r)| matches!(r, Row::Del { .. } | Row::MovSrc { .. }))
        .map(|(i, _)| i)
        .collect();
    (del_runs, run_of, del_ids)
}
