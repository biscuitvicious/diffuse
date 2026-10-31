//! Block-LCS matching algorithms and precomputed row validity tables.

use std::mem;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::{
    lcs::{bit_parallel::BitVector, lcs_pairs_ids},
    similarity::TokenInterner,
};

use super::{
    arena::DpRowMeta,
    config::MAX_BLOCK_K,
    tokens::{DpTokens, merge_multisets},
};

/// The block LCS: `old`'s tokens vs the concatenation of `block`'s lines.
#[derive(Default)]
pub struct BlockMatch {
    pub(crate) matched: usize,
    /// Per block line: its matched token indices.
    pub per_line: Vec<FxHashSet<usize>>,
    /// Per block line: number of matched alphanumeric/salient tokens.
    pub per_line_alnum: Vec<usize>,
    /// (block line, token) -> old-side token index.
    pub pair_of: FxHashMap<(usize, usize), usize>,
}

pub fn block_lcs(old: &DpTokens, block: &[DpTokens], interner: &TokenInterner) -> BlockMatch {
    let mut ids: SmallVec<[u32; 16]> = SmallVec::new();
    let mut coords: SmallVec<[(usize, usize); 16]> = SmallVec::new();
    for (li, lt) in block.iter().enumerate() {
        for (ti, &id) in lt.ids.iter().enumerate() {
            ids.push(id);
            coords.push((li, ti));
        }
    }
    let pairs = lcs_pairs_ids(&old.ids, &ids, &interner.ws, &interner.alnum);
    let mut bm = BlockMatch {
        per_line: vec![FxHashSet::default(); block.len()],
        per_line_alnum: vec![0; block.len()],
        ..Default::default()
    };
    let mut matches: Vec<(usize, usize)> = pairs.iter().map(|(&o, &n)| (o, n)).collect();
    matches.sort_unstable();
    for (oi, ni) in matches {
        let (li, ti) = coords[ni];
        bm.matched += block[li].token_chars[ti];
        bm.per_line[li].insert(ti);
        let id = block[li].ids[ti] as usize;
        if id < interner.alnum.len() && interner.alnum[id] {
            bm.per_line_alnum[li] += 1;
        }
        bm.pair_of.insert((li, ti), oi);
    }
    bm
}

#[derive(Clone, Default)]
pub(crate) struct PrecomputedBlock {
    pub(crate) k: usize,
    pub(crate) mask: [u64; 4],
    pub(crate) chars: usize,
    pub(crate) token_start: usize,
    pub(crate) token_end: usize,
    pub(crate) token_multiset: SmallVec<[(u32, u16); 32]>,
    pub(crate) opens_container: bool,
}

#[derive(Clone, Default)]
pub struct RowValidBlocks {
    pub(crate) blocks: [PrecomputedBlock; MAX_BLOCK_K],
    pub(crate) len: u8,
}

impl RowValidBlocks {
    #[inline(always)]
    pub(crate) fn as_slice(&self) -> &[PrecomputedBlock] {
        &self.blocks[..self.len as usize]
    }
}

pub(crate) fn precompute_valid_blocks(
    tokens: &[DpTokens],
    meta: &[DpRowMeta],
    row_token_starts: &[usize],
    out_blocks: &mut Vec<RowValidBlocks>,
) {
    let n = tokens.len();
    out_blocks.clear();
    out_blocks.resize(n, RowValidBlocks::default());
    if n < 2 {
        return;
    }
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
        let opens_container = meta[j].delim_delta > 0;

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

            if meta[row_idx - 1].ends_with_semi || meta[row_idx - 1].is_arm_terminal {
                break;
            }
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
            mem::swap(&mut block_multiset, &mut merge_scratch);

            row_blocks.blocks[row_blocks.len as usize] = PrecomputedBlock {
                k,
                mask,
                chars,
                token_start,
                token_end,
                token_multiset: block_multiset.clone(),
                opens_container,
            };
            row_blocks.len += 1;
        }
        out_blocks[j] = row_blocks;
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn block_lcs_fast_fixed_walk<const K: usize>(
    old_ids: &[u32],
    block_ids: &[u32],
    block_coords: &[(u8, u8)],
    block_token_chars: &[usize],
    start_row: usize,
    k: usize,
    min_block_line_coverage: f64,
    id_ws: &[bool],
    id_alnum: &[bool],
    head_is_stmt_start: bool,
    opens_container: bool,
    is_expand: bool,
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
    let mut most_alnum = 0usize;
    let mut most_n = 0usize;
    let mut moved_lines = 0usize;
    let mut lines_with_matches = 0usize;

    if is_expand && opens_container {
        let head_matches =
            line_matched_counts[0] > 0 && (!line_has_alnum[0] || line_matched_alnum[0] > 0);
        let tail_matches = (line_matched_counts[k - 1] > 0
            && (!line_has_alnum[k - 1] || line_matched_alnum[k - 1] > 0))
            || !line_has_alnum[k - 1];
        if !head_matches || !tail_matches {
            return (0, 0, 0);
        }
        for (li, &count) in line_matched_counts[..k].iter().enumerate() {
            if count == 0 || (line_has_alnum[li] && line_matched_alnum[li] == 0) {
                continue;
            }
            lines_with_matches += 1;
            let alnum = line_matched_alnum[li];
            if alnum > most_alnum || (alnum == most_alnum && count > most_n) {
                most_alnum = alnum;
                most_n = count;
                most = li;
            }
            if li > 0 && count > 0 {
                moved_lines += 1;
            }
        }
    } else {
        for (li, &count) in line_matched_counts[..k].iter().enumerate() {
            if count == 0 || (line_has_alnum[li] && line_matched_alnum[li] == 0) {
                return (0, 0, 0);
            }
            lines_with_matches += 1;
            let alnum = line_matched_alnum[li];
            if alnum > most_alnum || (alnum == most_alnum && count > most_n) {
                most_alnum = alnum;
                most_n = count;
                most = li;
            }
            if li > 0 && count > 0 {
                moved_lines += 1;
            }
        }
    }

    if lines_with_matches < 2 || (lines_with_matches as f64 / k as f64) < min_block_line_coverage {
        return (0, 0, 0);
    }
    let total_alnum: usize = line_matched_alnum[..k].iter().sum();
    let anchor = if let Some(h) = head {
        let head_alnum = line_matched_alnum[h];
        let override_anchor = if head_is_stmt_start {
            false
        } else {
            most_alnum >= head_alnum.saturating_mul(3)
                && most_alnum >= 3
                && most_alnum.saturating_mul(2) >= total_alnum
        };
        if override_anchor { most } else { h }
    } else {
        most
    };
    (matched, moved_lines, anchor)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn block_lcs_fast_fixed<const K: usize>(
    old_ids: &[u32],
    block_ids: &[u32],
    block_coords: &[(u8, u8)],
    block_token_chars: &[usize],
    start_row: usize,
    k: usize,
    min_block_line_coverage: f64,
    id_ws: &[bool],
    id_alnum: &[bool],
    head_is_stmt_start: bool,
    opens_container: bool,
    is_expand: bool,
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
            head_is_stmt_start,
            opens_container,
            is_expand,
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
            head_is_stmt_start,
            opens_container,
            is_expand,
            &p_keys[..p_len],
            &p_masks[..p_len],
            overflow_map.as_ref(),
            &mut v_history,
        )
    }
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn block_lcs_fast_precomputed(
    old_ids: &[u32],
    block_ids: &[u32],
    block_coords: &[(u8, u8)],
    block_token_chars: &[usize],
    start_row: usize,
    k: usize,
    min_block_line_coverage: f64,
    id_ws: &[bool],
    id_alnum: &[bool],
    head_is_stmt_start: bool,
    opens_container: bool,
    is_expand: bool,
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
            head_is_stmt_start,
            opens_container,
            is_expand,
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
            head_is_stmt_start,
            opens_container,
            is_expand,
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
            head_is_stmt_start,
            opens_container,
            is_expand,
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
            head_is_stmt_start,
            opens_container,
            is_expand,
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
            head_is_stmt_start,
            opens_container,
            is_expand,
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
            head_is_stmt_start,
            opens_container,
            is_expand,
        )
    } else {
        (0, 0, 0)
    }
}

/// The block line carrying the transition's anchor `Mod`.
pub fn block_anchor(bm: &BlockMatch, head_is_stmt_start: bool) -> usize {
    let mut head = None;
    let mut most = 0usize;
    let mut most_alnum = 0usize;
    let mut most_n = 0usize;
    for (li, s) in bm.per_line.iter().enumerate() {
        let alnum = bm.per_line_alnum.get(li).copied().unwrap_or(0);
        if alnum > most_alnum || (alnum == most_alnum && s.len() > most_n) {
            most_alnum = alnum;
            most_n = s.len();
            most = li;
        }
        for &ti in s {
            if bm.pair_of.get(&(li, ti)) == Some(&0) {
                head = Some(li);
            }
        }
    }
    let total_alnum: usize = bm.per_line_alnum.iter().sum();
    if let Some(h) = head {
        let head_alnum = bm.per_line_alnum.get(h).copied().unwrap_or(0);
        let override_anchor = if head_is_stmt_start {
            false
        } else {
            most_alnum >= head_alnum.saturating_mul(3)
                && most_alnum >= 3
                && most_alnum.saturating_mul(2) >= total_alnum
        };
        if override_anchor {
            return most;
        }
        return h;
    }
    most
}
