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

/// Closing delimiter classification for a line (e.g. `}`, `};`, `)`, `);`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CloseDelimiter {
    #[default]
    None,
    BraceSemi,
    Brace,
    ParenSemi,
    Paren,
    BracketSemi,
    Bracket,
    Keyword,
}

impl CloseDelimiter {
    /// Default trailing close classification for unknown languages (defaults to
    /// standard brace syntax).
    pub fn default_classify_tail(line: &str) -> Self {
        let t = line.trim();
        if t.ends_with("};") || t.ends_with("},") {
            CloseDelimiter::BraceSemi
        } else if t.ends_with('}') {
            CloseDelimiter::Brace
        } else if t.ends_with(");") || t.ends_with("),") {
            CloseDelimiter::ParenSemi
        } else if t.ends_with(')') {
            CloseDelimiter::Paren
        } else if t.ends_with("];") || t.ends_with("],") {
            CloseDelimiter::BracketSemi
        } else if t.ends_with(']') {
            CloseDelimiter::Bracket
        } else {
            CloseDelimiter::None
        }
    }

    /// Default exact close classification for unknown languages.
    pub fn default_classify_exact(line: &str) -> Self {
        let t = line.trim();
        if t == "};" || t == "}," {
            CloseDelimiter::BraceSemi
        } else if t == "}" {
            CloseDelimiter::Brace
        } else if t == ");" || t == ")," {
            CloseDelimiter::ParenSemi
        } else if t == ")" {
            CloseDelimiter::Paren
        } else if t == "];" || t == "]," {
            CloseDelimiter::BracketSemi
        } else if t == "]" {
            CloseDelimiter::Bracket
        } else {
            CloseDelimiter::None
        }
    }

    /// Default direct tail matching when no profile is available.
    pub fn default_is_direct_tail_match(tail_a: Self, tail_b: Self, matched_chars: usize) -> bool {
        tail_a != CloseDelimiter::None && tail_a == tail_b && matched_chars >= 8
    }

    /// Default method continuation matching when no profile is available.
    pub fn default_is_method_continuation_match(
        line_a: &str,
        line_b: &str,
        tail_a: Self,
        tail_b: Self,
        matched_chars: usize,
    ) -> bool {
        if matched_chars < 8 || tail_a == CloseDelimiter::None || tail_a != tail_b {
            return false;
        }
        fn extract(line: &str) -> Option<&str> {
            let t = line.trim_start();
            if t.starts_with('.') {
                crate::registry::first_ascii_word(t.trim_start_matches('.'))
                    .filter(|w| !w.is_empty())
            } else {
                None
            }
        }
        let contains = |line: &str, m: &str| -> bool {
            let mut search_from = 0;
            while let Some(idx) = line[search_from..].find('.') {
                let abs_idx = search_from + idx + 1;
                if line[abs_idx..].starts_with(m) {
                    let after = abs_idx + m.len();
                    if after == line.len()
                        || !line[after..]
                            .chars()
                            .next()
                            .unwrap()
                            .is_ascii_alphanumeric()
                    {
                        return true;
                    }
                }
                search_from += idx + 1;
            }
            false
        };
        let m_a = extract(line_a);
        let m_b = extract(line_b);
        match (m_a, m_b) {
            (Some(a), Some(b)) => a == b,
            (Some(a), None) => contains(line_b, a),
            (None, Some(b)) => contains(line_a, b),
            (None, None) => false,
        }
    }
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
        if t.starts_with("if ")
            || t.starts_with("let ")
            || t.starts_with("const ")
            || t.starts_with("static ")
            || t.starts_with("for ")
            || t.starts_with("while ")
            || t.starts_with("match ")
            || t.starts_with("return ")
            || t.starts_with("return;")
        {
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

    /// Classifies the trailing closing delimiter of a line according to this
    /// language's syntax.
    pub fn classify_close_tail(&self, line: &str) -> CloseDelimiter {
        let t = line.trim();
        match self.block_style {
            BlockDelimStyle::Braces => {
                if t.ends_with("};") || t.ends_with("},") {
                    CloseDelimiter::BraceSemi
                } else if t.ends_with('}') {
                    CloseDelimiter::Brace
                } else if t.ends_with(");") || t.ends_with("),") {
                    CloseDelimiter::ParenSemi
                } else if t.ends_with(')') {
                    CloseDelimiter::Paren
                } else if t.ends_with("];") || t.ends_with("],") {
                    CloseDelimiter::BracketSemi
                } else if t.ends_with(']') {
                    CloseDelimiter::Bracket
                } else {
                    CloseDelimiter::None
                }
            }
            BlockDelimStyle::IndentColon => {
                if t.ends_with(')') {
                    CloseDelimiter::Paren
                } else if t.ends_with(']') {
                    CloseDelimiter::Bracket
                } else if t.ends_with('}') {
                    CloseDelimiter::Brace
                } else {
                    CloseDelimiter::None
                }
            }
            BlockDelimStyle::Keywords => {
                if t == "fi" || t == "done" || t == "esac" {
                    CloseDelimiter::Keyword
                } else if t.ends_with('}') {
                    CloseDelimiter::Brace
                } else if t.ends_with(')') {
                    CloseDelimiter::Paren
                } else if t.ends_with(']') {
                    CloseDelimiter::Bracket
                } else {
                    CloseDelimiter::None
                }
            }
        }
    }

    /// Classifies whether a line consists solely of a closing delimiter in this
    /// language.
    pub fn classify_exact_close(&self, line: &str) -> CloseDelimiter {
        let t = line.trim();
        match self.block_style {
            BlockDelimStyle::Braces => {
                if t == "};" || t == "}," {
                    CloseDelimiter::BraceSemi
                } else if t == "}" {
                    CloseDelimiter::Brace
                } else if t == ");" || t == ")," {
                    CloseDelimiter::ParenSemi
                } else if t == ")" {
                    CloseDelimiter::Paren
                } else if t == "];" || t == "]," {
                    CloseDelimiter::BracketSemi
                } else if t == "]" {
                    CloseDelimiter::Bracket
                } else {
                    CloseDelimiter::None
                }
            }
            BlockDelimStyle::IndentColon => {
                if t == ")" {
                    CloseDelimiter::Paren
                } else if t == "]" {
                    CloseDelimiter::Bracket
                } else if t == "}" {
                    CloseDelimiter::Brace
                } else {
                    CloseDelimiter::None
                }
            }
            BlockDelimStyle::Keywords => {
                if t == "fi" || t == "done" || t == "esac" {
                    CloseDelimiter::Keyword
                } else if t == "}" {
                    CloseDelimiter::Brace
                } else if t == ")" {
                    CloseDelimiter::Paren
                } else if t == "]" {
                    CloseDelimiter::Bracket
                } else {
                    CloseDelimiter::None
                }
            }
        }
    }

    /// Checks whether two closing tails qualify as a direct tail match in this
    /// language across indentation shifts.
    pub fn is_direct_tail_match(
        &self,
        tail_a: CloseDelimiter,
        tail_b: CloseDelimiter,
        matched_chars: usize,
    ) -> bool {
        if matched_chars < 8 || tail_a == CloseDelimiter::None || tail_a != tail_b {
            return false;
        }
        match self.block_style {
            BlockDelimStyle::Braces => true,
            BlockDelimStyle::IndentColon => {
                matches!(tail_a, CloseDelimiter::Paren | CloseDelimiter::Bracket)
            }
            BlockDelimStyle::Keywords => {
                matches!(
                    tail_a,
                    CloseDelimiter::Keyword | CloseDelimiter::Paren | CloseDelimiter::Bracket
                )
            }
        }
    }

    /// Whether `line` begins with a method or member continuation in this
    /// language.
    pub fn is_method_continuation(&self, line: &str) -> bool {
        let t = line.trim_start();
        if t.starts_with('.') {
            return true;
        }
        if (self.name == "c_family" || self.name == "c" || self.name == "cpp")
            && t.starts_with("->")
        {
            return true;
        }
        false
    }

    /// Extracts the method name from a method continuation line in this
    /// language.
    pub fn method_continuation_name<'a>(&self, line: &'a str) -> Option<&'a str> {
        let t = line.trim_start();
        let rest = if t.starts_with('.') {
            t.trim_start_matches('.')
        } else if (self.name == "c_family" || self.name == "c" || self.name == "cpp")
            && t.starts_with("->")
        {
            &t[2..]
        } else {
            return None;
        };
        crate::registry::first_ascii_word(rest).filter(|w| !w.is_empty())
    }

    /// Whether `line` contains a call or access to `method_name` using member
    /// access syntax appropriate for this language.
    pub fn contains_method_call(&self, line: &str, method_name: &str) -> bool {
        if method_name.is_empty() {
            return false;
        }
        let dot_call = |op: &str| -> bool {
            let mut search_from = 0;
            while let Some(idx) = line[search_from..].find(op) {
                let abs_idx = search_from + idx + op.len();
                if line[abs_idx..].starts_with(method_name) {
                    let after = abs_idx + method_name.len();
                    if after == line.len()
                        || !line[after..]
                            .chars()
                            .next()
                            .unwrap()
                            .is_ascii_alphanumeric()
                    {
                        return true;
                    }
                }
                search_from += idx + op.len();
            }
            false
        };
        if dot_call(".") {
            return true;
        }
        if (self.name == "c_family" || self.name == "c" || self.name == "cpp") && dot_call("->") {
            return true;
        }
        false
    }

    /// Checks whether two lines represent a matching method continuation in
    /// this language, where at least one line is a continuation and the
    /// other line accesses the same method with compatible closing
    /// delimiter semantics.
    pub fn is_method_continuation_match(
        &self,
        line_a: &str,
        line_b: &str,
        tail_a: CloseDelimiter,
        tail_b: CloseDelimiter,
        matched_chars: usize,
    ) -> bool {
        if matched_chars < 8 || tail_a == CloseDelimiter::None || tail_a != tail_b {
            return false;
        }
        let m_a = if self.is_method_continuation(line_a) {
            self.method_continuation_name(line_a)
        } else {
            None
        };
        let m_b = if self.is_method_continuation(line_b) {
            self.method_continuation_name(line_b)
        } else {
            None
        };
        match (m_a, m_b) {
            (Some(a), Some(b)) => a == b,
            (Some(a), None) => self.contains_method_call(line_b, a),
            (None, Some(b)) => self.contains_method_call(line_a, b),
            (None, None) => false,
        }
    }

    /// Trims trailing syntax delimiters and whitespace from a statement or
    /// expression line (e.g. `;`, `,`, `{`, `}`, `)` in brace languages, or
    /// `:` in indent-colon languages).
    pub fn trim_trailing_delimiters<'a>(&self, line: &'a str) -> &'a str {
        line.trim_end_matches(|c: char| c.is_whitespace() || self.is_trailing_delimiter(c))
    }

    /// Whether a character acts as a trailing syntax delimiter in this
    /// language.
    pub fn is_trailing_delimiter(&self, c: char) -> bool {
        match self.block_style {
            BlockDelimStyle::Braces => matches!(c, '{' | '}' | ';' | ':' | ',' | ')' | ']'),
            BlockDelimStyle::IndentColon => matches!(c, ':' | ',' | ')' | ']'),
            BlockDelimStyle::Keywords => matches!(c, ',' | ')' | ']' | ';' | '{' | '}'),
        }
    }
}

/// The markdown SGR attributes (the renderer's `MD_SGR`).
pub const MD_SGR_BOLD: u32 = 1;
pub const MD_SGR_ITALIC: u32 = 3;
pub const MD_SGR_UNDERLINE: u32 = 4;
pub const MD_SGR_STRIKE: u32 = 9;
