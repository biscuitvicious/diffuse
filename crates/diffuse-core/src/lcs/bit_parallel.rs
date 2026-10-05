//! Bit-Parallel LCS (Myers 1999 / Hyyrö 2004 / Crochemore et al. 2001)
//!
//! Word-level parallel dynamic programming for sequence alignment.
//! Represents the DP matrix state as bit-vectors in CPU registers,
//! achieving $O(N \cdot \lceil M / W \rceil)$ throughput with zero heap
//! allocations.

use std::slice;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

/// Maximum sequence length supported by a single 64-bit machine word.
pub const BIT_PARALLEL_WORD_SIZE: usize = 64;

/// Computes the exact length of the Longest Common Subsequence between
/// sequences `a` and `b` where $|a| \le 64$.
///
/// Uses Crochemore et al. / Hyyrö's 4-operation bit-parallel formulation.
#[inline]
pub fn bit_parallel_lcs_len_64(a: &[u32], b: &[u32]) -> usize {
    let m = a.len();
    if m == 0 || b.is_empty() {
        return 0;
    }
    debug_assert!(m <= 64, "Sequence A exceeds 64 elements");

    // Precompute pattern match bitmasks P[c] for each unique symbol in A.
    // Small stack-based linear search or lookup for typical short lines.
    let mut v: u64 = 0;

    // Fast-path for small alphabets / lines: build bitmasks.
    // For small m <= 16, stack array is ultra fast.
    if m <= 16 {
        for &bj in b {
            let mut p_mask: u64 = 0;
            for (i, &ai) in a.iter().enumerate() {
                if ai == bj {
                    p_mask |= 1u64 << i;
                }
            }
            if p_mask != 0 {
                let u = v | p_mask;
                v = u & !(u.wrapping_sub((v << 1) | 1));
            }
        }
    } else {
        // Build position map for A
        let mut p_map: FxHashMap<u32, u64> = FxHashMap::default();
        for (i, &ai) in a.iter().enumerate() {
            *p_map.entry(ai).or_insert(0) |= 1u64 << i;
        }

        for &bj in b {
            if let Some(&p_mask) = p_map.get(&bj) {
                let u = v | p_mask;
                v = u & !(u.wrapping_sub((v << 1) | 1));
            }
        }
    }

    // Bits in V are restricted to the lower M bits.
    let mask = if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
    (v & mask).count_ones() as usize
}

/// Computes the exact length of the Longest Common Subsequence between
/// sequences `a` and `b` where $|a| \le 128$ using 128-bit integer words.
#[inline]
pub fn bit_parallel_lcs_len_128(a: &[u32], b: &[u32]) -> usize {
    let m = a.len();
    if m == 0 || b.is_empty() {
        return 0;
    }
    if m <= 64 {
        return bit_parallel_lcs_len_64(a, b);
    }
    debug_assert!(m <= 128, "Sequence A exceeds 128 elements");

    let mut p_map: FxHashMap<u32, u128> = FxHashMap::default();
    for (i, &ai) in a.iter().enumerate() {
        *p_map.entry(ai).or_insert(0) |= 1u128 << i;
    }

    let mut v: u128 = 0;
    for &bj in b {
        if let Some(&p_mask) = p_map.get(&bj) {
            let u = v | p_mask;
            v = u & !(u.wrapping_sub((v << 1) | 1));
        }
    }

    let mask = if m == 128 {
        u128::MAX
    } else {
        (1u128 << m) - 1
    };
    (v & mask).count_ones() as usize
}

/// Bit-parallel character-weighted LCS scorer for sequences where $|a| \le 64$.
///
/// Computes the number of non-whitespace matched characters between token
/// sequences `a` and `b` using word-level bit-parallel dynamic programming and
/// popcount. Uses a stack buffer of size up to 128 entries for $O(N)$ traceback
/// with zero heap allocations for standard lines.
pub fn bit_parallel_matched_chars_64(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    b_token_chars: &[usize],
) -> usize {
    let m = a.len();
    let n = b.len();
    if m == 0 || n == 0 {
        return 0;
    }
    debug_assert!(m <= 64);

    if n <= 128 {
        let mut v_history = [0u64; 128];
        bit_parallel_matched_chars_64_inner(a, b, id_ws, b_token_chars, &mut v_history[..n])
    } else {
        let mut v_history = vec![0u64; n];
        bit_parallel_matched_chars_64_inner(a, b, id_ws, b_token_chars, &mut v_history)
    }
}

fn bit_parallel_matched_chars_64_inner(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    b_token_chars: &[usize],
    v_history: &mut [u64],
) -> usize {
    let m = a.len();
    let n = b.len();

    // Fast-path pattern masks for small sequences
    let mut v: u64 = 0;
    if m <= 16 {
        for (j, &bj) in b.iter().enumerate() {
            let mut p_mask: u64 = 0;
            for (i, &ai) in a.iter().enumerate() {
                if ai == bj {
                    p_mask |= 1u64 << i;
                }
            }
            if p_mask != 0 {
                let u = v | p_mask;
                v = u & !(u.wrapping_sub((v << 1) | 1));
            }
            v_history[j] = v;
        }
    } else {
        let mut p_map: FxHashMap<u32, u64> = FxHashMap::default();
        for (i, &ai) in a.iter().enumerate() {
            *p_map.entry(ai).or_insert(0) |= 1u64 << i;
        }

        for (j, &bj) in b.iter().enumerate() {
            if let Some(&p_mask) = p_map.get(&bj) {
                let u = v | p_mask;
                v = u & !(u.wrapping_sub((v << 1) | 1));
            }
            v_history[j] = v;
        }
    }

    let mask = if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
    let total_matched_tokens = (v & mask).count_ones() as usize;
    if total_matched_tokens == 0 {
        return 0;
    }

    // Traceback using V_history:
    // L[i, j] = popcount(V_history[j-1] & ((1 << i) - 1))
    let mut i = m;
    let mut j = n;
    let mut matched_chars = 0usize;

    while i > 0 && j > 0 {
        let ai = a[i - 1];
        let bj = b[j - 1];
        if ai == bj {
            // Check if (i-1, j-1) -> (i, j) is a diagonal match step
            let l_curr = if j == 0 {
                0
            } else {
                let m_i = if i == 64 { u64::MAX } else { (1u64 << i) - 1 };
                (v_history[j - 1] & m_i).count_ones() as usize
            };
            let l_diag = if i == 1 || j == 1 {
                0
            } else {
                let m_prev = (1u64 << (i - 1)) - 1;
                (v_history[j - 2] & m_prev).count_ones() as usize
            };
            if l_curr == l_diag + 1 {
                if !id_ws[bj as usize] {
                    matched_chars += b_token_chars[j - 1];
                }
                i -= 1;
                j -= 1;
                continue;
            }
        }

        // Horizontal or vertical step
        let l_up = if i == 1 {
            0
        } else {
            let m_up = (1u64 << (i - 1)) - 1;
            (v_history[j - 1] & m_up).count_ones() as usize
        };
        let l_left = if j == 1 {
            0
        } else {
            let m_i = if i == 64 { u64::MAX } else { (1u64 << i) - 1 };
            (v_history[j - 2] & m_i).count_ones() as usize
        };

        if l_up >= l_left {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    matched_chars
}

/// Bit-parallel character-weighted LCS scorer for sequences where $65 \le |a|
/// \le 128$.
pub fn bit_parallel_matched_chars_128(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    b_token_chars: &[usize],
) -> usize {
    let m = a.len();
    let n = b.len();
    if m == 0 || n == 0 {
        return 0;
    }
    if m <= 64 {
        return bit_parallel_matched_chars_64(a, b, id_ws, b_token_chars);
    }
    debug_assert!(m <= 128);

    if n <= 128 {
        let mut v_history = [0u128; 128];
        bit_parallel_matched_chars_128_inner(a, b, id_ws, b_token_chars, &mut v_history[..n])
    } else {
        let mut v_history = vec![0u128; n];
        bit_parallel_matched_chars_128_inner(a, b, id_ws, b_token_chars, &mut v_history)
    }
}

fn bit_parallel_matched_chars_128_inner(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    b_token_chars: &[usize],
    v_history: &mut [u128],
) -> usize {
    let m = a.len();
    let n = b.len();

    let mut p_map: FxHashMap<u32, u128> = FxHashMap::default();
    for (i, &ai) in a.iter().enumerate() {
        *p_map.entry(ai).or_insert(0) |= 1u128 << i;
    }

    let mut v: u128 = 0;
    for (j, &bj) in b.iter().enumerate() {
        if let Some(&p_mask) = p_map.get(&bj) {
            let u = v | p_mask;
            v = u & !(u.wrapping_sub((v << 1) | 1));
        }
        v_history[j] = v;
    }

    let mask = if m == 128 {
        u128::MAX
    } else {
        (1u128 << m) - 1
    };
    let total_matched_tokens = (v & mask).count_ones() as usize;
    if total_matched_tokens == 0 {
        return 0;
    }

    let mut i = m;
    let mut j = n;
    let mut matched_chars = 0usize;

    while i > 0 && j > 0 {
        let ai = a[i - 1];
        let bj = b[j - 1];
        if ai == bj {
            let l_curr = if j == 0 {
                0
            } else {
                let m_i = if i == 128 {
                    u128::MAX
                } else {
                    (1u128 << i) - 1
                };
                (v_history[j - 1] & m_i).count_ones() as usize
            };
            let l_diag = if i == 1 || j == 1 {
                0
            } else {
                let m_prev = (1u128 << (i - 1)) - 1;
                (v_history[j - 2] & m_prev).count_ones() as usize
            };
            if l_curr == l_diag + 1 {
                if !id_ws[bj as usize] {
                    matched_chars += b_token_chars[j - 1];
                }
                i -= 1;
                j -= 1;
                continue;
            }
        }

        let l_up = if i == 1 {
            0
        } else {
            let m_up = (1u128 << (i - 1)) - 1;
            (v_history[j - 1] & m_up).count_ones() as usize
        };
        let l_left = if j == 1 {
            0
        } else {
            let m_i = if i == 128 {
                u128::MAX
            } else {
                (1u128 << i) - 1
            };
            (v_history[j - 2] & m_i).count_ones() as usize
        };

        if l_up >= l_left {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    matched_chars
}

/// Bit-parallel character-weighted LCS scorer with automatic word-size
/// selection. Returns `Some(matched_chars)` if $|a| \le 128$, or `None` if the
/// sequence exceeds register capacity.
#[inline]
pub fn bit_parallel_matched_chars(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    b_token_chars: &[usize],
) -> Option<usize> {
    if a.len() <= 64 {
        Some(bit_parallel_matched_chars_64(a, b, id_ws, b_token_chars))
    } else if a.len() <= 128 {
        Some(bit_parallel_matched_chars_128(a, b, id_ws, b_token_chars))
    } else {
        None
    }
}

/// Bit-parallel suffix LCS scorer for sequences where $|a| \le 64$.
///
/// Computes the exact suffix dynamic programming table via bit-parallel prefix
/// DP on reversed sequences, then evaluates the exact tie-breaking ladder in
/// forward document order without allocating a 2D matrix.
pub fn bit_parallel_suffix_matched_chars_64(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
) -> usize {
    let mut scratch = Vec::new();
    bit_parallel_suffix_matched_chars_64_scratch(a, b, id_ws, id_alnum, b_token_chars, &mut scratch)
}

pub struct SuffixPattern64<'a> {
    pub a: &'a [u32],
    pub p_keys: [u32; 64],
    pub p_masks: [u64; 64],
    pub p_len: usize,
    pub a_bloom: [u64; 4],
}

impl<'a> SuffixPattern64<'a> {
    pub fn new(a: &'a [u32]) -> Self {
        let n = a.len();
        debug_assert!(n <= 64);

        let mut a_rev = [0u32; 64];
        for (i, &ai) in a.iter().rev().enumerate() {
            a_rev[i] = ai;
        }
        let a_rev = &a_rev[..n];

        let mut p_keys = [0u32; 64];
        let mut p_masks = [0u64; 64];
        let mut p_len = 0;
        let mut a_bloom = [0u64; 4];

        for (i, &ai) in a_rev.iter().enumerate() {
            let bit = (ai as usize) & 255;
            a_bloom[bit / 64] |= 1u64 << (bit % 64);

            let mut found = false;
            for k in 0..p_len {
                if p_keys[k] == ai {
                    p_masks[k] |= 1u64 << i;
                    found = true;
                    break;
                }
            }
            if !found {
                p_keys[p_len] = ai;
                p_masks[p_len] = 1u64 << i;
                p_len += 1;
            }
        }

        Self {
            a,
            p_keys,
            p_masks,
            p_len,
            a_bloom,
        }
    }

    pub fn match_chars(
        &self,
        b: &[u32],
        id_ws: &[bool],
        id_alnum: &[bool],
        b_token_chars: &[usize],
        scratch: &mut Vec<u64>,
    ) -> usize {
        let m = b.len();
        if self.a.is_empty() || m == 0 {
            return 0;
        }
        if scratch.len() < m {
            scratch.resize(m, 0);
        }
        bit_parallel_suffix_walk_64(
            self.a,
            b,
            id_ws,
            id_alnum,
            b_token_chars,
            &self.p_keys[..self.p_len],
            &self.p_masks[..self.p_len],
            &self.a_bloom,
            &mut scratch[..m],
        )
    }
}

pub fn bit_parallel_suffix_matched_chars_64_scratch(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    scratch: &mut Vec<u64>,
) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    SuffixPattern64::new(a).match_chars(b, id_ws, id_alnum, b_token_chars, scratch)
}

#[allow(clippy::too_many_arguments)]
fn bit_parallel_suffix_walk_64(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    p_keys: &[u32],
    p_masks: &[u64],
    a_bloom: &[u64; 4],
    v_history: &mut [u64],
) -> usize {
    let n = a.len();
    let m = b.len();

    let mut v: u64 = 0;
    for (idx, &bj) in b.iter().rev().enumerate() {
        let mut p_mask = 0u64;
        let bit = (bj as usize) & 255;
        if (a_bloom[bit / 64] & (1u64 << (bit % 64))) != 0 {
            for k in 0..p_keys.len() {
                if p_keys[k] == bj {
                    p_mask = p_masks[k];
                    break;
                }
            }
        }
        if p_mask != 0 {
            let u = v | p_mask;
            v = u & !(u.wrapping_sub((v << 1) | 1));
        }
        v_history[idx] = v;
    }

    let dp = |i: usize, j: usize| -> u32 {
        let rem_a = n - i;
        let rem_b = m - j;
        if rem_a == 0 || rem_b == 0 {
            0
        } else {
            let mask = if rem_a == 64 {
                u64::MAX
            } else {
                (1u64 << rem_a) - 1
            };
            (v_history[rem_b - 1] & mask).count_ones()
        }
    };

    suffix_walk_ladder(a, b, id_ws, id_alnum, b_token_chars, dp)
}

/// Shared tie-breaking ladder walk for suffix dynamic programming.
/// Evaluates candidates in forward document order using suffix DP lookups.
#[inline(always)]
pub(crate) fn suffix_walk_ladder<F>(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    dp: F,
) -> usize
where
    F: Fn(usize, usize) -> u32,
{
    let n = a.len();
    let m = b.len();
    if dp(0, 0) == 0 {
        return 0;
    }

    let mut matched = 0usize;
    let mut i = 0;
    let mut j = 0;
    let mut in_add_run = false;

    while i < n && j < m {
        if a[i] == b[j] {
            if id_ws[a[i] as usize]
                && in_add_run
                && !(i + 1 < n && j + 1 < m && a[i + 1] == b[j + 1] && !id_ws[a[i + 1] as usize])
            {
                j += 1;
                continue;
            }
            if in_add_run
                && dp(i, j + 1) == dp(i, j)
                && (j + 1..m).any(|k| b[k] == a[i] && dp(i + 1, k + 1) + 1 == dp(i, j))
            {
                j += 1;
                continue;
            }
            matched += b_token_chars[j];
            i += 1;
            j += 1;
            in_add_run = false;
        } else {
            let down = dp(i + 1, j);
            let right = dp(i, j + 1);
            if down > right {
                i += 1;
            } else if down < right {
                j += 1;
                in_add_run = true;
            } else if id_ws[a[i] as usize] && !id_ws[b[j] as usize] {
                if i + 1 < n && a[i + 1] == b[j] {
                    i += 1;
                } else {
                    j += 1;
                    in_add_run = true;
                }
            } else if id_ws[b[j] as usize] && !id_ws[a[i] as usize] {
                j += 1;
                in_add_run = true;
            } else if !b[j + 1..].contains(&a[i])
                || (!id_alnum[a[i] as usize] && id_alnum[b[j] as usize])
            {
                i += 1;
            } else {
                j += 1;
                in_add_run = true;
            }
        }
    }

    matched
}

/// Bit-parallel suffix LCS scorer for sequences where $65 \le |a| \le 128$.
pub fn bit_parallel_suffix_matched_chars_128(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
) -> usize {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return 0;
    }
    if n <= 64 {
        return bit_parallel_suffix_matched_chars_64(a, b, id_ws, id_alnum, b_token_chars);
    }
    debug_assert!(n <= 128);

    let mut a_rev = [0u32; 128];
    for (i, &ai) in a.iter().rev().enumerate() {
        a_rev[i] = ai;
    }
    let a_rev = &a_rev[..n];

    let mut p_map: FxHashMap<u32, u128> = FxHashMap::default();
    for (i, &ai) in a_rev.iter().enumerate() {
        *p_map.entry(ai).or_insert(0) |= 1u128 << i;
    }

    if m <= 128 {
        let mut v_history = [0u128; 128];
        bit_parallel_suffix_walk_128(
            a,
            b,
            id_ws,
            id_alnum,
            b_token_chars,
            &p_map,
            &mut v_history[..m],
        )
    } else {
        let mut v_history = vec![0u128; m];
        bit_parallel_suffix_walk_128(a, b, id_ws, id_alnum, b_token_chars, &p_map, &mut v_history)
    }
}

#[allow(clippy::too_many_arguments)]
fn bit_parallel_suffix_walk_128(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    p_map: &FxHashMap<u32, u128>,
    v_history: &mut [u128],
) -> usize {
    let n = a.len();
    let m = b.len();

    let mut v: u128 = 0;
    for (idx, &bj) in b.iter().rev().enumerate() {
        if let Some(&p_mask) = p_map.get(&bj) {
            let u = v | p_mask;
            v = u & !(u.wrapping_sub((v << 1) | 1));
        }
        v_history[idx] = v;
    }

    let dp = |i: usize, j: usize| -> u32 {
        let rem_a = n - i;
        let rem_b = m - j;
        if rem_a == 0 || rem_b == 0 {
            0
        } else {
            let mask = if rem_a == 128 {
                u128::MAX
            } else {
                (1u128 << rem_a) - 1
            };
            (v_history[rem_b - 1] & mask).count_ones()
        }
    };

    suffix_walk_ladder(a, b, id_ws, id_alnum, b_token_chars, dp)
}

/// Fixed-capacity multi-word bitvector for bit-parallel LCS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct BitVector<const K: usize> {
    pub words: [u64; K],
}

impl<const K: usize> BitVector<K> {
    pub const ZERO: Self = Self { words: [0; K] };

    #[inline(always)]
    pub fn set_bit(&mut self, i: usize) {
        let w = i / 64;
        let b = i % 64;
        if w < K {
            self.words[w] |= 1u64 << b;
        }
    }

    #[inline(always)]
    pub fn step(&mut self, p: &Self) {
        let mut carry_bit = 1u64;
        let mut borrow = false;
        for k in 0..K {
            let u_k = self.words[k] | p.words[k];
            let next_carry = self.words[k] >> 63;
            let x_k = (self.words[k] << 1) | carry_bit;
            carry_bit = next_carry;

            let (diff1, b1) = u_k.overflowing_sub(x_k);
            let (diff2, b2) = diff1.overflowing_sub(borrow as u64);
            borrow = b1 || b2;

            self.words[k] = u_k & !diff2;
        }
    }

    #[inline(always)]
    pub fn count_ones_prefix(&self, len: usize) -> u32 {
        let full_words = len / 64;
        let rem_bits = len % 64;
        let mut total = 0u32;
        for k in 0..full_words.min(K) {
            total += self.words[k].count_ones();
        }
        if rem_bits > 0 && full_words < K {
            let mask = (1u64 << rem_bits) - 1;
            total += (self.words[full_words] & mask).count_ones();
        }
        total
    }
}

#[allow(clippy::too_many_arguments)]
fn bit_parallel_suffix_walk_fixed_inner<const K: usize>(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    p_keys: &[u32],
    p_masks: &[BitVector<K>],
    overflow_map: Option<&FxHashMap<u32, BitVector<K>>>,
    a_bloom: &[u64; 4],
    v_history: &mut [BitVector<K>],
) -> usize {
    let n = a.len();
    let m = b.len();

    let mut v = BitVector::<K>::ZERO;
    if let Some(map) = overflow_map {
        for (idx, &bj) in b.iter().rev().enumerate() {
            let bit = (bj as usize) & 255;
            if (a_bloom[bit / 64] & (1u64 << (bit % 64))) != 0
                && let Some(p_mask) = map.get(&bj)
            {
                v.step(p_mask);
            }
            v_history[idx] = v;
        }
    } else {
        for (idx, &bj) in b.iter().rev().enumerate() {
            let bit = (bj as usize) & 255;
            if (a_bloom[bit / 64] & (1u64 << (bit % 64))) != 0 {
                for k in 0..p_keys.len() {
                    if p_keys[k] == bj {
                        v.step(&p_masks[k]);
                        break;
                    }
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

    suffix_walk_ladder(a, b, id_ws, id_alnum, b_token_chars, dp)
}

pub struct SuffixPatternFixed<'a, const K: usize> {
    pub a: &'a [u32],
    pub p_keys: [u32; 64],
    pub p_masks: [BitVector<K>; 64],
    pub p_len: usize,
    pub overflow_map: Option<FxHashMap<u32, BitVector<K>>>,
    pub a_bloom: [u64; 4],
}

impl<'a, const K: usize> SuffixPatternFixed<'a, K> {
    pub fn new(a: &'a [u32]) -> Self {
        let n = a.len();
        debug_assert!(n <= K * 64);

        let mut p_keys = [0u32; 64];
        let mut p_masks = [BitVector::<K>::ZERO; 64];
        let mut p_len = 0;
        let mut overflow_map: Option<FxHashMap<u32, BitVector<K>>> = None;
        let mut a_bloom = [0u64; 4];

        for (i, &ai) in a.iter().rev().enumerate() {
            let bit = (ai as usize) & 255;
            a_bloom[bit / 64] |= 1u64 << (bit % 64);

            if let Some(ref mut map) = overflow_map {
                map.entry(ai).or_insert(BitVector::ZERO).set_bit(i);
            } else {
                let mut found = false;
                for k in 0..p_len {
                    if p_keys[k] == ai {
                        p_masks[k].set_bit(i);
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
                        for k in 0..p_len {
                            map.insert(p_keys[k], p_masks[k]);
                        }
                        map.entry(ai).or_insert(BitVector::ZERO).set_bit(i);
                        overflow_map = Some(map);
                    }
                }
            }
        }

        Self {
            a,
            p_keys,
            p_masks,
            p_len,
            overflow_map,
            a_bloom,
        }
    }

    pub fn match_chars(
        &self,
        b: &[u32],
        id_ws: &[bool],
        id_alnum: &[bool],
        b_token_chars: &[usize],
        scratch: &mut Vec<u64>,
    ) -> usize {
        let m = b.len();
        if self.a.is_empty() || m == 0 {
            return 0;
        }
        let needed = m * K;
        if scratch.len() < needed {
            scratch.resize(needed, 0);
        }
        let v_history: &mut [BitVector<K>] =
            unsafe { slice::from_raw_parts_mut(scratch.as_mut_ptr() as *mut BitVector<K>, m) };

        bit_parallel_suffix_walk_fixed_inner(
            self.a,
            b,
            id_ws,
            id_alnum,
            b_token_chars,
            &self.p_keys[..self.p_len],
            &self.p_masks[..self.p_len],
            self.overflow_map.as_ref(),
            &self.a_bloom,
            v_history,
        )
    }
}

fn bit_parallel_suffix_matched_chars_dyn(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    scratch: &mut Vec<u64>,
) -> usize {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return 0;
    }
    let k_words = n.div_ceil(64);

    let mut p_keys: SmallVec<[u32; 64]> = SmallVec::new();
    let mut p_masks: Vec<u64> = Vec::new();
    let mut overflow_map: Option<FxHashMap<u32, usize>> = None;
    let mut a_bloom = [0u64; 4];

    for (i, &ai) in a.iter().rev().enumerate() {
        let bit_b = (ai as usize) & 255;
        a_bloom[bit_b / 64] |= 1u64 << (bit_b % 64);

        let word_idx = i / 64;
        let bit = 1u64 << (i % 64);
        if let Some(ref map) = overflow_map {
            if let Some(&idx) = map.get(&ai) {
                p_masks[idx * k_words + word_idx] |= bit;
            }
        } else if let Some(idx) = p_keys.iter().position(|&k| k == ai) {
            p_masks[idx * k_words + word_idx] |= bit;
        } else if p_keys.len() < 64 {
            let idx = p_keys.len();
            p_keys.push(ai);
            p_masks.resize((idx + 1) * k_words, 0);
            p_masks[idx * k_words + word_idx] |= bit;
        } else {
            let mut map = FxHashMap::default();
            for (idx, &k) in p_keys.iter().enumerate() {
                map.insert(k, idx);
            }
            let idx = p_keys.len();
            p_keys.push(ai);
            map.insert(ai, idx);
            p_masks.resize((idx + 1) * k_words, 0);
            p_masks[idx * k_words + word_idx] |= bit;
            overflow_map = Some(map);
        }
    }

    let mut v = vec![0u64; k_words];
    let needed = m * k_words;
    if scratch.len() < needed {
        scratch.resize(needed, 0);
    }
    let v_history = &mut scratch[..needed];

    for (idx, &bj) in b.iter().rev().enumerate() {
        let bit_b = (bj as usize) & 255;
        let mask_idx = if (a_bloom[bit_b / 64] & (1u64 << (bit_b % 64))) == 0 {
            None
        } else if let Some(ref map) = overflow_map {
            map.get(&bj).copied()
        } else {
            p_keys.iter().position(|&k| k == bj)
        };

        if let Some(m_idx) = mask_idx {
            let p_mask = &p_masks[m_idx * k_words..(m_idx + 1) * k_words];
            let mut carry_bit = 1u64;
            let mut borrow = false;
            for k in 0..k_words {
                let u_k = v[k] | p_mask[k];
                let next_carry = v[k] >> 63;
                let x_k = (v[k] << 1) | carry_bit;
                carry_bit = next_carry;

                let (diff1, b1) = u_k.overflowing_sub(x_k);
                let (diff2, b2) = diff1.overflowing_sub(borrow as u64);
                borrow = b1 || b2;

                v[k] = u_k & !diff2;
            }
        }
        let hist_offset = idx * k_words;
        v_history[hist_offset..hist_offset + k_words].copy_from_slice(&v);
    }

    let dp = |i: usize, j: usize| -> u32 {
        let rem_a = n - i;
        let rem_b = m - j;
        if rem_a == 0 || rem_b == 0 {
            0
        } else {
            let row = &v_history[(rem_b - 1) * k_words..rem_b * k_words];
            let full_words = rem_a / 64;
            let rem_bits = rem_a % 64;
            let mut total = 0u32;
            for &word in &row[..full_words] {
                total += word.count_ones();
            }
            if rem_bits > 0 && full_words < k_words {
                let mask = (1u64 << rem_bits) - 1;
                total += (row[full_words] & mask).count_ones();
            }
            total
        }
    };

    suffix_walk_ladder(a, b, id_ws, id_alnum, b_token_chars, dp)
}

/// Precomputed bit-parallel suffix pattern for sequence A.
/// Allows repeatedly evaluating sequence A against multiple candidate
/// sequences B with zero recomputation of sequence A's pattern tables.
///
/// Variants up to 128 tokens are stored inline to avoid per-row heap
/// allocations during candidate scoring; larger, rare sizes are boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Default)]
pub enum SuffixPattern<'a> {
    #[default]
    Empty,
    P64(SuffixPattern64<'a>),
    P128(SuffixPatternFixed<'a, 2>),
    P256(Box<SuffixPatternFixed<'a, 4>>),
    P512(Box<SuffixPatternFixed<'a, 8>>),
    P1024(Box<SuffixPatternFixed<'a, 16>>),
    P2048(Box<SuffixPatternFixed<'a, 32>>),
    Dyn(&'a [u32]),
}

impl<'a> SuffixPattern<'a> {
    pub fn new(a: &'a [u32]) -> Self {
        let n = a.len();
        if n == 0 {
            Self::Empty
        } else if n <= 64 {
            Self::P64(SuffixPattern64::new(a))
        } else if n <= 128 {
            Self::P128(SuffixPatternFixed::<2>::new(a))
        } else if n <= 256 {
            Self::P256(Box::new(SuffixPatternFixed::<4>::new(a)))
        } else if n <= 512 {
            Self::P512(Box::new(SuffixPatternFixed::<8>::new(a)))
        } else if n <= 1024 {
            Self::P1024(Box::new(SuffixPatternFixed::<16>::new(a)))
        } else if n <= 2048 {
            Self::P2048(Box::new(SuffixPatternFixed::<32>::new(a)))
        } else {
            Self::Dyn(a)
        }
    }

    #[inline]
    pub fn match_chars(
        &self,
        b: &[u32],
        id_ws: &[bool],
        id_alnum: &[bool],
        b_token_chars: &[usize],
        scratch: &mut Vec<u64>,
    ) -> usize {
        match self {
            Self::Empty => 0,
            Self::P64(p) => p.match_chars(b, id_ws, id_alnum, b_token_chars, scratch),
            Self::P128(p) => p.match_chars(b, id_ws, id_alnum, b_token_chars, scratch),
            Self::P256(p) => p.match_chars(b, id_ws, id_alnum, b_token_chars, scratch),
            Self::P512(p) => p.match_chars(b, id_ws, id_alnum, b_token_chars, scratch),
            Self::P1024(p) => p.match_chars(b, id_ws, id_alnum, b_token_chars, scratch),
            Self::P2048(p) => p.match_chars(b, id_ws, id_alnum, b_token_chars, scratch),
            Self::Dyn(a) => {
                bit_parallel_suffix_matched_chars_dyn(a, b, id_ws, id_alnum, b_token_chars, scratch)
            }
        }
    }
}

/// Bit-parallel suffix LCS scorer with automatic word-size selection and
/// caller-managed scratch buffer. Evaluates sequences of arbitrary length at
/// word-level parallel speed with exact tie-breaking and zero per-call heap
/// allocations.
#[inline]
pub fn bit_parallel_suffix_matched_chars_scratch(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    scratch: &mut Vec<u64>,
) -> usize {
    SuffixPattern::new(a).match_chars(b, id_ws, id_alnum, b_token_chars, scratch)
}

/// Bit-parallel suffix LCS scorer with automatic word-size selection.
/// Evaluates sequences of arbitrary length at word-level parallel speed
/// with exact tie-breaking.
#[inline]
pub fn bit_parallel_suffix_matched_chars(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
) -> Option<usize> {
    let mut scratch = Vec::new();
    Some(bit_parallel_suffix_matched_chars_scratch(
        a,
        b,
        id_ws,
        id_alnum,
        b_token_chars,
        &mut scratch,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_mismatch_repro() {
        let a = [
            23, 24, 19, 9, 1, 25, 26, 27, 26, 28, 26, 29, 30, 28, 30, 31, 1, 20, 32, 16, 33,
        ];
        let b = [
            111, 3, 112, 9, 254, 1, 255, 30, 238, 1, 20, 30, 23, 85, 256, 180, 1, 46, 26, 29, 26,
            1, 194, 16,
        ];
        let b_token_chars = [
            4, 1, 3, 1, 4, 1, 6, 1, 2, 1, 1, 1, 3, 7, 5, 2, 1, 5, 1, 6, 1, 1, 2, 3,
        ];
        let ws = vec![false; 300];
        let mut alnum = vec![false; 300];
        let a_alnum = [
            true, true, true, false, false, true, false, true, false, true, false, true, false,
            true, false, true, false, false, true, true, true,
        ];
        let b_alnum = [
            true, false, true, false, true, false, true, false, true, false, false, false, true,
            true, true, true, false, true, false, true, false, false, true, true,
        ];
        for (&id, &is_al) in a.iter().zip(a_alnum.iter()) {
            alnum[id as usize] = is_al;
        }
        for (&id, &is_al) in b.iter().zip(b_alnum.iter()) {
            alnum[id as usize] = is_al;
        }

        let bp_suffix = bit_parallel_suffix_matched_chars_64(&a, &b, &ws, &alnum, &b_token_chars);
        assert_eq!(bp_suffix, 15);
    }

    #[test]
    fn test_differential_against_2d_lcs() {
        let ws = vec![false; 256];
        let alnum = vec![true; 256];
        let mut scratch = Vec::new();

        // Test varying sequence lengths from 1 to 60
        for len in [1, 5, 12, 25, 40, 60] {
            let mut a = Vec::new();
            let mut b = Vec::new();
            let mut b_chars = Vec::new();

            for i in 0..len {
                let token_a = ((i * 7 + 3) % 23) as u32;
                let token_b = ((i * 11 + 5) % 23) as u32;
                a.push(token_a);
                b.push(token_b);
                b_chars.push((token_b as usize % 10) + 1);
            }

            let bp_matched = bit_parallel_matched_chars_64(&a, &b, &ws, &b_chars);
            let scalar_matched =
                crate::lcs::lcs_matched_chars_ids(&a, &b, &ws, &alnum, &b_chars, &mut scratch);

            assert_eq!(
                bp_matched, scalar_matched,
                "Mismatch for len {len}: bp={bp_matched}, scalar={scalar_matched}"
            );
        }
    }

    #[test]
    fn test_differential_128_against_2d_lcs() {
        let ws = vec![false; 256];
        let alnum = vec![true; 256];
        let mut scratch = Vec::new();

        for len in [65, 80, 100, 128] {
            let mut a = Vec::new();
            let mut b = Vec::new();
            let mut b_chars = Vec::new();

            for i in 0..len {
                let token_a = ((i * 13 + 7) % 37) as u32;
                let token_b = ((i * 17 + 11) % 37) as u32;
                a.push(token_a);
                b.push(token_b);
                b_chars.push((token_b as usize % 10) + 1);
            }

            let bp_matched = bit_parallel_matched_chars_128(&a, &b, &ws, &b_chars);
            let scalar_matched =
                crate::lcs::lcs_matched_chars_ids(&a, &b, &ws, &alnum, &b_chars, &mut scratch);

            assert_eq!(
                bp_matched, scalar_matched,
                "Mismatch for len {len}: bp={bp_matched}, scalar={scalar_matched}"
            );
        }
    }

    #[test]
    fn test_fuzz_differential_with_duplicates_and_lengths() {
        let ws = vec![false; 128];
        let alnum = vec![true; 128];
        let mut scratch = Vec::new();

        // Pseudo-random deterministic generator
        let mut seed = 0xdead_beef_cafe_babe_u64;
        let mut rng = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        let mut token_lens = vec![0usize; 128];
        for len in &mut token_lens {
            *len = ((rng() % 8) + 1) as usize;
        }

        for _ in 0..10_000 {
            let len_a = ((rng() % 35) + 1) as usize; // up to 35 tokens
            let len_b = ((rng() % 50) + 1) as usize; // up to 50 tokens
            let mut a = Vec::with_capacity(len_a);
            let mut b = Vec::with_capacity(len_b);
            let mut b_chars = Vec::with_capacity(len_b);

            for _ in 0..len_a {
                // small alphabet 0..15 creates lots of duplicates and ties
                a.push((rng() % 15) as u32);
            }
            for _ in 0..len_b {
                let id = (rng() % 15) as u32;
                b.push(id);
                b_chars.push(token_lens[id as usize]);
            }

            let bp = bit_parallel_matched_chars_64(&a, &b, &ws, &b_chars);
            let sc = crate::lcs::lcs_matched_chars_ids(&a, &b, &ws, &alnum, &b_chars, &mut scratch);

            // Print detail on difference if any
            if bp != sc {
                eprintln!("Difference detected! bp={bp}, sc={sc}");
                eprintln!("a = {a:?}");
                eprintln!("b = {b:?}");
                eprintln!("b_chars = {b_chars:?}");
            }
            assert_eq!(bp, sc, "Mismatch between bit-parallel and scalar LCS");
        }
    }

    fn scalar_suffix_matched_chars_ref(
        a: &[u32],
        b: &[u32],
        id_ws: &[bool],
        id_alnum: &[bool],
        b_token_chars: &[usize],
    ) -> usize {
        let n = a.len();
        let m = b.len();
        if n == 0 || m == 0 {
            return 0;
        }
        let stride = m + 1;
        let mut dp = vec![0u32; (n + 1) * stride];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i * stride + j] = if a[i] == b[j] {
                    dp[(i + 1) * stride + (j + 1)] + 1
                } else {
                    dp[(i + 1) * stride + j].max(dp[i * stride + (j + 1)])
                };
            }
        }
        if dp[0] == 0 {
            return 0;
        }
        let mut matched = 0usize;
        let mut i = 0;
        let mut j = 0;
        let mut in_add_run = false;
        while i < n && j < m {
            if a[i] == b[j] {
                if id_ws[a[i] as usize]
                    && in_add_run
                    && !(i + 1 < n
                        && j + 1 < m
                        && a[i + 1] == b[j + 1]
                        && !id_ws[a[i + 1] as usize])
                {
                    j += 1;
                    continue;
                }
                if in_add_run
                    && dp[i * stride + j + 1] == dp[i * stride + j]
                    && (j + 1..m).any(|k| {
                        b[k] == a[i] && dp[(i + 1) * stride + k + 1] + 1 == dp[i * stride + j]
                    })
                {
                    j += 1;
                    continue;
                }
                matched += b_token_chars[j];
                i += 1;
                j += 1;
                in_add_run = false;
            } else if dp[(i + 1) * stride + j] > dp[i * stride + j + 1] {
                i += 1;
            } else if dp[(i + 1) * stride + j] < dp[i * stride + j + 1] {
                j += 1;
                in_add_run = true;
            } else if id_ws[a[i] as usize] && !id_ws[b[j] as usize] {
                if i + 1 < n && a[i + 1] == b[j] {
                    i += 1;
                } else {
                    j += 1;
                    in_add_run = true;
                }
            } else if id_ws[b[j] as usize] && !id_ws[a[i] as usize] {
                j += 1;
                in_add_run = true;
            } else if !b[j + 1..].contains(&a[i])
                || (!id_alnum[a[i] as usize] && id_alnum[b[j] as usize])
            {
                i += 1;
            } else {
                j += 1;
                in_add_run = true;
            }
        }
        matched
    }

    #[test]
    fn test_fuzz_suffix_differential_with_ws_and_alnum() {
        let mut seed = 0xcafe_babe_dead_beef_u64;
        let mut rng = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        let mut ws = vec![false; 32];
        let mut alnum = vec![false; 32];
        let mut token_lens = vec![0usize; 32];

        // Tokens 0..3: whitespace
        ws[0..4].fill(true);

        // Tokens 4..19: alphanumeric
        alnum[4..20].fill(true);

        // Tokens 20..31: delimiters (remain ws=false, alnum=false by default)

        // Assign character lengths to all 32 synthetic token IDs (1 for
        // whitespace, 1..7 for others)
        for i in 0..32 {
            token_lens[i] = if ws[i] { 1 } else { (i % 7) + 1 };
        }

        // Test 64-bit word
        for _ in 0..5_000 {
            let len_a = ((rng() % 35) + 1) as usize;
            let len_b = ((rng() % 50) + 1) as usize;
            let mut a = Vec::with_capacity(len_a);
            let mut b = Vec::with_capacity(len_b);
            let mut b_chars = Vec::with_capacity(len_b);

            for _ in 0..len_a {
                a.push((rng() % 24) as u32);
            }
            for _ in 0..len_b {
                let id = (rng() % 24) as u32;
                b.push(id);
                b_chars.push(token_lens[id as usize]);
            }

            let bp = bit_parallel_suffix_matched_chars_64(&a, &b, &ws, &alnum, &b_chars);
            let sc = scalar_suffix_matched_chars_ref(&a, &b, &ws, &alnum, &b_chars);

            assert_eq!(
                bp, sc,
                "Suffix 64 mismatch! bp={bp}, sc={sc}\na={a:?}\nb={b:?}"
            );
        }

        // Test 128-bit word
        for _ in 0..2_000 {
            let len_a = ((rng() % 45) + 65) as usize; // 65..110 tokens
            let len_b = ((rng() % 60) + 40) as usize; // 40..100 tokens
            let mut a = Vec::with_capacity(len_a);
            let mut b = Vec::with_capacity(len_b);
            let mut b_chars = Vec::with_capacity(len_b);

            for _ in 0..len_a {
                a.push((rng() % 28) as u32);
            }
            for _ in 0..len_b {
                let id = (rng() % 28) as u32;
                b.push(id);
                b_chars.push(token_lens[id as usize]);
            }

            let bp = bit_parallel_suffix_matched_chars_128(&a, &b, &ws, &alnum, &b_chars);
            let sc = scalar_suffix_matched_chars_ref(&a, &b, &ws, &alnum, &b_chars);

            assert_eq!(
                bp, sc,
                "Suffix 128 mismatch! bp={bp}, sc={sc}\na={a:?}\nb={b:?}"
            );
        }
    }

    #[test]
    fn test_suffix_matched_chars_above_128() {
        let mut ws = vec![false; 64];
        let mut alnum = vec![false; 64];
        let mut token_lens = vec![1usize; 64];
        ws[0..4].fill(true);
        alnum[4..30].fill(true);
        for i in 0..64 {
            token_lens[i] = if ws[i] { 1 } else { (i % 7) + 1 };
        }

        let mut seed = 0x1234_5678_9abc_def0_u64;
        let mut rng = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        for len_a in [150, 250, 350, 600, 1200] {
            let len_b = len_a + 50;
            let mut a = Vec::with_capacity(len_a);
            let mut b = Vec::with_capacity(len_b);
            let mut b_chars = Vec::with_capacity(len_b);

            for _ in 0..len_a {
                a.push((rng() % 40) as u32);
            }
            for _ in 0..len_b {
                let id = (rng() % 40) as u32;
                b.push(id);
                b_chars.push(token_lens[id as usize]);
            }

            let bp = bit_parallel_suffix_matched_chars(&a, &b, &ws, &alnum, &b_chars);
            assert!(bp.is_some(), "Expected Some for len_a = {len_a}");
            let sc = scalar_suffix_matched_chars_ref(&a, &b, &ws, &alnum, &b_chars);
            assert_eq!(bp.unwrap(), sc, "Mismatch for len_a = {len_a}");
        }
    }

    #[test]
    fn test_suffix_matched_chars_scratch_reuse() {
        let ws = vec![false; 64];
        let alnum = vec![true; 64];
        let a = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let b = vec![2, 4, 6, 8, 10];
        let b_chars = vec![1; 5];
        let mut scratch = Vec::new();
        let res1 =
            bit_parallel_suffix_matched_chars_scratch(&a, &b, &ws, &alnum, &b_chars, &mut scratch);
        let cap_before = scratch.capacity();
        assert!(cap_before > 0);
        let res2 =
            bit_parallel_suffix_matched_chars_scratch(&a, &b, &ws, &alnum, &b_chars, &mut scratch);
        assert_eq!(res1, res2);
        assert_eq!(
            scratch.capacity(),
            cap_before,
            "scratch capacity should be reused without reallocating"
        );
    }

    #[test]
    fn test_suffix_pattern_matches_direct() {
        let ws = vec![false; 64];
        let alnum = vec![true; 64];
        let mut scratch1 = Vec::new();
        let mut scratch2 = Vec::new();

        for len_a in [0, 10, 50, 100, 200, 400, 800, 1200, 2200] {
            let len_b = len_a + 20;
            let a: Vec<u32> = (0..len_a).map(|x| (x % 30) as u32).collect();
            let b1: Vec<u32> = (0..len_b).map(|x| ((x * 2) % 30) as u32).collect();
            let b2: Vec<u32> = (0..len_b).map(|x| ((x * 3) % 30) as u32).collect();
            let b1_chars = vec![1; b1.len()];
            let b2_chars = vec![1; b2.len()];

            let pat = SuffixPattern::new(&a);
            let m1_pat = pat.match_chars(&b1, &ws, &alnum, &b1_chars, &mut scratch1);
            let m1_dir = bit_parallel_suffix_matched_chars_scratch(
                &a,
                &b1,
                &ws,
                &alnum,
                &b1_chars,
                &mut scratch2,
            );
            assert_eq!(m1_pat, m1_dir, "mismatch on b1 for len_a = {len_a}");

            let m2_pat = pat.match_chars(&b2, &ws, &alnum, &b2_chars, &mut scratch1);
            let m2_dir = bit_parallel_suffix_matched_chars_scratch(
                &a,
                &b2,
                &ws,
                &alnum,
                &b2_chars,
                &mut scratch2,
            );
            assert_eq!(m2_pat, m2_dir, "mismatch on b2 for len_a = {len_a}");
        }
    }
}
