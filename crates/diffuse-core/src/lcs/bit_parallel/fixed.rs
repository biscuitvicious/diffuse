//! Fixed-Capacity Multi-Word Bit-Vector & Suffix Pattern.
//!
//! Multi-word bit-parallel dynamic programming using const-generic arrays of
//! 64-bit words (`BitVector<K>`), avoiding heap allocations for sequences up to
//! 2048 elements.

use std::slice;

use rustc_hash::FxHashMap;

use super::ladder::suffix_walk_ladder;

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
