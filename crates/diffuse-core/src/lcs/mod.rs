//! Word-level diff and longest common subsequence alignment engine.

pub mod arena;
pub mod bit_parallel;
pub mod pairs;
pub mod raw;
pub mod spans;
pub mod tokenizer;

#[cfg(test)]
mod tests;

pub use arena::{
    DEFAULT_TOKEN_PRODUCT_CAP, LcsScratchArena, Weights, set_token_product_cap, token_product_cap,
};
pub use pairs::{
    lcs_matched_chars_ids, lcs_pairs, lcs_pairs_anchored, lcs_pairs_ids, lcs_pairs_recursive,
    lcs_pairs_with_profile,
};
pub use raw::{
    is_container_delim, raw_lcs_pairs, raw_lcs_pairs_with_arena,
    raw_lcs_pairs_with_arena_and_weights, raw_lcs_pairs_with_context, raw_lcs_pairs_with_lookahead,
};
pub use spans::{
    WordSpans, is_affix_pair, merged_runs_for_texts_with_profile,
    merged_runs_for_texts_with_profile_and_cap, owned_runs_for_texts_with_profile, spans_from,
    word_spans, word_spans_and_runs_with_profile, word_spans_and_runs_with_profile_and_cap,
    word_spans_with_profile, word_spans_with_profile_and_cap,
};
pub use tokenizer::{
    LineTokenizer, char_len, char_slice, for_each_token, is_token_salient_in_context,
    line_has_salient_token, token_is_alnum, token_is_salient, token_is_ws, tokenize_line,
    unique_token_anchors,
};
pub(crate) use tokenizer::{are_contiguous_subwords, is_subword_token, tokens_can_match};
