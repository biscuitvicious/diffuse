//! Cluster Dynamic Programming transformation engine (Phase 3).
//!
//! Each [`ChangeCluster`] — an isolated (M deleted, N added) run bounded by
//! context lines or syntax-section barriers (Phase 2) — is solved as a 2D
//! DP over the line matrix. `DP[i][j]` is the optimal decomposition of old
//! lines `o[0..i]` into new lines `n[0..j]`; the transitions (plan §3.2)
//! are:
//!
//! | transition         | step              | rows emitted            |
//! |--------------------|-------------------|-------------------------|
//! | `Mod`              | `(i+1, j+1)`      | 1 `Mod`                 |
//! | `Mov`              | `(i+1, j+1)`      | 1 `Mov` (verbatim pair) |
//! | `Expand` (1-to-K)  | `(i+1, j+K)`      | 1 `Mod` + K−1 `Add`     |
//! | `Collapse` (K-to-1)| `(i+K, j+1)`      | K−1 `Del` + 1 `Mod`     |
//! | `Del`              | `(i+1, j)`        | 1 `Del`                 |
//! | `Add`              | `(i, j+1)`        | 1 `Add`                 |
//!
//! The objective (plan §3.3) is multi-objective:
//!
//! ```text
//! score = α·matched_chars − μ·unmatched_chars − β·row_count
//!       − γ·fragmentation + δ·syntax_alignment
//! ```
//!
//! - `matched_chars` — the token-LCS characters shared by the transition's old
//!   and new sides (whitespace tokens excluded, as everywhere in the engine).
//! - `unmatched_chars` — the character mismatch penalty: content the pairing
//!   failed to account for. This is what makes a garbage `Mod` lose to a
//!   standalone `Del` + `Add`.
//! - `row_count` — visual sprawl: every emitted row costs `β`.
//! - `fragmentation` — block transitions pay `γ` per extra line beyond the
//!   anchor (K−1), plus a softer `γ/2` per non-anchor line that still carries
//!   matched tokens (a reflow spanning lines reads worse than a clean anchor).
//! - `syntax_alignment` — statement-head keyword agreement bonus (`let …`
//!   pairing with `let …`); verbatim pairs take a small fixed bonus so a
//!   semantic `Mov` outranks an equal-scoring `Mod`.
//!
//! The traceback emits the optimal row sequence, and [`verify_projection`]
//! enforces the universal bidirectional invariant (plan Step 3.4): the
//! rows project back to exactly the cluster's old and new lines,
//! byte for byte, in order.
//!
//! The weights are named constants — the tuning surface for the visual
//! quality gates (`tests/AGENTS.md`).

use std::collections::{HashMap, HashSet};

use super::{ChangeCluster, SyntaxSectionKind, classify_syntax_section};
use crate::{
    lcs::{lcs_pairs_ids, tokenize_line},
    parse::Row,
    similarity::TokenInterner,
};

/// Maximum block size K for 1-to-K expansions and K-to-1 collapses.
pub const MAX_BLOCK_K: usize = 10;

/// The multi-objective weights (plan §3.3). Initial tuning; golden review
/// gates any change.
pub struct Weights {
    /// α — value of one matched character.
    pub alpha: f64,
    /// μ — penalty for one unmatched content character.
    pub mu: f64,
    /// β — visual cost of one emitted row.
    pub beta: f64,
    /// γ — fragmentation: per extra sprawl line of a block transition.
    pub gamma: f64,
    /// δ — statement-head syntax agreement (and verbatim-pair) bonus.
    pub delta: f64,
    /// ε — closing-brace proximity bonus for lines near a block end.
    pub epsilon: f64,
    /// Bonus for statement head keyword agreement when similarity is
    /// sufficient.
    pub head_agree_bonus: f64,
    /// Bonus for matching compound block closing delimiters (e.g. `};` or `}`).
    pub close_agree_bonus: f64,
    /// Multiplier on line indentation differences.
    pub indent_penalty_factor: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            alpha: 15.0,
            mu: 8.0,
            beta: 5.0,
            gamma: 2.0,
            delta: 800.0,
            epsilon: 500.0,
            head_agree_bonus: 80.0,
            close_agree_bonus: 250.0,
            indent_penalty_factor: 5.0,
        }
    }
}

/// A subline move: a token that crossed a line boundary inside a block
/// transition — the source token index on the OLD-side anchor line, the
/// target line offset within the NEW-side block (or vice versa for a
/// collapse) and the target token index there. Phase 4's `RunKind::Mov`
/// rendering consumes these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SublineMove {
    /// 0-based line offset of the target within the block (0 = anchor).
    /// For an Expand: which NEW line (in the block) received reflowed content
    /// from the anchor line. For a Collapse: which OLD line (in the block)
    /// had content absorbed into the anchor line.
    pub line: usize,
    /// Character span in the anchor line (the reference line) that was moved.
    pub anchor_span: (usize, usize),
    /// Character span in the target line (at index `line`) that received/was
    /// the moved content.
    pub line_span: (usize, usize),
}

/// One DP transition on the traceback path.
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
        spans: Vec<SublineMove>,
    },
    /// K-to-1 collapse: old lines i..i+k folded into new line j.
    Collapse {
        k: usize,
        matched: usize,
        anchor: usize,
        spans: Vec<SublineMove>,
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

/// The per-line tokenized form the DP works on: interned token ids, char
/// lengths, and each content token's byte offset within the raw line (for
/// later subword-span mapping in Phase 4).
pub struct DpTokens {
    ids: Vec<u32>,
    chars: usize,
    token_chars: Vec<usize>,
    /// Each content token's byte offset — consumed by Phase 4's subword
    /// span mapping (`RunKind::Mov` runs); `expect` so its eventual use
    /// un-silences this attribute.
    #[expect(dead_code)]
    offsets: Vec<usize>,
    trimmed: String,
}

pub fn dp_tokens(interner: &mut TokenInterner, text: &str) -> DpTokens {
    let lt = interner.line_tokens(text);
    // Byte offsets: walk the raw tokens, skipping whitespace-only ones in
    // lockstep with the content tokens.
    let mut offsets = Vec::with_capacity(lt.tokens.len());
    let mut pos = 0usize;
    for t in tokenize_line(text) {
        let start = text[pos..].find(t).map(|off| pos + off).unwrap_or(pos);
        pos = start + t.len();
        if !t.trim().is_empty() {
            offsets.push(start);
        }
    }
    DpTokens {
        ids: lt.ids,
        chars: lt.chars,
        token_chars: lt.token_chars,
        offsets,
        trimmed: lt.trimmed,
    }
}

/// The block LCS: `old`'s tokens vs the concatenation of `block`'s lines.
#[derive(Default)]
pub struct BlockMatch {
    matched: usize,
    /// Per block line: its matched token indices.
    pub per_line: Vec<HashSet<usize>>,
    /// (block line, token) -> old-side token index.
    pub pair_of: HashMap<(usize, usize), usize>,
}

pub fn block_lcs(old: &DpTokens, block: &[DpTokens], interner: &TokenInterner) -> BlockMatch {
    let mut ids = Vec::new();
    let mut coords = Vec::new();
    for (li, lt) in block.iter().enumerate() {
        for (ti, &id) in lt.ids.iter().enumerate() {
            ids.push(id);
            coords.push((li, ti));
        }
    }
    let pairs = lcs_pairs_ids(&old.ids, &ids, &interner.ws, &interner.alnum);
    let mut bm = BlockMatch {
        per_line: vec![HashSet::new(); block.len()],
        ..Default::default()
    };
    // pairs: old token index -> concatenated-block token index.
    let mut matches: Vec<(usize, usize)> = pairs.iter().map(|(&o, &n)| (o, n)).collect();
    matches.sort_unstable();
    for (oi, ni) in matches {
        let (li, ti) = coords[ni];
        bm.matched += block[li].token_chars[ti];
        bm.per_line[li].insert(ti);
        bm.pair_of.insert((li, ti), oi);
    }
    bm
}

fn is_reflow_candidate(text: &str) -> bool {
    !text.trim().is_empty()
}

/// The block line carrying the transition's anchor `Mod`: the line that
/// continues the SOURCE line's head (its first matched token), falling
/// back to the line with the most matched tokens. The plan's §4.2 renders
/// block transitions as the anchor `Mod` followed by the remaining
/// `Add`/`Del` rows — the anchor owns the head, the tail reads as
/// absorbed/reflowed.
pub fn block_anchor(bm: &BlockMatch) -> usize {
    let mut head = None;
    let mut most = 0usize;
    let mut most_n = 0usize;
    for (li, s) in bm.per_line.iter().enumerate() {
        if s.len() > most_n {
            most_n = s.len();
            most = li;
        }
        for &ti in s {
            if bm.pair_of.get(&(li, ti)) == Some(&0) {
                head = Some(li);
            }
        }
    }
    head.unwrap_or(most)
}

/// Subline move spans for a block transition: matched tokens landing on
/// non-anchor lines.
pub fn subline_moves(bm: &BlockMatch, anchor: usize) -> Vec<SublineMove> {
    let mut spans = Vec::new();
    for (li, s) in bm.per_line.iter().enumerate() {
        if li == anchor {
            continue;
        }
        let mut tis: Vec<usize> = s.iter().copied().collect();
        tis.sort_unstable();
        for ti in tis {
            if bm.pair_of.contains_key(&(li, ti)) {
                spans.push(SublineMove {
                    line: li,
                    anchor_span: (0, 0),
                    line_span: (0, 0),
                });
            }
        }
    }
    spans
}

/// Solve one cluster with the default weights.
pub fn solve_cluster(
    cluster: &ChangeCluster,
    profile: Option<&crate::syntax::LangProfile>,
) -> DpSolution {
    solve_cluster_with(cluster, profile, &Weights::default())
}

/// Solve with explicit weights (the tuning surface for tests and Phase 5
/// preference experiments).
pub fn solve_cluster_with(
    cluster: &ChangeCluster,
    profile: Option<&crate::syntax::LangProfile>,
    w: &Weights,
) -> DpSolution {
    let m = cluster.old_rows.len();
    let n = cluster.new_rows.len();

    let mut interner = TokenInterner::new();
    let old: Vec<DpTokens> = cluster
        .old_rows
        .iter()
        .map(|r| dp_tokens(&mut interner, r.text()))
        .collect();
    let new: Vec<DpTokens> = cluster
        .new_rows
        .iter()
        .map(|r| dp_tokens(&mut interner, r.text()))
        .collect();
    let old_comment: Vec<bool> = cluster
        .old_rows
        .iter()
        .map(|r| profile.is_some_and(|p| p.is_comment_line(r.text())))
        .collect();
    let new_comment: Vec<bool> = cluster
        .new_rows
        .iter()
        .map(|r| profile.is_some_and(|p| p.is_comment_line(r.text())))
        .collect();
    let old_kind: Vec<SyntaxSectionKind> = cluster
        .old_rows
        .iter()
        .map(|r| classify_syntax_section(r.text(), profile))
        .collect();
    let new_kind: Vec<SyntaxSectionKind> = cluster
        .new_rows
        .iter()
        .map(|r| classify_syntax_section(r.text(), profile))
        .collect();

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
            false
        }
    };
    let indent_of = |t: &str| -> usize { t.len() - t.trim_start().len() };

    let compute_close_dist = |rows: &[crate::parse::Row], following: &[String]| -> Vec<usize> {
        let count = rows.len();
        (0..count)
            .map(|i| {
                let txt = rows[i].text().trim();
                if is_close_line(txt) {
                    return 0;
                }
                let cur_ind = indent_of(rows[i].text());
                for d in 1..(count - i) {
                    let next_t = rows[i + d].text().trim();
                    if next_t.is_empty() {
                        continue;
                    }
                    if is_close_line(next_t) {
                        return d;
                    }
                    let next_ind = indent_of(rows[i + d].text());
                    if next_ind < cur_ind {
                        return d;
                    }
                }
                for (idx, line) in following.iter().enumerate() {
                    let next_t = line.trim();
                    if next_t.is_empty() {
                        continue;
                    }
                    if is_close_line(next_t) {
                        return count - i + idx;
                    }
                    let next_ind = indent_of(line);
                    if next_ind < cur_ind {
                        return count - i + idx;
                    }
                }
                usize::MAX
            })
            .collect()
    };

    let compute_open_dist = |rows: &[crate::parse::Row], preceding: &[String]| -> Vec<usize> {
        let count = rows.len();
        (0..count)
            .map(|i| {
                if let Some(k) = (0..=i).rev().find(|&k| is_open_line(rows[k].text())) {
                    i - k
                } else {
                    preceding
                        .iter()
                        .rev()
                        .enumerate()
                        .find_map(|(idx, line)| {
                            if is_open_line(line) {
                                Some(i + 1 + idx)
                            } else {
                                None
                            }
                        })
                        .unwrap_or(usize::MAX)
                }
            })
            .collect()
    };

    // Proximity to a block end / closing delimiter / unindent.
    let old_close_dist = compute_close_dist(&cluster.old_rows, &cluster.following_ctx);
    let new_close_dist = compute_close_dist(&cluster.new_rows, &cluster.following_ctx);

    // Proximity to a block start / opening delimiter.
    let old_open_dist = compute_open_dist(&cluster.old_rows, &cluster.preceding_ctx);
    let new_open_dist = compute_open_dist(&cluster.new_rows, &cluster.preceding_ctx);

    #[derive(Clone)]
    struct Cell {
        score: f64,
        back: Option<(usize, usize, &'static str, usize)>, // (i, j, op, k)
    }
    let neg = f64::NEG_INFINITY;
    let mut dp = vec![
        vec![
            Cell {
                score: neg,
                back: None
            };
            n + 1
        ];
        m + 1
    ];
    dp[0][0].score = 0.0;

    let block_score = |bm: &BlockMatch, side_chars: usize, block_chars: usize, k: usize| -> f64 {
        let total = side_chars.max(block_chars);
        let unmatched = total.saturating_sub(bm.matched);
        let moved_lines = bm.per_line.iter().skip(1).filter(|s| !s.is_empty()).count();
        w.alpha * bm.matched as f64
            - w.mu * unmatched as f64
            - w.beta * k as f64
            - w.gamma * (k - 1) as f64
            - w.gamma * moved_lines as f64 * 0.5
    };

    for i in 0..=m {
        for j in 0..=n {
            let base = dp[i][j].score;
            if base == neg {
                continue;
            }
            // Standalone Del / Add.
            if i < m && base - w.beta > dp[i + 1][j].score {
                dp[i + 1][j] = Cell {
                    score: base - w.beta,
                    back: Some((i, j, "del", 1)),
                };
            }
            if j < n && base - w.beta >= dp[i][j + 1].score {
                // `>=`: on a del/add tie the path prefers ending with the
                // add — the emission reads dels first, adds after (the
                // diff reading convention).
                dp[i][j + 1] = Cell {
                    score: base - w.beta,
                    back: Some((i, j, "add", 1)),
                };
            }
            // 1-to-1: Mod or Mov.
            if i < m
                && j < n
                && old_comment[i] == new_comment[j]
                && old_kind[i].is_compatible(new_kind[j])
            {
                let (ot, nt) = (&old[i], &new[j]);
                if ot.trimmed == nt.trimmed {
                    // Verbatim (trim-insensitive) — a Mov.
                    let length_diff = (ot.chars as i64 - nt.chars as i64).abs() as f64;
                    let s = base + w.alpha * ot.chars.max(nt.chars) as f64
                        - w.beta
                        - w.gamma * length_diff;
                    if s > dp[i + 1][j + 1].score {
                        dp[i + 1][j + 1] = Cell {
                            score: s,
                            back: Some((i, j, "mov", 1)),
                        };
                    }
                } else if ot.ids.is_empty() && nt.ids.is_empty() {
                    // blank ↔ blank: a trivial full match in one row.
                    let s = base - w.beta + w.delta;
                    if s > dp[i + 1][j + 1].score {
                        dp[i + 1][j + 1] = Cell {
                            score: s,
                            back: Some((i, j, "mod", 1)),
                        };
                    }
                } else if !ot.ids.is_empty() && !nt.ids.is_empty() {
                    let pairs = lcs_pairs_ids(&ot.ids, &nt.ids, &interner.ws, &interner.alnum);
                    let mut matched = 0usize;
                    for &jt in pairs.values() {
                        matched += nt.token_chars[jt];
                    }
                    let total = ot.chars.max(nt.chars);
                    let unmatched = total.saturating_sub(matched);
                    let statement_head_match = statement_heads_agree(
                        cluster.old_rows[i].text(),
                        cluster.new_rows[j].text(),
                    );
                    let syntax = if statement_head_match {
                        w.head_agree_bonus
                    } else {
                        0.0
                    };
                    // Distance-based closing-brace proximity bonus, tapering with distance
                    let d_old_close = old_close_dist[i];
                    let d_new_close = new_close_dist[j];
                    let proximity_old_close = if d_old_close == usize::MAX {
                        0.0
                    } else {
                        1.0 / (1.0 + (d_old_close as f64).powi(2))
                    };
                    let proximity_new_close = if d_new_close == usize::MAX {
                        0.0
                    } else {
                        1.0 / (1.0 + (d_new_close as f64).powi(2))
                    };
                    let close_bonus = w.epsilon * proximity_old_close * proximity_new_close;
                    // Distance-based opening-brace proximity bonus
                    let d_old_open = old_open_dist[i];
                    let d_new_open = new_open_dist[j];
                    let proximity_old_open = if d_old_open == usize::MAX {
                        0.0
                    } else {
                        1.0 / (1.0 + (d_old_open as f64).powi(2))
                    };
                    let proximity_new_open = if d_new_open == usize::MAX {
                        0.0
                    } else {
                        1.0 / (1.0 + (d_new_open as f64).powi(2))
                    };
                    let open_bonus = w.epsilon * proximity_old_open * proximity_new_open;

                    let indent_old = cluster.old_rows[i].text().len()
                        - cluster.old_rows[i].text().trim_start().len();
                    let indent_new = cluster.new_rows[j].text().len()
                        - cluster.new_rows[j].text().trim_start().len();
                    let indent_diff =
                        (indent_old as isize - indent_new as isize).unsigned_abs() as f64;
                    let indent_penalty = w.gamma * indent_diff * w.indent_penalty_factor;

                    let similarity_ratio = matched as f64 / total as f64;
                    let is_direct_tail_pair =
                        d_old_close == 1 && d_new_close == 1 && indent_diff == 0.0;
                    let effective_close_bonus = if is_direct_tail_pair {
                        w.delta
                    } else {
                        close_bonus
                    };
                    let proximity_gate = if similarity_ratio >= 0.25
                        || is_direct_tail_pair
                        || statement_head_match
                    {
                        1.0
                    } else {
                        0.0
                    };
                    let penalty = w.mu * unmatched as f64;

                    let s = base + w.alpha * matched as f64 - penalty - w.beta - indent_penalty
                        + syntax
                        + (effective_close_bonus + open_bonus) * proximity_gate;
                    if s > dp[i + 1][j + 1].score {
                        dp[i + 1][j + 1] = Cell {
                            score: s,
                            back: Some((i, j, "mod", 1)),
                        };
                    }
                }
            }
            // Expand: old i vs new block j..j+k (k >= 2).
            if i < m
                && j + 2 <= n
                && is_reflow_candidate(cluster.old_rows[i].text())
                && old_comment[i] == new_comment[j]
            {
                let max_k = MAX_BLOCK_K.min(n - j);
                for k in 2..=max_k {
                    let block = &new[j..j + k];
                    if !new[j..j + k].iter().enumerate().all(|(idx, _)| {
                        let text = cluster.new_rows[j + idx].text();
                        let ok = is_reflow_candidate(text);
                        let no_mid_semi = idx == k - 1 || !text.trim().ends_with(';');
                        ok && no_mid_semi && new_comment[j + idx] == old_comment[i]
                    }) {
                        break;
                    }
                    let bm = block_lcs(&old[i], block, &interner);
                    let block_chars: usize = block.iter().map(|t| t.chars).sum();
                    let total = old[i].chars.max(block_chars);
                    if bm.matched < 8 || (bm.matched as f64 / total as f64) < 0.35 {
                        continue;
                    }
                    let anchor = block_anchor(&bm);
                    let syntax = if statement_heads_agree(
                        cluster.old_rows[i].text(),
                        cluster.new_rows[j].text(),
                    ) || statement_heads_agree(
                        cluster.old_rows[i].text(),
                        cluster.new_rows[j + anchor].text(),
                    ) {
                        w.delta
                    } else {
                        0.0
                    };
                    let old_trimmed = cluster.old_rows[i].text().trim();
                    let last_new_trimmed = cluster.new_rows[j + k - 1].text().trim();
                    let close_agree = (old_trimmed.ends_with("};") && last_new_trimmed == "};")
                        || (old_trimmed.ends_with('}') && last_new_trimmed == "}")
                        || (old_trimmed.ends_with(");") && last_new_trimmed == ");")
                        || (old_trimmed.ends_with(')') && last_new_trimmed == ")");
                    let close_bonus = if close_agree {
                        w.close_agree_bonus
                    } else {
                        0.0
                    };

                    let s = base
                        + block_score(&bm, old[i].chars, block_chars, k)
                        + syntax
                        + close_bonus;
                    if s > dp[i + 1][j + k].score {
                        dp[i + 1][j + k] = Cell {
                            score: s,
                            back: Some((i, j, "expand", k)),
                        };
                    }
                }
            }
            // Collapse: old block i..i+k vs new j (k >= 2).
            if j < n
                && i + 2 <= m
                && is_reflow_candidate(cluster.new_rows[j].text())
                && old_comment[i] == new_comment[j]
            {
                let max_k = MAX_BLOCK_K.min(m - i);
                for k in 2..=max_k {
                    let block = &old[i..i + k];
                    if !old[i..i + k].iter().enumerate().all(|(idx, _)| {
                        let text = cluster.old_rows[i + idx].text();
                        let ok = is_reflow_candidate(text);
                        let no_mid_semi = idx == k - 1 || !text.trim().ends_with(';');
                        ok && no_mid_semi && old_comment[i + idx] == new_comment[j]
                    }) {
                        break;
                    }
                    let bm = block_lcs(&new[j], block, &interner);
                    let block_chars: usize = block.iter().map(|t| t.chars).sum();
                    let total = new[j].chars.max(block_chars);
                    if bm.matched < 8 || (bm.matched as f64 / total as f64) < 0.35 {
                        continue;
                    }
                    let anchor = block_anchor(&bm);
                    let syntax = if statement_heads_agree(
                        cluster.old_rows[i].text(),
                        cluster.new_rows[j].text(),
                    ) || statement_heads_agree(
                        cluster.old_rows[i + anchor].text(),
                        cluster.new_rows[j].text(),
                    ) {
                        w.delta
                    } else {
                        0.0
                    };
                    let new_trimmed = cluster.new_rows[j].text().trim();
                    let last_old_trimmed = cluster.old_rows[i + k - 1].text().trim();
                    let close_agree = (new_trimmed.ends_with("};") && last_old_trimmed == "};")
                        || (new_trimmed.ends_with('}') && last_old_trimmed == "}")
                        || (new_trimmed.ends_with(");") && last_old_trimmed == ");")
                        || (new_trimmed.ends_with(')') && last_old_trimmed == ")");
                    let close_bonus = if close_agree {
                        w.close_agree_bonus
                    } else {
                        0.0
                    };

                    let s = base
                        + block_score(&bm, new[j].chars, block_chars, k)
                        + syntax
                        + close_bonus;
                    if s > dp[i + k][j + 1].score {
                        dp[i + k][j + 1] = Cell {
                            score: s,
                            back: Some((i, j, "collapse", k)),
                        };
                    }
                }
            }
        }
    }

    // Traceback: collect (i, j, op, k) from (m, n) back to (0, 0).
    let mut ops: Vec<(usize, usize, &'static str, usize)> = Vec::new();
    let (mut i, mut j) = (m, n);
    while let Some(bp) = dp[i][j].back {
        let (pi, pj, op, k) = bp;
        ops.push((pi, pj, op, k));
        i = pi;
        j = pj;
    }
    ops.reverse();

    // Emit rows + transitions in path order.
    let old_line_of = |idx: usize| match &cluster.old_rows[idx] {
        Row::Del(d) => d.clone(),
        other => panic!("cluster old row is not a Del: {other:?}"),
    };
    let new_line_of = |idx: usize| match &cluster.new_rows[idx] {
        Row::Add(a) => a.clone(),
        other => panic!("cluster new row is not an Add: {other:?}"),
    };

    let mut rows: Vec<Row> = Vec::new();
    let mut transitions: Vec<DpOp> = Vec::new();
    for &(pi, pj, op, k) in &ops {
        match op {
            "del" => {
                rows.push(Row::Del(old_line_of(pi)));
                transitions.push(DpOp::Del);
            }
            "add" => {
                rows.push(Row::Add(new_line_of(pj)));
                transitions.push(DpOp::Add);
            }
            "mov" => {
                rows.push(Row::Mov {
                    old_line: old_line_of(pi),
                    new_line: new_line_of(pj),
                });
                transitions.push(DpOp::Mov);
            }
            "mod" => {
                let (ot, nt) = (&old[pi], &new[pj]);
                let pairs = lcs_pairs_ids(&ot.ids, &nt.ids, &interner.ws, &interner.alnum);
                let mut matched = 0usize;
                for &jt in pairs.values() {
                    matched += nt.token_chars[jt];
                }
                rows.push(Row::Mod {
                    old_line: old_line_of(pi),
                    new_line: new_line_of(pj),
                });
                transitions.push(DpOp::Mod { matched });
            }
            "expand" => {
                let block = &new[pj..pj + k];
                let bm = block_lcs(&old[pi], block, &interner);
                let anchor = block_anchor(&bm);
                transitions.push(DpOp::Expand {
                    k,
                    matched: bm.matched,
                    anchor,
                    spans: subline_moves(&bm, anchor),
                });
                for li in 0..k {
                    let line = new_line_of(pj + li);
                    if li == anchor {
                        rows.push(Row::Mod {
                            old_line: old_line_of(pi),
                            new_line: line,
                        });
                    } else {
                        rows.push(Row::Add(line));
                    }
                }
            }
            "collapse" => {
                let block = &old[pi..pi + k];
                let bm = block_lcs(&new[pj], block, &interner);
                let anchor = block_anchor(&bm);
                transitions.push(DpOp::Collapse {
                    k,
                    matched: bm.matched,
                    anchor,
                    spans: subline_moves(&bm, anchor),
                });
                for li in 0..k {
                    let line = old_line_of(pi + li);
                    if li == anchor {
                        rows.push(Row::Mod {
                            old_line: line,
                            new_line: new_line_of(pj),
                        });
                    } else {
                        rows.push(Row::Del(line));
                    }
                }
            }
            other => unreachable!("unknown op {other}"),
        }
    }

    DpSolution {
        rows,
        transitions,
        score: dp[m][n].score,
    }
}

/// The bidirectional projection verifier (plan Step 3.4): the solution's
/// rows must project to EXACTLY the cluster's old lines (Del texts plus
/// Mod/Mov old texts, in order) and new lines (Add texts plus Mod/Mov new
/// texts) — the universal invariant `Π_old ≡ old ∧ Π_new ≡ new`.
pub fn verify_projection(sol: &DpSolution, cluster: &ChangeCluster) -> Result<(), String> {
    let side = |old_side: bool| -> Vec<String> {
        sol.rows
            .iter()
            .filter_map(|r| match r {
                Row::Del(d) if old_side => Some(d.text.clone()),
                Row::Add(a) if !old_side => Some(a.text.clone()),
                Row::Mod { old_line, new_line } | Row::Mov { old_line, new_line } => {
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

/// Whether two statements share their leading word token (`c.is_alphanumeric()
/// || c == '_'`) — the language-agnostic syntax-alignment bonus.
fn statement_heads_agree(a: &str, b: &str) -> bool {
    fn first_word(t: &str) -> Option<&str> {
        let trimmed = t.trim_start();
        let word_len: usize = trimmed
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .map(char::len_utf8)
            .sum();
        if word_len > 0 {
            Some(&trimmed[..word_len])
        } else {
            None
        }
    }
    matches!((first_word(a), first_word(b)), (Some(x), Some(y)) if x == y)
}
