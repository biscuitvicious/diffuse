//! Config file language profiles — TOML, YAML, JSON.
//!
//! TOML and YAML share the hash-comment + colon-indent style but have no
//! prose classifier. JSON uses brace structure with no comments.

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile, PropertySyntax};

/// TOML — hash comments, section headers, colon/equals properties.
pub fn toml_profile() -> LangProfile {
    LangProfile {
        name: "toml",
        keywords: &[],
        literals: &["false", "true"],
        builtin_types: &[],
        operator_keywords: &[],
        control_flow_keywords: &[],
        declaration_keywords: &[],
        hash_comment: true,
        line_comment: false,
        types_by_case: false,
        block_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::IndentColon,
        significant_indent: false,
        tail_return_arrow: false,
        fn_decl: Some(Regex::new(r"^\s*\[+([A-Za-z0-9_.-]+)\]+").expect("toml section header")),
        decl_prefixes: &["["],
        stmt_head_prefixes: &[],
        operators: &[],
        scope_stop_prefixes: &["["],
        exact_delimiters: BlockDelimStyle::IndentColon.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::IndentColon.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::IndentColon.default_trailing_delimiters(),
        property_syntax: PropertySyntax::ColonOrEquals,
        attribute_prefixes: &[],
        continuation_prefixes: &[],
        import_prefixes: &[],
        section_headers: true,
        single_quote: crate::profile::SingleQuoteKind::String,
    }
}

/// YAML — hash comments, colon-indented blocks, significant indentation.
pub fn yaml_profile() -> LangProfile {
    LangProfile {
        name: "yaml",
        keywords: &[],
        literals: &["false", "null", "true"],
        builtin_types: &[],
        operator_keywords: &[],
        control_flow_keywords: &[],
        declaration_keywords: &[],
        hash_comment: true,
        line_comment: false,
        types_by_case: false,
        block_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::IndentColon,
        significant_indent: true,
        tail_return_arrow: false,
        fn_decl: None,
        decl_prefixes: &[],
        stmt_head_prefixes: &[],
        operators: &[],
        scope_stop_prefixes: &[],
        exact_delimiters: BlockDelimStyle::IndentColon.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::IndentColon.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::IndentColon.default_trailing_delimiters(),
        property_syntax: PropertySyntax::Colon,
        attribute_prefixes: &[],
        continuation_prefixes: &[],
        import_prefixes: &[],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::String,
    }
}

/// Backward-compatible profile constructor for toml/yaml.
pub fn toml_yaml_profile() -> LangProfile {
    toml_profile()
}

pub const JSON_LITERALS: &[&str] = &["false", "null", "true"];

/// JSON & JSON Lines (JSONL) — brace structure, no comments.
pub fn json_profile() -> LangProfile {
    LangProfile {
        name: "json",
        keywords: &[],
        literals: JSON_LITERALS,
        builtin_types: &[],
        operator_keywords: &[],
        control_flow_keywords: &[],
        declaration_keywords: &[],
        hash_comment: false,
        line_comment: false,
        types_by_case: false,
        block_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::Braces,
        significant_indent: false,
        tail_return_arrow: false,
        fn_decl: None,
        decl_prefixes: &[],
        stmt_head_prefixes: &[],
        operators: &[":"],
        scope_stop_prefixes: &[],
        exact_delimiters: BlockDelimStyle::Braces.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::Braces.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::Braces.default_trailing_delimiters(),
        property_syntax: PropertySyntax::Colon,
        attribute_prefixes: &[],
        continuation_prefixes: &[],
        import_prefixes: &[],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::None,
    }
}
