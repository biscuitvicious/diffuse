//! Markdown language profile (prose + fenced code regions).

use regex::Regex;

use crate::syntax::profile::{BlockDelimStyle, LangProfile};

pub fn profile() -> LangProfile {
    LangProfile {
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
        fn_decl: None,
        decl_prefixes: &[],
        scope_stop_prefixes: &[],
    }
}
