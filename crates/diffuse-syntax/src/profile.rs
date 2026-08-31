//! Language profile data model — the static description of a programming
//! language's syntactic shape (comment style, block delimiters, keywords,
//! function signature patterns, and fence/region rules).

use regex::Regex;

/// The block boundary delimiter style for a programming language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BlockDelimStyle {
    /// Braces (`{` and `}`) like Rust, TS/JS, C/C++, Go, Java.
    #[default]
    Braces,
    /// Indentation + colon (`:` and dedent) like Python, YAML.
    IndentColon,
    /// Shell keywords (`then`/`do` and `fi`/`done`/`esac`).
    Keywords,
}

/// A per-language syntax profile.
#[derive(Debug)]
pub struct LangProfile {
    /// Canonical language name (e.g. "rust", "typescript", "python").
    pub name: &'static str,
    /// Sorted keyword slice for `O(log n)` binary-search lookup.
    pub keywords: &'static [&'static str],
    /// `#` to end of line is a comment (python/shell/yaml/toml/markdown).
    pub hash_comment: bool,
    /// `//` to end of line.
    pub line_comment: bool,
    /// `/* ... */` block comment.
    pub block_comment: bool,
    /// Capitalized identifiers are types (Rust/TS/C/Go...).
    pub types_by_case: bool,
    /// The FENCE: a full-line pattern (the last capture group = the region
    /// language, possibly empty) that toggles a SYNTAX REGION — the span of
    /// lines between two matching lines classifies with a different
    /// language than the file's (md's `` ```lang ``, rust's doc-comment
    /// `` /// ```lang `` examples). None = the profile has no regions.
    pub fence: Option<Regex>,
    /// The language an UNLABELED fence (` ``` ` with no capture) defaults to.
    /// None = the plain "code" style (no syntax, no highlighting) — md's
    /// tree diagrams stay unhighlighted; rust's unlabeled doc examples
    /// default to the file's own rust.
    pub unlabeled_fence_lang: Option<&'static str>,
    /// A leading per-line pattern to strip before classifying the region's
    /// content: a rust doc fence's code lines are comment-prefixed
    /// (`/// let x = 5;`) — the prefix classifies as a comment, the
    /// remainder as the region language.
    pub fence_strip: Option<Regex>,
    /// The default region is the markdown PROSE (headings, emphasis — the
    /// `markdown_tokens` classifier). True only for the md family proper —
    /// toml/yaml share the profile's token set but not the prose tokens.
    pub markdown: bool,
    /// The block delimiter style (braces, colon + indentation, or keywords).
    pub block_style: BlockDelimStyle,
    /// Regex capturing the function name (group 1) from a function declaration
    /// line.
    pub fn_decl: Option<Regex>,
    /// Prefixes that introduce declarations/items in this language.
    /// Used to break multi-line statement expansion in `pass_assemble_mods`.
    pub decl_prefixes: &'static [&'static str],
    /// Sorted multi-character operators for `O(log n)` binary-search lookup.
    pub operators: &'static [&'static str],
    /// Structural scope-boundary prefixes — items that delimit top-level scopes
    /// and act as hard stops in `fn_of`'s backward scan (class, struct, impl,
    /// …). Distinct from `decl_prefixes` which may include variable
    /// declarations (`const`, `let`) that are valid inside function bodies.
    pub scope_stop_prefixes: &'static [&'static str],
}

impl LangProfile {
    /// Whether the keyword slice contains `word` (binary search on sorted
    /// slice).
    pub fn is_keyword(&self, word: &str) -> bool {
        self.keywords.binary_search(&word).is_ok()
    }

    /// Whether `t` is a recognized multi-character operator in this language.
    #[inline]
    pub fn is_operator_anchor(&self, t: &str) -> bool {
        self.operators.binary_search(&t).is_ok()
    }

    /// Whether a line acts as an opening delimiter of a syntactic block.
    pub fn is_block_open(&self, line: &str) -> bool {
        let t = line.trim();
        match self.block_style {
            BlockDelimStyle::Braces => t.ends_with('{'),
            BlockDelimStyle::IndentColon => t.ends_with(':'),
            BlockDelimStyle::Keywords => {
                t.ends_with("then") || t.ends_with("do") || t.ends_with("in") || t.ends_with('{')
            }
        }
    }

    /// Whether a line acts as an explicit closing delimiter of a block.
    pub fn is_block_close(&self, line: &str) -> bool {
        let t = line.trim();
        match self.block_style {
            BlockDelimStyle::Braces => t == "}" || t == "};" || t == "},",
            BlockDelimStyle::IndentColon => false,
            BlockDelimStyle::Keywords => t == "fi" || t == "done" || t == "esac" || t == "}",
        }
    }

    /// Whether a line acts as a closing delimiter for expressions or statements
    /// (parentheses, brackets, braces).
    pub fn is_expression_close(&self, line: &str) -> bool {
        let t = line.trim();
        match self.block_style {
            BlockDelimStyle::Braces => {
                t == "};" || t == "}," || t == "}" || t == ");" || t == ")," || t == ")"
            }
            BlockDelimStyle::IndentColon => t == ")" || t == "]",
            BlockDelimStyle::Keywords => {
                t == "fi" || t == "done" || t == "esac" || t == "}" || t == ")" || t == "]"
            }
        }
    }

    /// Whether a line is a declaration start or item boundary that breaks a
    /// multi-line continuation.
    pub fn is_decl_break(&self, line: &str) -> bool {
        let t = line.trim();
        if t.starts_with("///") || t.starts_with("//!") || t.starts_with("#[") {
            return true;
        }
        self.decl_prefixes.iter().any(|&pf| t.starts_with(pf))
    }

    /// Whether a line is a continuation expression (e.g. operator lead or
    /// compound block start).
    pub fn is_continuation_line(&self, line: &str) -> bool {
        let t = line.trim();
        t.starts_with("&&") || t.starts_with("||") || t.starts_with('.') || t == "{" || t == "}"
    }

    /// Whether a line is an import/use statement that can reflow multi-line
    /// braces.
    pub fn is_import_statement(&self, line: &str) -> bool {
        let t = line.trim();
        (t.starts_with("use ") || t.starts_with("import ") || t.contains('{'))
            && t.trim_end().ends_with(';')
    }

    /// Whether a line starts a comment in this language.
    pub fn is_comment_line(&self, line: &str) -> bool {
        let t = line.trim();
        if self.line_comment
            && (t.starts_with("//") || t.starts_with("///") || t.starts_with("//!"))
        {
            return true;
        }
        if self.block_comment && (t.starts_with("/*") || t.starts_with('*')) {
            return true;
        }
        if self.hash_comment && t.starts_with('#') {
            return true;
        }
        false
    }
}

/// The markdown SGR attributes (the renderer's `MD_SGR`).
pub const MD_SGR_BOLD: u32 = 1;
pub const MD_SGR_ITALIC: u32 = 3;
pub const MD_SGR_UNDERLINE: u32 = 4;
pub const MD_SGR_STRIKE: u32 = 9;
