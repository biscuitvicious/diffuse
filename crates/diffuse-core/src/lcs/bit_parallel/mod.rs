//! Bit-Parallel LCS (Myers 1999 / Hyyrö 2004 / Crochemore et al. 2001)
//!
//! Word-level parallel dynamic programming for sequence alignment.
//! Represents the DP matrix state as bit-vectors in CPU registers,
//! achieving $O(N \cdot \lceil M / W \rceil)$ throughput with zero heap
//! allocations.

pub mod fixed;
pub mod ladder;
pub mod pattern;
pub mod word;

#[cfg(test)]
mod tests;

pub use fixed::{BitVector, SuffixPatternFixed};
pub use pattern::{
    SuffixPattern, bit_parallel_suffix_matched_chars, bit_parallel_suffix_matched_chars_scratch,
};
pub use word::{
    BIT_PARALLEL_WORD_SIZE, SuffixPattern64, bit_parallel_lcs_len_64, bit_parallel_lcs_len_128,
    bit_parallel_matched_chars, bit_parallel_matched_chars_64, bit_parallel_matched_chars_128,
    bit_parallel_suffix_matched_chars_64, bit_parallel_suffix_matched_chars_64_scratch,
    bit_parallel_suffix_matched_chars_128,
};
