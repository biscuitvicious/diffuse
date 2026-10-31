//! Note-agnostic terminal Markdown classification.
//!
//! Parses Markdown into a block model ([`Block`]/[`Flow`]) and inline spans
//! ([`Span`]/[`Inline`]) without emitting source marker characters. Pure
//! classification — no width, no ANSI. The renderer consumes this.

pub mod block;
pub mod inline;

#[cfg(test)]
mod tests;

pub use block::{Block, ColumnAlign, Flow, ListItem, ListMarker, blocks};
pub use inline::{Inline, Span, TaskState, spans};
use pulldown_cmark::Options;

/// GFM parsing options shared by block and inline parsing.
pub(crate) fn md_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
}
