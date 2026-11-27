//! TypeScript / JavaScript / JSX / MJS language profiles (one shared profile).

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile};

pub const KEYWORDS: &[&str] = &[
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "of",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];

pub const LITERALS: &[&str] = &["false", "null", "true"];

pub const OPERATORS: &[&str] = &[
    "!=", "!==", "%=", "&&", "&&=", "&=", "**", "**=", "*=", "++", "+=", "--", "-=", "->", "/=",
    "<<", "<<=", "<=", "==", "===", "=>", ">=", ">>", ">>=", ">>>", ">>>=", "??", "??=", "^=",
    "|=", "||", "||=",
];

pub const CONTROL_FLOW: &[&str] = &[
    "break", "case", "catch", "continue", "default", "do", "else", "finally", "for", "if",
    "return", "switch", "throw", "try", "while", "yield",
];

pub const DECLARATIONS: &[&str] = &[
    "class",
    "const",
    "enum",
    "function",
    "interface",
    "let",
    "type",
    "var",
];

pub const BUILTIN_TYPES: &[&str] = &[
    "any",
    "bigint",
    "boolean",
    "never",
    "number",
    "object",
    "string",
    "symbol",
    "undefined",
    "unknown",
    "void",
];

pub const OPERATOR_KEYWORDS: &[&str] = &["as", "in", "instanceof", "of", "typeof"];

pub fn profile() -> LangProfile {
    LangProfile {
        name: "web",
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
        tail_return_arrow: false,
        fn_decl: Some(
            Regex::new(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z0-9_]+)")
                .expect("ts fn_decl"),
        ),
        decl_prefixes: &[
            "function ",
            "async function ",
            "class ",
            "interface ",
            "type ",
            "enum ",
            "const ",
            "let ",
            "var ",
            "export ",
            "export default ",
            "export async function ",
            "export class ",
            "export interface ",
            "export type ",
            "export enum ",
            "export const ",
            "export let ",
        ],
        stmt_head_prefixes: &[],
        operators: OPERATORS,
        scope_stop_prefixes: &[
            "class ",
            "interface ",
            "type ",
            "enum ",
            "export class ",
            "export interface ",
            "export type ",
            "export enum ",
        ],
        exact_delimiters: BlockDelimStyle::Braces.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::Braces.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::Braces.default_trailing_delimiters(),
        property_syntax: crate::profile::PropertySyntax::Colon,
        attribute_prefixes: &["@"],
        continuation_prefixes: &["&&", "||", ".", "?.", "as ", "satisfies "],
        trailing_continuation_ops: &[
            "=", "=>", "&&", "||", "??", "+", "-", "*", "/", "%", "&", "|", "^", ".", "?.", "?",
            "+=", "-=", "*=", "/=", "%=", "&&=", "||=", "??=",
        ],
        import_prefixes: &["import ", "export {"],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::String,
    }
}
