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

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::{ChangeCluster, SyntaxSectionKind, classify_syntax_section};
use crate::{
    algo::skyline::{AdaptiveEnvelopeBuilder, SkylineMatrix},
    lcs::{
        bit_parallel::{BitVector, SuffixPattern},
        lcs_matched_chars_ids, lcs_pairs_ids,
    },
    parse::Row,
    similarity::TokenInterner,
    syntax::{
        prelude::{CloseDelimiter, first_ascii_word},
        profile::{LangProfile, PropertySyntax},
    },
};

/// Maximum block size K for 1-to-K expansions and K-to-1 collapses.
pub const MAX_BLOCK_K: usize = 10;

pub const DEFAULT_MIN_CONTAINMENT_LEN: usize = 8;

/// Default maximum cluster cell budget allowed for DP pairing before falling
/// back to wholesale deletion/addition.
pub const DEFAULT_CLUSTER_BUDGET: usize = 10_000_000;
pub const DEFAULT_CLUSTER_PRODUCT_CAP: usize = DEFAULT_CLUSTER_BUDGET;

/// Default minimum corridor half-bandwidth (K_min) for Block-DP exploration.
pub const DEFAULT_CLUSTER_BANDWIDTH_MIN: usize = 64;

/// Default bandwidth slack added to sequence length difference |M - N|.
pub const DEFAULT_CLUSTER_BANDWIDTH_SLACK: usize = 32;

thread_local! {
    static CLUSTER_BUDGET: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_CLUSTER_BUDGET) };
    static CLUSTER_BANDWIDTH_MIN: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_CLUSTER_BANDWIDTH_MIN) };
    static CLUSTER_BANDWIDTH_SLACK: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_CLUSTER_BANDWIDTH_SLACK) };
}

/// Set the maximum cluster cell budget allowed for DP pairing before falling
/// back.
pub fn set_cluster_budget(budget: usize) {
    CLUSTER_BUDGET.with(|c| c.set(budget));
}

/// Get the current cluster cell budget.
pub fn cluster_budget() -> usize {
    CLUSTER_BUDGET.with(|c| c.get())
}

/// Backwards-compatible alias for set_cluster_budget.
pub fn set_cluster_product_cap(cap: usize) {
    set_cluster_budget(cap);
}

/// Backwards-compatible alias for cluster_budget.
pub fn cluster_product_cap() -> usize {
    cluster_budget()
}

/// Set the minimum corridor half-bandwidth (K_min).
pub fn set_cluster_bandwidth_min(min: usize) {
    CLUSTER_BANDWIDTH_MIN.with(|c| c.set(min));
}

/// Get the minimum corridor half-bandwidth (K_min).
pub fn cluster_bandwidth_min() -> usize {
    CLUSTER_BANDWIDTH_MIN.with(|c| c.get())
}

/// Set the bandwidth slack added to |M - N|.
pub fn set_cluster_bandwidth_slack(slack: usize) {
    CLUSTER_BANDWIDTH_SLACK.with(|c| c.set(slack));
}

/// Get the bandwidth slack added to |M - N|.
pub fn cluster_bandwidth_slack() -> usize {
    CLUSTER_BANDWIDTH_SLACK.with(|c| c.get())
}

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
    /// Minimum trimmed character length for 100% prefix containment bonus.
    pub min_containment_len: usize,
    /// Maximum indentation shift (in characters) permitted for prefix
    /// containment and block reflow before being disqualified.
    pub max_indent_shift: usize,
    /// Minimum character count required for direct closing delimiter tail
    /// pairing.
    pub min_tail_matched_chars: usize,
    /// Minimum ratio of lines in a 1-to-K or K-to-1 block that must have
    /// matching content.
    pub min_block_line_coverage: f64,
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
            min_containment_len: DEFAULT_MIN_CONTAINMENT_LEN,
            max_indent_shift: 16,
            min_tail_matched_chars: 8,
            min_block_line_coverage: 0.50,
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
    token_mask: [u64; 4],
    token_multiset: SmallVec<[(u32, u16); 16]>,
}

#[inline(always)]
fn mask_overlap(a: &[u64; 4], b: &[u64; 4]) -> bool {
    ((a[0] & b[0]) | (a[1] & b[1]) | (a[2] & b[2]) | (a[3] & b[3])) != 0
}

#[inline(always)]
fn token_intersection_bound(a: &[(u32, u16)], b: &[(u32, u16)], char_lens: &[u32]) -> usize {
    let mut i = 0;
    let mut j = 0;
    let mut bound = 0;
    while i < a.len() && j < b.len() {
        let (id_a, count_a) = a[i];
        let (id_b, count_b) = b[j];
        if id_a == id_b {
            let count = count_a.min(count_b) as usize;
            bound += (char_lens[id_a as usize] as usize) * count;
            i += 1;
            j += 1;
        } else if id_a < id_b {
            i += 1;
        } else {
            j += 1;
        }
    }
    bound
}

/// Strip trailing delimiters for prefix-containment checks, shared by the
/// fill loop and the parallel candidate pre-pass below.
fn trim_trailing_delims<'a>(t: &'a str, profile: Option<&LangProfile>) -> &'a str {
    if let Some(p) = profile {
        p.trim_trailing_delimiters(t)
    } else {
        t.trim_end_matches(|c: char| {
            c.is_whitespace() || matches!(c, '{' | '}' | ';' | ':' | ',' | ')' | ']')
        })
    }
}

/// Whether `longer` starts with `shorter` at a token boundary, shared by
/// the fill loop and the parallel candidate pre-pass below.
fn is_prefix_at_boundary(longer: &str, shorter: &str) -> bool {
    if let Some(rest) = longer.strip_prefix(shorter) {
        rest.is_empty()
            || shorter.ends_with(|c: char| !c.is_alphanumeric() && c != '_')
            || rest.starts_with(|c: char| c.is_whitespace() || (!c.is_alphanumeric() && c != '_'))
    } else {
        false
    }
}

/// Calculate count of common leading whitespace characters between two lines.
#[inline]
fn calc_common_indent(old_text: &str, new_text: &str) -> usize {
    old_text
        .chars()
        .take_while(|c| c.is_whitespace())
        .zip(new_text.chars().take_while(|c| c.is_whitespace()))
        .take_while(|(c1, c2)| c1 == c2)
        .count()
}

/// Minimum candidate count for parallel subline scoring. Below this the
/// Rayon dispatch overhead outweighs the gain and scoring stays inline.
/// Tunable; chosen so typical hunks (tens of pairs) never pay for threads
/// while massive corridors (hundreds of thousands) parallelize.
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

/// Whether traceback cell $(i, j)$ touches the band edge: the column sits
/// on the row's corridor boundary strictly inside the grid (natural grid
/// edges $j = 0$ and $j = N$ never count). Missing bounds defensively
/// count as touched so an undersized envelope always retries.
pub(crate) fn cell_touches_band_edge(
    bounds: &[(usize, usize)],
    n: usize,
    i: usize,
    j: usize,
) -> bool {
    match bounds.get(i) {
        Some(&(start, end)) => (j == start && start > 0) || (j == end && end < n),
        None => true,
    }
}

/// Grid-independent preconditions for a mod pair `(i, j)` to reach subline
/// LCS scoring in the fill loop below.
///
/// MUST stay in sync with the guard chain guarding the `lcs_matched_chars_ids`
/// call: over-inclusion only wastes parallel work, under-inclusion falls back
/// to inline scoring via the precomputed map miss path — either way the
/// emitted pairing is unaffected. The `mod_pair_candidate_count` test locks
/// the enumeration on a fixed fixture so guard drift surfaces immediately.
#[allow(clippy::too_many_arguments)]
pub(crate) fn mod_pair_needs_scoring(
    i: usize,
    j: usize,
    cluster: &ChangeCluster,
    old_tokens: &[DpTokens],
    new_tokens: &[DpTokens],
    old_meta: &[DpRowMeta],
    new_meta: &[DpRowMeta],
    interner: &TokenInterner,
    profile: Option<&LangProfile>,
    w: &Weights,
) -> bool {
    let ot = &old_tokens[i];
    let om = old_meta[i];
    let nt = &new_tokens[j];
    let nm = new_meta[j];
    if om.comment != nm.comment || !om.kind.is_compatible(nm.kind) {
        return false;
    }
    if ot.ids.is_empty() && nt.ids.is_empty() {
        return false;
    }
    if om.trimmed_id == nm.trimmed_id {
        return false;
    }
    if ot.ids.is_empty() || nt.ids.is_empty() {
        return false;
    }
    let token_overlap = mask_overlap(&ot.token_mask, &nt.token_mask);
    if !token_overlap {
        return false;
    }
    let min_chars = ot.chars.min(nt.chars);
    let char_bound =
        token_intersection_bound(&ot.token_multiset, &nt.token_multiset, &interner.char_lens)
            .min(min_chars);
    if char_bound == 0 {
        return false;
    }

    let total = ot.chars.max(nt.chars);
    let max_matched = char_bound;
    let statement_head_match = match (om.first_ascii_word, nm.first_ascii_word) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    let is_property_match = om.property_key.is_some() && om.property_key == nm.property_key;
    let indent_shift = (om.indent as isize - nm.indent as isize).unsigned_abs();
    let offset_diff = (i as isize - j as isize).unsigned_abs();

    let min_ratio = if (om.comment || nm.comment) && offset_diff > 0 {
        0.65
    } else if om.comment || nm.comment {
        0.35
    } else {
        0.25
    };
    let could_be_tail_pair = if om.close_dist == 1 && nm.close_dist == 1 {
        if om.close_tail != CloseDelimiter::None && nm.close_tail != CloseDelimiter::None {
            if let Some(p) = profile {
                p.is_direct_tail_match(
                    om.close_tail,
                    nm.close_tail,
                    max_matched,
                    w.min_tail_matched_chars,
                )
            } else {
                CloseDelimiter::default_is_direct_tail_match(
                    om.close_tail,
                    nm.close_tail,
                    max_matched,
                    w.min_tail_matched_chars,
                )
            }
        } else if om.is_terminal_return && nm.is_terminal_return {
            if indent_shift == 0 {
                let has_common_indent = if om.indent == 0 || nm.indent == 0 {
                    false
                } else {
                    calc_common_indent(cluster.old_rows[i].text(), cluster.new_rows[j].text()) > 0
                };
                if has_common_indent {
                    true
                } else {
                    max_matched >= w.min_tail_matched_chars
                }
            } else {
                max_matched >= w.min_tail_matched_chars
            }
        } else {
            false
        }
    } else {
        false
    };
    let max_similarity_ratio = max_matched as f64 / total as f64;
    let can_have_containment = indent_shift <= w.max_indent_shift
        && (om.comment == nm.comment)
        && om.stripped_len >= w.min_containment_len
        && nm.stripped_len >= w.min_containment_len;

    let could_be_valid_mod_fast = max_similarity_ratio >= min_ratio
        || is_property_match
        || statement_head_match
        || could_be_tail_pair;

    let mut is_prefix_containment = None;
    if !could_be_valid_mod_fast {
        if !can_have_containment {
            return false;
        }
        let ot_strip = &cluster.old_rows[i].text()[om.indent..om.indent + om.stripped_len];
        let nt_strip = &cluster.new_rows[j].text()[nm.indent..nm.indent + nm.stripped_len];
        let is_pc =
            is_prefix_at_boundary(ot_strip, nt_strip) || is_prefix_at_boundary(nt_strip, ot_strip);
        if !is_pc {
            return false;
        }
        is_prefix_containment = Some(true);
    }

    let max_syn = if is_property_match {
        w.head_agree_bonus + 200.0
    } else if statement_head_match {
        w.head_agree_bonus
    } else {
        0.0
    };
    let max_close = if could_be_tail_pair {
        w.delta
    } else {
        w.epsilon * om.prox_close * nm.prox_close
    };
    let max_open = w.epsilon * om.prox_open * nm.prox_open;
    let indent_penalty = w.gamma * (indent_shift as f64) * w.indent_penalty_factor;
    let base_delta = w.alpha * max_matched as f64
        - w.gamma * (total - max_matched) as f64
        - w.beta
        - indent_penalty
        + max_syn
        + max_close
        + max_open;

    if base_delta <= -2.0 * w.beta {
        if is_prefix_containment == Some(false)
            || (!can_have_containment && is_prefix_containment.is_none())
            || (base_delta + w.head_agree_bonus <= -2.0 * w.beta)
        {
            return false;
        }
        let is_pc = match is_prefix_containment {
            Some(v) => v,
            None => {
                let ot_strip = &cluster.old_rows[i].text()[om.indent..om.indent + om.stripped_len];
                let nt_strip = &cluster.new_rows[j].text()[nm.indent..nm.indent + nm.stripped_len];
                is_prefix_at_boundary(ot_strip, nt_strip)
                    || is_prefix_at_boundary(nt_strip, ot_strip)
            }
        };
        if !is_pc {
            return false;
        }
    }

    // Pairs the fill loop scores inline without LCS (identical token
    // streams, or nothing to align) need no precomputation.
    ot.ids != nt.ids
}

const PAR_CANDIDATE_CHUNK_SIZE: usize = 256;

/// Score all candidate pairs, in parallel past [`PAR_CANDIDATE_THRESHOLD`].
/// Results are written into `matrix_out` (indexed via SkylineMatrix corridor)
/// and `scores_out`.
fn score_lcs_candidates(
    candidates: &[(usize, usize)],
    old_tokens: &[DpTokens],
    new_tokens: &[DpTokens],
    interner: &TokenInterner,
    scores_out: &mut Vec<u32>,
    matrix_out: &mut SkylineMatrix<u32>,
) {
    if candidates.is_empty() {
        return;
    }
    if candidates.len() >= PAR_CANDIDATE_THRESHOLD {
        score_lcs_candidates_par(candidates, old_tokens, new_tokens, interner, scores_out);
    } else {
        score_lcs_candidates_seq(candidates, old_tokens, new_tokens, interner, scores_out);
    }
    for (&(i, j), &score) in candidates.iter().zip(scores_out.iter()) {
        if let Some(cell) = matrix_out.get_mut(i, j) {
            *cell = score;
        }
    }
}

pub(crate) fn score_lcs_candidates_seq(
    candidates: &[(usize, usize)],
    old_tokens: &[DpTokens],
    new_tokens: &[DpTokens],
    interner: &TokenInterner,
    scores_out: &mut Vec<u32>,
) {
    scores_out.clear();
    scores_out.reserve(candidates.len());
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
        scores_out.push(score);
    }
}

pub(crate) fn score_lcs_candidates_par(
    candidates: &[(usize, usize)],
    old_tokens: &[DpTokens],
    new_tokens: &[DpTokens],
    interner: &TokenInterner,
    scores_out: &mut Vec<u32>,
) {
    scores_out.resize(candidates.len(), 0);
    candidates
        .par_chunks(PAR_CANDIDATE_CHUNK_SIZE)
        .zip(scores_out.par_chunks_mut(PAR_CANDIDATE_CHUNK_SIZE))
        .for_each_init(Vec::<u64>::new, |scratch, (cand_chunk, score_chunk)| {
            let mut curr_i = usize::MAX;
            let mut pat = SuffixPattern::Empty;
            for (&(i, j), score_out) in cand_chunk.iter().zip(score_chunk.iter_mut()) {
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
            }
        });
}

pub fn dp_tokens(interner: &mut TokenInterner, text: &str) -> DpTokens {
    let mut ids = Vec::new();
    let mut token_chars = Vec::new();
    let mut token_mask = [0u64; 4];
    let mut chars = 0;
    crate::lcs::for_each_token(text, |t| {
        if !t.trim().is_empty() {
            let id = interner.intern(t);
            ids.push(id);
            let tc = crate::lcs::char_len(t);
            token_chars.push(tc);
            chars += tc;
            if !interner.ws[id as usize] {
                let bit = (id as usize) % 256;
                token_mask[bit / 64] |= 1u64 << (bit % 64);
            }
        }
    });
    let mut non_ws_ids: SmallVec<[u32; 16]> = ids
        .iter()
        .copied()
        .filter(|&id| !interner.ws[id as usize])
        .collect();
    non_ws_ids.sort_unstable();
    let mut token_multiset: SmallVec<[(u32, u16); 16]> = SmallVec::new();
    for &id in &non_ws_ids {
        if let Some(last) = token_multiset.last_mut()
            && last.0 == id
        {
            last.1 = last.1.saturating_add(1);
        } else {
            token_multiset.push((id, 1));
        }
    }
    DpTokens {
        ids,
        chars,
        token_chars,
        token_mask,
        token_multiset,
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

#[inline(always)]
fn merge_multisets(a: &[(u32, u16)], b: &[(u32, u16)], out: &mut SmallVec<[(u32, u16); 32]>) {
    out.clear();
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        let (id_a, c_a) = a[i];
        let (id_b, c_b) = b[j];
        if id_a == id_b {
            out.push((id_a, c_a.saturating_add(c_b)));
            i += 1;
            j += 1;
        } else if id_a < id_b {
            out.push((id_a, c_a));
            i += 1;
        } else {
            out.push((id_b, c_b));
            j += 1;
        }
    }
    while i < a.len() {
        out.push(a[i]);
        i += 1;
    }
    while j < b.len() {
        out.push(b[j]);
        j += 1;
    }
}

#[derive(Clone, Default)]
pub(crate) struct PrecomputedBlock {
    k: usize,
    mask: [u64; 4],
    chars: usize,
    token_start: usize,
    token_end: usize,
    token_multiset: SmallVec<[(u32, u16); 32]>,
}

#[derive(Clone, Default)]
pub(crate) struct RowValidBlocks {
    pub(crate) blocks: [PrecomputedBlock; MAX_BLOCK_K],
    pub(crate) len: u8,
}

impl RowValidBlocks {
    #[inline(always)]
    pub(crate) fn as_slice(&self) -> &[PrecomputedBlock] {
        &self.blocks[..self.len as usize]
    }
}

fn precompute_valid_blocks(
    tokens: &[DpTokens],
    meta: &[DpRowMeta],
    row_token_starts: &[usize],
    out_blocks: &mut Vec<RowValidBlocks>,
) {
    let n = tokens.len();
    out_blocks.clear();
    out_blocks.resize(n, RowValidBlocks::default());
    let mut merge_scratch: SmallVec<[(u32, u16); 32]> = SmallVec::new();

    for j in 0..n {
        if !meta[j].is_reflow {
            continue;
        }
        let max_k = MAX_BLOCK_K.min(n - j);
        let mut mask = [0u64; 4];
        let mut chars = 0usize;

        let head_key = meta[j].property_key;
        let is_comment = meta[j].comment;

        // idx = 0: row j
        mask[0] |= tokens[j].token_mask[0];
        mask[1] |= tokens[j].token_mask[1];
        mask[2] |= tokens[j].token_mask[2];
        mask[3] |= tokens[j].token_mask[3];
        chars += tokens[j].chars;
        let token_start = row_token_starts[j];

        let mut row_blocks = RowValidBlocks::default();
        let mut block_multiset: SmallVec<[(u32, u16); 32]> =
            SmallVec::from_slice(&tokens[j].token_multiset);

        for k in 2..=max_k {
            let idx = k - 1;
            let row_idx = j + idx;

            // Check if previous row (idx - 1) ended with semicolon
            if meta[row_idx - 1].ends_with_semi {
                break;
            }
            // Check candidate conditions for new row
            if !meta[row_idx].is_reflow || meta[row_idx].comment != is_comment {
                break;
            }
            if !is_comment {
                if meta[row_idx].is_stmt_start {
                    break;
                }
                if let Some(hk) = head_key
                    && let Some(ck) = meta[row_idx].property_key
                    && hk == ck
                {
                    break;
                }
            }

            // Append tokens for row_idx
            mask[0] |= tokens[row_idx].token_mask[0];
            mask[1] |= tokens[row_idx].token_mask[1];
            mask[2] |= tokens[row_idx].token_mask[2];
            mask[3] |= tokens[row_idx].token_mask[3];
            chars += tokens[row_idx].chars;
            let token_end = row_token_starts[row_idx + 1];

            merge_multisets(
                &block_multiset,
                &tokens[row_idx].token_multiset,
                &mut merge_scratch,
            );
            std::mem::swap(&mut block_multiset, &mut merge_scratch);

            row_blocks.blocks[row_blocks.len as usize] = PrecomputedBlock {
                k,
                mask,
                chars,
                token_start,
                token_end,
                token_multiset: block_multiset.clone(),
            };
            row_blocks.len += 1;
        }
        out_blocks[j] = row_blocks;
    }
}

#[allow(clippy::too_many_arguments)]
fn block_lcs_fast_fixed_walk<const K: usize>(
    old_ids: &[u32],
    block_ids: &[u32],
    block_coords: &[(u8, u8)],
    block_token_chars: &[usize],
    start_row: usize,
    k: usize,
    min_block_line_coverage: f64,
    id_ws: &[bool],
    id_alnum: &[bool],
    p_keys: &[u32],
    p_masks: &[BitVector<K>],
    overflow_map: Option<&FxHashMap<u32, BitVector<K>>>,
    v_history: &mut [BitVector<K>],
) -> (usize, usize, usize) {
    let n = old_ids.len();
    let m = block_ids.len();

    let mut v = BitVector::<K>::ZERO;
    if let Some(map) = overflow_map {
        for (idx, &bj) in block_ids.iter().rev().enumerate() {
            if let Some(p_mask) = map.get(&bj) {
                v.step(p_mask);
            }
            v_history[idx] = v;
        }
    } else {
        for (idx, &bj) in block_ids.iter().rev().enumerate() {
            for idx_k in 0..p_keys.len() {
                if p_keys[idx_k] == bj {
                    v.step(&p_masks[idx_k]);
                    break;
                }
            }
            v_history[idx] = v;
        }
    }

    let dp = |i: usize, j: usize| -> u32 {
        let rem_a = n - i;
        let rem_b = m - j;
        if rem_a == 0 || rem_b == 0 {
            0
        } else {
            v_history[rem_b - 1].count_ones_prefix(rem_a)
        }
    };

    if dp(0, 0) == 0 {
        return (0, 0, 0);
    }

    let mut matched = 0usize;
    let mut line_matched_counts = [0usize; MAX_BLOCK_K + 1];
    let mut line_matched_alnum = [0usize; MAX_BLOCK_K + 1];
    let mut line_has_alnum = [false; MAX_BLOCK_K + 1];
    for (&id, &(abs_row, _)) in block_ids.iter().zip(block_coords.iter()) {
        let li = (abs_row as usize).saturating_sub(start_row);
        if li < line_has_alnum.len() && id_alnum[id as usize] {
            line_has_alnum[li] = true;
        }
    }
    let mut head = None;
    let mut i = 0;
    let mut j = 0;
    let mut in_add_run = false;

    while i < n && j < m {
        if old_ids[i] == block_ids[j] {
            if id_ws[old_ids[i] as usize]
                && in_add_run
                && !(i + 1 < n
                    && j + 1 < m
                    && old_ids[i + 1] == block_ids[j + 1]
                    && !id_ws[old_ids[i + 1] as usize])
            {
                j += 1;
                continue;
            }
            if in_add_run
                && dp(i, j + 1) == dp(i, j)
                && (j + 1..m)
                    .any(|k| block_ids[k] == old_ids[i] && dp(i + 1, k + 1) + 1 == dp(i, j))
            {
                j += 1;
                continue;
            }
            let (abs_row, _ti) = block_coords[j];
            let li = (abs_row as usize).saturating_sub(start_row);
            matched += block_token_chars[j];
            if li < line_matched_counts.len() {
                line_matched_counts[li] += 1;
                if id_alnum[block_ids[j] as usize] {
                    line_matched_alnum[li] += 1;
                }
            }
            if i == 0 && head.is_none() {
                head = Some(li);
            }
            i += 1;
            j += 1;
            in_add_run = false;
        } else if dp(i + 1, j) > dp(i, j + 1) {
            i += 1;
        } else if dp(i + 1, j) < dp(i, j + 1) {
            j += 1;
            in_add_run = true;
        } else if id_ws[old_ids[i] as usize] && !id_ws[block_ids[j] as usize] {
            if i + 1 < n && old_ids[i + 1] == block_ids[j] {
                i += 1;
            } else {
                j += 1;
                in_add_run = true;
            }
        } else if id_ws[block_ids[j] as usize] && !id_ws[old_ids[i] as usize] {
            j += 1;
            in_add_run = true;
        } else if !block_ids[j + 1..].contains(&old_ids[i])
            || (!id_alnum[old_ids[i] as usize] && id_alnum[block_ids[j] as usize])
        {
            i += 1;
        } else {
            j += 1;
            in_add_run = true;
        }
    }
    let mut most = 0usize;
    let mut most_n = 0usize;
    let mut moved_lines = 0usize;
    let mut lines_with_matches = 0usize;
    for (li, &count) in line_matched_counts[..k].iter().enumerate() {
        if count == 0 || (line_has_alnum[li] && line_matched_alnum[li] == 0) {
            return (0, 0, 0);
        }
        lines_with_matches += 1;
        if count > most_n {
            most_n = count;
            most = li;
        }
        if li > 0 && count > 0 {
            moved_lines += 1;
        }
    }
    if lines_with_matches < 2 || (lines_with_matches as f64 / k as f64) < min_block_line_coverage {
        return (0, 0, 0);
    }
    let anchor = if let Some(h) = head {
        let head_count = line_matched_counts[h];
        if most_n >= head_count.saturating_mul(3) && most_n >= 6 {
            most
        } else {
            h
        }
    } else {
        most
    };
    (matched, moved_lines, anchor)
}

#[allow(clippy::too_many_arguments)]
fn block_lcs_fast_fixed<const K: usize>(
    old_ids: &[u32],
    block_ids: &[u32],
    block_coords: &[(u8, u8)],
    block_token_chars: &[usize],
    start_row: usize,
    k: usize,
    min_block_line_coverage: f64,
    id_ws: &[bool],
    id_alnum: &[bool],
) -> (usize, usize, usize) {
    let n = old_ids.len();
    let m = block_ids.len();
    if n == 0 || m == 0 {
        return (0, 0, 0);
    }
    debug_assert!(n <= K * 64);

    let mut p_keys = [0u32; 64];
    let mut p_masks = [BitVector::<K>::ZERO; 64];
    let mut p_len = 0;
    let mut overflow_map: Option<FxHashMap<u32, BitVector<K>>> = None;

    for (i, &ai) in old_ids.iter().rev().enumerate() {
        if let Some(ref mut map) = overflow_map {
            map.entry(ai).or_insert(BitVector::ZERO).set_bit(i);
        } else {
            let mut found = false;
            for idx in 0..p_len {
                if p_keys[idx] == ai {
                    p_masks[idx].set_bit(i);
                    found = true;
                    break;
                }
            }
            if !found {
                if p_len < 64 {
                    p_keys[p_len] = ai;
                    p_masks[p_len].set_bit(i);
                    p_len += 1;
                } else {
                    let mut map = FxHashMap::default();
                    for idx in 0..p_len {
                        map.insert(p_keys[idx], p_masks[idx]);
                    }
                    map.entry(ai).or_insert(BitVector::ZERO).set_bit(i);
                    overflow_map = Some(map);
                }
            }
        }
    }

    if m <= 256 {
        let mut v_history = [BitVector::<K>::ZERO; 256];
        block_lcs_fast_fixed_walk(
            old_ids,
            block_ids,
            block_coords,
            block_token_chars,
            start_row,
            k,
            min_block_line_coverage,
            id_ws,
            id_alnum,
            &p_keys[..p_len],
            &p_masks[..p_len],
            overflow_map.as_ref(),
            &mut v_history[..m],
        )
    } else {
        let mut v_history = vec![BitVector::<K>::ZERO; m];
        block_lcs_fast_fixed_walk(
            old_ids,
            block_ids,
            block_coords,
            block_token_chars,
            start_row,
            k,
            min_block_line_coverage,
            id_ws,
            id_alnum,
            &p_keys[..p_len],
            &p_masks[..p_len],
            overflow_map.as_ref(),
            &mut v_history,
        )
    }
}

#[expect(clippy::too_many_arguments)]
fn block_lcs_fast_precomputed(
    old_ids: &[u32],
    block_ids: &[u32],
    block_coords: &[(u8, u8)],
    block_token_chars: &[usize],
    start_row: usize,
    k: usize,
    min_block_line_coverage: f64,
    id_ws: &[bool],
    id_alnum: &[bool],
    _lcs_scratch: &mut Vec<u64>,
) -> (usize, usize, usize) {
    let n = old_ids.len();
    let m = block_ids.len();
    if n == 0 || m == 0 || n * m > 40_000 {
        return (0, 0, 0);
    }
    if n <= 64 {
        block_lcs_fast_fixed::<1>(
            old_ids,
            block_ids,
            block_coords,
            block_token_chars,
            start_row,
            k,
            min_block_line_coverage,
            id_ws,
            id_alnum,
        )
    } else if n <= 128 {
        block_lcs_fast_fixed::<2>(
            old_ids,
            block_ids,
            block_coords,
            block_token_chars,
            start_row,
            k,
            min_block_line_coverage,
            id_ws,
            id_alnum,
        )
    } else if n <= 256 {
        block_lcs_fast_fixed::<4>(
            old_ids,
            block_ids,
            block_coords,
            block_token_chars,
            start_row,
            k,
            min_block_line_coverage,
            id_ws,
            id_alnum,
        )
    } else if n <= 512 {
        block_lcs_fast_fixed::<8>(
            old_ids,
            block_ids,
            block_coords,
            block_token_chars,
            start_row,
            k,
            min_block_line_coverage,
            id_ws,
            id_alnum,
        )
    } else if n <= 1024 {
        block_lcs_fast_fixed::<16>(
            old_ids,
            block_ids,
            block_coords,
            block_token_chars,
            start_row,
            k,
            min_block_line_coverage,
            id_ws,
            id_alnum,
        )
    } else if n <= 2048 {
        block_lcs_fast_fixed::<32>(
            old_ids,
            block_ids,
            block_coords,
            block_token_chars,
            start_row,
            k,
            min_block_line_coverage,
            id_ws,
            id_alnum,
        )
    } else {
        (0, 0, 0)
    }
}

fn is_reflow_candidate(text: &str) -> bool {
    !text.trim().is_empty()
}

pub(crate) fn is_statement_start(text: &str, profile: Option<&LangProfile>) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if let Some(p) = profile {
        if p.decl_prefixes.iter().any(|&pf| t.starts_with(pf))
            || p.scope_stop_prefixes.iter().any(|&pf| t.starts_with(pf))
        {
            return true;
        }
        if (p.property_syntax == PropertySyntax::Equals
            || (p.hash_comment && !p.line_comment && !p.block_comment))
            && p.extract_property_key(t).is_some()
        {
            return true;
        }
        if p.section_headers && t.starts_with('[') && t.ends_with(']') {
            return true;
        }
        return false;
    }
    // Fallback check for section headers when no profile is available:
    // `[section]`
    t.starts_with('[') && t.ends_with(']')
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
    if let Some(h) = head {
        let head_n = bm.per_line[h].len();
        if most_n >= head_n.saturating_mul(3) && most_n >= 6 {
            return most;
        }
        return h;
    }
    most
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
pub fn solve_cluster(cluster: &ChangeCluster, profile: Option<&LangProfile>) -> DpSolution {
    solve_cluster_with(cluster, profile, &Weights::default())
}

#[derive(Clone, Copy, Default)]
pub(crate) struct DpRowMeta {
    pub trimmed_id: u32,
    pub comment: bool,
    pub kind: SyntaxSectionKind,
    pub first_ascii_word: Option<u32>,
    pub indent: usize,
    pub stripped_len: usize,
    pub close_dist: usize,
    pub prox_close: f64,
    pub prox_open: f64,
    pub is_reflow: bool,
    pub ends_with_semi: bool,
    pub is_stmt_start: bool,
    pub property_key: Option<u32>,
    pub close_tail: CloseDelimiter,
    pub exact_close: CloseDelimiter,
    pub is_terminal_return: bool,
}

/// Scratch arena for DP cluster solving to reuse memory across clusters.
#[derive(Default)]
pub(crate) struct DpScratchArena {
    pub(crate) interner: TokenInterner,
    pub(crate) old_tokens: Vec<DpTokens>,
    pub(crate) new_tokens: Vec<DpTokens>,
    pub(crate) old_meta: Vec<DpRowMeta>,
    pub(crate) new_meta: Vec<DpRowMeta>,
    pub(crate) old_close_dist: Vec<usize>,
    pub(crate) new_close_dist: Vec<usize>,
    pub(crate) old_open_dist: Vec<usize>,
    pub(crate) new_open_dist: Vec<usize>,
    pub(crate) all_old_ids: Vec<u32>,
    pub(crate) all_old_coords: Vec<(u8, u8)>,
    pub(crate) all_old_token_chars: Vec<usize>,
    pub(crate) old_row_token_starts: Vec<usize>,
    pub(crate) all_new_ids: Vec<u32>,
    pub(crate) all_new_coords: Vec<(u8, u8)>,
    pub(crate) all_new_token_chars: Vec<usize>,
    pub(crate) new_row_token_starts: Vec<usize>,
    pub(crate) old_valid_blocks: Vec<RowValidBlocks>,
    pub(crate) new_valid_blocks: Vec<RowValidBlocks>,
    pub(crate) lcs_scratch: Vec<u64>,
    grid: SkylineMatrix<Cell>,
    precomputed_lcs: SkylineMatrix<u32>,
    lcs_candidates: Vec<(usize, usize)>,
    lcs_scores: Vec<u32>,
    pub(crate) anchor_token_ids: Vec<u32>,
    pub(crate) old_anchor_ranges: Vec<(usize, usize, usize)>,
    pub(crate) new_anchor_ranges: Vec<(usize, usize, usize)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(u8)]
enum OpKind {
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
struct Cell {
    score: f64,
    prev_i: u32,
    prev_j: u32,
    k: u8,
    op: OpKind,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            score: f64::NEG_INFINITY,
            prev_i: 0,
            prev_j: 0,
            k: 0,
            op: OpKind::None,
        }
    }
}

impl DpScratchArena {
    pub fn clear(&mut self) {
        self.interner.clear();
        self.old_tokens.clear();
        self.new_tokens.clear();
        self.old_meta.clear();
        self.new_meta.clear();
        self.old_close_dist.clear();
        self.new_close_dist.clear();
        self.old_open_dist.clear();
        self.new_open_dist.clear();
        self.all_old_ids.clear();
        self.all_old_coords.clear();
        self.all_old_token_chars.clear();
        self.old_row_token_starts.clear();
        self.all_new_ids.clear();
        self.all_new_coords.clear();
        self.all_new_token_chars.clear();
        self.new_row_token_starts.clear();
        self.lcs_candidates.clear();
        self.lcs_scores.clear();
        self.anchor_token_ids.clear();
        self.old_anchor_ranges.clear();
        self.new_anchor_ranges.clear();
    }
}

thread_local! {
    static ARENA: std::cell::RefCell<DpScratchArena> = std::cell::RefCell::new(DpScratchArena::default());
}

/// Solves a cluster using thread-local scratch arena to eliminate allocations
/// in multithreaded runs.
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(
        old_rows = cluster.m(),
        new_rows = cluster.n(),
        section = ?cluster.section
    )
)]
pub fn solve_cluster_with_tls(
    cluster: &ChangeCluster,
    profile: Option<&LangProfile>,
    w: &Weights,
) -> DpSolution {
    let sol = ARENA.with(|arena_cell| {
        let mut arena = arena_cell.borrow_mut();
        solve_cluster_with_arena(cluster, profile, w, &mut arena)
    });
    tracing::debug!(
        score = sol.score,
        transition_count = sol.transitions.len(),
        "Cluster solved"
    );
    sol
}

/// Solve with explicit weights (the tuning surface for tests and Phase 5
/// preference experiments).
pub fn solve_cluster_with(
    cluster: &ChangeCluster,
    profile: Option<&LangProfile>,
    w: &Weights,
) -> DpSolution {
    solve_cluster_with_tls(cluster, profile, w)
}

pub(crate) const ANCHOR_DECOMPOSITION_MIN_ROWS: usize = 256;

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

    if m >= ANCHOR_DECOMPOSITION_MIN_ROWS
        && n >= ANCHOR_DECOMPOSITION_MIN_ROWS
        && let Some(sol) = solve_cluster_with_anchors(cluster, profile, w, arena)
    {
        return sol;
    }

    solve_cluster_matrix(cluster, profile, w, arena)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct AnchorTokenSlice<'a> {
    ids: &'a [u32],
    total_chars: usize,
}

impl std::hash::Hash for AnchorTokenSlice<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
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
        crate::algo::find_unique_anchors(&old_trimmed, &new_trimmed, |s| s.len() >= 4);
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
            crate::algo::find_unique_anchors(&old_slices, &new_slices, |ts| ts.total_chars >= 16);
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
                section: cluster.section,
            };
            let sub_sol = solve_cluster_with_arena(&sub_cluster, profile, w, arena);
            total_rows.extend(sub_sol.rows);
            total_transitions.extend(sub_sol.transitions);
            total_score += sub_sol.score;
        }

        if cluster.old_rows[ai].text().trim() == cluster.new_rows[aj].text().trim() {
            let old_line = match &cluster.old_rows[ai] {
                Row::Del(d) | Row::MovSrc(d) => d.clone(),
                other => panic!("cluster old row is not a Del: {other:?}"),
            };
            let new_line = match &cluster.new_rows[aj] {
                Row::Add(a) | Row::MovTgt(a) => a.clone(),
                other => panic!("cluster new row is not an Add: {other:?}"),
            };

            total_rows.push(Row::MovSrc(old_line));
            total_rows.push(Row::MovTgt(new_line));
            total_transitions.push(DpOp::Mov);

            let old_chars = cluster.old_rows[ai].text().chars().count();
            let new_chars = cluster.new_rows[aj].text().chars().count();
            let length_diff = (old_chars as f64 - new_chars as f64).abs();
            total_score +=
                w.alpha * (old_chars.max(new_chars) as f64) - w.beta - w.gamma * length_diff;
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
                Row::Del(d) | Row::MovSrc(d) => {
                    rows.push(Row::Del(d.clone()));
                    transitions.push(DpOp::Del);
                }
                _ => {}
            }
        }
        for r in &cluster.new_rows {
            match r {
                Row::Add(a) | Row::MovTgt(a) => {
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
        let dt = dp_tokens(&mut arena.interner, text);
        let comment = profile.is_some_and(|p| p.is_comment_line(text));
        let kind = classify_syntax_section(text, profile);
        let first_ascii_word = if comment {
            let comment_body = trimmed
                .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
                .trim_start();
            first_ascii_word(comment_body).map(|w| arena.interner.intern(w))
        } else {
            first_ascii_word(text).map(|w| arena.interner.intern(w))
        };
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
        let property_key = profile
            .and_then(|p| p.extract_property_key(text))
            .map(|w| arena.interner.intern(w));
        let close_tail = classify_close(text);
        let exact_close = classify_exact(text);
        let is_terminal_return = if let Some(p) = profile {
            p.is_terminal_return_expr(text)
        } else {
            let t = text.trim();
            !t.is_empty() && (t.starts_with("return ") || t == "return;" || !t.ends_with(';'))
        };
        let stripped_len = trim_trailing_delims(trimmed, profile).len();
        let trimmed_id = arena.interner.intern(trimmed);

        arena.old_tokens.push(dt);
        arena.old_meta.push(DpRowMeta {
            trimmed_id,
            comment,
            kind,
            first_ascii_word,
            indent,
            stripped_len,
            close_dist,
            prox_close,
            prox_open,
            is_reflow,
            ends_with_semi,
            is_stmt_start,
            property_key,
            close_tail,
            exact_close,
            is_terminal_return,
        });
    }

    for (j, r) in cluster.new_rows.iter().enumerate() {
        let text = r.text();
        let trimmed = text.trim();
        let dt = dp_tokens(&mut arena.interner, text);
        let comment = profile.is_some_and(|p| p.is_comment_line(text));
        let kind = classify_syntax_section(text, profile);
        let first_ascii_word = if comment {
            let comment_body = trimmed
                .trim_start_matches(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
                .trim_start();
            first_ascii_word(comment_body).map(|w| arena.interner.intern(w))
        } else {
            first_ascii_word(text).map(|w| arena.interner.intern(w))
        };
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
        let property_key = profile
            .and_then(|p| p.extract_property_key(text))
            .map(|w| arena.interner.intern(w));
        let close_tail = classify_close(text);
        let exact_close = classify_exact(text);
        let is_terminal_return = if let Some(p) = profile {
            p.is_terminal_return_expr(text)
        } else {
            let t = text.trim();
            !t.is_empty() && (t.starts_with("return ") || t == "return;" || !t.ends_with(';'))
        };
        let stripped_len = trim_trailing_delims(trimmed, profile).len();
        let trimmed_id = arena.interner.intern(trimmed);

        arena.new_tokens.push(dt);
        arena.new_meta.push(DpRowMeta {
            trimmed_id,
            comment,
            kind,
            first_ascii_word,
            indent,
            stripped_len,
            close_dist,
            prox_close,
            prox_open,
            is_reflow,
            ends_with_semi,
            is_stmt_start,
            property_key,
            close_tail,
            exact_close,
            is_terminal_return,
        });
    }

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
            lcs_candidates,
            lcs_scores,
            ..
        } = arena;

        let neg = f64::NEG_INFINITY;
        let init_cell = Cell {
            score: neg,
            prev_i: 0,
            prev_j: 0,
            k: 0,
            op: OpKind::None,
        };
        grid.init_envelope(m, n, bounds, init_cell);
        precomputed_lcs.init_envelope(m, n, bounds, u32::MAX);
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
            w.alpha * matched as f64
                - w.mu * unmatched as f64
                - w.beta * k as f64
                - w.gamma * (k - 1) as f64
                - w.gamma * moved_lines as f64 * 0.5
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

        let dummy_tok = DpTokens {
            ids: Vec::new(),
            chars: 0,
            token_chars: Vec::new(),
            token_mask: [0; 4],
            token_multiset: SmallVec::new(),
        };
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
                    i, j, cluster, old_tokens, new_tokens, old_meta, new_meta, interner, profile, w,
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
        );

        for i in 0..=m {
            let has_old = i < m;
            let (ot, om, old_valid_blocks_i) = if has_old {
                (&old_tokens[i], old_meta[i], &old_valid_blocks[i])
            } else {
                (&dummy_tok, DpRowMeta::default(), &empty_blocks)
            };

            for j in grid.row_range(i) {
                let base = grid.get(i, j).map_or(neg, |c| c.score);
                if base == neg {
                    continue;
                }
                // Standalone Del / Add.
                if has_old
                    && let Some(target) = grid.get_mut(i + 1, j)
                    && base - w.beta > target.score
                {
                    *target = Cell {
                        score: base - w.beta,
                        prev_i: i as u32,
                        prev_j: j as u32,
                        k: 1,
                        op: OpKind::Del,
                    };
                }
                if j < n
                    && let Some(target) = grid.get_mut(i, j + 1)
                    && base - w.beta >= target.score
                {
                    // `>=`: on a del/add tie the path prefers ending with
                    // the add — the emission reads
                    // dels first, adds after (the
                    // diff reading convention).
                    *target = Cell {
                        score: base - w.beta,
                        prev_i: i as u32,
                        prev_j: j as u32,
                        k: 1,
                        op: OpKind::Add,
                    };
                }
                // 1-to-1: Mod or Mov.
                let nm = if j < n {
                    new_meta[j]
                } else {
                    DpRowMeta::default()
                };
                if has_old && j < n && om.comment == nm.comment && om.kind.is_compatible(nm.kind) {
                    let nt = &new_tokens[j];
                    if ot.ids.is_empty() && nt.ids.is_empty() {
                        // blank ↔ blank: a trivial full match in one row.
                        let s = base - w.beta + w.delta;
                        if let Some(target) = grid.get_mut(i + 1, j + 1)
                            && s > target.score
                        {
                            *target = Cell {
                                score: s,
                                prev_i: i as u32,
                                prev_j: j as u32,
                                k: 1,
                                op: OpKind::Mod,
                            };
                        }
                    } else if om.trimmed_id == nm.trimmed_id {
                        // Verbatim (trim-insensitive) — a Mov.
                        let length_diff = (ot.chars as i64 - nt.chars as i64).abs() as f64;
                        let s = base + w.alpha * ot.chars.max(nt.chars) as f64
                            - w.beta
                            - w.gamma * length_diff;
                        if let Some(target) = grid.get_mut(i + 1, j + 1)
                            && s > target.score
                        {
                            *target = Cell {
                                score: s,
                                prev_i: i as u32,
                                prev_j: j as u32,
                                k: 1,
                                op: OpKind::Mov,
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
                            continue;
                        } else {
                            let ci = calc_common_indent(
                                cluster.old_rows[i].text(),
                                cluster.new_rows[j].text(),
                            );
                            if ci == 0 {
                                continue;
                            }
                            Some(ci)
                        };
                        let total = ot.chars.max(nt.chars);
                        let max_matched = char_bound;

                        let Some(target_diag) = grid.get(i + 1, j + 1) else {
                            continue;
                        };

                        let statement_head_match = match (om.first_ascii_word, nm.first_ascii_word)
                        {
                            (Some(a), Some(b)) => a == b,
                            _ => false,
                        };
                        let is_property_match =
                            om.property_key.is_some() && om.property_key == nm.property_key;
                        let max_syn = if is_property_match {
                            w.head_agree_bonus + 200.0
                        } else if statement_head_match {
                            w.head_agree_bonus
                        } else {
                            0.0
                        };

                        let indent_shift = (om.indent as isize - nm.indent as isize).unsigned_abs();
                        let offset_diff = (i as isize - j as isize).unsigned_abs();

                        let min_ratio = if (om.comment || nm.comment) && offset_diff > 0 {
                            0.65
                        } else if om.comment || nm.comment {
                            0.35
                        } else {
                            0.25
                        };
                        let could_be_tail_pair = if om.close_dist == 1 && nm.close_dist == 1 {
                            if om.close_tail != CloseDelimiter::None
                                && nm.close_tail != CloseDelimiter::None
                            {
                                if let Some(p) = profile {
                                    p.is_direct_tail_match(
                                        om.close_tail,
                                        nm.close_tail,
                                        max_matched,
                                        w.min_tail_matched_chars,
                                    )
                                } else {
                                    CloseDelimiter::default_is_direct_tail_match(
                                        om.close_tail,
                                        nm.close_tail,
                                        max_matched,
                                        w.min_tail_matched_chars,
                                    )
                                }
                            } else if om.is_terminal_return && nm.is_terminal_return {
                                let has_common_indent = if indent_shift == 0 {
                                    *common_indent.get_or_insert_with(|| {
                                        if om.indent == 0 || nm.indent == 0 {
                                            0
                                        } else {
                                            calc_common_indent(
                                                cluster.old_rows[i].text(),
                                                cluster.new_rows[j].text(),
                                            )
                                        }
                                    }) > 0
                                } else {
                                    false
                                };
                                if has_common_indent {
                                    true
                                } else {
                                    max_matched >= w.min_tail_matched_chars
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        let max_similarity_ratio = max_matched as f64 / total as f64;
                        let can_have_containment = indent_shift <= w.max_indent_shift
                            && (om.comment == nm.comment)
                            && om.stripped_len >= w.min_containment_len
                            && nm.stripped_len >= w.min_containment_len;

                        let mut is_prefix_containment = None;
                        let could_be_valid_mod_fast = max_similarity_ratio >= min_ratio
                            || is_property_match
                            || statement_head_match
                            || could_be_tail_pair;
                        if !could_be_valid_mod_fast {
                            if !can_have_containment {
                                continue;
                            }
                            let ot_strip =
                                &cluster.old_rows[i].text()[om.indent..om.indent + om.stripped_len];
                            let nt_strip =
                                &cluster.new_rows[j].text()[nm.indent..nm.indent + nm.stripped_len];
                            let is_pc = is_prefix_at_boundary(ot_strip, nt_strip)
                                || is_prefix_at_boundary(nt_strip, ot_strip);
                            if !is_pc {
                                continue;
                            }
                            is_prefix_containment = Some(true);
                        }

                        let max_close = if could_be_tail_pair {
                            w.delta
                        } else {
                            w.epsilon * om.prox_close * nm.prox_close
                        };
                        let max_open = w.epsilon * om.prox_open * nm.prox_open;

                        let indent_penalty =
                            w.gamma * (indent_shift as f64) * w.indent_penalty_factor;

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
                        let max_possible_mod_s = base + w.alpha * max_matched as f64
                            - w.gamma * (total - max_matched) as f64
                            - w.beta
                            - indent_penalty
                            + max_syn_bonus
                            + max_close
                            + max_open;
                        let next_diag_score = target_diag.score;
                        let mod_threshold = (base - 2.0 * w.beta).max(next_diag_score);
                        if max_possible_mod_s <= mod_threshold {
                            continue;
                        }

                        let matched = if ot.ids == nt.ids {
                            ot.chars
                        } else if token_overlap && char_bound > 0 {
                            // Precomputed in parallel up front; recomputed
                            // inline on a miss so
                            // output never depends on enumeration
                            // coverage.
                            match precomputed_lcs.get(i, j).copied() {
                                Some(m) if m != u32::MAX => m as usize,
                                _ => lcs_matched_chars_ids(
                                    &ot.ids,
                                    &nt.ids,
                                    &interner.ws,
                                    &interner.alnum,
                                    &nt.token_chars,
                                    lcs_scratch,
                                ),
                            }
                        } else {
                            0usize
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
                                continue;
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
                            let pen = w.mu * core_unmatched as f64 + w.gamma * length_diff;
                            let syn = if is_prefix_containment {
                                max_syn + w.head_agree_bonus
                            } else {
                                max_syn
                            };
                            (pen, syn)
                        } else {
                            (w.mu * unmatched as f64, max_syn)
                        };

                        let close_bonus = w.epsilon * om.prox_close * nm.prox_close;
                        let open_bonus = w.epsilon * om.prox_open * nm.prox_open;
                        let indent_diff = indent_shift as f64;
                        let indent_penalty = w.gamma * indent_diff * w.indent_penalty_factor;

                        let similarity_ratio = matched as f64 / total as f64;
                        let is_direct_tail_pair = if om.close_dist == 1 && nm.close_dist == 1 {
                            let delim_match = if om.close_tail != CloseDelimiter::None
                                && nm.close_tail != CloseDelimiter::None
                            {
                                if let Some(p) = profile {
                                    p.is_direct_tail_match(
                                        om.close_tail,
                                        nm.close_tail,
                                        matched,
                                        w.min_tail_matched_chars,
                                    )
                                } else {
                                    CloseDelimiter::default_is_direct_tail_match(
                                        om.close_tail,
                                        nm.close_tail,
                                        matched,
                                        w.min_tail_matched_chars,
                                    )
                                }
                            } else {
                                false
                            };
                            if delim_match {
                                true
                            } else if om.is_terminal_return && nm.is_terminal_return {
                                let has_common_indent = if indent_diff == 0.0 {
                                    *common_indent.get_or_insert_with(|| {
                                        if om.indent == 0 || nm.indent == 0 {
                                            0
                                        } else {
                                            calc_common_indent(
                                                cluster.old_rows[i].text(),
                                                cluster.new_rows[j].text(),
                                            )
                                        }
                                    }) > 0
                                } else {
                                    false
                                };
                                if has_common_indent {
                                    true
                                } else {
                                    matched >= w.min_tail_matched_chars
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        let is_valid_mod_pair = if (om.comment || nm.comment) && offset_diff > 0 {
                            is_prefix_containment || similarity_ratio >= 0.65
                        } else if om.comment || nm.comment {
                            similarity_ratio >= 0.35 || is_prefix_containment
                        } else {
                            similarity_ratio >= 0.25
                                || is_direct_tail_pair
                                || is_property_match
                                || statement_head_match
                                || is_prefix_containment
                        };

                        if !is_valid_mod_pair {
                            continue;
                        }

                        let effective_close_bonus = if is_direct_tail_pair {
                            w.delta
                        } else {
                            close_bonus
                        };
                        let proximity_gate = if similarity_ratio >= 0.25
                            || is_direct_tail_pair
                            || is_property_match
                            || statement_head_match
                            || is_prefix_containment
                        {
                            1.0
                        } else {
                            0.0
                        };

                        let s = base + w.alpha * matched as f64 - penalty - w.beta - indent_penalty
                            + syntax
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
                            };
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
                        let max_syntax = if om.first_ascii_word.is_some() {
                            w.delta
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
                        let expand_threshold = (base - (1 + k) as f64 * w.beta).max(target_score);
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
                            lcs_scratch,
                        );
                        if bm_matched < 8 || (bm_matched as f64 / total as f64) < 0.35 {
                            continue;
                        }
                        let syntax = if om.first_ascii_word.is_some()
                            && (om.first_ascii_word == nm.first_ascii_word
                                || om.first_ascii_word == new_meta[j + anchor].first_ascii_word)
                        {
                            w.delta
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
                                + close_bonus;
                        if let Some(target) = grid.get_mut(i + 1, j + k)
                            && s > target.score
                        {
                            *target = Cell {
                                score: s,
                                prev_i: i as u32,
                                prev_j: j as u32,
                                k: k as u8,
                                op: OpKind::Expand,
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
                        let max_syntax = if nm.first_ascii_word.is_some() {
                            w.delta
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
                        let collapse_threshold = (base - (k + 1) as f64 * w.beta).max(target_score);
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
                            lcs_scratch,
                        );
                        if bm_matched < 8 || (bm_matched as f64 / total as f64) < 0.35 {
                            continue;
                        }
                        let syntax = if nm.first_ascii_word.is_some()
                            && (nm.first_ascii_word == om.first_ascii_word
                                || nm.first_ascii_word == old_meta[i + anchor].first_ascii_word)
                        {
                            w.delta
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
                                + close_bonus;
                        if let Some(target) = grid.get_mut(i + k, j + 1)
                            && s > target.score
                        {
                            *target = Cell {
                                score: s,
                                prev_i: i as u32,
                                prev_j: j as u32,
                                k: k as u8,
                                op: OpKind::Collapse,
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
                    Row::Del(d) | Row::MovSrc(d) => {
                        rows.push(Row::Del(d.clone()));
                        transitions.push(DpOp::Del);
                    }
                    _ => {}
                }
            }
            for r in &cluster.new_rows {
                match r {
                    Row::Add(a) | Row::MovTgt(a) => {
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
            Row::Del(d) | Row::MovSrc(d) => d.clone(),
            other => panic!("cluster old row is not a Del: {other:?}"),
        };
        let new_line_of = |idx: usize| match &cluster.new_rows[idx] {
            Row::Add(a) | Row::MovTgt(a) => a.clone(),
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
                    rows.push(Row::MovSrc(old_line_of(pi)));
                    rows.push(Row::MovTgt(new_line_of(pj)));
                    transitions.push(DpOp::Mov);
                }
                OpKind::Mod => {
                    rows.push(Row::Mod {
                        old_line: old_line_of(pi),
                        new_line: new_line_of(pj),
                    });
                    transitions.push(DpOp::Mod { matched: 0 });
                }
                OpKind::Expand => {
                    let bm = block_lcs(&old[pi], &new[pj..pj + k], interner);
                    let anchor = block_anchor(&bm);
                    let spans = subline_moves(&bm, anchor);

                    for idx in 0..k {
                        let nl = new_line_of(pj + idx);
                        if idx == anchor {
                            rows.push(Row::Mod {
                                old_line: old_line_of(pi),
                                new_line: nl,
                            });
                        } else {
                            rows.push(Row::Add(nl));
                        }
                    }
                    transitions.push(DpOp::Expand {
                        matched: bm.matched,
                        k,
                        anchor,
                        spans,
                    });
                }
                OpKind::Collapse => {
                    let bm = block_lcs(&new[pj], &old[pi..pi + k], interner);
                    let anchor = block_anchor(&bm);
                    let spans = subline_moves(&bm, anchor);

                    for idx in 0..k {
                        let ol = old_line_of(pi + idx);
                        if idx == anchor {
                            rows.push(Row::Mod {
                                old_line: ol,
                                new_line: new_line_of(pj),
                            });
                        } else {
                            rows.push(Row::Del(ol));
                        }
                    }
                    transitions.push(DpOp::Collapse {
                        matched: bm.matched,
                        k,
                        anchor,
                        spans,
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

/// The bidirectional projection verifier (plan Step 3.4): the solution's
/// rows must project to EXACTLY the cluster's old lines (Del texts plus
/// Mod/Mov old texts, in order) and new lines (Add texts plus Mod/Mov new
/// texts) — the universal invariant `Π_old ≡ old ∧ Π_new ≡ new`.
pub fn verify_projection(sol: &DpSolution, cluster: &ChangeCluster) -> Result<(), String> {
    let side = |old_side: bool| -> Vec<String> {
        sol.rows
            .iter()
            .filter_map(|r| match r {
                Row::Del(d) | Row::MovSrc(d) if old_side => Some(d.text.clone()),
                Row::Add(a) | Row::MovTgt(a) if !old_side => Some(a.text.clone()),
                Row::Mod { old_line, new_line } => {
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
