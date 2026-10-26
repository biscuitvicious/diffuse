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
    "false",
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
    "true",
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

pub const CONTROL_FLOW: &[&str] = &[
    "break", "continue", "else", "for", "if", "loop", "match", "return", "while", "yield",
];

pub const DECLARATIONS: &[&str] = &[
    "const", "enum", "fn", "impl", "let", "mod", "static", "struct", "trait", "type",
];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "rust",
        keywords: KEYWORDS,
        literals: &[],
        control_flow_keywords: CONTROL_FLOW,
        declaration_keywords: DECLARATIONS,
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
        significant_indent: false,
        tail_return_arrow: true,
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
        stmt_head_prefixes: &["let ", "const ", "static "],
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
        trailing_delimiters: &['{', '}', ';', ',', ')', ']'],
        property_syntax: crate::profile::PropertySyntax::Colon,
        attribute_prefixes: &["#[", "#!"],
        continuation_prefixes: &["&&", "||", ".", "as "],
        import_prefixes: &["use ", "pub use "],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::Char(
            crate::profile::CharLiteralProfile::RUST,
        ),
    }
}
