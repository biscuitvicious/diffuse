//! One-for-one port of `review.ts`'s syntax classification (PLAN.md §3):
//! `langProfile`, `tokenReFor`, `classifyTokens`. The output carries the
//! SEMANTIC colors (`syntaxKeyword`, `syntaxString`, …) — the renderer
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
            ClassifiedToken, classify_line, classify_tokens, for_each_classified_token,
            for_each_markdown_token, markdown_tokens,
        },
        fence::{FenceRegion, fence_regions, region_at},
        profile::{
            BlockDelimStyle, CloseDelimiter, LangProfile, MD_SGR_BOLD, MD_SGR_ITALIC,
            MD_SGR_STRIKE, MD_SGR_UNDERLINE, PropertySyntax,
        },
        registry::{
            REGISTERED_EXTENSIONS, all_registered_profiles, ascii_word_prefix,
            ascii_word_prefix_len, fence_lang_ext, first_ascii_word, is_ascii_word_char,
            lang_profile,
        },
    };
}

pub use classifier::md_inner_tokens;

#[cfg(test)]
mod tests;
