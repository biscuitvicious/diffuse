//! Formal verification and bidirectional projection invariant checks for DP
//! solutions.

use crate::{cluster::ChangeCluster, parse::Row};

use super::solver::DpSolution;

/// The bidirectional projection verifier (plan Step 3.4): the solution's
/// rows must project to EXACTLY the cluster's old lines (Del texts plus
/// Mod/Mov old texts, in order) and new lines (Add texts plus Mod/Mov new
/// texts) — the universal invariant `Π_old ≡ old ∧ Π_new ≡ new`.
pub fn verify_projection(sol: &DpSolution, cluster: &ChangeCluster) -> Result<(), String> {
    let side = |old_side: bool| -> Vec<String> {
        sol.rows
            .iter()
            .filter_map(|r| match r {
                Row::Del { line: d, .. }
                | Row::MovSrc { line: d, .. }
                | Row::Del { line: d, .. }
                    if old_side =>
                {
                    Some(d.text.clone())
                }
                Row::Add { line: a, .. }
                | Row::MovTgt { line: a, .. }
                | Row::Add { line: a, .. }
                    if !old_side =>
                {
                    Some(a.text.clone())
                }
                Row::Mod {
                    old_line, new_line, ..
                } => {
                    if old_side {
                        Some(old_line.text.clone())
                    } else {
                        Some(new_line.text.clone())
                    }
                }
                _ => None,
            })
            .collect()
    };
    let want = |old_side: bool| -> Vec<String> {
        if old_side {
            cluster
                .old_rows
                .iter()
                .map(|r| r.text().to_string())
                .collect()
        } else {
            cluster
                .new_rows
                .iter()
                .map(|r| r.text().to_string())
                .collect()
        }
    };
    for old_side in [true, false] {
        let (got, expected) = (side(old_side), want(old_side));
        if got != expected {
            let which = if old_side { "old" } else { "new" };
            return Err(format!(
                "{which} projection mismatch:\n  got:  {got:?}\n  want: {expected:?}"
            ));
        }
    }
    Ok(())
}
