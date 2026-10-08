//! Markdown language profile (prose + fenced code regions).

use regex::Regex;

use crate::profile::{BlockDelimStyle, LangProfile};

pub fn profile() -> LangProfile {
    LangProfile {
        name: "markdown",
        keywords: &[],
        hash_comment: true,
        line_comment: false,
        types_by_case: false,
        block_comment: false,
        // The md fence: ` ```lang ` — group 1 = the region language
        // (commas allowed for ` ```rust,no_run ` inside an .md file).
        fence: Some(Regex::new(r"^\s*```([a-z0-9_+,-]*)\s*$").expect("the md fence")),
        // An unlabeled fence = the plain "code" style (no syntax).
        unlabeled_fence_lang: None,
        fence_strip: None,
        markdown: true,
        block_style: BlockDelimStyle::Braces,
        significant_indent: false,
        implicit_return: false,
        fn_decl: None,
        decl_prefixes: &[],
        operators: &[],
        scope_stop_prefixes: &[],
        exact_delimiters: BlockDelimStyle::Braces.default_exact_delimiters(),
        tail_delimiters: BlockDelimStyle::Braces.default_tail_delimiters(),
        trailing_delimiters: BlockDelimStyle::Braces.default_trailing_delimiters(),
        property_syntax: crate::profile::PropertySyntax::None,
        attribute_prefixes: &[],
        continuation_prefixes: &[],
        import_prefixes: &[],
        section_headers: false,
        single_quote: crate::profile::SingleQuoteKind::String,
    }
}
