//! Semantic token classification, information entropy calculation, and
//! multi-token phrase landmark detection for robust alignment anchoring.

pub mod class;
pub mod container;
pub mod entropy;
pub mod whitespace;

#[cfg(test)]
mod tests;

pub(crate) use class::is_function_word;
pub use class::{TokenClass, classify_token, is_anchor_content_token};
pub(crate) use container::get_enclosing_caller;
pub use container::{ContainerKind, compute_parenthesis_depth, compute_token_containers};
pub use entropy::{
    MIN_PHRASE_ENTROPY, MIN_SINGLE_TOKEN_ENTROPY, entropy_anchors, multi_token_phrase_anchors,
    token_self_information,
};
pub use whitespace::{WhitespaceKind, classify_token_whitespace, classify_whitespace};
