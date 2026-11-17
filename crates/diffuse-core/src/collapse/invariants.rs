//! Contract postcondition invariants and structural integrity assertions for
//! collapsed hunks.

use rustc_hash::FxHashSet;

use diffuse_syntax::profile::LangProfile;

use crate::{
    merge::{RunKind, merged_runs},
    parse::{Hunk, Row, RowGroup},
};

use super::reflow::ReflowFilter;

/// Asserts reflow group invariants on collapsed rows:
/// 1. Old-side reconstruction: removing novel additions across the group in
///    visual order reproduces the surviving old-side text.
/// 2. Moved-text conservation: anchor MovedSrc == sum(aux MovedTgt).
/// 3. Aux line numbers strictly ascending.
/// 4. No orphan aux rows.
pub(crate) fn assert_collapsed_reflow_invariants(
    rows: &[Row],
    profile: Option<&LangProfile>,
) -> Result<(), String> {
    let strip_ws = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();

    let anchor_indices: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.group() == Some(RowGroup::Anchor))
        .map(|(i, _)| i)
        .collect();

    for &anchor_idx in &anchor_indices {
        let anchor_row = &rows[anchor_idx];
        if anchor_row.old_text().is_none() {
            continue;
        }
        let anchor_nl = anchor_row.new_line();
        let anchor_ol = anchor_row.old_line();
        let anchor_num = anchor_nl.or(anchor_ol).unwrap_or(0);

        let mut group_rows: Vec<&Row> = vec![anchor_row];
        for (i, r) in rows.iter().enumerate() {
            if i == anchor_idx {
                continue;
            }
            if let Some(RowGroup::Aux { anchor }) = r.group()
                && Some(anchor) == anchor_nl
            {
                group_rows.push(r);
            }
        }

        let aux_adds: Vec<&&Row> = group_rows
            .iter()
            .filter(|r| matches!(r, Row::AuxAdd { .. }))
            .collect();
        let aux_dels: Vec<&&Row> = group_rows
            .iter()
            .filter(|r| matches!(r, Row::AuxDel { .. }))
            .collect();
        let aux_mods: Vec<&&Row> = group_rows
            .iter()
            .filter(|r| matches!(r, Row::Mod { .. }) && r.group() != Some(RowGroup::Anchor))
            .collect();
        let has_aux_dels = group_rows
            .iter()
            .any(|r| matches!(r, Row::AuxDel { .. } | Row::MovSrc { .. }));
        let has_aux_adds = group_rows
            .iter()
            .any(|r| matches!(r, Row::AuxAdd { .. } | Row::MovTgt { .. }));

        if !aux_mods.is_empty() {
            let group_moved_src: String = group_rows
                .iter()
                .flat_map(|r| merged_runs(r))
                .filter(|run| run.kind == RunKind::MovedSrc)
                .map(|run| run.text)
                .collect();
            let group_moved_tgt: String = group_rows
                .iter()
                .flat_map(|r| merged_runs(r))
                .filter(|run| run.kind == RunKind::MovedTgt)
                .map(|run| run.text)
                .collect();

            let mut s1: Vec<char> = group_moved_src
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let mut s2: Vec<char> = group_moved_tgt
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            s1.sort_unstable();
            s2.sort_unstable();
            if s1 != s2 {
                return Err(format!(
                    "Anchor line {anchor_num} compound mod group character conservation mismatch:\n  group MovedSrc chars: {s1:?}\n  group MovedTgt chars: {s2:?}"
                ));
            }

            let mut last_old = None;
            for r in &group_rows {
                if let Some(ol) = r.old_line() {
                    if let Some(prev) = last_old
                        && ol <= prev
                    {
                        return Err(format!(
                            "Compound mod group old line {ol} <= previous old line {prev}"
                        ));
                    }
                    last_old = Some(ol);
                }
            }

            let mut last_new = None;
            for r in &group_rows {
                if let Some(nl) = r.new_line() {
                    if let Some(prev) = last_new
                        && nl <= prev
                    {
                        return Err(format!(
                            "Compound mod group new line {nl} <= previous new line {prev}"
                        ));
                    }
                    last_new = Some(nl);
                }
            }
        } else if !aux_adds.is_empty() && !has_aux_dels {
            let is_import = profile.is_some_and(|p| {
                anchor_row
                    .old_text()
                    .is_some_and(|dt| p.is_import_statement(dt))
            });

            let anchor_moved_src: String = merged_runs(anchor_row)
                .into_iter()
                .filter(|run| run.kind == RunKind::MovedSrc)
                .map(|run| run.text)
                .collect();
            let aux_non_add: String = aux_adds
                .iter()
                .flat_map(|aux| merged_runs(aux))
                .filter(|run| run.kind == RunKind::MovedTgt || run.kind == RunKind::Del)
                .map(|run| run.text)
                .collect();

            // 1. Old-side reconstruction invariant (mandatory for import statements)
            if is_import {
                let expected_old: String = merged_runs(anchor_row)
                    .into_iter()
                    .filter(|run| run.kind == RunKind::Ctx || run.kind == RunKind::MovedSrc)
                    .map(|run| run.text)
                    .collect();
                let mut reconstructed_old = String::new();
                for r in &group_rows {
                    if matches!(r, Row::Mod { .. }) {
                        reconstructed_old.extend(
                            merged_runs(r)
                                .into_iter()
                                .filter(|run| run.kind == RunKind::Ctx)
                                .map(|run| run.text),
                        );
                    } else if matches!(r, Row::AuxAdd { .. }) {
                        reconstructed_old.extend(
                            merged_runs(r)
                                .into_iter()
                                .filter(|run| {
                                    run.kind == RunKind::MovedTgt || run.kind == RunKind::Del
                                })
                                .map(|run| run.text),
                        );
                    }
                }
                if strip_ws(&reconstructed_old) != strip_ws(&expected_old) {
                    return Err(format!(
                        "Anchor line {anchor_num} old-text reconstruction mismatch:\n  expected: {expected_old:?}\n  got:      {reconstructed_old:?}"
                    ));
                }
                if strip_ws(&anchor_moved_src) != strip_ws(&aux_non_add) {
                    return Err(format!(
                        "Anchor line {anchor_num} moved-text conservation mismatch:\n  anchor MovedSrc: {anchor_moved_src:?}\n  aux non-add:     {aux_non_add:?}"
                    ));
                }
            } else {
                // 2. Character conservation invariant across all reflow groups
                let mut s1: Vec<char> = anchor_moved_src
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                let mut s2: Vec<char> =
                    aux_non_add.chars().filter(|c| !c.is_whitespace()).collect();
                s1.sort_unstable();
                s2.sort_unstable();
                if s1 != s2 {
                    return Err(format!(
                        "Anchor line {anchor_num} moved-text character conservation mismatch:\n  anchor MovedSrc chars: {s1:?}\n  aux non-add chars:     {s2:?}"
                    ));
                }
            }

            // 4. Suffix aux add line numbers strictly ascending
            let mut last_aux_line = None;
            for aux in group_rows
                .iter()
                .filter(|r| matches!(r, Row::AuxAdd { .. }))
            {
                if let Some(nl) = aux.new_line() {
                    if let Some(prev) = last_aux_line
                        && nl <= prev
                    {
                        return Err(format!("Aux row line {nl} <= previous aux line {prev}"));
                    }
                    last_aux_line = Some(nl);
                }
            }
        } else if !aux_dels.is_empty() && !has_aux_adds {
            let anchor_moved_tgt: String = merged_runs(anchor_row)
                .into_iter()
                .filter(|run| run.kind == RunKind::MovedTgt)
                .map(|run| run.text)
                .collect();
            let aux_moved_src: String = aux_dels
                .iter()
                .flat_map(|aux| merged_runs(aux))
                .filter(|run| run.kind == RunKind::MovedSrc)
                .map(|run| run.text)
                .collect();
            let mut s1: Vec<char> = anchor_moved_tgt
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let mut s2: Vec<char> = aux_moved_src
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            s1.sort_unstable();
            s2.sort_unstable();
            if s1 != s2 {
                return Err(format!(
                    "Anchor line {anchor_num} moved-text character conservation mismatch:\n  anchor MovedTgt chars: {s1:?}\n  aux MovedSrc chars: {s2:?}"
                ));
            }

            // Suffix aux del line numbers strictly ascending
            let mut last_aux_line = None;
            for aux in &aux_dels {
                if let Some(ol) = aux.old_line() {
                    if let Some(prev) = last_aux_line
                        && ol <= prev
                    {
                        return Err(format!("Aux del line {ol} <= previous aux del line {prev}"));
                    }
                    last_aux_line = Some(ol);
                }
            }
        }
    }

    // 4. Reflow Group Atomicity: Anchor row and all its companion aux additions
    // (both prefix and suffix) and aux deletions must form an unbroken contiguous
    // slice in visual row order.
    for &anchor_idx in &anchor_indices {
        let anchor_row = &rows[anchor_idx];
        let anchor_nl = anchor_row.new_line();
        let anchor_ol = anchor_row.old_line();

        let mut group_indices: Vec<usize> = vec![anchor_idx];
        for (i, r) in rows.iter().enumerate() {
            if i == anchor_idx {
                continue;
            }
            if let Some(RowGroup::Aux { anchor }) = r.group()
                && Some(anchor) == anchor_nl
            {
                group_indices.push(i);
            }
        }

        if group_indices.len() > 1 {
            group_indices.sort_unstable();
            for window in group_indices.windows(2) {
                if window[1] != window[0] + 1 {
                    let anchor_ident = anchor_nl.or(anchor_ol).unwrap_or(0);
                    return Err(format!(
                        "Reflow atomicity violation: Reflow group for anchor line {anchor_ident} is interrupted at row indices {} and {}",
                        window[0], window[1]
                    ));
                }
            }
        }
    }

    // Verify no orphan aux rows
    for (i, r) in rows.iter().enumerate() {
        if let Some(RowGroup::Aux { anchor }) = r.group() {
            let has_anchor = anchor_indices
                .iter()
                .any(|&ai| rows[ai].new_line() == Some(anchor));
            if !has_anchor {
                return Err(format!(
                    "Orphan aux row {i} (line {:?}) pointing to non-existent anchor {anchor}",
                    r.new_line().or_else(|| r.old_line())
                ));
            }
        }
    }
    Ok(())
}

/// Contract Invariant 1: Reflow Cluster Confinement.
///
/// Standalone del/add pairs synthesized into `reflow_ctx` must come from the
/// same change cluster in `h`: there must be no intervening `Row::Ctx` in
/// `h.rows` between the original deletion and addition.
pub fn check_reflow_cluster_confinement(h: &Hunk, reflow: &ReflowFilter) -> Result<(), String> {
    for r in &reflow.reflow_ctx {
        let (Some(d_num), Some(a_num)) = (r.old_line(), r.new_line()) else {
            continue;
        };
        let di = h.rows.iter().position(|row| row.old_line() == Some(d_num));
        let ai = h.rows.iter().position(|row| row.new_line() == Some(a_num));
        if let (Some(d_idx), Some(a_idx)) = (di, ai) {
            let (min_idx, max_idx) = (d_idx.min(a_idx), d_idx.max(a_idx));
            for i in (min_idx + 1)..max_idx {
                if matches!(h.rows[i], Row::Ctx(_)) {
                    return Err(format!(
                        "Reflow pair (old line {d_num}, new line {a_num}) crosses intervening context line at raw hunk row {i} (old {:?}, new {:?}): {:?}",
                        h.rows[i].old_line(),
                        h.rows[i].new_line(),
                        h.rows[i].text()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Contract Invariant 2: Syntactic Continuation Continuity.
///
/// A standalone or orphaned deletion sequence in the visual row stream must
/// not start with a syntactic continuation operator (e.g. `&&`, `||`, access
/// `.`, `,`, `)`, `}`) if its syntactic parent line in the old file (`d_old -
/// 1`) was also deleted in the diff.
pub fn check_no_orphaned_continuation_dels(h: &Hunk, res: &[Row]) -> Result<(), String> {
    for (i, r) in res.iter().enumerate() {
        let Row::Del(d) = r else {
            continue;
        };
        // Only inspect the head of a deletion run
        if i > 0 && matches!(res[i - 1], Row::Del(_) | Row::AuxDel { .. }) {
            continue;
        }
        let t = d.text.trim_start();
        let is_continuation = t.starts_with("&&")
            || t.starts_with("||")
            || (t.starts_with('.') && !t.starts_with(".."));
        if is_continuation {
            let prev_old = d.num.saturating_sub(1);
            // Walk backwards in res to see if a context line intervened between prev_old
            // and d.
            let mut ctx_intervened = false;
            for prev in res[..i].iter().rev() {
                if matches!(prev, Row::Ctx(_)) {
                    ctx_intervened = true;
                    break;
                }
                if let Some(ol) = prev.old_line()
                    && ol <= prev_old
                {
                    // Found a non-context row at or before prev_old in the same change cluster.
                    break;
                }
            }
            if ctx_intervened {
                let prev_was_del_in_hunk = h
                    .rows
                    .iter()
                    .any(|row| matches!(row, Row::Del(d0) if d0.num == prev_old));
                if prev_was_del_in_hunk {
                    return Err(format!(
                        "Orphaned continuation deletion at visual row {i} (old line {}): {:?} (syntactic parent old line {prev_old} was deleted in the hunk but separated across context lines from its continuation)",
                        d.num,
                        d.text.trim()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Contract Invariant 3: Context-Relative Old-Line Monotonicity.
///
/// Context lines represent immutable checkpoints present in both old and new
/// files. In the visual stream, for every context row C with old line C_old:
/// 1. Every row before C with an old line must satisfy old < C_old.
/// 2. Every row after C with an old line must satisfy old > C_old.
pub fn check_context_old_line_monotonicity(res: &[Row]) -> Result<(), String> {
    for (i, r) in res.iter().enumerate() {
        let Row::Ctx(c) = r else {
            continue;
        };
        let c_old = c.old_pos;
        for (j, prev) in res[..i].iter().enumerate() {
            if let Some(old) = prev.old_line()
                && old >= c_old
            {
                return Err(format!(
                    "Monotonicity violation: row at visual index {j} has old line {old} >= context line at visual index {i} with old line {c_old} (row: {:?}, context: {:?})",
                    prev.text().trim(),
                    c.text.trim()
                ));
            }
        }
        for (j, next) in res[i + 1..].iter().enumerate() {
            let actual_idx = i + 1 + j;
            if let Some(old) = next.old_line()
                && old <= c_old
            {
                return Err(format!(
                    "Monotonicity violation: row at visual index {actual_idx} has old line {old} <= context line at visual index {i} with old line {c_old} (row: {:?}, context: {:?})",
                    next.text().trim(),
                    c.text.trim()
                ));
            }
        }
    }
    Ok(())
}

/// Contract Invariant 4: Addition Block Structural Non-Penetration (Topological
/// Del/Add Invariant).
///
/// In a raw diff cluster where deletions precede additions, an unpaired
/// deletion from that cluster must not be placed after the additions of that
/// cluster (or inside compound addition statements after opening delimiters).
pub fn check_del_add_topological_order(h: &Hunk, res: &[Row]) -> Result<(), String> {
    let segments = crate::cluster::partition_hunk_segments(h, None, None, None);
    for seg in segments {
        let crate::cluster::HunkSegment::Cluster(cluster) = seg else {
            continue;
        };
        if cluster.old_rows.is_empty() || cluster.new_rows.is_empty() {
            continue;
        }
        let raw_del_indices: Vec<usize> = cluster
            .old_rows
            .iter()
            .filter_map(|r| r.old_line())
            .filter_map(|num| h.rows.iter().position(|r| r.old_line() == Some(num)))
            .collect();
        let raw_add_indices: Vec<usize> = cluster
            .new_rows
            .iter()
            .filter_map(|r| r.new_line())
            .filter_map(|num| h.rows.iter().position(|r| r.new_line() == Some(num)))
            .collect();

        if raw_del_indices.is_empty() || raw_add_indices.is_empty() {
            continue;
        }
        let max_raw_del = *raw_del_indices.iter().max().unwrap();
        let min_raw_add = *raw_add_indices.iter().min().unwrap();

        if max_raw_del < min_raw_add {
            let cluster_dels: FxHashSet<u32> =
                cluster.old_rows.iter().filter_map(Row::old_line).collect();
            let cluster_adds: FxHashSet<u32> =
                cluster.new_rows.iter().filter_map(Row::new_line).collect();

            // Find all Mod rows in res that belong to this cluster
            let cluster_mods: Vec<(u32, u32)> = res
                .iter()
                .filter_map(|r| match r {
                    Row::Mod {
                        old_line, new_line, ..
                    } if cluster_dels.contains(&old_line.num)
                        && cluster_adds.contains(&new_line.num) =>
                    {
                        Some((old_line.num, new_line.num))
                    }
                    _ => None,
                })
                .collect();

            for (vi, r) in res.iter().enumerate() {
                if let Row::Del(d) = r
                    && cluster_dels.contains(&d.num)
                {
                    // Find the anchor new line for deletion d: the new line of the
                    // latest Mod row in the cluster with old_line <= d.num.
                    let d_anchor_new = cluster_mods
                        .iter()
                        .filter(|&&(old_num, _)| old_num <= d.num)
                        .map(|&(_, new_num)| new_num)
                        .max()
                        .unwrap_or(0);

                    for (vj, prev) in res[..vi].iter().enumerate() {
                        if let Some(nl) = prev.new_line()
                            && cluster_adds.contains(&nl)
                            && matches!(prev, Row::Add(_))
                            && nl > d_anchor_new
                        {
                            return Err(format!(
                                "Topological del/add inversion: unpaired deletion at visual row {vi} (old line {}, anchor new {d_anchor_new}) appears after pure addition at visual row {vj} (new line {nl}): deletion {:?} after addition {:?}",
                                d.num,
                                d.text.trim(),
                                prev.text().trim()
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
