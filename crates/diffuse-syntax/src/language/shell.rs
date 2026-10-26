//! Shell / Bash language profile.

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile};

pub const KEYWORDS: &[&str] = &[
    "break", "case", "continue", "do", "done", "echo", "elif", "else", "esac", "exit", "export",
    "false", "fi", "for", "function", "if", "in", "local", "return", "set", "then", "true",
    "unset", "until", "while",
];

pub const OPERATORS: &[&str] = &["!=", "&&", "++", "--", "<<", "<=", "==", ">=", ">>", "||"];

pub const CONTROL_FLOW: &[&str] = &[
    "break", "case", "continue", "do", "done", "elif", "else", "esac", "fi", "for", "if", "return",
    "then", "until", "while",
];

pub const DECLARATIONS: &[&str] = &["export", "function", "local"];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "shell",
        keywords: KEYWORDS,
        literals: &[],
        control_flow_keywords: CONTROL_FLOW,
        declaration_keywords: DECLARATIONS,
        hash_comment: true,
        line_comment: false,
        types_by_case: false,
        block_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::Keywords,
        significant_indent: false,
        tail_return_arrow: false,
        fn_decl: Some(
            Regex::new(r"^\s*(?:function\s+)?([A-Za-z0-9_]+)\s*\(\)").expect("sh fn_decl"),
        ),
        decl_prefixes: &["function "],
        stmt_head_prefixes: &["local ", "export "],
        operators: OPERATORS,
        scope_stop_prefixes: &[],
        exact_delimiters: BlockDelimStyle::Keywords.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::Keywords.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::Keywords.default_trailing_delimiters(),
        property_syntax: crate::profile::PropertySyntax::None,
        attribute_prefixes: &[],
        continuation_prefixes: &["&&", "||", "|", "\\"],
        import_prefixes: &["source ", ". "],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::String,
    }
}
