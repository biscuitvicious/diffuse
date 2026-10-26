//! Go language profile.

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile};

pub const KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

pub const LITERALS: &[&str] = &["false", "nil", "true"];

pub const CONTROL_FLOW: &[&str] = &[
    "break",
    "case",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "go",
    "goto",
    "if",
    "return",
    "select",
    "switch",
];

pub const DECLARATIONS: &[&str] = &["const", "func", "type", "var"];

pub const OPERATORS: &[&str] = &[
    "!=", "%=", "&&", "&=", "*=", "++", "+=", "--", "-=", "->", "/=", ":=", "<<", "<<=", "<=",
    "==", "=>", ">=", ">>", ">>=", "^=", "|=", "||",
];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "go",
        keywords: KEYWORDS,
        literals: LITERALS,
        control_flow_keywords: CONTROL_FLOW,
        declaration_keywords: DECLARATIONS,
        line_comment: true,
        block_comment: true,
        types_by_case: true,
        hash_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::Braces,
        significant_indent: false,
        tail_return_arrow: false,
        fn_decl: Some(
            Regex::new(r"^\s*func\s+(?:\([^)]*\)\s+)?([A-Za-z0-9_]+)").expect("go fn_decl"),
        ),
        decl_prefixes: &["func ", "type ", "var ", "const ", "package ", "import "],
        stmt_head_prefixes: &[],
        operators: OPERATORS,
        scope_stop_prefixes: &["type ", "package ", "import "],
        exact_delimiters: BlockDelimStyle::Braces.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::Braces.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::Braces.default_trailing_delimiters(),
        property_syntax: crate::profile::PropertySyntax::Colon,
        attribute_prefixes: &["//go:"],
        continuation_prefixes: &["&&", "||", "."],
        import_prefixes: &["import "],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::Char(crate::profile::CharLiteralProfile::GO),
    }
}
