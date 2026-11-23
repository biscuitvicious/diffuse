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

pub const LITERALS: &[&str] = &["NULL", "false", "nullptr", "true"];

pub const CONTROL_FLOW: &[&str] = &[
    "break", "case", "catch", "continue", "default", "do", "else", "for", "goto", "if", "return",
    "switch", "throw", "try", "while",
];

pub const DECLARATIONS: &[&str] = &["class", "enum", "struct", "typedef", "using"];

pub const BUILTIN_TYPES: &[&str] = &[
    "bool",
    "char",
    "char16_t",
    "char32_t",
    "char8_t",
    "double",
    "float",
    "int",
    "int16_t",
    "int32_t",
    "int64_t",
    "int8_t",
    "intptr_t",
    "long",
    "ptrdiff_t",
    "short",
    "signed",
    "size_t",
    "ssize_t",
    "uint16_t",
    "uint32_t",
    "uint64_t",
    "uint8_t",
    "uintptr_t",
    "unsigned",
    "void",
    "wchar_t",
];

pub const OPERATOR_KEYWORDS: &[&str] = &["sizeof"];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "c_family",
        keywords: KEYWORDS,
        literals: LITERALS,
        builtin_types: BUILTIN_TYPES,
        operator_keywords: OPERATOR_KEYWORDS,
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
