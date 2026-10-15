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

/// Style of key-value property, struct field, or dict entry definitions.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PropertySyntax {
    #[default]
    None,
    /// Colon-separated: `key:` or `"key":` (JSON, YAML, Rust structs, TS/JS
    /// objects, Python dicts, Go)
    Colon,
    /// Equals-separated: `key =` (TOML, INI)
    Equals,
    /// Both colon- and equals-separated (`key:`, `key =`, `"key":`)
    ColonOrEquals,
}

/// Style of Unicode escapes supported inside character literals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UnicodeEscapeStyle {
    #[default]
    None,
    /// Braced Unicode scalar escape, e.g. `\u{1F980}` with 1 to 6 hex digits
    /// (Rust).
    Braced,
    /// Fixed 4-hex-digit escape, e.g. `\u0041` (Java, C++, Go).
    Fixed4,
    /// Fixed 4 or 8 hex digits, e.g. `\u0041` or `\U0001F980` (C++, Go).
    Fixed4Or8,
}

/// Character literal specification describing the constructs permitted inside
/// `'...'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharLiteralProfile {
    /// Whether arbitrary Unicode characters/scalars are permitted directly
    /// (e.g. `'🦀'`).
    pub unicode: bool,
    /// Maximum hex digits in `\xNN` escape (0 if unsupported, e.g. 2 for 8-bit
    /// hex).
    pub max_hex_digits: u8,
    /// If true, `\xNN` hex escape values must be <= 0x7F (Rust char literals
    /// restrict `\x` to 7-bit ASCII).
    pub hex_ascii_only: bool,
    /// Unicode escape format supported (`\u{...}`, `\uXXXX`, etc.).
    pub unicode_escape: UnicodeEscapeStyle,
    /// Whether octal escapes (`\0` or `\NNN`) are permitted.
    pub octal_escape: bool,
}

impl CharLiteralProfile {
    /// Rust char literal specification:
    /// - Unicode scalar values allowed (e.g. `'a'`, `'🦀'`)
    /// - Standard escapes: `\'`, `\"`, `\n`, `\r`, `\t`, `\\`, `\0`
    /// - Hex escape `\xNN` restricted to 7-bit ASCII (0x00..=0x7F)
    /// - Braced Unicode escape `\u{1..6 hex digits}` representing valid Unicode
    ///   scalar
    pub const RUST: Self = Self {
        unicode: true,
        max_hex_digits: 2,
        hex_ascii_only: true,
        unicode_escape: UnicodeEscapeStyle::Braced,
        octal_escape: false,
    };

    /// C / C++ char literal specification:
    /// - Supports hex escape `\xNN`
    /// - Fixed Unicode escape `\uXXXX` or `\UXXXXXXXX`
    /// - Octal escapes `\0`..`\777`
    pub const C_FAMILY: Self = Self {
        unicode: true,
        max_hex_digits: 2,
        hex_ascii_only: false,
        unicode_escape: UnicodeEscapeStyle::Fixed4Or8,
        octal_escape: true,
    };

    /// Go rune literal specification:
    /// - Unicode characters allowed
    /// - Hex escape `\xNN`
    /// - Fixed Unicode escape `\uXXXX` or `\UXXXXXXXX`
    /// - Octal escapes `\000`..`\377`
    pub const GO: Self = Self {
        unicode: true,
        max_hex_digits: 2,
        hex_ascii_only: false,
        unicode_escape: UnicodeEscapeStyle::Fixed4Or8,
        octal_escape: true,
    };

    /// Validate the inner character sequence (between the opening and closing
    /// single quotes). Returns true if the sequence represents a valid
    /// single character literal under this profile.
    pub fn validate_inner(&self, inner: &str) -> bool {
        if inner.is_empty() {
            return false;
        }
        if !inner.starts_with('\\') {
            let mut chars = inner.chars();
            let Some(c) = chars.next() else {
                return false;
            };
            if chars.next().is_some() {
                return false;
            }
            if c == '\'' || c == '\n' || c == '\r' {
                return false;
            }
            if !self.unicode && !c.is_ascii() {
                return false;
            }
            return true;
        }

        // Starts with '\'
        let rest = &inner[1..];
        if rest.is_empty() {
            return false;
        }

        let first = rest.chars().next().unwrap();
        match first {
            // Standard single-character escapes
            '\'' | '"' | '\\' | 'n' | 'r' | 't' => rest.len() == 1,
            'a' | 'b' | 'f' | 'v' => {
                // Supported in C, Go
                if self.octal_escape || self.unicode_escape == UnicodeEscapeStyle::Fixed4Or8 {
                    rest.len() == 1
                } else {
                    false
                }
            }
            '0' if rest.len() == 1 => true,
            '0'..='7' if self.octal_escape => {
                // Octal escape: 1 to 3 octal digits
                if rest.len() > 3 {
                    return false;
                }
                rest.chars().all(|c| ('0'..='7').contains(&c))
            }
            'x' if self.max_hex_digits > 0 => {
                let hex = &rest[1..];
                if hex.len() != self.max_hex_digits as usize {
                    return false;
                }
                if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return false;
                }
                if self.hex_ascii_only {
                    if let Ok(val) = u32::from_str_radix(hex, 16) {
                        if val > 0x7F {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            }
            'u' if self.unicode_escape == UnicodeEscapeStyle::Braced => {
                let u_rest = &rest[1..];
                if !u_rest.starts_with('{') || !u_rest.ends_with('}') {
                    return false;
                }
                let hex = &u_rest[1..u_rest.len() - 1];
                if hex.is_empty() || hex.len() > 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return false;
                }
                let Ok(val) = u32::from_str_radix(hex, 16) else {
                    return false;
                };
                char::from_u32(val).is_some()
            }
            'u' if matches!(
                self.unicode_escape,
                UnicodeEscapeStyle::Fixed4 | UnicodeEscapeStyle::Fixed4Or8
            ) =>
            {
                let hex = &rest[1..];
                if hex.len() != 4 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return false;
                }
                let Ok(val) = u32::from_str_radix(hex, 16) else {
                    return false;
                };
                char::from_u32(val).is_some()
            }
            'U' if self.unicode_escape == UnicodeEscapeStyle::Fixed4Or8 => {
                let hex = &rest[1..];
                if hex.len() != 8 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    return false;
                }
                let Ok(val) = u32::from_str_radix(hex, 16) else {
                    return false;
                };
                char::from_u32(val).is_some()
            }
            _ => false,
        }
    }
}

/// Parsing mode for single quotes (`'...'`) in a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SingleQuoteKind {
    /// Multi-character string literal delimited by single quotes: `'...'`
    /// (Python, JS/TS, Shell, TOML, YAML).
    #[default]
    String,
    /// Single-character / rune literal only (Rust, C-family, Go).
    /// Single quotes only parse as valid tokens if their inner content conforms
    /// to the specified character literal rules.
    Char(CharLiteralProfile),
    /// Single quotes are not used for strings or character literals (e.g.
    /// JSON).
    None,
}

pub const BRACE_EXACT_DELIMITERS: &[(&str, CloseDelimiter)] = &[
    ("};", CloseDelimiter::BraceSemi),
    ("},", CloseDelimiter::BraceSemi),
    ("}", CloseDelimiter::Brace),
    (");", CloseDelimiter::ParenSemi),
    ("),", CloseDelimiter::ParenSemi),
    (")", CloseDelimiter::Paren),
    ("];", CloseDelimiter::BracketSemi),
    ("],", CloseDelimiter::BracketSemi),
    ("]", CloseDelimiter::Bracket),
];

pub const BRACE_TAIL_DELIMITERS: &[(&str, CloseDelimiter)] = &[
    ("};", CloseDelimiter::BraceSemi),
    ("},", CloseDelimiter::BraceSemi),
    ("}", CloseDelimiter::Brace),
    (");", CloseDelimiter::ParenSemi),
    ("),", CloseDelimiter::ParenSemi),
    (")", CloseDelimiter::Paren),
    ("];", CloseDelimiter::BracketSemi),
    ("],", CloseDelimiter::BracketSemi),
    ("]", CloseDelimiter::Bracket),
];

pub const BRACE_TRAILING_DELIMITERS: &[char] = &['{', '}', ';', ':', ',', ')', ']'];

pub const INDENT_COLON_EXACT_DELIMITERS: &[(&str, CloseDelimiter)] = &[
    (")", CloseDelimiter::Paren),
    ("]", CloseDelimiter::Bracket),
    ("}", CloseDelimiter::Brace),
];

pub const INDENT_COLON_TAIL_DELIMITERS: &[(&str, CloseDelimiter)] = &[
    (")", CloseDelimiter::Paren),
    ("]", CloseDelimiter::Bracket),
    ("}", CloseDelimiter::Brace),
];

pub const INDENT_COLON_TRAILING_DELIMITERS: &[char] = &[':', ',', ')', ']'];

pub const KEYWORDS_EXACT_DELIMITERS: &[(&str, CloseDelimiter)] = &[
    ("fi", CloseDelimiter::Keyword),
    ("done", CloseDelimiter::Keyword),
    ("esac", CloseDelimiter::Keyword),
    ("}", CloseDelimiter::Brace),
    (")", CloseDelimiter::Paren),
    ("]", CloseDelimiter::Bracket),
];

pub const KEYWORDS_TAIL_DELIMITERS: &[(&str, CloseDelimiter)] = &[
    ("}", CloseDelimiter::Brace),
    (")", CloseDelimiter::Paren),
    ("]", CloseDelimiter::Bracket),
];

pub const KEYWORDS_TRAILING_DELIMITERS: &[char] = &[',', ')', ']', ';', '{', '}'];

impl BlockDelimStyle {
    /// Default exact closing delimiter table for this block delimiter style.
    pub const fn default_exact_delimiters(&self) -> &'static [(&'static str, CloseDelimiter)] {
        match self {
            Self::Braces => BRACE_EXACT_DELIMITERS,
            Self::IndentColon => INDENT_COLON_EXACT_DELIMITERS,
            Self::Keywords => KEYWORDS_EXACT_DELIMITERS,
        }
    }

    /// Default tail closing delimiter table for this block delimiter style.
    pub const fn default_tail_delimiters(&self) -> &'static [(&'static str, CloseDelimiter)] {
        match self {
            Self::Braces => BRACE_TAIL_DELIMITERS,
            Self::IndentColon => INDENT_COLON_TAIL_DELIMITERS,
            Self::Keywords => KEYWORDS_TAIL_DELIMITERS,
        }
    }

    /// Default trailing delimiter character table for this block delimiter
    /// style.
    pub const fn default_trailing_delimiters(&self) -> &'static [char] {
        match self {
            Self::Braces => BRACE_TRAILING_DELIMITERS,
            Self::IndentColon => INDENT_COLON_TRAILING_DELIMITERS,
            Self::Keywords => KEYWORDS_TRAILING_DELIMITERS,
        }
    }
}

impl CloseDelimiter {
    /// Default trailing close classification for unknown languages (defaults to
    /// standard brace syntax).
    pub fn default_classify_tail(line: &str) -> Self {
        let t = line.trim();
        for &(pattern, delim) in BRACE_TAIL_DELIMITERS {
            if t.ends_with(pattern) {
                return delim;
            }
        }
        CloseDelimiter::None
    }

    /// Default exact close classification for unknown languages.
    pub fn default_classify_exact(line: &str) -> Self {
        let t = line.trim();
        for &(pattern, delim) in BRACE_EXACT_DELIMITERS {
            if t == pattern {
                return delim;
            }
        }
        CloseDelimiter::None
    }

    /// Default direct tail matching when no profile is available.
    pub fn default_is_direct_tail_match(
        tail_a: Self,
        tail_b: Self,
        matched_chars: usize,
        min_chars: usize,
    ) -> bool {
        tail_a != CloseDelimiter::None && tail_a == tail_b && matched_chars >= min_chars
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
    /// Whether indentation is syntactically significant in this language
    /// (e.g. Python, YAML). When false, indentation is strictly a soft
    /// cost hint and cannot be used to make semantic assertions.
    pub significant_indent: bool,
    /// Whether the language supports implicit return expressions without a
    /// `return` keyword (e.g. Rust, Ruby).
    pub implicit_return: bool,
    /// Whether multi-line function tails spell the return type as `) -> T`
    /// (Rust, Python, C++ trailing returns). Only these profiles answer
    /// `is_return_tail_position` affirmatively. Shared profiles cover
    /// languages without the construct (C, Java): the shape essentially
    /// cannot occur there, so the flag is dead in practice for them.
    pub tail_return_arrow: bool,
    /// Whether multi-line function tails spell the return type as `) -> T`
    /// (Rust, Python, C++ trailing returns). Only these profiles answer
    /// `is_return_tail_position` affirmatively. Shared profiles cover
    /// languages without the construct (C, Java): the shape essentially
    /// cannot occur there, so the flag is dead in practice for them.
    /// Regex capturing the function name (group 1) from a function declaration
    /// line.
    pub fn_decl: Option<Regex>,
    /// Prefixes that introduce declarations/items in this language.
    /// Used to break multi-line statement expansion in `pass_assemble_mods`.
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
    /// Prefixes that introduce import or use statements (e.g. `use ` for Rust,
    /// `import ` for Python/Go).
    pub import_prefixes: &'static [&'static str],
    /// Whether the language uses section headers like `[section]` (TOML, INI).
    pub section_headers: bool,
    /// Single-quote parsing mode (strings, char literals, or none).
    pub single_quote: SingleQuoteKind,
}

impl LangProfile {
    /// Whether a trimmed line is a function-tail return position (`) -> T`,
    /// as in Rust, Python, and C++ trailing returns. Only profiles that opt
    /// in via `tail_return_arrow` answer true; every other language — and
    /// files without a known profile — is silent, so the engine never
    /// assumes function syntax it has not been told about. The shape is
    /// deliberately narrow: no other construct in these languages starts a
    /// stripped line with `)` and continues with `->`.
    pub fn is_return_tail_position(&self, trimmed: &str) -> bool {
        self.tail_return_arrow && trimmed.starts_with(')') && trimmed.contains("->")
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
    /// expressions (`{`, `(`, `[`).
    pub fn is_expression_open(&self, line: &str) -> bool {
        let t = line.trim();
        t == "{" || t == "(" || t == "["
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

    /// Whether a line is an attribute, decorator, or annotation line.
    pub fn is_attribute_line(&self, line: &str) -> bool {
        let t = line.trim();
        self.attribute_prefixes.iter().any(|&pf| t.starts_with(pf))
    }

    /// Whether a line is a return statement or (for languages with implicit
    /// returns like Rust) a terminal return expression.
    pub fn is_terminal_return_expr(&self, line: &str) -> bool {
        let t = line.trim();
        if t.is_empty()
            || self.is_comment_line(t)
            || self.is_block_open(t)
            || self.is_block_close(t)
            || t.chars().all(|c| matches!(c, '}' | ')' | ']' | ';' | ','))
        {
            return false;
        }
        if t.starts_with("return ") || t == "return;" || t == "return" || t.starts_with("return\t")
        {
            return true;
        }
        if self.implicit_return && !t.ends_with(';') {
            return true;
        }
        false
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

    /// Whether a line is an import/use statement that can reflow multi-line
    /// braces or parens.
    pub fn is_import_statement(&self, line: &str) -> bool {
        let t = line.trim();
        if self.import_prefixes.is_empty() {
            return false;
        }
        self.import_prefixes.iter().any(|&pf| t.starts_with(pf))
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

    /// Checks whether two closing tails qualify as a direct tail match in this
    /// language across indentation shifts.
    pub fn is_direct_tail_match(
        &self,
        tail_a: CloseDelimiter,
        tail_b: CloseDelimiter,
        matched_chars: usize,
        min_chars: usize,
    ) -> bool {
        if matched_chars < min_chars || tail_a == CloseDelimiter::None || tail_a != tail_b {
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

    /// Trims trailing syntax delimiters and whitespace from a statement or
    /// expression line (e.g. `;`, `,`, `{`, `}`, `)` in brace languages, or
    /// `:` in indent-colon languages).
    pub fn trim_trailing_delimiters<'a>(&self, line: &'a str) -> &'a str {
        line.trim_end_matches(|c: char| c.is_whitespace() || self.is_trailing_delimiter(c))
    }

    /// Whether a character acts as a trailing syntax delimiter in this
    /// language.
    pub fn is_trailing_delimiter(&self, c: char) -> bool {
        self.trailing_delimiters.contains(&c)
    }
}

/// The ANSI SGR attributes used in styled runs.
pub const SGR_BOLD: u32 = 1;
pub const SGR_DIM: u32 = 2;
pub const SGR_ITALIC: u32 = 3;
pub const SGR_UNDERLINE: u32 = 4;
pub const SGR_STRIKE: u32 = 9;
pub const SGR_BOLD_UNDERLINE: u32 = 14;
