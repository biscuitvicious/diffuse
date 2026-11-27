//! Language profile data model and syntactic queries.

use regex::Regex;
use unicode_segmentation::UnicodeSegmentation;

use super::{
    block::{BlockDelimStyle, CloseDelimiter, PropertySyntax},
    delimiters::{
        is_closing_delimiter_char, is_opening_delimiter_char, is_statement_terminator_char,
    },
    literal::SingleQuoteKind,
};

/// A per-language syntax profile.
#[derive(Debug)]
pub struct LangProfile {
    /// Canonical language name (e.g. "rust", "typescript", "python").
    pub name: &'static str,
    /// Sorted keyword slice for `O(log n)` binary-search lookup.
    pub keywords: &'static [&'static str],
    /// Sorted literal token slice for `O(log n)` binary-search lookup (e.g.
    /// `true`, `false`, `nil`, `null`).
    pub literals: &'static [&'static str],
    /// Sorted built-in type slice for `O(log n)` binary-search lookup (e.g.
    /// `bool`, `f64`, `int`, `string`, `usize`).
    pub builtin_types: &'static [&'static str],
    /// Sorted operator keyword slice for `O(log n)` binary-search lookup (e.g.
    /// `as`, `in`, `is`, `typeof`).
    pub operator_keywords: &'static [&'static str],
    /// Sorted control-flow keyword slice for `O(log n)` binary-search lookup
    /// (e.g. `if`, `else`, `match`, `for`, `while`, `return`).
    pub control_flow_keywords: &'static [&'static str],
    /// Sorted declaration keyword slice for `O(log n)` binary-search lookup
    /// (e.g. `let`, `const`, `var`, `fn`, `def`, `class`, `struct`).
    pub declaration_keywords: &'static [&'static str],
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
    /// Whether indentation is syntactically significant in this language
    /// (e.g. Python, YAML). When false, indentation is strictly a soft
    /// cost hint and cannot be used to make semantic assertions.
    pub significant_indent: bool,
    /// Whether multi-line function tails spell the return type as `) -> T`
    /// (Rust, Python, C++ trailing returns). Only these profiles answer
    /// `is_return_tail_position` affirmatively. Shared profiles cover
    /// languages without the construct (C, Java): the shape essentially
    /// cannot occur there, so the flag is dead in practice for them.
    pub tail_return_arrow: bool,
    /// Regex capturing the function name (group 1) from a function declaration
    /// line.
    pub fn_decl: Option<Regex>,
    /// Prefixes that introduce declarations/items in this language.
    /// Consulted by section classification and cluster cut barriers so
    /// distinct declarations are never merged into one reflow group.
    pub decl_prefixes: &'static [&'static str],
    /// Prefixes that begin a statement head anywhere, including unfinished
    /// continuations (e.g. variable declarations like `let x =`). Consulted
    /// by statement-start classification. Unlike `decl_prefixes`, implies
    /// nothing about sections: a `let` inside a function body is Code.
    pub stmt_head_prefixes: &'static [&'static str],
    /// Sorted multi-character operators for `O(log n)` binary-search lookup.
    pub operators: &'static [&'static str],
    /// Structural scope-boundary prefixes — items that delimit top-level scopes
    /// and act as hard stops in `fn_of`'s backward scan (class, struct, impl,
    /// …). Distinct from `decl_prefixes` which may include variable
    /// declarations (`const`, `let`) that are valid inside function bodies.
    pub scope_stop_prefixes: &'static [&'static str],
    /// Exact closing delimiters (e.g. `}` or `fi`).
    pub exact_delimiters: &'static [(&'static str, CloseDelimiter)],
    /// Trailing closing delimiters checked with `ends_with`.
    pub tail_delimiters: &'static [(&'static str, CloseDelimiter)],
    /// Characters that act as trailing statement/expression delimiters.
    pub trailing_delimiters: &'static [char],
    /// Syntax style used for key-value properties, struct fields, or dict
    /// entries.
    pub property_syntax: PropertySyntax,
    /// Prefixes that introduce attributes, annotations, or decorators (e.g.
    /// `#[` for Rust, `@` for Python/JS).
    pub attribute_prefixes: &'static [&'static str],
    /// Prefixes that introduce multi-line continuation expressions (e.g. `&&`,
    /// `||`, `.`).
    pub continuation_prefixes: &'static [&'static str],
    /// Operators and connectives that, when appearing as the final code token
    /// on a line (after stripping trailing comments), leave expression valence
    /// unsatisfied and signal continuation onto an indented following line.
    /// Defined atomically per language so syntax such as `:` can act as a
    /// field/property continuation in Rust or JSON without affecting block
    /// headers in Python or YAML.
    pub trailing_continuation_ops: &'static [&'static str],
    /// Prefixes that introduce import or use statements (e.g. `use ` for Rust,
    /// `import ` for Python/Go).
    pub import_prefixes: &'static [&'static str],
    /// Whether the language uses section headers like `[section]` (TOML, INI).
    pub section_headers: bool,
    /// Single-quote parsing mode (strings, char literals, or none).
    pub single_quote: SingleQuoteKind,
}

impl LangProfile {
    /// Extracts a return-slot tail's return type: the text between the
    /// last return arrow and the language's own block opener (`-> T {` in
    /// brace languages, `-> T:` in indent-colon languages). `None` when the
    /// language opts out of return slots (`tail_return_arrow`), the line
    /// does not end in the block opener, the return type is absent
    /// (`-> {`), or the arrow trails the opener. Only the type's existence
    /// is ever asserted — its spelling is never inspected.
    pub fn return_slot_type<'a>(&self, trimmed: &'a str) -> Option<&'a str> {
        if !self.tail_return_arrow {
            return None;
        }
        let opener = match self.block_style {
            BlockDelimStyle::Braces => '{',
            BlockDelimStyle::IndentColon => ':',
            BlockDelimStyle::Keywords => return None,
        };
        let head = trimmed.trim_end().strip_suffix(opener)?;
        let idx = head.rfind("->")?;
        let body = head[idx + 2..].trim();
        body.contains(char::is_alphanumeric).then_some(body)
    }

    /// Whether a trimmed line is a return-slot tail (`-> T {`, `-> T:`): a
    /// return-arrow line whose return type exists. Language knowledge stays
    /// in the profile — `Self::return_slot_type` decides the shape, the
    /// engine only consults the answer. Only profiles that opt in via
    /// `tail_return_arrow` answer true; every other language — and files
    /// without a known profile — is silent, so the engine never assumes
    /// function syntax it has not been told about. The shape is
    /// deliberately narrow: no other construct in these languages carries
    /// an arrow followed by a type and the block opener.
    pub fn is_return_tail_position(&self, trimmed: &str) -> bool {
        self.return_slot_type(trimmed).is_some()
    }

    /// The current function name in context for a return-slot line: the
    /// line's own `fn_decl` capture when the signature head sits on the
    /// line itself, else the nearest `fn_decl` capture among the preceding
    /// context lines (a wrapped signature whose head is on an earlier
    /// line). `None` when no function definition is at-or-before the line.
    pub fn current_fn_name<'t>(&self, line: &'t str, preceding: &[&'t str]) -> Option<&'t str> {
        if !self.tail_return_arrow {
            return None;
        }
        let re = self.fn_decl.as_ref()?;
        if let Some(c) = re.captures(line) {
            return c.get(1).map(|m| m.as_str());
        }
        preceding
            .iter()
            .rev()
            .find_map(|l| re.captures(l))
            .and_then(|c| c.get(1).map(|m| m.as_str()))
    }

    /// Extracts the property or field name from a line if it matches this
    /// language's property syntax.
    pub fn extract_property_key<'a>(&self, line: &'a str) -> Option<&'a str> {
        if self.property_syntax == PropertySyntax::None {
            return None;
        }
        let t = line.trim();
        // 1. Quoted key: `"key":` or `'key':` or `"key" =`
        if t.starts_with('"') || t.starts_with('\'') {
            let quote = t.chars().next()?;
            let rest = &t[1..];
            if let Some(end_quote) = rest.find(quote) {
                let key = &rest[..end_quote];
                let after = rest[end_quote + 1..].trim_start();
                if matches!(
                    self.property_syntax,
                    PropertySyntax::Colon | PropertySyntax::ColonOrEquals
                ) && after.starts_with(':')
                    && !after.starts_with("::")
                {
                    return Some(key);
                }
                if matches!(
                    self.property_syntax,
                    PropertySyntax::Equals | PropertySyntax::ColonOrEquals
                ) && after.starts_with('=')
                    && !after.starts_with("==")
                    && !after.starts_with("=>")
                {
                    return Some(key);
                }
            }
        }

        // 2. Unquoted identifier: `key: value` or `key = value`
        // In Rust, optionally strip `pub `, `pub(crate) `, `pub(super) `
        let id_start = if self.name == "rust" && t.starts_with("pub") {
            let rem = t.strip_prefix("pub")?.trim_start();
            if rem.starts_with('(') {
                let end_paren = rem.find(')')?;
                rem[end_paren + 1..].trim_start()
            } else {
                rem
            }
        } else {
            t
        };

        let id_len = id_start
            .chars()
            .take_while(|&c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            .map(char::len_utf8)
            .sum::<usize>();
        if id_len == 0 {
            return None;
        }
        let key = &id_start[..id_len];
        let after = id_start[id_len..].trim_start();

        if matches!(
            self.property_syntax,
            PropertySyntax::Colon | PropertySyntax::ColonOrEquals
        ) && after.starts_with(':')
            && !after.starts_with("::")
        {
            return Some(key);
        }

        if matches!(
            self.property_syntax,
            PropertySyntax::Equals | PropertySyntax::ColonOrEquals
        ) && after.starts_with('=')
            && !after.starts_with("==")
            && !after.starts_with("=>")
        {
            return Some(key);
        }

        None
    }

    /// Whether the keyword slice contains `word` (binary search on sorted
    /// slice).
    pub fn is_keyword(&self, word: &str) -> bool {
        self.keywords.binary_search(&word).is_ok()
    }

    /// Whether the literal slice contains `word` (binary search on sorted
    /// slice).
    #[inline]
    pub fn is_literal(&self, word: &str) -> bool {
        self.literals.binary_search(&word).is_ok()
    }

    /// Whether the builtin types slice contains `word` (binary search on
    /// sorted slice).
    #[inline]
    pub fn is_builtin_type(&self, word: &str) -> bool {
        self.builtin_types.binary_search(&word).is_ok()
    }

    /// Whether the operator keyword slice contains `word` (binary search on
    /// sorted slice).
    #[inline]
    pub fn is_operator_keyword(&self, word: &str) -> bool {
        self.operator_keywords.binary_search(&word).is_ok()
    }

    /// Whether `word` represents a non-salient syntax token in this language
    /// (keyword, literal, built-in type, or operator keyword).
    #[inline]
    pub fn is_non_salient(&self, word: &str) -> bool {
        self.is_keyword(word)
            || self.is_literal(word)
            || self.is_builtin_type(word)
            || self.is_operator_keyword(word)
    }

    /// Whether the control-flow keyword slice contains `word` (binary search on
    /// sorted slice).
    #[inline]
    pub fn is_control_flow(&self, word: &str) -> bool {
        self.control_flow_keywords.binary_search(&word).is_ok()
    }

    /// Whether the declaration keyword slice contains `word` (binary search on
    /// sorted slice).
    #[inline]
    pub fn is_declaration_keyword(&self, word: &str) -> bool {
        self.declaration_keywords.binary_search(&word).is_ok()
    }

    /// Whether `word` represents an import statement head in this language
    /// (e.g. `use`, `import`, `from`).
    #[inline]
    pub fn is_import(&self, word: &str) -> bool {
        self.import_prefixes
            .iter()
            .any(|prefix| prefix.trim() == word)
    }

    /// Whether `word` can open a statement or declaration clause in this
    /// language (declaration keyword, control-flow keyword, import head, or
    /// modifier prefix from `decl_prefixes`).
    #[inline]
    pub fn is_statement_head(&self, word: &str) -> bool {
        self.is_declaration_keyword(word)
            || self.is_control_flow(word)
            || self.is_import(word)
            || self
                .decl_prefixes
                .iter()
                .any(|pf| pf.split_whitespace().next() == Some(word))
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

    /// Whether a line consists solely of an opening delimiter for blocks or
    /// expressions (`{`, `(`, `[`, or Unicode paired openers).
    pub fn is_expression_open(&self, line: &str) -> bool {
        let t = line.trim();
        t == "{"
            || t == "("
            || t == "["
            || (t.graphemes(true).count() == 1
                && t.chars().next().is_some_and(is_opening_delimiter_char))
    }

    /// Whether a token or text fragment acts as an expression or block
    /// opening delimiter (e.g. `(`, `{`, `[`, `（`, `【`, `«`).
    pub fn is_opening_delimiter(&self, text: &str) -> bool {
        let t = text.trim();
        self.is_expression_open(t)
            || (t.graphemes(true).count() == 1
                && t.chars().next().is_some_and(is_opening_delimiter_char))
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

    /// Whether a token or text fragment acts as an expression or statement
    /// closing delimiter (e.g. `);`, `}`, `;`, `）`, `】`, `»`, `；`).
    pub fn is_closing_delimiter(&self, text: &str) -> bool {
        let t = text.trim();
        self.classify_exact_close(t) != CloseDelimiter::None
            || self.classify_close_tail(t) != CloseDelimiter::None
            || (t.graphemes(true).count() == 1
                && t.chars().next().is_some_and(|c| {
                    (self.is_trailing_delimiter(c) && c != ':') || is_closing_delimiter_char(c)
                }))
    }

    /// Whether text represents a statement terminator or closing statement
    /// delimiter (e.g. `;`, `);`, `};`, `];`, `)`, `}`, `]`, `；`).
    pub fn is_statement_terminator(&self, text: &str) -> bool {
        let t = text.trim();
        if t.is_empty() || t == "," || t == ":" || t == "，" || t == "：" {
            return false;
        }
        t == ";"
            || t == "；"
            || t == "؛"
            || t == ";"
            || (t.ends_with(';') && (self.is_expression_close(t) || self.is_block_close(t)))
            || (t.ends_with('；') && (self.is_expression_close(t) || self.is_block_close(t)))
            || self.is_block_close(t)
            || self.is_expression_close(t)
            || self.classify_exact_close(t) != CloseDelimiter::None
            || (t.graphemes(true).count() == 1
                && t.chars().next().is_some_and(is_statement_terminator_char))
    }

    /// Whether text represents a syntax continuation phrase
    /// (e.g. `=>`, `->`, `=> {`, `-> {`, `) {`, `] {`).
    pub fn is_continuation_phrase(&self, text: &str) -> bool {
        let trim = text.trim();
        if trim.is_empty() {
            return false;
        }
        let starts_arrow = trim.starts_with("=>") || trim.starts_with("->");
        let starts_close = trim.starts_with(')')
            || trim.starts_with(']')
            || trim.chars().next().is_some_and(is_closing_delimiter_char);
        let ends_open = trim.ends_with('{')
            || trim.ends_with('(')
            || trim.ends_with('[')
            || trim
                .chars()
                .next_back()
                .is_some_and(is_opening_delimiter_char);

        trim == "=>" || trim == "->" || (starts_arrow && ends_open) || (starts_close && ends_open)
    }

    /// Whether a line is an attribute, decorator, or annotation line.
    pub fn is_attribute_line(&self, line: &str) -> bool {
        let t = line.trim();
        self.attribute_prefixes.iter().any(|&pf| t.starts_with(pf))
    }

    /// Whether a line is a declaration start or item boundary that breaks a
    /// multi-line continuation.
    pub fn is_decl_break(&self, line: &str) -> bool {
        let t = line.trim();
        if self.is_comment_line(t) || self.is_attribute_line(t) {
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
            || t == "return"
        {
            return true;
        }
        self.decl_prefixes.iter().any(|&pf| t.starts_with(pf))
    }

    /// Whether a line is a continuation expression (e.g. operator lead or
    /// compound block start).
    pub fn is_continuation_line(&self, line: &str) -> bool {
        let t = line.trim();
        self.continuation_prefixes
            .iter()
            .any(|&pf| t.starts_with(pf))
            || t == "{"
            || t == "}"
    }

    /// Whether a line begins with one of this language's leading continuation
    /// operators (such as `&&`, `||`, `.`, `and `, `|`), excluding standalone
    /// braces, comments, and range prefixes (`..`).
    pub fn has_leading_continuation_operator(&self, line: &str) -> bool {
        if self.continuation_prefixes.is_empty() || self.is_comment_line(line) {
            return false;
        }
        let t = line.trim();
        if t.is_empty() {
            return false;
        }
        self.continuation_prefixes.iter().any(|&pf| {
            if pf == "." {
                t.starts_with('.') && !t.starts_with("..")
            } else {
                t.starts_with(pf)
            }
        })
    }

    /// Whether the final code token on `line` (ignoring trailing comments and
    /// string literals) is one of this language's `trailing_continuation_ops`.
    pub fn has_open_trailing_operator(&self, line: &str) -> bool {
        if self.trailing_continuation_ops.is_empty() || self.is_comment_line(line) {
            return false;
        }
        let code = self.strip_trailing_line_comment(line).trim_end();
        let Some(tail_tok) = trailing_operator_or_word(code) else {
            return false;
        };
        self.trailing_continuation_ops.contains(&tail_tok)
    }

    /// Strips a trailing single-line comment (`//` or `#`, per this language's
    /// comment flags) while ignoring comment markers inside string or char
    /// literals.
    fn strip_trailing_line_comment<'a>(&self, line: &'a str) -> &'a str {
        if !self.line_comment && !self.hash_comment {
            return line;
        }
        let bytes = line.as_bytes();
        let len = bytes.len();
        let mut in_str = false;
        let mut str_char = b'"';
        let mut escaped = false;
        let mut i = 0;

        while i < len {
            let b = bytes[i];
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if in_str {
                if b == b'\\' {
                    escaped = true;
                } else if b == str_char {
                    in_str = false;
                }
                i += 1;
                continue;
            }
            if b == b'"' || b == b'`' {
                in_str = true;
                str_char = b;
                i += 1;
                continue;
            }
            if b == b'\'' {
                match self.single_quote {
                    SingleQuoteKind::String => {
                        in_str = true;
                        str_char = b'\'';
                    }
                    SingleQuoteKind::Char(_) => {
                        let is_char_lit = (i + 2 < len && bytes[i + 2] == b'\'')
                            || (i + 3 < len && bytes[i + 1] == b'\\' && bytes[i + 3] == b'\'');
                        if is_char_lit {
                            in_str = true;
                            str_char = b'\'';
                        }
                    }
                    SingleQuoteKind::None => {}
                }
                i += 1;
                continue;
            }
            if self.line_comment && b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
                return &line[..i];
            }
            if self.hash_comment && b == b'#' {
                return &line[..i];
            }
            i += 1;
        }
        line
    }

    /// Whether a line is an import/use statement that can reflow multi-line
    /// braces or parens.
    pub fn is_import_statement(&self, line: &str) -> bool {
        let t = line.trim();
        if self.import_prefixes.is_empty() {
            return false;
        }
        self.import_prefixes.iter().any(|&pf| t.starts_with(pf))
    }

    /// Returns the leading comment marker (`///`, `//!`, `//`, `/*`, `*`, `#`)
    /// for `line` when recognized by this language profile, or `None`
    /// otherwise.
    ///
    /// Canonical single source of truth for comment-line detection shared by
    /// [`Self::is_comment_line`] and prose/comment reflow hygiene.
    pub fn leading_comment_prefix(&self, line: &str) -> Option<&'static str> {
        let t = line.trim_start();
        if self.line_comment {
            for marker in ["///", "//!", "//"] {
                if t.starts_with(marker) {
                    return Some(marker);
                }
            }
        }
        if self.block_comment {
            for marker in ["/*", "*"] {
                if t.starts_with(marker) {
                    return Some(marker);
                }
            }
        }
        if self.hash_comment && t.starts_with('#') {
            return Some("#");
        }
        None
    }

    /// Whether a line starts a comment in this language.
    pub fn is_comment_line(&self, line: &str) -> bool {
        self.leading_comment_prefix(line).is_some()
    }

    /// Whether a line is a doc comment line (`///` or `//!` in Rust).
    pub fn is_doc_comment(&self, line: &str) -> bool {
        let t = line.trim();
        self.line_comment && (t.starts_with("///") || t.starts_with("//!"))
    }

    /// Classifies the trailing closing delimiter of a line according to this
    /// language's syntax.
    pub fn classify_close_tail(&self, line: &str) -> CloseDelimiter {
        let t = line.trim();
        let exact = self.classify_exact_close(line);
        if exact == CloseDelimiter::Keyword {
            return CloseDelimiter::Keyword;
        }
        for &(pattern, delim) in self.tail_delimiters {
            if t.ends_with(pattern) {
                return delim;
            }
        }
        CloseDelimiter::None
    }

    /// Classifies whether a line consists solely of a closing delimiter in this
    /// language.
    pub fn classify_exact_close(&self, line: &str) -> CloseDelimiter {
        let t = line.trim();
        for &(pattern, delim) in self.exact_delimiters {
            if t == pattern {
                return delim;
            }
        }
        CloseDelimiter::None
    }

    /// Trims trailing syntax delimiters and whitespace from a statement or
    /// expression line (e.g. `;`, `,`, `{`, `}`, `)` in brace languages, or
    /// `:` in indent-colon languages).
    pub fn trim_trailing_delimiters<'a>(&self, line: &'a str) -> &'a str {
        let mut s = line.trim_end();
        while let Some(g) = s.graphemes(true).next_back() {
            if self.is_trailing_delimiter_str(g) {
                if g == ":" && s.ends_with("::") {
                    break;
                }
                s = &s[..s.len() - g.len()];
                s = s.trim_end();
            } else {
                break;
            }
        }
        s
    }

    /// Whether a character acts as a trailing syntax delimiter in this
    /// language.
    pub fn is_trailing_delimiter(&self, c: char) -> bool {
        self.trailing_delimiters.contains(&c)
    }

    /// Whether a string or grapheme cluster acts as a trailing syntax delimiter
    /// in this language or universally.
    pub fn is_trailing_delimiter_str(&self, text: &str) -> bool {
        let t = text.trim();
        if t.is_empty() {
            return false;
        }
        if t.chars().count() == 1 {
            let c = t.chars().next().unwrap();
            return self.is_trailing_delimiter(c) || is_closing_delimiter_char(c);
        }
        self.classify_exact_close(t) != CloseDelimiter::None
            || self.classify_close_tail(t) != CloseDelimiter::None
    }

    /// Whether `c` is a syntax delimiter in this language (paired bracket,
    /// statement terminator, or trailing syntax delimiter).
    #[inline]
    pub fn is_delimiter_char(&self, c: char) -> bool {
        is_opening_delimiter_char(c)
            || is_closing_delimiter_char(c)
            || self.is_trailing_delimiter(c)
    }
}

/// Extracts the maximal trailing identifier word or non-bracket operator run
/// from a right-trimmed line of code. Returns `None` when the line is empty or
/// ends in a bracket or quote delimiter.
fn trailing_operator_or_word(code: &str) -> Option<&str> {
    let last = code.chars().next_back()?;
    if last.is_alphanumeric() || last == '_' {
        let start = code
            .char_indices()
            .rev()
            .take_while(|&(_, c)| c.is_alphanumeric() || c == '_')
            .last()
            .map(|(idx, _)| idx)?;
        return Some(&code[start..]);
    }
    if is_opening_delimiter_char(last)
        || is_closing_delimiter_char(last)
        || matches!(last, '"' | '\'' | '`')
    {
        return None;
    }
    let start = code
        .char_indices()
        .rev()
        .take_while(|&(_, c)| {
            !c.is_whitespace()
                && !c.is_alphanumeric()
                && c != '_'
                && !is_opening_delimiter_char(c)
                && !is_closing_delimiter_char(c)
                && !matches!(c, '"' | '\'' | '`')
        })
        .last()
        .map(|(idx, _)| idx)?;
    Some(&code[start..])
}
