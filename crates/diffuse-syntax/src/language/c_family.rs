//! C / C++ / Java language profiles (one shared profile).

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile};

pub const KEYWORDS: &[&str] = &[
    "auto", "break", "case", "char", "const", "continue", "default", "do", "double", "else",
    "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long", "register",
    "restrict", "return", "short", "signed", "sizeof", "static", "struct", "switch", "this",
    "typedef", "union", "unsigned", "void", "volatile", "while",
];

pub const OPERATORS: &[&str] = &[
    "!=", "%=", "&&", "&=", "*=", "++", "+=", "--", "-=", "->", "/=", "::", "<<", "<<=", "<=",
    "==", ">=", ">>", ">>=", "^=", "|=", "||",
];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "c_family",
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
        significant_indent: false,
        tail_return_arrow: true,
        // C/C++ function definitions are hard to distinguish from declarations;
        // capture the name from the last word-token before the opening paren.
        fn_decl: Some(
            Regex::new(r"^\s*(?:[A-Za-z0-9_<>:*&\s]+\s+)+([A-Za-z0-9_]+)\s*\([^;]*\)\s*\{?\s*$")
                .expect("c fn_decl"),
        ),
        decl_prefixes: &[
            "class ",
            "struct ",
            "enum ",
            "typedef ",
            "template ",
            "namespace ",
        ],
        stmt_head_prefixes: &[],
        operators: OPERATORS,
        scope_stop_prefixes: &["class ", "struct ", "enum ", "namespace "],
        exact_delimiters: BlockDelimStyle::Braces.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::Braces.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::Braces.default_trailing_delimiters(),
        property_syntax: crate::profile::PropertySyntax::None,
        attribute_prefixes: &["[[", "__attribute__"],
        continuation_prefixes: &["&&", "||", ".", "->"],
        import_prefixes: &["#include ", "import "],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::Char(
            crate::profile::CharLiteralProfile::C_FAMILY,
        ),
    }
}
