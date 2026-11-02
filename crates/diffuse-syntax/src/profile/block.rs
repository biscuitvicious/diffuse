//! Block delimiters, closing styles, and property syntax.

use super::delimiters::{is_closing_delimiter_char, is_opening_delimiter_char};

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
}

/// Default continuation phrase check when no language profile is available.
pub fn default_is_continuation_phrase(text: &str) -> bool {
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
