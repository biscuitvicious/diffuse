//! Token classifier — semantic syntax coloring of text spans.
//!
//! `classify_tokens` is the hot path: it splits a text fragment into tokens
//! via the hand-rolled scanner and maps each token to a semantic color name
//! (`skw`, `str`, …). The markdown classifiers
//! (`markdown_tokens`, `md_inner_tokens`) handle doc-comment prose and the
//! markdown default region.

pub mod markdown;
pub mod style;
pub mod table;
pub mod token;

pub use markdown::{
    for_each_markdown_token, for_each_markdown_token_with_header, for_each_md_inner_token,
    markdown_document_tokens, markdown_tokens, md_inner_tokens,
};
pub use table::is_line_alignment_row;
pub use token::{
    ClassifiedToken, classify_line, classify_line_with_next, classify_tokens,
    for_each_classified_token, for_each_classified_token_with_header,
    for_each_line_classified_token, for_each_line_classified_token_with_next,
};
