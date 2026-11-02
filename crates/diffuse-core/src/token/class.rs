//! Semantic and syntactic token classification and role categorization.

use diffuse_syntax::profile::LangProfile;

use crate::lcs::{token_is_alnum, token_is_ws};

/// Semantic role and classification of a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenClass {
    Whitespace,
    Delimiter,
    Operator,
    Keyword,
    Identifier,
    Literal,
    ProseWord,
    FunctionWord,
}

impl TokenClass {
    /// Information-theoretic class weight used in Shannon self-information
    /// calculation.
    pub fn class_weight(&self) -> f64 {
        match self {
            TokenClass::Identifier => 2.0,
            TokenClass::Keyword => 1.6,
            TokenClass::Literal => 1.5,
            TokenClass::ProseWord => 1.5,
            TokenClass::Operator => 0.8,
            TokenClass::FunctionWord => 0.5,
            TokenClass::Delimiter => 0.2,
            TokenClass::Whitespace => 0.0,
        }
    }
}

#[inline]
pub(crate) fn is_delim(t: &str) -> bool {
    if t.is_ascii() {
        t.len() == 1
            && matches!(
                t.as_bytes()[0],
                b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';'
            )
    } else {
        use unicode_segmentation::UnicodeSegmentation;
        t.graphemes(true).count() == 1
            && t.chars().next().is_some_and(|c| {
                diffuse_syntax::profile::is_opening_delimiter_char(c)
                    || diffuse_syntax::profile::is_closing_delimiter_char(c)
            })
    }
}

#[inline]
fn is_numeric_literal(t: &str) -> bool {
    if t.is_empty() {
        return false;
    }
    let b = t.as_bytes();
    if b[0].is_ascii_digit() {
        return true;
    }
    if (t.starts_with("0x") || t.starts_with("0b") || t.starts_with("0o")) && t.len() > 2 {
        return true;
    }
    false
}

#[inline]
fn is_string_literal(t: &str) -> bool {
    (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
        || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
        || (t.starts_with('`') && t.ends_with('`') && t.len() >= 2)
}

#[inline]
fn is_builtin_keyword(t: &str) -> bool {
    matches!(
        t,
        "fn" | "let"
            | "pub"
            | "mut"
            | "if"
            | "else"
            | "match"
            | "return"
            | "struct"
            | "enum"
            | "impl"
            | "trait"
            | "type"
            | "async"
            | "await"
            | "const"
            | "static"
            | "mod"
            | "use"
            | "super"
            | "self"
            | "Self"
            | "crate"
            | "def"
            | "class"
            | "import"
            | "from"
            | "function"
            | "var"
            | "val"
            | "package"
            | "func"
            | "interface"
            | "select"
            | "case"
            | "default"
            | "switch"
            | "for"
            | "while"
            | "loop"
            | "break"
            | "continue"
            | "yield"
    )
}

#[inline]
pub(crate) fn is_function_word(t: &str) -> bool {
    matches!(
        t,
        "the"
            | "The"
            | "THE"
            | "a"
            | "A"
            | "an"
            | "An"
            | "AN"
            | "and"
            | "And"
            | "AND"
            | "or"
            | "Or"
            | "OR"
            | "in"
            | "In"
            | "IN"
            | "on"
            | "On"
            | "ON"
            | "at"
            | "At"
            | "AT"
            | "to"
            | "To"
            | "TO"
            | "for"
            | "For"
            | "FOR"
            | "of"
            | "Of"
            | "OF"
            | "with"
            | "With"
            | "WITH"
            | "by"
            | "By"
            | "BY"
            | "is"
            | "Is"
            | "IS"
            | "it"
            | "It"
            | "IT"
            | "its"
            | "Its"
            | "ITS"
            | "as"
            | "As"
            | "AS"
    )
}

#[inline]
fn is_op_char(c: char) -> bool {
    matches!(
        c,
        '=' | '+'
            | '-'
            | '*'
            | '/'
            | '%'
            | '&'
            | '|'
            | '^'
            | '!'
            | '<'
            | '>'
            | '?'
            | ':'
            | '~'
            | '@'
            | '#'
            | '$'
    )
}

#[inline]
fn is_identifier_like(t: &str) -> bool {
    if t.is_empty() {
        return false;
    }
    if t.is_ascii() {
        let b = t.as_bytes();
        let starts_valid = b[0].is_ascii_alphabetic() || b[0] == b'_';
        if !starts_valid {
            return false;
        }
        let all_alphanumeric_or_underscore =
            t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !all_alphanumeric_or_underscore {
            return false;
        }
        t.contains('_') || t.chars().any(|c| c.is_ascii_uppercase())
    } else {
        let mut chars = t.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        let starts_valid = first.is_alphabetic() || first == '_';
        if !starts_valid {
            return false;
        }
        let all_alphanumeric_or_underscore = t.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !all_alphanumeric_or_underscore {
            return false;
        }
        t.contains('_') || t.chars().any(char::is_uppercase)
    }
}

/// Classifies a token into its syntactic/semantic role.
pub fn classify_token(token: &str, profile: Option<&LangProfile>) -> TokenClass {
    if token.is_empty() || token_is_ws(token) {
        return TokenClass::Whitespace;
    }
    if is_delim(token) {
        return TokenClass::Delimiter;
    }
    if profile.is_some_and(|p| p.is_operator_anchor(token)) {
        return TokenClass::Operator;
    }
    if is_string_literal(token) || is_numeric_literal(token) {
        return TokenClass::Literal;
    }
    if profile.is_some_and(|p| p.is_keyword(token)) || is_builtin_keyword(token) {
        return TokenClass::Keyword;
    }
    if is_identifier_like(token) {
        return TokenClass::Identifier;
    }
    if token.chars().all(is_op_char) {
        return TokenClass::Operator;
    }
    if is_function_word(token) {
        return TokenClass::FunctionWord;
    }
    TokenClass::ProseWord
}

/// Returns true if a token can serve as an anchor point or phrase boundary.
#[inline]
pub fn is_anchor_content_token(t: &str, profile: Option<&LangProfile>) -> bool {
    if token_is_ws(t) || is_delim(t) || is_function_word(t) {
        return false;
    }
    let is_op = profile.is_some_and(|p| p.is_operator_anchor(t));
    let is_alnum = token_is_alnum(t);
    is_alnum || is_op
}
