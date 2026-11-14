//! Core dynamic programming transition matrix and path reconstruction.

use std::hash::{Hash, Hasher};

use rayon::prelude::*;

use diffuse_algo::skyline::{AdaptiveEnvelopeBuilder, SkylineMatrix};
use diffuse_syntax::{
    prelude::{CloseDelimiter, first_word},
    profile::LangProfile,
};

use crate::{
    cluster::{ChangeCluster, classify_syntax_section},
    lcs::{bit_parallel::SuffixPattern, lcs_matched_chars_ids},
    parse::Row,
    similarity::TokenInterner,
};

use super::{
    arena::{DpRowMeta, DpScratchArena, solve_cluster_with_tls},
    block::{
        RowValidBlocks, block_anchor, block_lcs, block_lcs_fast_precomputed,
        precompute_valid_blocks,
    },
    bounds::{
        boundary_salient_chars, cell_touches_band_edge, graded_salient_bound, is_match_arm_line,
        is_prefix_at_boundary, is_reflow_candidate, is_statement_head_keyword, is_statement_start,
        mask_overlap, mod_pair_needs_scoring, token_intersection_bound,
    },
    config::{
        MAX_OFFSET_KEYWORD_ZERO_SALIENT, MIN_RATIO_CHAR_SCALE, MIN_RATIO_CODE, MIN_RATIO_COMMENT,
        MIN_RATIO_COMMENT_OFFSET, MIN_RATIO_KEYWORD_ZERO_SALIENT, Weights, cluster_bandwidth_min,
        cluster_bandwidth_slack, cluster_budget,
    },
    tokens::{DpTokens, calc_common_indent, dp_tokens, trim_trailing_delims},
};
#[derive(Clone, Debug)]
pub enum DpOp {
    /// 1-to-1 in-place modification: old line i ↔ new line j.
    Mod { matched: usize },
    /// Verbatim (trim-insensitive) relocation: old line i ↦ new line j.
    Mov,
    /// 1-to-K expansion: old line i split across new lines j..j+k.
    Expand {
        k: usize,
        matched: usize,
        anchor: usize,
    },
    /// K-to-1 collapse: old lines i..i+k folded into new line j.
    Collapse {
        k: usize,
        matched: usize,
        anchor: usize,
    },
    /// Standalone deletion of old line i.
    Del,
    /// Standalone addition of new line j.
    Add,
}

/// The DP's solution for one cluster: the optimal row stream, the
/// transition path (one entry per transition — a block transition owns
/// several rows), and the path score.
pub struct DpSolution {
    pub rows: Vec<Row>,
    pub transitions: Vec<DpOp>,
    pub score: f64,
}

const PAR_CANDIDATE_THRESHOLD: usize = 2048;

/// Minimum cluster dimension for a narrow-band solve attempt. Below this
/// the legacy band applies directly, so small inputs never change behavior
/// or pay retry overhead. Tunable.
const NARROW_BAND_MIN_ROWS: usize = 1024;

/// Narrow corridor floor ($K$) and slack for large clusters. The legacy
/// floor explores $\pm 64$ rows per line even when the optimal path hugs
/// the diagonal; the narrow attempt shrinks that while the
/// touch-triggered retry guarantees byte-identical fallback. Tunable.
const NARROW_BANDWIDTH_MIN: usize = 16;
const NARROW_BANDWIDTH_SLACK: usize = 8;

const PAR_CANDIDATE_CHUNK_SIZE: usize = 256;

/// Score all candidate pairs, in parallel past [`PAR_CANDIDATE_THRESHOLD`].
/// Results are written into `matrix_out` (indexed via SkylineMatrix corridor)
/// and `scores_out`.
#[allow(clippy::too_many_arguments)]
fn score_lcs_candidates(
    candidates: &[(usize, usize)],
    old_tokens: &[DpTokens],
    new_tokens: &[DpTokens],
    interner: &TokenInterner,
    scores_out: &mut Vec<u32>,
    matrix_out: &mut SkylineMatrix<u32>,
    salient_scores_out: &mut Vec<u32>,
    salient_matrix_out: &mut SkylineMatrix<u32>,
) {
    if candidates.is_empty() {
        return;
    }
    if candidates.len() >= PAR_CANDIDATE_THRESHOLD {
        score_lcs_candidates_par(
            candidates,
            old_tokens,
            new_tokens,
            interner,
            scores_out,
            salient_scores_out,
        );
    } else {
        score_lcs_candidates_seq(
            candidates,
            old_tokens,
            new_tokens,
            interner,
            scores_out,
            salient_scores_out,
        );
    }
    for (&(i, j), (&score, &sal_score)) in candidates
        .iter()
        .zip(scores_out.iter().zip(salient_scores_out.iter()))
    {
        if let Some(cell) = matrix_out.get_mut(i, j) {
            *cell = score;
        }
        if let Some(cell) = salient_matrix_out.get_mut(i, j) {
            *cell = sal_score;
        }
    }
}

pub(crate) fn score_lcs_candidates_seq(
    candidates: &[(usize, usize)],
    old_tokens: &[DpTokens],
    new_tokens: &[DpTokens],
    interner: &TokenInterner,
    scores_out: &mut Vec<u32>,
    salient_scores_out: &mut Vec<u32>,
) {
    scores_out.clear();
    scores_out.reserve(candidates.len());
    salient_scores_out.clear();
    salient_scores_out.reserve(candidates.len());
    let mut scratch: Vec<u64> = Vec::new();
    let mut curr_i = usize::MAX;
    let mut pat = SuffixPattern::Empty;
    for &(i, j) in candidates {
        if i != curr_i {
            curr_i = i;
            pat = SuffixPattern::new(&old_tokens[i].ids);
        }
        let score = pat.match_chars(
            &new_tokens[j].ids,
            &interner.ws,
            &interner.alnum,
            &new_tokens[j].token_chars,
            &mut scratch,
        ) as u32;
        let sal_score = if new_tokens[j].salient_chars > 0 {
            pat.match_chars(
                &new_tokens[j].ids,
                &interner.ws,
                &interner.alnum,
                &new_tokens[j].salient_token_chars,
                &mut scratch,
            ) as u32
        } else {
            0
        };
        scores_out.push(score);
        salient_scores_out.push(sal_score);
    }
}

pub(crate) fn score_lcs_candidates_par(
    candidates: &[(usize, usize)],
    old_tokens: &[DpTokens],
    new_tokens: &[DpTokens],
    interner: &TokenInterner,
    scores_out: &mut Vec<u32>,
    salient_scores_out: &mut Vec<u32>,
) {
    scores_out.resize(candidates.len(), 0);
    salient_scores_out.resize(candidates.len(), 0);
    candidates
        .par_chunks(PAR_CANDIDATE_CHUNK_SIZE)
        .zip(scores_out.par_chunks_mut(PAR_CANDIDATE_CHUNK_SIZE))
        .zip(salient_scores_out.par_chunks_mut(PAR_CANDIDATE_CHUNK_SIZE))
        .for_each_init(
            Vec::<u64>::new,
            |scratch, ((cand_chunk, score_chunk), sal_chunk)| {
                let mut curr_i = usize::MAX;
                let mut pat = SuffixPattern::Empty;
                for (&(i, j), (score_out, sal_score_out)) in cand_chunk
                    .iter()
                    .zip(score_chunk.iter_mut().zip(sal_chunk.iter_mut()))
                {
                    if i != curr_i {
                        curr_i = i;
                        pat = SuffixPattern::new(&old_tokens[i].ids);
                    }
                    *score_out = pat.match_chars(
                        &new_tokens[j].ids,
                        &interner.ws,
                        &interner.alnum,
                        &new_tokens[j].token_chars,
                        scratch,
                    ) as u32;
                    *sal_score_out = if new_tokens[j].salient_chars > 0 {
                        pat.match_chars(
                            &new_tokens[j].ids,
                            &interner.ws,
                            &interner.alnum,
                            &new_tokens[j].salient_token_chars,
                            scratch,
                        ) as u32
                    } else {
                        0
                    };
                }
            },
        );
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(u8)]
pub(crate) enum OpKind {
    #[default]
    None = 0,
    Del,
    Add,
    Mod,
    Mov,
    Expand,
    Collapse,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct Cell {
    score: f64,
    prev_i: u32,
    prev_j: u32,
    k: u8,
    op: OpKind,
    diagonal_streak: u8,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            score: f64::NEG_INFINITY,
            prev_i: 0,
            prev_j: 0,
            k: 0,
            op: OpKind::None,
            diagonal_streak: 0,
        }
    }
}

/// Solve one cluster with the default weights.
pub fn solve_cluster(cluster: &ChangeCluster, profile: Option<&LangProfile>) -> DpSolution {
    solve_cluster_with(cluster, profile, &Weights::default())
}

/// preference experiments).
pub fn solve_cluster_with(
    cluster: &ChangeCluster,
    profile: Option<&LangProfile>,
    w: &Weights,
) -> DpSolution {
    solve_cluster_with_tls(cluster, profile, w)
}

pub(crate) const ANCHOR_DECOMPOSITION_MIN_ROWS: usize = 256;

pub(crate) fn is_admissible_mod_pair(
    del_text: &str,
    add_text: &str,
    profile: Option<&LangProfile>,
) -> bool {
    if del_text == add_text {
        return true;
    }

    let (_spans, runs) = crate::lcs::word_spans_and_runs_with_profile(del_text, add_text, profile);
    let del_has_salient = crate::lcs::line_has_salient_token(del_text, profile);
    let add_has_salient = crate::lcs::line_has_salient_token(add_text, profile);

    if del_has_salient && add_has_salient {
        let has_salient_ctx = runs.iter().any(|(kind, text)| {
            *kind == crate::merge::RunKind::Ctx && crate::lcs::line_has_salient_token(text, profile)
        });
        if !has_salient_ctx {
            // Safety fallback: if neither line shares any salient context, check whether
            // this is an entity declaration statement (let, fn, const, def, struct, …).
            // Declaration keywords do not confer statement identity when both the declared
            // entity name and value expressions are replaced, unless common context
            // accounts for the majority (>= 50%) of the line.
            let is_decl_head = runs.first().is_some_and(|(kind, text)| {
                if *kind != crate::merge::RunKind::Ctx {
                    return false;
                }
                let mut found = false;
                crate::lcs::for_each_token(text, |t| {
                    if !found && profile.is_some_and(|p| p.is_declaration_keyword(t)) {
                        found = true;
                    }
                });
                found
            });
            if is_decl_head {
                let ctx_chars: usize = runs
                    .iter()
                    .filter(|(k, _)| *k == crate::merge::RunKind::Ctx)
                    .map(|(_, t)| t.chars().count())
                    .sum();
                let total_chars = del_text.chars().count().max(add_text.chars().count());
                if ctx_chars * 2 < total_chars {
                    return false;
                }
            }
        }
    } else {
        // Context must not be purely whitespace
        let has_non_ws_ctx = runs
            .iter()
            .any(|(kind, text)| *kind == crate::merge::RunKind::Ctx && !text.trim().is_empty());
        if !has_non_ws_ctx && (!del_text.trim().is_empty() || !add_text.trim().is_empty()) {
            return false;
        }
    }
    true
}

/// Solves a cluster using a reusable scratch arena for zero-allocation
/// buffers.
pub(crate) fn solve_cluster_with_arena(
    cluster: &ChangeCluster,
    profile: Option<&LangProfile>,
    w: &Weights,
    arena: &mut DpScratchArena,
) -> DpSolution {
    let m = cluster.old_rows.len();
    let n = cluster.new_rows.len();

    if arena.forbidden_pairs.is_empty()
        && m >= ANCHOR_DECOMPOSITION_MIN_ROWS
        && n >= ANCHOR_DECOMPOSITION_MIN_ROWS
        && let Some(sol) = solve_cluster_with_anchors(cluster, profile, w, arena)
    {
        return sol;
    }

    arena.forbidden_pairs.clear();
    let mut sol = solve_cluster_matrix(cluster, profile, w, arena);

    // DP Feedback Loop: validate proposed Mod and block Anchor pairings.
    // If any proposed pair degraded to all-whitespace context, forbid it and
    // re-solve.
    for _ in 0..m.min(n).min(16) {
        let mut invalid_pair = None;
        let mut oi = 0usize;
        let mut ni = 0usize;
        for t in &sol.transitions {
            match t {
                DpOp::Del => oi += 1,
                DpOp::Add => ni += 1,
                DpOp::Mod { .. } | DpOp::Mov => {
                    let cur_oi = oi;
                    let cur_ni = ni;
                    oi += 1;
                    ni += 1;
                    if let (Some(r), Some(a)) =
                        (cluster.old_rows.get(cur_oi), cluster.new_rows.get(cur_ni))
                        && !is_admissible_mod_pair(r.text(), a.text(), profile)
                    {
                        invalid_pair = Some((cur_oi, cur_ni));
                        break;
                    }
                }
                DpOp::Expand { k, .. } => {
                    oi += 1;
                    ni += k;
                }
                DpOp::Collapse { k, .. } => {
                    oi += k;
                    ni += 1;
                }
            }
        }

        if let Some((bad_oi, bad_ni)) = invalid_pair {
            arena.forbidden_pairs.insert((bad_oi, bad_ni));
            sol = solve_cluster_matrix(cluster, profile, w, arena);
        } else {
            break;
        }
    }

    sol
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct AnchorTokenSlice<'a> {
    ids: &'a [u32],
    total_chars: usize,
}

impl Hash for AnchorTokenSlice<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ids.hash(state);
    }
}

/// Extracts substantive word tokens from a line, interning each token and
/// appending its ID into `anchor_ids`. Returns `(start_idx, end_idx,
/// total_chars)`.
#[inline]
fn extract_anchor_tokens(
    text: &str,
    interner: &mut TokenInterner,
    anchor_ids: &mut Vec<u32>,
) -> (usize, usize, usize) {
    let start = anchor_ids.len();
    let mut total_chars = 0;
    crate::lcs::for_each_token(text, |t| {
        if t.len() >= 2 && t.chars().all(|c| c.is_alphabetic()) {
            let id = interner.intern(t);
            anchor_ids.push(id);
            total_chars += crate::lcs::char_len(t);
        }
    });
    let end = anchor_ids.len();
    (start, end, total_chars)
}

/// Attempts to solve a large cluster by decomposing it along unique matching
/// anchors (MUMs).
///
/// Returns `None` if the cluster is smaller than
/// [`ANCHOR_DECOMPOSITION_MIN_ROWS`] or if no valid anchors within the corridor
/// bandwidth are found.
pub(crate) fn solve_cluster_with_anchors(
    cluster: &ChangeCluster,
    profile: Option<&LangProfile>,
    w: &Weights,
    arena: &mut DpScratchArena,
) -> Option<DpSolution> {
    let m = cluster.old_rows.len();
    let n = cluster.new_rows.len();
    if m < ANCHOR_DECOMPOSITION_MIN_ROWS || n < ANCHOR_DECOMPOSITION_MIN_ROWS {
        return None;
    }

    let old_trimmed: Vec<&str> = cluster.old_rows.iter().map(|r| r.text().trim()).collect();
    let new_trimmed: Vec<&str> = cluster.new_rows.iter().map(|r| r.text().trim()).collect();

    let max_corridor = cluster_bandwidth_min() + cluster_bandwidth_slack() + m.abs_diff(n);
    let filter_anchors = |raw: Vec<(usize, usize)>| -> Vec<(usize, usize)> {
        let mut valid = Vec::new();
        for (ai, aj) in raw {
            let expected_j = (ai * n) / m;
            if aj.abs_diff(expected_j) > max_corridor {
                continue;
            }
            let old_indent = cluster.old_rows[ai].text().len() - old_trimmed[ai].len();
            let new_indent = cluster.new_rows[aj].text().len() - new_trimmed[aj].len();
            if old_indent.abs_diff(new_indent) > 8 {
                continue;
            }
            valid.push((ai, aj));
        }
        valid
    };

    let raw_anchors =
        diffuse_algo::find_unique_anchors(&old_trimmed, &new_trimmed, |s| s.len() >= 4);
    let mut anchors = filter_anchors(raw_anchors);

    if anchors.is_empty() {
        arena.anchor_token_ids.clear();
        arena.old_anchor_ranges.clear();
        arena.new_anchor_ranges.clear();
        arena.interner.clear();

        for row in &cluster.old_rows {
            let range =
                extract_anchor_tokens(row.text(), &mut arena.interner, &mut arena.anchor_token_ids);
            arena.old_anchor_ranges.push(range);
        }

        for row in &cluster.new_rows {
            let range =
                extract_anchor_tokens(row.text(), &mut arena.interner, &mut arena.anchor_token_ids);
            arena.new_anchor_ranges.push(range);
        }

        let ids = arena.anchor_token_ids.as_slice();
        let old_slices: Vec<AnchorTokenSlice> = arena
            .old_anchor_ranges
            .iter()
            .map(|&(start, end, total_chars)| AnchorTokenSlice {
                ids: &ids[start..end],
                total_chars,
            })
            .collect();
        let new_slices: Vec<AnchorTokenSlice> = arena
            .new_anchor_ranges
            .iter()
            .map(|&(start, end, total_chars)| AnchorTokenSlice {
                ids: &ids[start..end],
                total_chars,
            })
            .collect();

        let raw_skeleton_anchors =
            diffuse_algo::find_unique_anchors(&old_slices, &new_slices, |ts| ts.total_chars >= 16);
        anchors = filter_anchors(raw_skeleton_anchors);
    }

    if anchors.is_empty() {
        return None;
    }

    let mut total_rows = Vec::with_capacity(m + n);
    let mut total_transitions = Vec::with_capacity(m + n);
    let mut total_score = 0.0;

    let mut curr_i = 0;
    let mut curr_j = 0;

    for &(ai, aj) in &anchors {
        if curr_i < ai || curr_j < aj {
            let preceding_ctx = if curr_i == 0 {
                cluster.preceding_ctx.clone()
            } else {
                vec![cluster.old_rows[curr_i - 1].text()]
            };
            let following_ctx = vec![cluster.old_rows[ai].text()];
            let sub_cluster = ChangeCluster {
                preceding_ctx,
                old_rows: cluster.old_rows[curr_i..ai].to_vec(),
                new_rows: cluster.new_rows[curr_j..aj].to_vec(),
                following_ctx,
                del_survival: cluster.del_survival.clone(),
                section: cluster.section,
            };
            let sub_sol = solve_cluster_with_arena(&sub_cluster, profile, w, arena);
            total_rows.extend(sub_sol.rows);
            total_transitions.extend(sub_sol.transitions);
            total_score += sub_sol.score;
        }

        if cluster.old_rows[ai].text().trim() == cluster.new_rows[aj].text().trim() {
            let old_line = match &cluster.old_rows[ai] {
                Row::Del(d) | Row::MovSrc { line: d, .. } => d.clone(),
                other => panic!("cluster old row is not a Del: {other:?}"),
            };
            let new_line = match &cluster.new_rows[aj] {
                Row::Add(a) | Row::MovTgt { line: a, .. } => a.clone(),
                other => panic!("cluster new row is not an Add: {other:?}"),
            };

            total_rows.push(Row::mov_src(old_line));
            total_rows.push(Row::mov_tgt(new_line));
            total_transitions.push(DpOp::Mov);

            let old_chars = cluster.old_rows[ai].text().chars().count();
            let new_chars = cluster.new_rows[aj].text().chars().count();
            let length_diff = (old_chars as f64 - new_chars as f64).abs();
            total_score += w.matched_char_reward * (old_chars.max(new_chars) as f64)
                - w.row_cost
                - w.sprawl_penalty * length_diff;
        } else {
            let preceding_ctx = if ai == 0 {
                cluster.preceding_ctx.clone()
            } else {
                vec![cluster.old_rows[ai - 1].text()]
            };
            let following_ctx = if ai + 1 < m {
                vec![cluster.old_rows[ai + 1].text()]
            } else {
                cluster.following_ctx.clone()
            };
            let sub_cluster = ChangeCluster {
                preceding_ctx,
                old_rows: vec![cluster.old_rows[ai].clone()],
                new_rows: vec![cluster.new_rows[aj].clone()],
                following_ctx,
                del_survival: cluster.del_survival.clone(),
                section: cluster.section,
            };
            let sub_sol = solve_cluster_with_arena(&sub_cluster, profile, w, arena);
            total_rows.extend(sub_sol.rows);
            total_transitions.extend(sub_sol.transitions);
            total_score += sub_sol.score;
        }

        curr_i = ai + 1;
        curr_j = aj + 1;
    }

    if curr_i < m || curr_j < n {
        let preceding_ctx = if curr_i == 0 {
            cluster.preceding_ctx.clone()
        } else {
            vec![cluster.old_rows[curr_i - 1].text()]
        };
        let following_ctx = cluster.following_ctx.clone();
        let sub_cluster = ChangeCluster {
            preceding_ctx,
            old_rows: cluster.old_rows[curr_i..m].to_vec(),
            new_rows: cluster.new_rows[curr_j..n].to_vec(),
            following_ctx,
            del_survival: cluster.del_survival.clone(),
            section: cluster.section,
        };
        let sub_sol = solve_cluster_with_arena(&sub_cluster, profile, w, arena);
        total_rows.extend(sub_sol.rows);
        total_transitions.extend(sub_sol.transitions);
        total_score += sub_sol.score;
    }

    Some(DpSolution {
        score: total_score,
        rows: total_rows,
        transitions: total_transitions,
    })
}

/// Solves a cluster using matrix DP with Skyline envelope.
pub(crate) fn solve_cluster_matrix(
    cluster: &ChangeCluster,
    profile: Option<&LangProfile>,
    w: &Weights,
    arena: &mut DpScratchArena,
) -> DpSolution {
    let m = cluster.old_rows.len();
    let n = cluster.new_rows.len();

    let envelope = AdaptiveEnvelopeBuilder::new(m, n)
        .with_bandwidth_min(cluster_bandwidth_min())
        .with_bandwidth_slack(cluster_bandwidth_slack())
        .with_budget(cluster_budget())
        .build();

    if m == 0 || n == 0 || envelope.total_cells > cluster_budget() {
        let mut rows = Vec::with_capacity(m + n);
        let mut transitions = Vec::with_capacity(m + n);
        for r in &cluster.old_rows {
            match r {
                Row::Del(d) | Row::MovSrc { line: d, .. } => {
                    rows.push(Row::Del(d.clone()));
                    transitions.push(DpOp::Del);
                }
                _ => {}
            }
        }
        for r in &cluster.new_rows {
            match r {
                Row::Add(a) | Row::MovTgt { line: a, .. } => {
                    rows.push(Row::Add(a.clone()));
                    transitions.push(DpOp::Add);
                }
                _ => {}
            }
        }
        return DpSolution {
            score: 0.0,
            rows,
            transitions,
        };
    }

    arena.clear();

    let is_open_line = |t: &str| -> bool {
        if let Some(p) = profile {
            p.is_block_open(t)
        } else {
            false
        }
    };
    let is_close_line = |t: &str| -> bool {
        if let Some(p) = profile {
            p.is_block_close(t) || p.is_expression_close(t)
        } else {
            CloseDelimiter::default_classify_exact(t) != CloseDelimiter::None
        }
    };
    let indent_of = |t: &str| -> usize { t.len() - t.trim_start().len() };
    let classify_close = |t: &str| -> CloseDelimiter {
        if let Some(p) = profile {
            p.classify_close_tail(t)
        } else {
            CloseDelimiter::default_classify_tail(t)
        }
    };
    let classify_exact = |t: &str| -> CloseDelimiter {
        if let Some(p) = profile {
            p.classify_exact_close(t)
        } else {
            CloseDelimiter::default_classify_exact(t)
        }
    };
    let compute_close_dist =
        |rows: &[crate::parse::Row], following: &[&str], out: &mut Vec<usize>| {
            let count = rows.len();
            let get_row_meta = |r: &crate::parse::Row| {
                let txt = r.text().trim();
                (is_close_line(txt), indent_of(r.text()), txt.is_empty())
            };
            let get_foll_meta = |line: &str| {
                let txt = line.trim();
                (is_close_line(txt), indent_of(line), txt.is_empty())
            };

            for i in 0..count {
                let (is_c_i, cur_ind, _) = get_row_meta(&rows[i]);
                if is_c_i {
                    out.push(0);
                    continue;
                }
                let mut found = None;
                let max_d = (count - i).min(64);
                for d in 1..max_d {
                    let (is_c, ind, is_emp) = get_row_meta(&rows[i + d]);
                    if is_emp {
                        continue;
                    }
                    if is_c || ind < cur_ind {
                        found = Some(d);
                        break;
                    }
                }
                if found.is_none() {
                    let max_f = (64usize).saturating_sub(count - i);
                    for (idx, &line) in following.iter().take(max_f).enumerate() {
                        let (is_c, ind, is_emp) = get_foll_meta(line);
                        if is_emp {
                            continue;
                        }
                        if is_c || ind < cur_ind {
                            found = Some(count - i + idx);
                            break;
                        }
                    }
                }
                out.push(found.unwrap_or(usize::MAX));
            }
        };

    let compute_open_dist =
        |rows: &[crate::parse::Row], preceding: &[&str], out: &mut Vec<usize>| {
            let mut last_open_idx: Option<usize> = None;
            let prec_dist = preceding.iter().rev().enumerate().find_map(|(idx, line)| {
                if is_open_line(line) {
                    Some(1 + idx)
                } else {
                    None
                }
            });
            for (i, row) in rows.iter().enumerate() {
                if is_open_line(row.text()) {
                    last_open_idx = Some(i);
                    out.push(0);
                } else if let Some(k) = last_open_idx {
                    out.push(i - k);
                } else if let Some(d) = prec_dist {
                    out.push(i + d);
                } else {
                    out.push(usize::MAX);
                }
            }
        };

    // Proximity to a block end / closing delimiter / unindent.
    compute_close_dist(
        &cluster.old_rows,
        &cluster.following_ctx,
        &mut arena.old_close_dist,
    );
    compute_close_dist(
        &cluster.new_rows,
        &cluster.following_ctx,
        &mut arena.new_close_dist,
    );

    // Proximity to a block start / opening delimiter.
    compute_open_dist(
        &cluster.old_rows,
        &cluster.preceding_ctx,
        &mut arena.old_open_dist,
    );
    compute_open_dist(
        &cluster.new_rows,
        &cluster.preceding_ctx,
        &mut arena.new_open_dist,
    );

    for (i, r) in cluster.old_rows.iter().enumerate() {
        let text = r.text();
        let trimmed = text.trim();
        let dt = dp_tokens(&mut arena.interner, text, profile);
        let comment = profile.is_some_and(|p| p.is_comment_line(text));
        let kind = classify_syntax_section(text, profile);
        let first_word_str = if comment {
            let comment_body = trimmed
                .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
                .trim_start();
            first_word(comment_body)
        } else {
            first_word(text)
        };
        let head_is_keyword = first_word_str.is_some_and(|w| is_statement_head_keyword(w, profile));
        let first_word = first_word_str.map(|w| arena.interner.intern(w));
        let indent = text.len() - text.trim_start().len();
        let close_dist = arena.old_close_dist[i];
        let prox_close = if close_dist == usize::MAX {
            0.0
        } else {
            1.0 / (1.0 + (close_dist as f64).powi(2))
        };
        let open_dist = arena.old_open_dist[i];
        let prox_open = if open_dist == usize::MAX {
            0.0
        } else {
            1.0 / (1.0 + (open_dist as f64).powi(2))
        };
        let is_reflow = is_reflow_candidate(text);
        let ends_with_semi = trimmed.ends_with(';');
        let is_stmt_start = is_statement_start(text, profile);
        let is_arm_terminal = is_match_arm_line(trimmed, profile)
            && (trimmed.ends_with(',') || trimmed.ends_with('}') || trimmed.ends_with("},"));
        let property_key = profile
            .and_then(|p| p.extract_property_key(text))
            .map(|w| arena.interner.intern(w));
        let close_tail = classify_close(text);
        let exact_close = classify_exact(text);
        let is_return_tail = profile.is_some_and(|p| p.is_return_tail_position(trimmed));
        let slot_owner = is_return_tail
            .then(|| {
                profile
                    .and_then(|p| p.current_fn_name(trimmed, &cluster.preceding_ctx))
                    .map(|name| arena.interner.intern(name))
            })
            .flatten();
        let stripped_len = trim_trailing_delims(trimmed, profile).len();
        let trimmed_id = arena.interner.intern(trimmed);
        let delim_delta = crate::collapse::line_delim_delta(text) as i16;

        arena.old_tokens.push(dt);
        arena.old_meta.push(DpRowMeta {
            trimmed_id,
            comment,
            kind,
            first_word,
            indent,
            stripped_len,
            prox_close,
            prox_open,
            is_reflow,
            ends_with_semi,
            is_stmt_start,
            is_arm_terminal,
            property_key,
            close_tail,
            exact_close,
            head_is_keyword,
            is_return_tail,
            slot_owner,
            survives_in_new: r
                .old_line()
                .is_some_and(|n| cluster.del_survival.contains(&n)),
            delim_delta,
        });
    }

    for (j, r) in cluster.new_rows.iter().enumerate() {
        let text = r.text();
        let trimmed = text.trim();
        let dt = dp_tokens(&mut arena.interner, text, profile);
        let comment = profile.is_some_and(|p| p.is_comment_line(text));
        let kind = classify_syntax_section(text, profile);
        let first_word_str = if comment {
            let comment_body = trimmed
                .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
                .trim_start();
            first_word(comment_body)
        } else {
            first_word(text)
        };
        let head_is_keyword = first_word_str.is_some_and(|w| is_statement_head_keyword(w, profile));
        let first_word = first_word_str.map(|w| arena.interner.intern(w));
        let indent = text.len() - text.trim_start().len();
        let close_dist = arena.new_close_dist[j];
        let prox_close = if close_dist == usize::MAX {
            0.0
        } else {
            1.0 / (1.0 + (close_dist as f64).powi(2))
        };
        let open_dist = arena.new_open_dist[j];
        let prox_open = if open_dist == usize::MAX {
            0.0
        } else {
            1.0 / (1.0 + (open_dist as f64).powi(2))
        };
        let is_reflow = is_reflow_candidate(text);
        let ends_with_semi = trimmed.ends_with(';');
        let is_stmt_start = is_statement_start(text, profile);
        let is_arm_terminal = is_match_arm_line(trimmed, profile)
            && (trimmed.ends_with(',') || trimmed.ends_with('}') || trimmed.ends_with("},"));
        let property_key = profile
            .and_then(|p| p.extract_property_key(text))
            .map(|w| arena.interner.intern(w));
        let close_tail = classify_close(text);
        let exact_close = classify_exact(text);
        let is_return_tail = profile.is_some_and(|p| p.is_return_tail_position(trimmed));
        let slot_owner = is_return_tail
            .then(|| {
                profile
                    .and_then(|p| p.current_fn_name(trimmed, &cluster.preceding_ctx))
                    .map(|name| arena.interner.intern(name))
            })
            .flatten();
        let stripped_len = trim_trailing_delims(trimmed, profile).len();
        let trimmed_id = arena.interner.intern(trimmed);
        let delim_delta = crate::collapse::line_delim_delta(text) as i16;

        arena.new_tokens.push(dt);
        arena.new_meta.push(DpRowMeta {
            trimmed_id,
            comment,
            kind,
            first_word,
            indent,
            stripped_len,
            prox_close,
            prox_open,
            is_reflow,
            ends_with_semi,
            is_stmt_start,
            is_arm_terminal,
            property_key,
            close_tail,
            exact_close,
            head_is_keyword,
            is_return_tail,
            slot_owner,
            survives_in_new: false,
            delim_delta,
        });
    }

    // No line-level return-slot affinity: whether a line is a complete
    // terminal expression or a continuation fragment cannot be decided from
    // the line and its neighbours alone. A `return` head can be an
    // initializer, a `)`/`}` close can be an expression continuation, and
    // multi-line string/comment state is not tracked. Content similarity,
    // exact-delimiter tails, and the `) -> T` signature slot cover the
    // structurally sound cases.
    //
    // Return-tail slot claims require an unambiguous slot: exactly one
    // return tail on each side. Two same-shape tails must not claim each
    // other; they fall back to `Del` + `Add`.
    let unique_return_slot = arena.old_meta.iter().filter(|m| m.is_return_tail).count() == 1
        && arena.new_meta.iter().filter(|m| m.is_return_tail).count() == 1;

    // Full matrix solve with the given corridor bounds, reporting whether
    // the optimal traceback touched the band edge. Shared by the narrow
    // attempt and the legacy solve below: tokenize/flatten run once above,
    // matrices re-initialize on every call.
    let mut solve_once = |bounds: &[(usize, usize)]| -> (DpSolution, bool) {
        let DpScratchArena {
            interner,
            old_tokens,
            new_tokens,
            old_meta,
            new_meta,
            all_old_ids,
            all_old_coords,
            all_old_token_chars,
            old_row_token_starts,
            all_new_ids,
            all_new_coords,
            all_new_token_chars,
            new_row_token_starts,
            old_valid_blocks,
            new_valid_blocks,
            lcs_scratch,
            grid,
            precomputed_lcs,
            precomputed_salient_lcs,
            lcs_candidates,
            lcs_scores,
            lcs_salient_scores,
            ..
        } = arena;

        let neg = f64::NEG_INFINITY;
        let init_cell = Cell {
            score: neg,
            prev_i: 0,
            prev_j: 0,
            k: 0,
            op: OpKind::None,
            diagonal_streak: 0,
        };
        grid.init_envelope(m, n, bounds, init_cell);
        precomputed_lcs.init_envelope(m, n, bounds, u32::MAX);
        precomputed_salient_lcs.init_envelope(m, n, bounds, u32::MAX);
        if let Some(cell0) = grid.get_mut(0, 0) {
            cell0.score = 0.0;
        }

        let calc_block_score = |matched: usize,
                                moved_lines: usize,
                                side_chars: usize,
                                block_chars: usize,
                                k: usize|
         -> f64 {
            let total = side_chars.max(block_chars);
            let unmatched = total.saturating_sub(matched);
            w.matched_char_reward * matched as f64
                - w.unmatched_char_penalty * unmatched as f64
                - w.row_cost * k as f64
                - w.sprawl_penalty * (k - 1) as f64
                - w.sprawl_penalty * moved_lines as f64 * 0.5
        };

        // Flatten old tokens
        all_old_ids.clear();
        all_old_coords.clear();
        all_old_token_chars.clear();
        old_row_token_starts.clear();
        for (row_idx, dt) in old_tokens.iter().enumerate() {
            old_row_token_starts.push(all_old_ids.len());
            for (ti, &id) in dt.ids.iter().enumerate() {
                all_old_ids.push(id);
                all_old_coords.push((row_idx as u8, ti as u8));
                all_old_token_chars.push(dt.token_chars[ti]);
            }
        }
        old_row_token_starts.push(all_old_ids.len());

        // Flatten new tokens
        all_new_ids.clear();
        all_new_coords.clear();
        all_new_token_chars.clear();
        new_row_token_starts.clear();
        for (row_idx, dt) in new_tokens.iter().enumerate() {
            new_row_token_starts.push(all_new_ids.len());
            for (ti, &id) in dt.ids.iter().enumerate() {
                all_new_ids.push(id);
                all_new_coords.push((row_idx as u8, ti as u8));
                all_new_token_chars.push(dt.token_chars[ti]);
            }
        }
        new_row_token_starts.push(all_new_ids.len());

        precompute_valid_blocks(old_tokens, old_meta, old_row_token_starts, old_valid_blocks);

        precompute_valid_blocks(new_tokens, new_meta, new_row_token_starts, new_valid_blocks);

        let dummy_tok = DpTokens::default();
        let empty_blocks = RowValidBlocks::default();

        // Phase A: enumerate mod-pair LCS candidates and score them up front
        // (in parallel past PAR_CANDIDATE_THRESHOLD). The fill loop below reads
        // precomputed scores; any pair the enumeration missed falls back to
        // inline scoring, so output is unaffected either way.
        lcs_candidates.clear();
        for i in 0..m {
            for j in grid.row_range(i) {
                if j >= n || !grid.is_in_envelope(i + 1, j + 1) {
                    continue;
                }
                if mod_pair_needs_scoring(
                    i, j, cluster, old_tokens, new_tokens, old_meta, new_meta, interner, w,
                ) {
                    lcs_candidates.push((i, j));
                }
            }
        }
        score_lcs_candidates(
            lcs_candidates,
            old_tokens,
            new_tokens,
            interner,
            lcs_scores,
            precomputed_lcs,
            lcs_salient_scores,
            precomputed_salient_lcs,
        );

        for i in 0..=m {
            let has_old = i < m;
            let (ot, om, old_valid_blocks_i) = if has_old {
                (&old_tokens[i], old_meta[i], &old_valid_blocks[i])
            } else {
                (&dummy_tok, DpRowMeta::default(), &empty_blocks)
            };

            for j in grid.row_range(i) {
                let (base, curr_op, curr_streak) = match grid.get(i, j) {
                    Some(c) if c.score != neg => (c.score, c.op, c.diagonal_streak),
                    _ => continue,
                };
                let is_diagonal_run =
                    (curr_op == OpKind::Mod || curr_op == OpKind::Mov) && curr_streak >= 2;
                // Standalone Del / Add.
                let del_cost = w.row_cost
                    + if is_diagonal_run {
                        w.detour_open_penalty
                    } else {
                        0.0
                    };
                if has_old
                    && let Some(target) = grid.get_mut(i + 1, j)
                    && base - del_cost > target.score
                {
                    *target = Cell {
                        score: base - del_cost,
                        prev_i: i as u32,
                        prev_j: j as u32,
                        k: 1,
                        op: OpKind::Del,
                        diagonal_streak: 0,
                    };
                }
                let add_cost = w.row_cost
                    + if is_diagonal_run {
                        w.detour_open_penalty
                    } else {
                        0.0
                    };
                if j < n
                    && let Some(target) = grid.get_mut(i, j + 1)
                    && base - add_cost >= target.score
                {
                    // `>=`: on a del/add tie the path prefers ending with
                    // the add — the emission reads
                    // dels first, adds after (the
                    // diff reading convention).
                    *target = Cell {
                        score: base - add_cost,
                        prev_i: i as u32,
                        prev_j: j as u32,
                        k: 1,
                        op: OpKind::Add,
                        diagonal_streak: 0,
                    };
                }
                // 1-to-1: Mod or Mov.
                let nm = if j < n {
                    new_meta[j]
                } else {
                    DpRowMeta::default()
                };
                let (continuity_bonus, next_streak) =
                    if curr_op == OpKind::Mod || curr_op == OpKind::Mov {
                        (w.diagonal_run_bonus, curr_streak.saturating_add(1).min(15))
                    } else {
                        (0.0, 1)
                    };
                'eval_mod: {
                    if has_old
                        && j < n
                        && om.comment == nm.comment
                        && om.kind.is_compatible(nm.kind)
                    {
                        if arena.forbidden_pairs.contains(&(i, j)) {
                            break 'eval_mod;
                        }
                        let nt = &new_tokens[j];
                        if ot.ids.is_empty() && nt.ids.is_empty() {
                            // blank ↔ blank: a trivial full match in one row.
                            let s = base + continuity_bonus - w.row_cost + w.slot_continuity_bonus;
                            if let Some(target) = grid.get_mut(i + 1, j + 1)
                                && s > target.score
                            {
                                *target = Cell {
                                    score: s,
                                    prev_i: i as u32,
                                    prev_j: j as u32,
                                    k: 1,
                                    op: OpKind::Mod,
                                    diagonal_streak: next_streak,
                                };
                            }
                        } else if om.trimmed_id == nm.trimmed_id {
                            // Verbatim (trim-insensitive) — a Mov.
                            // Structural parity with Mod scoring: a verbatim
                            // match at a shared structural boundary is the
                            // strongest possible evidence, so Mov earns the
                            // same boundary-proximity bonuses. Without this,
                            // a fuzzy Mod with a close-delimiter bonus (e.g.
                            // `};` vs `}`) outbids verbatim identity and the
                            // DP strands identical lines for worse partners.
                            let mov_close =
                                w.boundary_proximity_bonus * om.prox_close * nm.prox_close;
                            let mov_open = w.boundary_proximity_bonus * om.prox_open * nm.prox_open;
                            let length_diff = (ot.chars as i64 - nt.chars as i64).abs() as f64;
                            let s = base
                                + continuity_bonus
                                + w.matched_char_reward * ot.chars.max(nt.chars) as f64
                                - w.row_cost
                                - w.sprawl_penalty * length_diff
                                + mov_close
                                + mov_open;
                            if let Some(target) = grid.get_mut(i + 1, j + 1)
                                && s > target.score
                            {
                                *target = Cell {
                                    score: s,
                                    prev_i: i as u32,
                                    prev_j: j as u32,
                                    k: 1,
                                    op: OpKind::Mov,
                                    diagonal_streak: next_streak,
                                };
                            }
                        } else if !ot.ids.is_empty() && !nt.ids.is_empty() {
                            let token_overlap = mask_overlap(&ot.token_mask, &nt.token_mask);
                            let min_chars = ot.chars.min(nt.chars);
                            let char_bound = if token_overlap {
                                token_intersection_bound(
                                    &ot.token_multiset,
                                    &nt.token_multiset,
                                    &interner.char_lens,
                                )
                                .min(min_chars)
                            } else {
                                0
                            };
                            // Disjoint lines with zero overlapping tokens and zero
                            // common indentation cannot be in-place
                            // substitutions (wholesale replacements).
                            let mut common_indent: Option<usize> = if char_bound > 0 {
                                None
                            } else if om.indent == 0 || nm.indent == 0 {
                                break 'eval_mod;
                            } else {
                                let ci = calc_common_indent(
                                    cluster.old_rows[i].text(),
                                    cluster.new_rows[j].text(),
                                );
                                if ci == 0 {
                                    break 'eval_mod;
                                }
                                Some(ci)
                            };
                            let total = ot.chars.max(nt.chars);
                            let max_matched = char_bound;

                            let Some(target_diag) = grid.get(i + 1, j + 1) else {
                                break 'eval_mod;
                            };

                            let statement_head_match = match (om.first_word, nm.first_word) {
                                (Some(a), Some(b)) => a == b,
                                _ => false,
                            };
                            let is_property_match =
                                om.property_key.is_some() && om.property_key == nm.property_key;
                            let same_indent_head_match = statement_head_match
                                && om.indent == nm.indent
                                && !om.comment
                                && !nm.comment
                                && om.head_is_keyword
                                && nm.head_is_keyword
                                && !om.survives_in_new;
                            let max_syn = if is_property_match {
                                w.head_agree_bonus + 200.0
                            } else if same_indent_head_match {
                                w.same_indent_head_bonus
                            } else if statement_head_match {
                                w.head_agree_bonus
                            } else {
                                0.0
                            };

                            let indent_shift =
                                (om.indent as isize - nm.indent as isize).unsigned_abs();
                            let offset_diff = (i as isize - j as isize).unsigned_abs();

                            // Return-slot claim: both lines open a function
                            // body through `-> T` and express the same
                            // current function (the signature head on the
                            // line itself, otherwise resolved from the
                            // preceding context) at equal indent in a
                            // unique-slot cluster.
                            let is_return_slot = unique_return_slot
                                && om.is_return_tail
                                && nm.is_return_tail
                                && indent_shift == 0
                                && om.slot_owner.is_some()
                                && om.slot_owner == nm.slot_owner;

                            let min_ratio = if (om.comment || nm.comment) && offset_diff > 0 {
                                MIN_RATIO_COMMENT_OFFSET
                            } else if om.comment || nm.comment {
                                MIN_RATIO_COMMENT
                            } else {
                                MIN_RATIO_CODE
                            };
                            // Salient-free pairs score on the
                            // non-whitespace char scale, so they keep the
                            // legacy floor instead of the salient one.
                            let min_exact = if (om.comment || nm.comment) && offset_diff > 0 {
                                MIN_RATIO_COMMENT_OFFSET
                            } else if om.comment || nm.comment {
                                MIN_RATIO_COMMENT
                            } else {
                                MIN_RATIO_CHAR_SCALE
                            };
                            // The char scale is the exact inverse of the
                            // salient regime: with either line carrying no
                            // salient tokens the salient ratio is undefined,
                            // and the pair is scored on non-whitespace
                            // chars.
                            let is_char_scale_pair = ot.salient_chars == 0 || nt.salient_chars == 0;

                            let boundary_salient = if w.boundary_salient_multiplier > 1.0
                                || w.contiguous_boundary_bonus > 0.0
                            {
                                let bs = boundary_salient_chars(ot, nt);
                                if bs >= w.min_containment_len { bs } else { 0 }
                            } else {
                                0
                            };
                            let salient_bound = if mask_overlap(&ot.salient_mask, &nt.salient_mask)
                            {
                                graded_salient_bound(
                                    &ot.salient_multiset,
                                    &nt.salient_multiset,
                                    &ot.salient_anchored_multiset,
                                    &nt.salient_anchored_multiset,
                                    &ot.salient_buried_containers,
                                    &nt.salient_buried_containers,
                                    &interner.char_lens,
                                    w.embedded_salient_factor,
                                )
                                .min(ot.salient_chars.min(nt.salient_chars))
                            } else {
                                0
                            };
                            let max_salient_similarity_ratio: Option<f64> =
                                if ot.salient_chars == 0 || nt.salient_chars == 0 {
                                    None
                                } else {
                                    let total_salient = ot.salient_chars.max(nt.salient_chars);
                                    let effective_bound = salient_bound as f64
                                        + (w.boundary_salient_multiplier - 1.0)
                                            * boundary_salient as f64;
                                    Some((effective_bound / total_salient as f64).min(1.0))
                                };
                            let can_have_containment = indent_shift <= w.max_indent_shift
                                && (om.comment == nm.comment)
                                && ((om.stripped_len >= w.min_containment_len
                                    && nm.stripped_len >= w.min_containment_len)
                                    || same_indent_head_match);

                            let mut is_prefix_containment = None;
                            let keyword_head_admissible_fast = same_indent_head_match
                                && (salient_bound > 0
                                    || (offset_diff <= MAX_OFFSET_KEYWORD_ZERO_SALIENT
                                        && (char_bound as f64 / total as f64)
                                            >= MIN_RATIO_KEYWORD_ZERO_SALIENT));
                            let could_be_valid_mod_fast = if is_char_scale_pair {
                                (char_bound as f64 / total as f64) >= min_exact
                            } else {
                                max_salient_similarity_ratio.is_some_and(|r| r >= min_ratio)
                                    || is_property_match
                                    || is_return_slot
                                    || keyword_head_admissible_fast
                            };
                            if !could_be_valid_mod_fast {
                                if !can_have_containment {
                                    break 'eval_mod;
                                }
                                let ot_strip = &cluster.old_rows[i].text()
                                    [om.indent..om.indent + om.stripped_len];
                                let nt_strip = &cluster.new_rows[j].text()
                                    [nm.indent..nm.indent + nm.stripped_len];
                                let is_pc = is_prefix_at_boundary(ot_strip, nt_strip)
                                    || is_prefix_at_boundary(nt_strip, ot_strip);
                                if !is_pc {
                                    break 'eval_mod;
                                }
                                is_prefix_containment = Some(true);
                            }

                            let max_close = if is_return_slot {
                                w.slot_continuity_bonus
                            } else {
                                w.boundary_proximity_bonus * om.prox_close * nm.prox_close
                            };
                            let max_open = w.boundary_proximity_bonus * om.prox_open * nm.prox_open;

                            let indent_penalty =
                                w.sprawl_penalty * (indent_shift as f64) * w.indent_penalty_factor;

                            let max_syn_bonus = match is_prefix_containment {
                                Some(true) => max_syn + w.head_agree_bonus,
                                Some(false) => max_syn,
                                None => {
                                    max_syn
                                        + (if can_have_containment {
                                            w.head_agree_bonus
                                        } else {
                                            0.0
                                        })
                                }
                            };
                            let max_possible_mod_s = base
                                + continuity_bonus
                                + w.matched_char_reward * max_matched as f64
                                - w.sprawl_penalty * (total - max_matched) as f64
                                - w.row_cost
                                - indent_penalty
                                - if om.survives_in_new {
                                    w.survives_elsewhere_penalty
                                } else {
                                    0.0
                                }
                                + max_syn_bonus
                                + max_close
                                + max_open;
                            let next_diag_score = target_diag.score;
                            let mod_threshold = (base - 2.0 * w.row_cost).max(next_diag_score);
                            if max_possible_mod_s <= mod_threshold {
                                break 'eval_mod;
                            }

                            let (matched, matched_salient) = if ot.ids == nt.ids {
                                (ot.chars, ot.salient_chars)
                            } else if token_overlap && char_bound > 0 {
                                // Precomputed in parallel up front; recomputed
                                // inline on a miss so
                                // output never depends on enumeration
                                // coverage.
                                let m_all = match precomputed_lcs.get(i, j).copied() {
                                    Some(m) if m != u32::MAX => m as usize,
                                    _ => lcs_matched_chars_ids(
                                        &ot.ids,
                                        &nt.ids,
                                        &interner.ws,
                                        &interner.alnum,
                                        &nt.token_chars,
                                        lcs_scratch,
                                    ),
                                };
                                let m_sal = match precomputed_salient_lcs.get(i, j).copied() {
                                    Some(m) if m != u32::MAX => m as usize,
                                    _ => lcs_matched_chars_ids(
                                        &ot.ids,
                                        &nt.ids,
                                        &interner.ws,
                                        &interner.alnum,
                                        &nt.salient_token_chars,
                                        lcs_scratch,
                                    ),
                                };
                                (m_all, m_sal)
                            } else {
                                (0usize, 0usize)
                            };
                            if matched == 0 {
                                let ci = *common_indent.get_or_insert_with(|| {
                                    if om.indent == 0 || nm.indent == 0 {
                                        0
                                    } else {
                                        calc_common_indent(
                                            cluster.old_rows[i].text(),
                                            cluster.new_rows[j].text(),
                                        )
                                    }
                                });
                                if ci == 0 {
                                    break 'eval_mod;
                                }
                            }

                            let is_prefix_containment = match is_prefix_containment {
                                Some(v) => v,
                                None => {
                                    if can_have_containment {
                                        let ot_strip = &cluster.old_rows[i].text()
                                            [om.indent..om.indent + om.stripped_len];
                                        let nt_strip = &cluster.new_rows[j].text()
                                            [nm.indent..nm.indent + nm.stripped_len];
                                        is_prefix_at_boundary(ot_strip, nt_strip)
                                            || is_prefix_at_boundary(nt_strip, ot_strip)
                                    } else {
                                        false
                                    }
                                }
                            };

                            let unmatched = total.saturating_sub(matched);
                            let (penalty, syntax) = if is_property_match || is_prefix_containment {
                                let core_unmatched = min_chars.saturating_sub(matched);
                                let length_diff = (total - min_chars) as f64;
                                let pen = w.unmatched_char_penalty * core_unmatched as f64
                                    + w.sprawl_penalty * length_diff;
                                let syn = if is_prefix_containment {
                                    max_syn + w.head_agree_bonus
                                } else {
                                    max_syn
                                };
                                (pen, syn)
                            } else {
                                (w.unmatched_char_penalty * unmatched as f64, max_syn)
                            };

                            let close_bonus =
                                w.boundary_proximity_bonus * om.prox_close * nm.prox_close;
                            let open_bonus =
                                w.boundary_proximity_bonus * om.prox_open * nm.prox_open;
                            let indent_diff = indent_shift as f64;
                            let indent_penalty =
                                w.sprawl_penalty * indent_diff * w.indent_penalty_factor;

                            let boundary_salient = if w.boundary_salient_multiplier > 1.0
                                || w.contiguous_boundary_bonus > 0.0
                            {
                                let bs = boundary_salient_chars(ot, nt);
                                if bs >= w.min_containment_len { bs } else { 0 }
                            } else {
                                0
                            };
                            let salient_similarity_ratio: Option<f64> =
                                if ot.salient_chars == 0 || nt.salient_chars == 0 {
                                    None
                                } else {
                                    let total_salient = ot.salient_chars.max(nt.salient_chars);
                                    let effective_matched = matched_salient as f64
                                        + (w.boundary_salient_multiplier - 1.0)
                                            * boundary_salient as f64;
                                    Some((effective_matched / total_salient as f64).min(1.0))
                                };
                            let keyword_head_admissible = same_indent_head_match
                                && (matched_salient > 0
                                    || (offset_diff <= MAX_OFFSET_KEYWORD_ZERO_SALIENT
                                        && (matched as f64 / total as f64)
                                            >= MIN_RATIO_KEYWORD_ZERO_SALIENT));
                            let is_valid_mod_pair = if is_char_scale_pair {
                                (matched as f64 / total as f64) >= min_exact
                            } else if (om.comment || nm.comment) && offset_diff > 0 {
                                is_prefix_containment
                                    || salient_similarity_ratio.is_some_and(|r| r >= 0.65)
                            } else if om.comment || nm.comment {
                                is_prefix_containment
                                    || salient_similarity_ratio.is_some_and(|r| r >= 0.35)
                            } else {
                                salient_similarity_ratio.is_some_and(|r| r >= min_ratio)
                                    || is_prefix_containment
                                    || is_property_match
                                    || is_return_slot
                                    || keyword_head_admissible
                            };

                            if !is_valid_mod_pair {
                                break 'eval_mod;
                            }

                            let effective_close_bonus = if is_return_slot {
                                w.slot_continuity_bonus
                            } else {
                                close_bonus
                            };
                            let proximity_gate = if is_char_scale_pair {
                                if (matched as f64 / total as f64) >= min_exact {
                                    1.0
                                } else {
                                    0.0
                                }
                            } else if salient_similarity_ratio.is_some_and(|r| r >= min_ratio)
                                || is_prefix_containment
                                || is_property_match
                                || is_return_slot
                                || keyword_head_admissible
                            {
                                1.0
                            } else {
                                0.0
                            };

                            // Verbatim twins surviving elsewhere pay the survival
                            // penalty instead of being vetoed: clear content
                            // wins still form, slot-coincidence pairings fall
                            // back to `Del` + `Add`.
                            let survives_penalty = if om.survives_in_new {
                                w.survives_elsewhere_penalty
                            } else {
                                0.0
                            };

                            let boundary_bonus = if w.contiguous_boundary_bonus > 0.0
                                && boundary_salient >= w.min_containment_len
                            {
                                w.contiguous_boundary_bonus
                            } else {
                                0.0
                            };

                            let s =
                                base + continuity_bonus + w.matched_char_reward * matched as f64
                                    - penalty
                                    - w.row_cost
                                    - indent_penalty
                                    - survives_penalty
                                    + syntax
                                    + boundary_bonus
                                    + (effective_close_bonus + open_bonus) * proximity_gate;
                            if let Some(target) = grid.get_mut(i + 1, j + 1)
                                && s > target.score
                            {
                                *target = Cell {
                                    score: s,
                                    prev_i: i as u32,
                                    prev_j: j as u32,
                                    k: 1,
                                    op: OpKind::Mod,
                                    diagonal_streak: next_streak,
                                };
                            }
                        }
                    }
                }
                // Expand: old i vs new block j..j+k (k >= 2).
                if has_old
                    && j < n
                    && ot.chars >= 8
                    && om.is_reflow
                    && om.comment == nm.comment
                    && om.indent == nm.indent
                {
                    for pblock in new_valid_blocks[j].as_slice() {
                        let k = pblock.k;
                        let Some(target_cell) = grid.get(i + 1, j + k) else {
                            continue;
                        };
                        if !mask_overlap(&ot.token_mask, &pblock.mask) {
                            continue;
                        }
                        let total = ot.chars.max(pblock.chars);
                        let min_chars = ot.chars.min(pblock.chars);
                        if min_chars < 8 || (min_chars as f64 / total as f64) < 0.35 {
                            continue;
                        }
                        let char_bound = token_intersection_bound(
                            &ot.token_multiset,
                            &pblock.token_multiset,
                            &interner.char_lens,
                        )
                        .min(min_chars);
                        if char_bound < 8 || (char_bound as f64 / total as f64) < 0.35 {
                            continue;
                        }
                        let max_syntax = if om.first_word.is_some() {
                            w.slot_continuity_bonus
                        } else {
                            0.0
                        };
                        let max_close = if om.close_tail != CloseDelimiter::None {
                            w.close_agree_bonus
                        } else {
                            0.0
                        };
                        let max_possible_s = base
                            + calc_block_score(char_bound, 0, ot.chars, pblock.chars, k)
                            + max_syntax
                            + max_close;
                        let target_score = target_cell.score;
                        let expand_threshold =
                            (base - (1 + k) as f64 * w.row_cost).max(target_score);
                        if max_possible_s <= expand_threshold {
                            continue;
                        }

                        let (bm_matched, bm_moved_lines, anchor) = block_lcs_fast_precomputed(
                            &ot.ids,
                            &all_new_ids[pblock.token_start..pblock.token_end],
                            &all_new_coords[pblock.token_start..pblock.token_end],
                            &all_new_token_chars[pblock.token_start..pblock.token_end],
                            j,
                            k,
                            w.min_block_line_coverage,
                            &interner.ws,
                            &interner.alnum,
                            new_meta[j].is_stmt_start,
                            pblock.opens_container,
                            true,
                            lcs_scratch,
                        );
                        if bm_matched < 8 || (bm_matched as f64 / total as f64) < 0.35 {
                            continue;
                        }
                        if arena.forbidden_pairs.contains(&(i, j + anchor)) {
                            continue;
                        }
                        if pblock.opens_container && (bm_matched as f64 / ot.chars as f64) < 0.50 {
                            continue;
                        }
                        let syntax = if om.first_word.is_some()
                            && (om.first_word == nm.first_word
                                || om.first_word == new_meta[j + anchor].first_word)
                        {
                            w.slot_continuity_bonus
                        } else {
                            0.0
                        };
                        let close_agree = om.close_tail != CloseDelimiter::None
                            && om.close_tail == new_meta[j + k - 1].exact_close;
                        let close_bonus = if close_agree {
                            w.close_agree_bonus
                        } else {
                            0.0
                        };

                        let s =
                            base + calc_block_score(
                                bm_matched,
                                bm_moved_lines,
                                ot.chars,
                                pblock.chars,
                                k,
                            ) + syntax
                                + close_bonus
                                - if is_diagonal_run {
                                    w.detour_open_penalty
                                } else {
                                    0.0
                                };
                        if let Some(target) = grid.get_mut(i + 1, j + k)
                            && s > target.score
                        {
                            *target = Cell {
                                score: s,
                                prev_i: i as u32,
                                prev_j: j as u32,
                                k: k as u8,
                                op: OpKind::Expand,
                                diagonal_streak: 0,
                            };
                        }
                    }
                }
                // Collapse: old block i..i+k vs new j (k >= 2).
                let collapse_indent_diff = (om.indent as isize - nm.indent as isize).unsigned_abs();
                if j < n
                    && has_old
                    && new_tokens[j].chars >= 8
                    && nm.is_reflow
                    && om.comment == nm.comment
                    && collapse_indent_diff <= w.max_indent_shift
                {
                    for pblock in old_valid_blocks_i.as_slice() {
                        let k = pblock.k;
                        let Some(target_cell) = grid.get(i + k, j + 1) else {
                            continue;
                        };
                        if !mask_overlap(&new_tokens[j].token_mask, &pblock.mask) {
                            continue;
                        }
                        let total = new_tokens[j].chars.max(pblock.chars);
                        let min_chars = new_tokens[j].chars.min(pblock.chars);
                        if min_chars < 8 || (min_chars as f64 / total as f64) < 0.35 {
                            continue;
                        }
                        let char_bound = token_intersection_bound(
                            &new_tokens[j].token_multiset,
                            &pblock.token_multiset,
                            &interner.char_lens,
                        )
                        .min(min_chars);
                        if char_bound < 8 || (char_bound as f64 / total as f64) < 0.35 {
                            continue;
                        }
                        let max_syntax = if nm.first_word.is_some() {
                            w.slot_continuity_bonus
                        } else {
                            0.0
                        };
                        let max_close = if nm.close_tail != CloseDelimiter::None {
                            w.close_agree_bonus
                        } else {
                            0.0
                        };
                        let max_possible_s = base
                            + calc_block_score(char_bound, 0, new_tokens[j].chars, pblock.chars, k)
                            + max_syntax
                            + max_close;
                        let target_score = target_cell.score;
                        let collapse_threshold =
                            (base - (k + 1) as f64 * w.row_cost).max(target_score);
                        if max_possible_s <= collapse_threshold {
                            continue;
                        }

                        let (bm_matched, bm_moved_lines, anchor) = block_lcs_fast_precomputed(
                            &new_tokens[j].ids,
                            &all_old_ids[pblock.token_start..pblock.token_end],
                            &all_old_coords[pblock.token_start..pblock.token_end],
                            &all_old_token_chars[pblock.token_start..pblock.token_end],
                            i,
                            k,
                            w.min_block_line_coverage,
                            &interner.ws,
                            &interner.alnum,
                            old_meta[i].is_stmt_start,
                            false,
                            false,
                            lcs_scratch,
                        );
                        if bm_matched < 8 || (bm_matched as f64 / total as f64) < 0.35 {
                            continue;
                        }
                        if arena.forbidden_pairs.contains(&(i, j + anchor)) {
                            continue;
                        }
                        let syntax = if nm.first_word.is_some()
                            && (nm.first_word == om.first_word
                                || nm.first_word == old_meta[i + anchor].first_word)
                        {
                            w.slot_continuity_bonus
                        } else {
                            0.0
                        };
                        let close_agree = nm.close_tail != CloseDelimiter::None
                            && nm.close_tail == old_meta[i + k - 1].exact_close;
                        let close_bonus = if close_agree {
                            w.close_agree_bonus
                        } else {
                            0.0
                        };

                        let s =
                            base + calc_block_score(
                                bm_matched,
                                bm_moved_lines,
                                new_tokens[j].chars,
                                pblock.chars,
                                k,
                            ) + syntax
                                + close_bonus
                                - if is_diagonal_run {
                                    w.detour_open_penalty
                                } else {
                                    0.0
                                };
                        if let Some(target) = grid.get_mut(i + k, j + 1)
                            && s > target.score
                        {
                            *target = Cell {
                                score: s,
                                prev_i: i as u32,
                                prev_j: j as u32,
                                k: k as u8,
                                op: OpKind::Collapse,
                                diagonal_streak: 0,
                            };
                        }
                    }
                }
            }
        }

        // Traceback: collect (i, j, op, k) from (m, n) back to (0, 0),
        // recording whether the optimal path touches the band edge.
        let mut ops: Vec<(usize, usize, OpKind, usize)> = Vec::new();
        let (mut i, mut j) = (m, n);
        let mut touched = cell_touches_band_edge(bounds, n, i, j);
        while let Some(cell) = grid.get(i, j) {
            if cell.op == OpKind::None {
                break;
            }
            ops.push((
                cell.prev_i as usize,
                cell.prev_j as usize,
                cell.op,
                cell.k as usize,
            ));
            i = cell.prev_i as usize;
            j = cell.prev_j as usize;
            touched |= cell_touches_band_edge(bounds, n, i, j);
        }

        if i != 0 || j != 0 {
            let mut rows = Vec::with_capacity(m + n);
            let mut transitions = Vec::with_capacity(m + n);
            for r in &cluster.old_rows {
                match r {
                    Row::Del(d) | Row::MovSrc { line: d, .. } => {
                        rows.push(Row::Del(d.clone()));
                        transitions.push(DpOp::Del);
                    }
                    _ => {}
                }
            }
            for r in &cluster.new_rows {
                match r {
                    Row::Add(a) | Row::MovTgt { line: a, .. } => {
                        rows.push(Row::Add(a.clone()));
                        transitions.push(DpOp::Add);
                    }
                    _ => {}
                }
            }
            // Disconnected path: treat as band contact so the caller retries
            // with the legacy corridor.
            return (
                DpSolution {
                    score: 0.0,
                    rows,
                    transitions,
                },
                true,
            );
        }

        ops.reverse();

        let old = old_tokens;
        let new = new_tokens;

        // Emit rows + transitions in path order.
        let old_line_of = |idx: usize| match &cluster.old_rows[idx] {
            Row::Del(d) | Row::MovSrc { line: d, .. } => d.clone(),
            other => panic!("cluster old row is not a Del: {other:?}"),
        };
        let new_line_of = |idx: usize| match &cluster.new_rows[idx] {
            Row::Add(a) | Row::MovTgt { line: a, .. } => a.clone(),
            other => panic!("cluster new row is not an Add: {other:?}"),
        };

        let mut rows: Vec<Row> = Vec::new();
        let mut transitions: Vec<DpOp> = Vec::new();
        for &(pi, pj, op, k) in &ops {
            match op {
                OpKind::Del => {
                    rows.push(Row::Del(old_line_of(pi)));
                    transitions.push(DpOp::Del);
                }
                OpKind::Add => {
                    rows.push(Row::Add(new_line_of(pj)));
                    transitions.push(DpOp::Add);
                }
                OpKind::Mov => {
                    rows.push(Row::mov_src(old_line_of(pi)));
                    rows.push(Row::mov_tgt(new_line_of(pj)));
                    transitions.push(DpOp::Mov);
                }
                OpKind::Mod => {
                    rows.push(Row::mod_row(old_line_of(pi), new_line_of(pj)));
                    transitions.push(DpOp::Mod { matched: 0 });
                }
                OpKind::Expand => {
                    let bm = block_lcs(&old[pi], &new[pj..pj + k], interner);
                    let anchor = block_anchor(&bm, new_meta[pj].is_stmt_start);

                    for idx in 0..k {
                        let nl = new_line_of(pj + idx);
                        if idx == anchor {
                            rows.push(Row::mod_row(old_line_of(pi), nl));
                        } else {
                            rows.push(Row::Add(nl));
                        }
                    }
                    transitions.push(DpOp::Expand {
                        matched: bm.matched,
                        k,
                        anchor,
                    });
                }
                OpKind::Collapse => {
                    let bm = block_lcs(&new[pj], &old[pi..pi + k], interner);
                    let anchor = block_anchor(&bm, old_meta[pi].is_stmt_start);

                    for idx in 0..k {
                        let ol = old_line_of(pi + idx);
                        if idx == anchor {
                            rows.push(Row::mod_row(ol, new_line_of(pj)));
                        } else {
                            rows.push(Row::Del(ol));
                        }
                    }
                    transitions.push(DpOp::Collapse {
                        matched: bm.matched,
                        k,
                        anchor,
                    });
                }
                OpKind::None => unreachable!(),
            }
        }

        (
            DpSolution {
                rows,
                transitions,
                score: grid.get(m, n).map_or(0.0, |c| c.score),
            },
            touched,
        )
    };

    // Adaptive corridor narrowing for large clusters: attempt a narrow band
    // first and accept it only if the optimal path never touches the band
    // edge (hence equals the unconstrained optimum). Otherwise fall back to
    // the legacy band, byte-identical to previous behavior.
    if m.min(n) >= NARROW_BAND_MIN_ROWS {
        let narrow = AdaptiveEnvelopeBuilder::new(m, n)
            .with_bandwidth_min(NARROW_BANDWIDTH_MIN)
            .with_bandwidth_slack(NARROW_BANDWIDTH_SLACK)
            .with_budget(cluster_budget())
            .build();
        let legacy_cells = envelope.total_cells;
        if narrow.total_cells * 2 <= legacy_cells {
            let (sol, touched) = solve_once(&narrow.bounds);
            if !touched {
                return sol;
            }
        }
    }
    solve_once(&envelope.bounds).0
}
