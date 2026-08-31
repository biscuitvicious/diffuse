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

pub const OPERATORS: &[&str] = &[
    "!=", "%=", "&&", "&=", "*=", "++", "+=", "--", "-=", "->", "/=", ":=", "<<", "<<=", "<=",
    "==", "=>", ">=", ">>", ">>=", "^=", "|=", "||",
];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "go",
        keywords: KEYWORDS,
        line_comment: true,
        block_comment: true,
        types_by_case: true,
        hash_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::Braces,
        fn_decl: Some(
            Regex::new(r"^\s*func\s+(?:\([^)]*\)\s+)?([A-Za-z0-9_]+)").expect("go fn_decl"),
        ),
        decl_prefixes: &["func ", "type ", "var ", "const ", "package ", "import "],
        operators: OPERATORS,
        scope_stop_prefixes: &["type ", "package ", "import "],
    }
}
