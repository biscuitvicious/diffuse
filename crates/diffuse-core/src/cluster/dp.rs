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
    lcs::{lcs_matched_chars_ids, lcs_pairs_ids},
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

/// Default maximum cluster product (old lines × new lines) allowed for DP
/// pairing before falling back to wholesale deletion/addition.
pub const DEFAULT_CLUSTER_PRODUCT_CAP: usize = 100_000;

thread_local! {
    static CLUSTER_PRODUCT_CAP: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_CLUSTER_PRODUCT_CAP) };
}

/// Set the maximum cluster product (old lines × new lines) allowed for DP
/// pairing before falling back.
pub fn set_cluster_product_cap(cap: usize) {
    CLUSTER_PRODUCT_CAP.with(|c| c.set(cap));
}

/// Get the current cluster product cap.
pub fn cluster_product_cap() -> usize {
    CLUSTER_PRODUCT_CAP.with(|c| c.get())
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
}

#[inline(always)]
fn mask_overlap(a: &[u64; 4], b: &[u64; 4]) -> bool {
    ((a[0] & b[0]) | (a[1] & b[1]) | (a[2] & b[2]) | (a[3] & b[3])) != 0
}

#[inline(always)]
fn token_mask_matched_chars_bound(
    tokens: &DpTokens,
    target_mask: &[u64; 4],
    id_ws: &[bool],
) -> usize {
    let mut bound = 0;
    for (ti, &id) in tokens.ids.iter().enumerate() {
        if !id_ws[id as usize] {
            let bit = (id as usize) % 256;
            if (target_mask[bit / 64] & (1u64 << (bit % 64))) != 0 {
                bound += tokens.token_chars[ti];
            }
        }
    }
    bound
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
    DpTokens {
        ids,
        chars,
        token_chars,
        token_mask,
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

#[derive(Clone, Copy, Default)]
pub(crate) struct PrecomputedBlock {
    k: usize,
    mask: [u64; 4],
    chars: usize,
    token_start: usize,
    token_end: usize,
}

#[derive(Clone, Copy, Default)]
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

            row_blocks.blocks[row_blocks.len as usize] = PrecomputedBlock {
                k,
                mask,
                chars,
                token_start,
                token_end,
            };
            row_blocks.len += 1;
        }
        out_blocks[j] = row_blocks;
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
    lcs_scratch: &mut Vec<u32>,
) -> (usize, usize, usize) {
    let n = old_ids.len();
    let m = block_ids.len();
    if n == 0 || m == 0 {
        return (0, 0, 0);
    }
    let stride = m + 1;
    let total_len = (n + 1) * stride;
    if lcs_scratch.len() < total_len {
        lcs_scratch.resize(total_len, 0);
    }
    let dp_ptr = lcs_scratch.as_mut_ptr();
    unsafe {
        let mut next_row_ptr = dp_ptr.add(n * stride);
        for col in 0..=m {
            *next_row_ptr.add(col) = 0;
        }
        let mut i = n;
        while i > 0 {
            i -= 1;
            let curr_row_ptr = dp_ptr.add(i * stride);
            *curr_row_ptr.add(m) = 0;
            let ai = *old_ids.get_unchecked(i);
            let mut j = m;
            let mut right = 0;
            while j > 0 {
                j -= 1;
                let bj = *block_ids.get_unchecked(j);
                let val = if ai == bj {
                    *next_row_ptr.add(j + 1) + 1
                } else {
                    let down = *next_row_ptr.add(j);
                    down.max(right)
                };
                *curr_row_ptr.add(j) = val;
                right = val;
            }
            next_row_ptr = curr_row_ptr;
        }
    }
    let dp = &mut lcs_scratch[..total_len];
    if dp[0] == 0 {
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
                && dp[i * stride + j + 1] == dp[i * stride + j]
                && (j + 1..m).any(|k| {
                    block_ids[k] == old_ids[i]
                        && dp[(i + 1) * stride + k + 1] + 1 == dp[i * stride + j]
                })
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
        } else if dp[(i + 1) * stride + j] > dp[i * stride + j + 1] {
            i += 1;
        } else if dp[(i + 1) * stride + j] < dp[i * stride + j + 1] {
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
        } else if !block_ids[j + 1..].iter().any(|t| *t == old_ids[i])
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
    pub(crate) lcs_scratch: Vec<u32>,
    dp: Vec<Cell>,
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

#[derive(Clone, Copy)]
#[repr(C)]
struct Cell {
    score: f64,
    prev_i: u32,
    prev_j: u32,
    k: u8,
    op: OpKind,
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
        self.dp.clear();
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

    if m == 0 || n == 0 || m.saturating_mul(n) > cluster_product_cap() {
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
    fn trim_trailing_delims<'a>(t: &'a str, profile: Option<&LangProfile>) -> &'a str {
        if let Some(p) = profile {
            p.trim_trailing_delimiters(t)
        } else {
            t.trim_end_matches(|c: char| {
                c.is_whitespace() || matches!(c, '{' | '}' | ';' | ':' | ',' | ')' | ']')
            })
        }
    }

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
        let trimmed_id = arena.interner.intern(trimmed);

        arena.old_tokens.push(dt);
        arena.old_meta.push(DpRowMeta {
            trimmed_id,
            comment,
            kind,
            first_ascii_word,
            indent,
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
        let trimmed_id = arena.interner.intern(trimmed);

        arena.new_tokens.push(dt);
        arena.new_meta.push(DpRowMeta {
            trimmed_id,
            comment,
            kind,
            first_ascii_word,
            indent,
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
        dp,
        ..
    } = arena;

    let neg = f64::NEG_INFINITY;
    let stride = n + 1;
    let total_cells = (m + 1) * stride;
    dp.clear();
    dp.resize(
        total_cells,
        Cell {
            score: neg,
            prev_i: 0,
            prev_j: 0,
            k: 0,
            op: OpKind::None,
        },
    );
    dp[0].score = 0.0;

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

    let use_band = m * n > 2500;
    let band_limit_j = 64.max((n as isize) / 10);
    let band_limit_i = 64.max((m as isize) / 10);
    let expected_i_for_j: Vec<isize> = if use_band {
        (0..=n)
            .map(|j| (j * m).checked_div(n).unwrap_or(0) as isize)
            .collect()
    } else {
        Vec::new()
    };

    let dummy_tok = DpTokens {
        ids: Vec::new(),
        chars: 0,
        token_chars: Vec::new(),
        token_mask: [0; 4],
    };
    let empty_blocks = RowValidBlocks::default();

    for i in 0..=m {
        let expected_j = (i * n).checked_div(m).unwrap_or(0) as isize;
        let row_curr_idx = i * stride;
        let row_next_idx = (i + 1) * stride;

        let has_old = i < m;
        let (ot, om, old_valid_blocks_i) = if has_old {
            (&old_tokens[i], old_meta[i], &old_valid_blocks[i])
        } else {
            (&dummy_tok, DpRowMeta::default(), &empty_blocks)
        };

        for j in 0..=n {
            let curr = row_curr_idx + j;
            let base = dp[curr].score;
            if base == neg {
                continue;
            }
            // Standalone Del / Add.
            if has_old && base - w.beta > dp[row_next_idx + j].score {
                dp[row_next_idx + j] = Cell {
                    score: base - w.beta,
                    prev_i: i as u32,
                    prev_j: j as u32,
                    k: 1,
                    op: OpKind::Del,
                };
            }
            if j < n && base - w.beta >= dp[curr + 1].score {
                // `>=`: on a del/add tie the path prefers ending with the
                // add — the emission reads dels first, adds after (the
                // diff reading convention).
                dp[curr + 1] = Cell {
                    score: base - w.beta,
                    prev_i: i as u32,
                    prev_j: j as u32,
                    k: 1,
                    op: OpKind::Add,
                };
            }
            if use_band {
                let diff_j = (j as isize - expected_j).abs();
                let diff_i = (i as isize - expected_i_for_j[j]).abs();
                if diff_j > band_limit_j && diff_i > band_limit_i {
                    continue;
                }
            }
            // 1-to-1: Mod or Mov.
            let nm = if j < n {
                new_meta[j]
            } else {
                DpRowMeta::default()
            };
            if has_old && j < n && om.comment == nm.comment && om.kind.is_compatible(nm.kind) {
                let next_diag = row_next_idx + j + 1;
                let nt = &new_tokens[j];
                if ot.ids.is_empty() && nt.ids.is_empty() {
                    // blank ↔ blank: a trivial full match in one row.
                    let s = base - w.beta + w.delta;
                    if s > dp[next_diag].score {
                        dp[next_diag] = Cell {
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
                    if s > dp[next_diag].score {
                        dp[next_diag] = Cell {
                            score: s,
                            prev_i: i as u32,
                            prev_j: j as u32,
                            k: 1,
                            op: OpKind::Mov,
                        };
                    }
                } else if !ot.ids.is_empty() && !nt.ids.is_empty() {
                    let old_text = cluster.old_rows[i].text();
                    let new_text = cluster.new_rows[j].text();
                    let common_indent = old_text
                        .chars()
                        .take_while(|c| c.is_whitespace())
                        .zip(new_text.chars().take_while(|c| c.is_whitespace()))
                        .take_while(|(c1, c2)| c1 == c2)
                        .count();

                    let token_overlap = mask_overlap(&ot.token_mask, &nt.token_mask);
                    let min_chars = ot.chars.min(nt.chars);
                    let char_bound = if token_overlap {
                        token_mask_matched_chars_bound(ot, &nt.token_mask, &interner.ws)
                            .min(min_chars)
                    } else {
                        0
                    };
                    // Disjoint lines with zero overlapping tokens and zero
                    // common indentation cannot be in-place
                    // substitutions (wholesale replacements).
                    if char_bound + common_indent == 0 {
                        continue;
                    }
                    let total = ot.chars.max(nt.chars);
                    let max_matched = char_bound;

                    let statement_head_match = match (om.first_ascii_word, nm.first_ascii_word) {
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
                    let max_close = if om.close_dist == 1 && nm.close_dist == 1 {
                        w.delta
                    } else {
                        w.epsilon
                    };

                    let max_possible_mod_s = base + w.alpha * max_matched as f64
                        - w.gamma * (total - max_matched) as f64
                        - w.beta
                        + max_syn
                        + max_close
                        + w.head_agree_bonus
                        + w.epsilon;
                    let mod_threshold = (base - 2.0 * w.beta).max(dp[next_diag].score);
                    if max_possible_mod_s <= mod_threshold {
                        continue;
                    }

                    let matched = if token_overlap && char_bound > 0 {
                        lcs_matched_chars_ids(
                            &ot.ids,
                            &nt.ids,
                            &interner.ws,
                            &interner.alnum,
                            &nt.token_chars,
                            lcs_scratch,
                        )
                    } else {
                        0usize
                    };
                    if matched + common_indent == 0 {
                        continue;
                    }
                    let ot_trim = cluster.old_rows[i].text().trim();
                    let nt_trim = cluster.new_rows[j].text().trim();

                    let indent_shift = (om.indent as isize - nm.indent as isize).unsigned_abs();
                    let is_prefix_containment = {
                        let ot_strip = trim_trailing_delims(ot_trim, profile);
                        let nt_strip = trim_trailing_delims(nt_trim, profile);
                        let min_len = ot_strip.len().min(nt_strip.len());
                        let is_prefix_at_boundary = |longer: &str, shorter: &str| -> bool {
                            if let Some(rest) = longer.strip_prefix(shorter) {
                                rest.is_empty()
                                    || shorter.ends_with(|c: char| !c.is_alphanumeric() && c != '_')
                                    || rest.starts_with(|c: char| {
                                        c.is_whitespace() || (!c.is_alphanumeric() && c != '_')
                                    })
                            } else {
                                false
                            }
                        };
                        min_len >= w.min_containment_len
                            && indent_shift <= w.max_indent_shift
                            && (om.comment == nm.comment)
                            && (is_prefix_at_boundary(ot_strip, nt_strip)
                                || is_prefix_at_boundary(nt_strip, ot_strip))
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
                    // Distance-based closing-brace proximity bonus, tapering
                    // with distance
                    let proximity_old_close = om.prox_close;
                    let proximity_new_close = nm.prox_close;
                    let close_bonus = w.epsilon * proximity_old_close * proximity_new_close;
                    // Distance-based opening-brace proximity bonus
                    let proximity_old_open = om.prox_open;
                    let proximity_new_open = nm.prox_open;
                    let open_bonus = w.epsilon * proximity_old_open * proximity_new_open;

                    let indent_diff = indent_shift as f64;
                    let indent_penalty = w.gamma * indent_diff * w.indent_penalty_factor;

                    let offset_diff = (i as isize - j as isize).unsigned_abs();
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
                            if indent_diff == 0.0 && common_indent > 0 {
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
                    if s > dp[next_diag].score {
                        dp[next_diag] = Cell {
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
                    if !mask_overlap(&ot.token_mask, &pblock.mask) {
                        continue;
                    }
                    let total = ot.chars.max(pblock.chars);
                    let min_chars = ot.chars.min(pblock.chars);
                    let char_bound = token_mask_matched_chars_bound(ot, &pblock.mask, &interner.ws)
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
                    let expand_threshold =
                        (base - (1 + k) as f64 * w.beta).max(dp[(i + 1) * stride + j + k].score);
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

                    let s = base
                        + calc_block_score(bm_matched, bm_moved_lines, ot.chars, pblock.chars, k)
                        + syntax
                        + close_bonus;
                    if s > dp[(i + 1) * stride + j + k].score {
                        dp[(i + 1) * stride + j + k] = Cell {
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
                    if !mask_overlap(&new_tokens[j].token_mask, &pblock.mask) {
                        continue;
                    }
                    let total = new_tokens[j].chars.max(pblock.chars);
                    let min_chars = new_tokens[j].chars.min(pblock.chars);
                    let char_bound =
                        token_mask_matched_chars_bound(&new_tokens[j], &pblock.mask, &interner.ws)
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
                    let collapse_threshold =
                        (base - (k + 1) as f64 * w.beta).max(dp[(i + k) * stride + j + 1].score);
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
                    if s > dp[(i + k) * stride + j + 1].score {
                        dp[(i + k) * stride + j + 1] = Cell {
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

    // Traceback: collect (i, j, op, k) from (m, n) back to (0, 0).
    let mut ops: Vec<(usize, usize, OpKind, usize)> = Vec::new();
    let (mut i, mut j) = (m, n);
    while dp[i * stride + j].op != OpKind::None {
        let cell = dp[i * stride + j];
        ops.push((
            cell.prev_i as usize,
            cell.prev_j as usize,
            cell.op,
            cell.k as usize,
        ));
        i = cell.prev_i as usize;
        j = cell.prev_j as usize;
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

    DpSolution {
        rows,
        transitions,
        score: dp[m * stride + n].score,
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
