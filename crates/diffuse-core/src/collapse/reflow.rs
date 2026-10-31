//! Verbatim reflow filtering and context row synthesis pass (Pass 3).

use rustc_hash::FxHashSet;

use diffuse_syntax::profile::LangProfile;

use crate::parse::{AddLine, CtxLine, DelLine, Hunk, Row};

use super::invariants::check_reflow_cluster_confinement;

pub struct ReflowFilter {
    pub meaningful_dels: Vec<usize>,
    pub kept_adds: Vec<usize>,
    /// Context rows synthesized from verbatim-reflow del/add pairs: identical
    /// content that the collapse absorbs as unchanged rather than dropping,
    /// keeping the visible row stream contiguous.
    pub reflow_ctx: Vec<Row>,
}

/// Pass 3: Filter verbatim reflow dels and adds.
pub fn pass_filter_reflows(
    h: &Hunk,
    out: &[Row],
    kept_dels: &[usize],
    pure_adds: &[usize],
    profile: Option<&LangProfile>,
) -> ReflowFilter {
    // Del rows render unless a Collapse block member (skipped in
    // assembly — member text joins the anchor's del_text) or a verbatim
    // reflow pair below claims them. (Containment-absorbing run members
    // into a 1-to-1 anchor would drop old lines silently: the anchor's
    // del_text never displays them. There is deliberately no absorption
    // step here.)
    // Candidate dels that could pair with verbatim standalone adds.
    let candidate_dels: Vec<usize> = kept_dels
        .iter()
        .copied()
        .filter(|di| !h.rows[*di].is_moved())
        .collect();

    // Candidate pure adds that are not moved.
    let candidate_adds: Vec<usize> = pure_adds
        .iter()
        .copied()
        .filter(|&ai| !h.rows[ai].is_moved())
        .collect();

    // 3. Pair verbatim (identical text) standalone del/add lines into Ctx rows.
    let mut reflow_ctx: Vec<Row> = Vec::new();
    let mut used_dels: FxHashSet<usize> = FxHashSet::default();
    let mut used_adds: FxHashSet<usize> = FxHashSet::default();

    // Precompute cluster identifier per row in `h.rows`. Rows separated by
    // `Row::Ctx` belong to different clusters; reflow pairs must never cross
    // context boundaries.
    let mut cluster_id = 0usize;
    let mut row_cluster: Vec<usize> = Vec::with_capacity(h.rows.len());
    for r in &h.rows {
        if matches!(r, Row::Ctx(_)) {
            cluster_id += 1;
            row_cluster.push(usize::MAX);
        } else {
            row_cluster.push(cluster_id);
        }
    }

    for &ai in &candidate_adds {
        let a = &h.rows[ai];
        let at = a.text().trim();
        if at.is_empty() {
            continue;
        }
        for &di in &candidate_dels {
            if used_dels.contains(&di) {
                continue;
            }
            if row_cluster[ai] != row_cluster[di] || row_cluster[ai] == usize::MAX {
                continue;
            }
            let (Some(dold), Some(anew)) = (h.rows[di].old_line(), a.new_line()) else {
                continue;
            };
            if h.rows[di].text() == a.text() {
                // Synthesizing a Ctx row requires strict monotonicity against all
                // rows with old and new lines in `out` and prior `reflow_ctx`.
                let crosses_any = out.iter().chain(reflow_ctx.iter()).any(|m| {
                    if let (Some(m_old), Some(m_new)) = (m.old_line(), m.new_line()) {
                        (m_new < anew && m_old >= dold) || (m_new > anew && m_old <= dold)
                    } else {
                        false
                    }
                });
                if crosses_any {
                    continue;
                }
                used_dels.insert(di);
                used_adds.insert(ai);
                reflow_ctx.push(Row::Ctx(CtxLine::new(dold, anew, a.text().to_string())));
                break;
            } else if h.rows[di].text().trim() == at {
                // Synthesizing a Mod row cannot invert order relative to existing Ctx rows.
                let crosses_ctx = out.iter().chain(reflow_ctx.iter()).any(|m| {
                    if matches!(m, Row::Ctx(_))
                        && let (Some(c_old), Some(c_new)) = (m.old_line(), m.new_line())
                    {
                        (c_new < anew && c_old >= dold) || (c_new > anew && c_old <= dold)
                    } else {
                        false
                    }
                });
                if crosses_ctx {
                    continue;
                }
                used_dels.insert(di);
                used_adds.insert(ai);
                let (spans, runs) = crate::lcs::word_spans_and_runs_with_profile(
                    h.rows[di].text(),
                    a.text(),
                    profile,
                );
                reflow_ctx.push(Row::Mod {
                    old_line: DelLine::new(
                        h.rows[di].old_line().expect("old line must be present"),
                        h.rows[di].text().to_string(),
                    ),
                    new_line: AddLine::new(
                        a.new_line().expect("new line must be present"),
                        a.text(),
                    ),
                    spans,
                    cached_runs: runs,
                    is_anchor: false,
                });
                break;
            }
        }
    }

    // 4. Meaningful dels: candidate dels that were NOT converted to Ctx,
    // plus moved dels (relocations always render).
    let meaningful: Vec<usize> = kept_dels
        .iter()
        .copied()
        .filter(|di| h.rows[*di].is_moved() || !used_dels.contains(di))
        .collect();

    // 5. Kept adds: pure_adds that survived (moved adds + adds not converted to
    //    Ctx).
    let kept_adds: Vec<usize> = pure_adds
        .iter()
        .copied()
        .filter(|ai| {
            let a = &h.rows[*ai];
            if a.is_moved() {
                return true;
            }
            !used_adds.contains(ai)
        })
        .collect();

    let result = ReflowFilter {
        meaningful_dels: meaningful,
        kept_adds,
        reflow_ctx,
    };

    crate::ensure!(
        check_reflow_cluster_confinement(h, &result).is_ok(),
        "{}",
        check_reflow_cluster_confinement(h, &result)
            .err()
            .unwrap_or_default()
    );

    result
}
