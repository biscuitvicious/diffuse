//! Rust language profile.

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile};

pub const KEYWORDS: &[&str] = &[
    "Self",
    "as",
    "async",
    "await",
    "break",
    "const",
    "continue",
    "crate",
    "dyn",
    "else",
    "enum",
    "extern",
    "fn",
    "for",
    "if",
    "impl",
    "in",
    "let",
    "loop",
    "macro_rules",
    "match",
    "mod",
    "move",
    "mut",
    "pub",
    "ref",
    "return",
    "self",
    "static",
    "struct",
    "super",
    "trait",
    "type",
    "unsafe",
    "use",
    "where",
    "while",
    "yield",
];

pub const OPERATORS: &[&str] = &[
    "!=", "%=", "&&", "&=", "*=", "+=", "-=", "->", "..", "...", "..=", "/=", "::", "<<", "<<=",
    "<=", "==", "=>", ">=", ">>", ">>=", "^=", "|=", "||",
];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "rust",
        keywords: KEYWORDS,
        line_comment: true,
        block_comment: true,
        types_by_case: true,
        hash_comment: false,
        // Doc-comment examples: `/// ```rust` opens a rust-syntax region.
        // The capture allows commas: a rust doc fence carries a
        // comma-separated doctest attribute list —
        // `/// ```rust,no_run`, `/// ```should_panic`, `/// ```edition2021`.
        fence: Some(
            Regex::new(r"^\s*(///|//!)\s*```([a-z0-9_+,-]*)\s*$").expect("the rust doc fence"),
        ),
        // An unlabeled doc fence is a rust example.
        unlabeled_fence_lang: Some("rust"),
        // The region's code lines are comment-prefixed (`/// let x = 5;`)
        // — strip the prefix, classify the remainder.
        fence_strip: Some(Regex::new(r"^\s*(///|//!)?\s*").expect("the doc prefix")),
        markdown: false,
        block_style: BlockDelimStyle::Braces,
        fn_decl: Some(
            Regex::new(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?fn\s+([A-Za-z0-9_]+)")
                .expect("rs fn_decl"),
        ),
        decl_prefixes: &[
            "pub fn ",
            "pub async fn ",
            "pub const ",
            "pub static ",
            "pub struct ",
            "pub enum ",
            "pub trait ",
            "pub mod ",
            "pub type ",
            "fn ",
            "async fn ",
            "impl ",
            "struct ",
            "enum ",
            "trait ",
            "mod ",
            "type ",
        ],
        operators: OPERATORS,
        scope_stop_prefixes: &[
            "struct ",
            "pub struct ",
            "pub(super) struct ",
            "enum ",
            "pub enum ",
            "impl ",
            "pub impl ",
            "type ",
            "pub type ",
        ],
        exact_delimiters: BlockDelimStyle::Braces.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::Braces.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::Braces.default_trailing_delimiters(),
        property_syntax: crate::profile::PropertySyntax::Colon,
    }
}
