//! Contract postcondition invariants and structural integrity assertions for
//! collapsed hunks.

use rustc_hash::FxHashSet;

use crate::parse::{Hunk, Row};

/// Contract Invariant 1: Reflow Cluster Confinement.
///
/// Contract Invariant 2: Syntactic Continuation Continuity.
///
/// A standalone or orphaned deletion sequence in the visual row stream must
/// not start with a syntactic continuation operator (e.g. `&&`, `||`, access
/// `.`, `,`, `)`, `}`) if its syntactic parent line in the old file (`d_old -
/// 1`) was also deleted in the diff.
pub fn check_no_orphaned_continuation_dels(h: &Hunk, res: &[Row]) -> Result<(), String> {
    for (i, r) in res.iter().enumerate() {
        let Row::Del {
            line: d,
            group: None,
            ..
        } = r
        else {
            continue;
        };
        // Only inspect the head of a deletion run
        if i > 0 && matches!(res[i - 1], Row::Del { .. }) {
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
                if matches!(prev, Row::Ctx { .. }) {
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
                    .any(|row| matches!(row, Row::Del { line: d0, .. } if d0.num == prev_old));
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
        let Row::Ctx { line: c, .. } = r else {
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

            // Find all Mod or Ctx reference rows in res that belong to this cluster
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
                    Row::Ctx { line: c, .. }
                        if cluster_dels.contains(&c.old_pos)
                            && cluster_adds.contains(&c.new_pos) =>
                    {
                        Some((c.old_pos, c.new_pos))
                    }
                    _ => None,
                })
                .collect();

            for (vi, r) in res.iter().enumerate() {
                if let Row::Del {
                    line: d,
                    group: None,
                    ..
                } = r
                    && cluster_dels.contains(&d.num)
                {
                    // Find the seed new line for deletion d: the new line of the
                    // latest Mod/Ctx row in the cluster with old_line <= d.num.
                    let d_seed_new = cluster_mods
                        .iter()
                        .filter(|&&(old_num, _)| old_num <= d.num)
                        .map(|&(_, new_num)| new_num)
                        .max()
                        .unwrap_or(0);

                    for (vj, prev) in res[..vi].iter().enumerate().rev() {
                        if matches!(prev, Row::Ctx { .. }) {
                            break;
                        }
                        if let Some(nl) = prev.new_line()
                            && cluster_adds.contains(&nl)
                            && matches!(prev, Row::Add { group: None, .. })
                            && nl > d_seed_new
                        {
                            return Err(format!(
                                "Topological del/add inversion: unpaired deletion at visual row {vi} (old line {}, seed new {d_seed_new}) appears after pure addition at visual row {vj} (new line {nl}): deletion {:?} after addition {:?}",
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
