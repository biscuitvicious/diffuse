//! Single-Word (64-bit) & Double-Word (128-bit) Bit-Parallel LCS.
//!
//! Word-level parallel dynamic programming using 64-bit and 128-bit CPU
//! registers, achieving $O(N)$ traceback and high throughput with zero heap
//! allocations for sequences up to 128 elements.

use rustc_hash::FxHashMap;

use super::ladder::suffix_walk_ladder;

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
