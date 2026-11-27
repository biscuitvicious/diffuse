//! Python language profile.

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile};

pub const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "self", "try",
    "while", "with", "yield",
];

pub const OPERATORS: &[&str] = &[
    "!=", "%=", "&=", "**", "**=", "*=", "+=", "-=", "->", "//", "//=", "/=", ":=", "<<", "<<=",
    "<=", "==", ">=", ">>", ">>=", "^=", "|=",
];

pub const CONTROL_FLOW: &[&str] = &[
    "break", "case", "continue", "elif", "else", "except", "finally", "for", "if", "match", "pass",
    "raise", "return", "try", "while", "yield",
];

pub const DECLARATIONS: &[&str] = &["class", "def"];

pub const BUILTIN_TYPES: &[&str] = &[
    "bool", "bytes", "complex", "dict", "float", "int", "list", "object", "set", "str", "tuple",
];

pub const OPERATOR_KEYWORDS: &[&str] = &["and", "as", "in", "is", "not", "or"];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "python",
        keywords: KEYWORDS,
        literals: &[],
        builtin_types: BUILTIN_TYPES,
        operator_keywords: OPERATOR_KEYWORDS,
        control_flow_keywords: CONTROL_FLOW,
        declaration_keywords: DECLARATIONS,
        hash_comment: true,
        line_comment: false,
        types_by_case: true,
        block_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::IndentColon,
        significant_indent: true,
        tail_return_arrow: true,
        fn_decl: Some(Regex::new(r"^\s*(?:async\s+)?def\s+([A-Za-z0-9_]+)").expect("py fn_decl")),
        decl_prefixes: &["def ", "async def ", "class "],
        stmt_head_prefixes: &[],
        operators: OPERATORS,
        scope_stop_prefixes: &["class "],
        exact_delimiters: BlockDelimStyle::IndentColon.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::IndentColon.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::IndentColon.default_trailing_delimiters(),
        property_syntax: crate::profile::PropertySyntax::Colon,
        attribute_prefixes: &["@"],
        continuation_prefixes: &["and ", "or ", ".", "\\"],
        trailing_continuation_ops: &[
            "=", ":=", "+", "-", "*", "/", "//", "%", "**", "&", "|", "^", "<<", ">>", "and", "or",
            "\\", "+=", "-=", "*=", "/=", "//=", "%=", "**=", "&=", "|=", "^=", "<<=", ">>=",
        ],
        import_prefixes: &["import ", "from "],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::String,
    }
}
