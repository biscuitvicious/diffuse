//! Config file language profiles — TOML, YAML, JSON.
//!
//! TOML and YAML share the hash-comment + colon-indent style but have no
//! prose classifier. JSON uses brace structure with no comments.

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile};

/// TOML and YAML — hash comments, colon-indented blocks, no prose.
pub fn toml_yaml_profile() -> LangProfile {
    LangProfile {
        keywords: &[],
        hash_comment: true,
        line_comment: false,
        types_by_case: false,
        block_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::IndentColon,
        fn_decl: Some(Regex::new(r"^\s*\[+([A-Za-z0-9_.-]+)\]+").expect("toml section header")),
        decl_prefixes: &["["],
        scope_stop_prefixes: &["["],
    }
}

/// JSON — brace structure, no comments.
pub fn json_profile() -> LangProfile {
    LangProfile {
        keywords: &[],
        hash_comment: false,
        line_comment: false,
        types_by_case: false,
        block_comment: false,
        fence: None,
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: false,
        block_style: BlockDelimStyle::Braces,
        fn_decl: None,
        decl_prefixes: &[],
        scope_stop_prefixes: &[],
    }
}
