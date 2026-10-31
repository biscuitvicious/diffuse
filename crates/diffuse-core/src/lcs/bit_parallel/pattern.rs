//! Unified Suffix Pattern & Dynamic Fallback LCS.
//!
//! Provides the primary [`SuffixPattern`] enum dispatcher that selects between
//! inline single/double word registers, fixed-capacity multi-word bitvectors,
//! and dynamic heap fallback based on sequence length.

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::{fixed::SuffixPatternFixed, ladder::suffix_walk_ladder, word::SuffixPattern64};

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
