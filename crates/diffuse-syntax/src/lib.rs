//! One-for-one port of `review.ts`'s syntax classification (PLAN.md §3):
//! `langProfile`, `tokenReFor`, `classifyTokens`. The output carries the
//! SEMANTIC colors (`skw`, `str`, …) — the renderer
//! maps them to theme colors.
//!
//! ## Module structure
//!
//! | Submodule | Contents |
//! |-----------|----------|
//! | [`profile`] | `LangProfile`, `BlockDelimStyle`, SGR constants |
//! | [`language`] | Per-language keyword slices and profile constructors |
//! | [`registry`] | `lang_profile`, `fence_lang_ext`, `token_re_for`, scanner |
//! | [`classifier`] | `classify_tokens`, `classify_line`, markdown classifiers |
//! | [`fence`] | `FenceRegion`, `fence_regions`, `region_at` |
//!
//! ## Usage
//!
//! Import from the specific submodule, or use [`prelude`] to pull in the
//! types that routinely appear together across the engine.

pub mod classifier;
pub mod color;
pub mod fence;
pub mod language;
pub mod markdown;
pub mod profile;
pub mod registry;

/// The syntax prelude — the handful of items that routinely appear together
/// across the pipeline, render, and notes layers. Import with
/// `use crate::prelude::*`.
pub mod prelude {
    pub use super::{
        classifier::{
            ClassifiedToken, classify_line, classify_line_with_next, classify_tokens,
            for_each_classified_token, for_each_line_classified_token_with_next,
            for_each_markdown_token, for_each_markdown_token_with_header, is_line_alignment_row,
            markdown_document_tokens, markdown_tokens,
        },
        color::*,
        fence::{FenceRegion, fence_regions, region_at},
        profile::{
            BlockDelimStyle, CharLiteralProfile, CloseDelimiter, LangProfile, PropertySyntax,
            SGR_BOLD, SGR_BOLD_UNDERLINE, SGR_DIM, SGR_ITALIC, SGR_STRIKE, SGR_UNDERLINE,
            SingleQuoteKind, UnicodeEscapeStyle,
        },
        registry::{
            REGISTERED_EXTENSIONS, all_registered_profiles, fence_lang_ext, first_word,
            is_word_char, lang_profile, word_prefix, word_prefix_len,
        },
    };
}

pub use classifier::md_inner_tokens;
pub use color::TokenColor;

#[cfg(test)]
mod tests;
