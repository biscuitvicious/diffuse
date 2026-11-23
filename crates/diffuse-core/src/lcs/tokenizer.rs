//! Line tokenization, character slicing, and token character classifications.

use crate::token::WhitespaceKind;
use diffuse_syntax::profile::LangProfile;

/// Slice `s` by char offsets (the port's span convention). The JS
/// `String.slice(start, end)` clamps out-of-range indices and returns an
/// empty slice for `start >= end` — the port must be total too: a
/// whole-line gate or an empty del/add line yields empty spans `(0,0)`,
/// and `wholesalePair` slices them (an empty del line is a valid row).
pub fn char_slice(s: &str, start: usize, end: usize) -> &str {
    let len = char_len(s);
    let start = start.min(len);
    let end = end.min(len).max(start);
    if start >= end {
        return "";
    }
    if s.is_ascii() {
        return &s[start..end];
    }
    let start_byte = s.char_indices().nth(start).map_or(s.len(), |(i, _)| i);
    let end_byte = match s.char_indices().nth(end - 1) {
        Some((i, c)) => i + c.len_utf8(),
        None => s.len(),
    };
    &s[start_byte..end_byte]
}

/// Number of chars in `s` (the port's span unit).
#[inline]
pub fn char_len(s: &str) -> usize {
    if s.is_ascii() {
        s.len()
    } else {
        s.chars().count()
    }
}

#[inline]
fn is_ws_char(c: char) -> bool {
    c.is_whitespace() || c == '\u{200b}' || c == '\u{feff}'
}

#[inline]
fn is_op_char(c: char) -> bool {
    matches!(
        c,
        '<' | '>'
            | '!'
            | '='
            | '+'
            | '-'
            | '*'
            | '/'
            | '\\'
            | '|'
            | '&'
            | '^'
            | '%'
            | '~'
            | '?'
            | ':'
            | '.'
            | '_'
    )
}

#[inline]
fn is_delim_char(c: char) -> bool {
    matches!(
        c,
        '(' | ')' | '[' | ']' | '{' | '}' | ';' | ',' | '@' | '#' | '`' | '"'
    ) || diffuse_syntax::profile::is_opening_delimiter_char(c)
        || diffuse_syntax::profile::is_closing_delimiter_char(c)
}

const KIND_WORD: u8 = 0;
const KIND_WS: u8 = 1;
const KIND_DELIM: u8 = 2;
const KIND_OP: u8 = 3;
const KIND_DIGIT: u8 = 4;

const ASCII_KIND: [u8; 128] = {
    let mut table = [KIND_WORD; 128];
    // Whitespace
    table[b' ' as usize] = KIND_WS;
    table[b'\t' as usize] = KIND_WS;
    table[b'\n' as usize] = KIND_WS;
    table[b'\r' as usize] = KIND_WS;
    table[0x0C] = KIND_WS; // form feed \f
    table[0x0B] = KIND_WS; // vertical tab \v

    // Delimiters
    table[b'(' as usize] = KIND_DELIM;
    table[b')' as usize] = KIND_DELIM;
    table[b'[' as usize] = KIND_DELIM;
    table[b']' as usize] = KIND_DELIM;
    table[b'{' as usize] = KIND_DELIM;
    table[b'}' as usize] = KIND_DELIM;
    table[b';' as usize] = KIND_DELIM;
    table[b',' as usize] = KIND_DELIM;
    table[b'@' as usize] = KIND_DELIM;
    table[b'#' as usize] = KIND_DELIM;
    table[b'`' as usize] = KIND_DELIM;
    table[b'"' as usize] = KIND_DELIM;

    // Operators
    table[b'<' as usize] = KIND_OP;
    table[b'>' as usize] = KIND_OP;
    table[b'!' as usize] = KIND_OP;
    table[b'=' as usize] = KIND_OP;
    table[b'+' as usize] = KIND_OP;
    table[b'-' as usize] = KIND_OP;
    table[b'*' as usize] = KIND_OP;
    table[b'/' as usize] = KIND_OP;
    table[b'\\' as usize] = KIND_OP;
    table[b'|' as usize] = KIND_OP;
    table[b'&' as usize] = KIND_OP;
    table[b'^' as usize] = KIND_OP;
    table[b'%' as usize] = KIND_OP;
    table[b'~' as usize] = KIND_OP;
    table[b'?' as usize] = KIND_OP;
    table[b':' as usize] = KIND_OP;
    table[b'.' as usize] = KIND_OP;
    table[b'_' as usize] = KIND_OP;

    // Digits
    let mut d = b'0' as usize;
    while d <= b'9' as usize {
        table[d] = KIND_DIGIT;
        d += 1;
    }

    table
};

/// Tokenize a diff line into word/operator tokens (whitespace + punctuation
/// are separate tokens so `->` → `=>` highlights a single character). Zero-copy
/// slices.
/// Iterate through word/operator tokens with zero allocations.
#[inline]
pub fn for_each_token<'a>(text: &'a str, mut f: impl FnMut(&'a str)) {
    if text.is_empty() {
        return;
    }

    if text.is_ascii() {
        let bytes = text.as_bytes();
        let len = bytes.len();
        let mut i = 0;
        while i < len {
            let start = i;
            let kind = ASCII_KIND[bytes[i] as usize];
            match kind {
                KIND_DELIM => {
                    i += 1;
                    f(&text[start..i]);
                }
                KIND_WS => {
                    i += 1;
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_WS {
                        i += 1;
                    }
                    f(&text[start..i]);
                }
                KIND_OP => {
                    i += 1;
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_OP {
                        i += 1;
                    }
                    f(&text[start..i]);
                }
                KIND_DIGIT => {
                    i += 1;
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_DIGIT {
                        i += 1;
                    }
                    f(&text[start..i]);
                }
                _ => {
                    i += 1;
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_WORD {
                        if bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase() {
                            break;
                        }
                        if i + 1 < len
                            && bytes[i - 1].is_ascii_uppercase()
                            && bytes[i].is_ascii_uppercase()
                            && bytes[i + 1].is_ascii_lowercase()
                        {
                            break;
                        }
                        i += 1;
                    }
                    f(&text[start..i]);
                }
            }
        }
        return;
    }

    let mut chars = text.char_indices().peekable();
    while let Some(&(start_byte, c)) = chars.peek() {
        if is_ws_char(c) {
            chars.next();
            while let Some(&(_, next_c)) = chars.peek() {
                if is_ws_char(next_c) {
                    chars.next();
                } else {
                    break;
                }
            }
            let end_byte = chars.peek().map_or(text.len(), |&(i, _)| i);
            f(&text[start_byte..end_byte]);
        } else if is_delim_char(c) {
            chars.next();
            let end_byte = chars.peek().map_or(text.len(), |&(i, _)| i);
            f(&text[start_byte..end_byte]);
        } else if is_op_char(c) {
            chars.next();
            while let Some(&(_, next_c)) = chars.peek() {
                if is_op_char(next_c) {
                    chars.next();
                } else {
                    break;
                }
            }
            let end_byte = chars.peek().map_or(text.len(), |&(i, _)| i);
            f(&text[start_byte..end_byte]);
        } else if c.is_ascii_digit() {
            chars.next();
            while let Some(&(_, next_c)) = chars.peek() {
                if next_c.is_ascii_digit() {
                    chars.next();
                } else {
                    break;
                }
            }
            let end_byte = chars.peek().map_or(text.len(), |&(i, _)| i);
            f(&text[start_byte..end_byte]);
        } else {
            let mut prev_c = c;
            chars.next();
            while let Some(&(_, next_c)) = chars.peek() {
                if is_ws_char(next_c)
                    || is_delim_char(next_c)
                    || is_op_char(next_c)
                    || next_c.is_ascii_digit()
                    || (prev_c.is_lowercase() && next_c.is_uppercase())
                {
                    break;
                }
                prev_c = next_c;
                chars.next();
            }
            let end_byte = chars.peek().map_or(text.len(), |&(i, _)| i);
            f(&text[start_byte..end_byte]);
        }
    }
}

/// Zero-allocation iterator over word/operator tokens in a line.
#[derive(Clone)]
pub struct LineTokenizer<'a> {
    text: &'a str,
    pos: usize,
    is_ascii: bool,
}

impl<'a> Iterator for LineTokenizer<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.text.len() {
            return None;
        }
        if self.is_ascii {
            let bytes = self.text.as_bytes();
            let len = bytes.len();
            let start = self.pos;
            let kind = ASCII_KIND[bytes[start] as usize];
            let mut i = start + 1;
            match kind {
                KIND_DELIM => {}
                KIND_WS => {
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_WS {
                        i += 1;
                    }
                }
                KIND_OP => {
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_OP {
                        i += 1;
                    }
                }
                KIND_DIGIT => {
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_DIGIT {
                        i += 1;
                    }
                }
                _ => {
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_WORD {
                        if bytes[i - 1].is_ascii_lowercase() && bytes[i].is_ascii_uppercase() {
                            break;
                        }
                        if i + 1 < len
                            && bytes[i - 1].is_ascii_uppercase()
                            && bytes[i].is_ascii_uppercase()
                            && bytes[i + 1].is_ascii_lowercase()
                        {
                            break;
                        }
                        i += 1;
                    }
                }
            }
            self.pos = i;
            Some(&self.text[start..i])
        } else {
            let rem = &self.text[self.pos..];
            let mut chars = rem.chars();
            let c = chars.next()?;
            let c_len = c.len_utf8();
            if is_ws_char(c) {
                let mut advance = c_len;
                for next_c in chars {
                    if is_ws_char(next_c) {
                        advance += next_c.len_utf8();
                    } else {
                        break;
                    }
                }
                let start = self.pos;
                self.pos += advance;
                Some(&self.text[start..self.pos])
            } else if is_delim_char(c) {
                let start = self.pos;
                self.pos += c_len;
                Some(&self.text[start..self.pos])
            } else if is_op_char(c) {
                let mut advance = c_len;
                for next_c in chars {
                    if is_op_char(next_c) {
                        advance += next_c.len_utf8();
                    } else {
                        break;
                    }
                }
                let start = self.pos;
                self.pos += advance;
                Some(&self.text[start..self.pos])
            } else if c.is_ascii_digit() {
                let mut advance = c_len;
                for next_c in chars {
                    if next_c.is_ascii_digit() {
                        advance += next_c.len_utf8();
                    } else {
                        break;
                    }
                }
                let start = self.pos;
                self.pos += advance;
                Some(&self.text[start..self.pos])
            } else {
                let mut advance = c_len;
                let mut prev_c = c;
                while let Some(next_c) = chars.next() {
                    if is_ws_char(next_c)
                        || is_delim_char(next_c)
                        || is_op_char(next_c)
                        || next_c.is_ascii_digit()
                        || (prev_c.is_lowercase() && next_c.is_uppercase())
                        || (prev_c.is_uppercase()
                            && next_c.is_uppercase()
                            && chars
                                .clone()
                                .next()
                                .is_some_and(|after| after.is_lowercase()))
                    {
                        break;
                    }
                    advance += next_c.len_utf8();
                    prev_c = next_c;
                }
                let start = self.pos;
                self.pos += advance;
                Some(&self.text[start..self.pos])
            }
        }
    }
}

/// Tokenize a diff line into word/operator tokens (whitespace + punctuation
/// are separate tokens so `->` → `=>` highlights a single character). Zero-copy
/// iterator.
#[inline]
pub fn tokenize_line<'a>(text: &'a str) -> LineTokenizer<'a> {
    LineTokenizer {
        text,
        pos: 0,
        is_ascii: text.is_ascii(),
    }
}

/// Checks whether `t` contains whitespace (ASCII or Unicode whitespace
/// characters).
pub fn token_is_ws(t: &str) -> bool {
    t.chars().any(is_ws_char)
}

/// The alphanumeric test — any Unicode alphanumeric char.
pub fn token_is_alnum(t: &str) -> bool {
    t.chars().any(|c| c.is_alphanumeric())
}

/// The salient test: alphanumeric, not a keyword or literal in `profile`.
/// Falls back to counting all alphanumerics as salient when no profile is
/// available.
pub const COMMON_KEYWORDS: &[&str] = &[
    "as", "let", "mut", "const", "static", "pub", "fn", "for", "in", "if", "else", "match",
    "return", "impl", "trait", "struct", "enum", "type", "where", "use", "mod", "crate", "self",
    "super", "async", "await", "while", "loop",
];

/// The salient test: alphanumeric, contains at least one alphabetic character
/// (not a pure numeric literal), and not a keyword, literal, or builtin type in
/// `profile`. Falls back to counting all alphabetic tokens as salient when no
/// profile is available.
pub fn token_is_salient(t: &str, profile: Option<&LangProfile>) -> bool {
    token_is_alnum(t)
        && t.chars().any(char::is_alphabetic)
        && !profile.is_some_and(|p| p.is_non_salient(t))
}

/// Checks whether `text` contains at least one salient token.
pub fn line_has_salient_token(text: &str, profile: Option<&LangProfile>) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| !COMMON_KEYWORDS.contains(&w) && token_is_salient(w, profile))
}

/// Checks whether token `tokens[idx]` is salient within its slice context.
///
/// Expands contiguous subword tokens (`are_contiguous_subwords`) to check the
/// full identifier against the profile's non-salient table, ensuring that
/// split primitive types (such as `f` + `64` from `f64`) are recognized as
/// non-salient.
pub fn is_token_salient_in_context(
    tokens: &[&str],
    idx: usize,
    profile: Option<&LangProfile>,
) -> bool {
    if idx >= tokens.len() {
        return false;
    }
    let t = tokens[idx];
    if !token_is_alnum(t) || !t.chars().any(char::is_alphabetic) {
        return false;
    }
    let mut start = idx;
    while start > 0 && are_contiguous_subwords(tokens[start - 1], tokens[start]) {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < tokens.len() && are_contiguous_subwords(tokens[end - 1], tokens[end]) {
        end += 1;
    }
    let full_start = tokens[start].as_ptr();
    let full_len =
        (tokens[end - 1].as_ptr() as usize + tokens[end - 1].len()) - full_start as usize;
    let full_word =
        unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(full_start, full_len)) };

    !COMMON_KEYWORDS.contains(&full_word) && token_is_salient(full_word, profile)
}

/// Computes unambiguous unique token anchors (Patience diff pre-pass).
pub fn unique_token_anchors(
    a: &[&str],
    b: &[&str],
    profile: Option<&LangProfile>,
) -> Vec<(usize, usize)> {
    crate::token::entropy_anchors(a, b, profile)
}

#[inline]
pub(crate) fn is_subword_token(t: &str) -> bool {
    t == "_" || token_is_alnum(t)
}

#[inline]
pub(crate) fn are_contiguous_subwords(t1: &str, t2: &str) -> bool {
    t1.as_ptr() as usize + t1.len() == t2.as_ptr() as usize
        && !token_is_ws(t1)
        && !token_is_ws(t2)
        && is_subword_token(t1)
        && is_subword_token(t2)
}

pub(crate) fn compound_identifier_root_and_stem<'a>(
    slice: &[&'a str],
    idx: usize,
) -> Option<(&'a str, &'a str)> {
    if idx >= slice.len() || !is_subword_token(slice[idx]) {
        return None;
    }
    let mut start = idx;
    while start > 0
        && is_subword_token(slice[start - 1])
        && are_contiguous_subwords(slice[start - 1], slice[start])
    {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < slice.len()
        && is_subword_token(slice[end])
        && are_contiguous_subwords(slice[end - 1], slice[end])
    {
        end += 1;
    }

    let word_slice = &slice[start..end];
    let mut alnum_tokens = word_slice.iter().copied().filter(|&t| token_is_alnum(t));
    let root = alnum_tokens.next()?;
    let stem = alnum_tokens.next_back().unwrap_or(root);

    if word_slice.len() > 1 && (root != stem || word_slice.contains(&"_")) {
        Some((root, stem))
    } else {
        None
    }
}

/// Checks whether token `a[i]` and `b[j]` are eligible to match.
///
/// Enforces:
/// 1. Literal token string equality (`a[i] == b[j]`).
/// 2. **Opposite-boundary subword invariant**: A subword token that is strictly
///    a prefix of a compound token (no left subword neighbor, has right subword
///    neighbor) cannot match a subword token that is strictly a suffix of a
///    compound token (has left subword neighbor, no right subword neighbor).
///    Matching inverted boundaries would splice a new prefix onto an old suffix
///    and produce chimeric mutant tokens (e.g. `s_add` vs `add_style` or
///    `row_in_vh` vs `scrollbar_scroll_for_row`).
/// 3. Path root and stem compatibility.
/// 4. Compound identifier root and stem compatibility: If both tokens belong to
///    compound identifiers, and neither their prefixes (roots) nor suffixes
///    (stems) match, the identifiers represent disjoint entities and their
///    internal subwords cannot match.
#[inline]
pub(crate) fn token_ws_kind(
    tokens: &[&str],
    prev: &[&str],
    next: Option<&str>,
    idx: usize,
) -> WhitespaceKind {
    let is_leading =
        prev.iter().all(|t| token_is_ws(t)) && tokens[..idx].iter().all(|t| token_is_ws(t));
    let has_non_ws_after =
        tokens[idx + 1..].iter().any(|t| !token_is_ws(t)) || next.is_some_and(|t| !token_is_ws(t));

    if is_leading {
        WhitespaceKind::Leading
    } else if !has_non_ws_after {
        WhitespaceKind::Trailing
    } else {
        WhitespaceKind::Internal
    }
}

#[inline]
#[allow(clippy::too_many_arguments)]
pub(crate) fn tokens_can_match(
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
    i: usize,
    j: usize,
) -> bool {
    if a[i] != b[j] {
        return false;
    }

    if token_is_ws(a[i]) {
        let kind_a = token_ws_kind(a, prev_a, next_a, i);
        let kind_b = token_ws_kind(b, prev_b, next_b, j);
        if kind_a != kind_b {
            return false;
        }
    }

    if is_subword_token(a[i]) {
        let la = i > 0 && is_subword_token(a[i - 1]);
        let ra = i + 1 < a.len() && is_subword_token(a[i + 1]);
        let lb = j > 0 && is_subword_token(b[j - 1]);
        let rb = j + 1 < b.len() && is_subword_token(b[j + 1]);
        if (la && !ra && !lb && rb) || (!la && ra && lb && !rb) {
            return false;
        }
    }

    let a_ident = compound_identifier_root_and_stem(a, i);
    let b_ident = compound_identifier_root_and_stem(b, j);
    if let (Some((ra, sa)), Some((rb, sb))) = (a_ident, b_ident)
        && ra != rb
        && sa != sb
    {
        return false;
    }

    if a[i] == "(" {
        let sig_a = is_method_decl_open_paren(prev_a, a, i);
        let sig_b = is_method_decl_open_paren(prev_b, b, j);
        if sig_a != sig_b {
            return false;
        }
    } else if a[i] == ")" {
        let sig_a = is_method_decl_close_paren(prev_a, a, i, next_a);
        let sig_b = is_method_decl_close_paren(prev_b, b, j, next_b);
        if sig_a != sig_b {
            return false;
        }
    } else if a[i] == "{" {
        let sig_a = is_method_decl_body_brace(prev_a, a, i);
        let sig_b = is_method_decl_body_brace(prev_b, b, j);
        if sig_a != sig_b {
            return false;
        }
    }

    true
}

/// Determines whether the opening parenthesis at `tokens[idx]` belongs to a
/// function or method declaration signature.
fn is_method_decl_open_paren(prev: &[&str], tokens: &[&str], idx: usize) -> bool {
    let mut rev_tokens = tokens[..idx].iter().rev().chain(prev.iter().rev());

    let mut tok = loop {
        match rev_tokens.next() {
            Some(&t) if token_is_ws(t) => continue,
            Some(&t) => break t,
            None => return false,
        }
    };

    if tok == ">" {
        let mut depth = 1usize;
        for &t in rev_tokens.by_ref() {
            if t == ">" {
                depth += 1;
            } else if t == "<" {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            } else if matches!(t, ";" | "{" | "}" | "(" | ")") {
                return false;
            }
        }
        tok = loop {
            match rev_tokens.next() {
                Some(&t) if token_is_ws(t) => continue,
                Some(&t) => break t,
                None => return false,
            }
        };
    }

    if !token_is_alnum(tok) {
        return false;
    }

    let mut saw_ws = false;
    for &t in rev_tokens.by_ref() {
        if token_is_ws(t) {
            saw_ws = true;
            break;
        }
        if token_is_alnum(t) || t == "_" {
            continue;
        }
        return false;
    }
    if !saw_ws {
        return false;
    }

    let kw = loop {
        match rev_tokens.next() {
            Some(&t) if token_is_ws(t) => continue,
            Some(&t) => break t,
            None => return false,
        }
    };

    matches!(kw, "fn" | "def" | "func" | "function" | "sub")
}

/// Determines whether the closing parenthesis at `tokens[idx]` terminates a
/// function or method parameter list.
fn is_method_decl_close_paren(
    prev: &[&str],
    tokens: &[&str],
    idx: usize,
    next_tok: Option<&str>,
) -> bool {
    let mut depth = 1usize;
    for (offset, &t) in tokens[..idx].iter().enumerate().rev() {
        if t == ")" {
            depth += 1;
        } else if t == "(" {
            depth -= 1;
            if depth == 0 {
                return is_method_decl_open_paren(prev, tokens, offset);
            }
        } else if matches!(t, ";" | "{" | "}") {
            return false;
        }
    }

    for (offset, &t) in prev.iter().enumerate().rev() {
        if t == ")" {
            depth += 1;
        } else if t == "(" {
            depth -= 1;
            if depth == 0 {
                return is_method_decl_open_paren(&[], prev, offset);
            }
        } else if matches!(t, ";" | "{" | "}") {
            return false;
        }
    }

    let is_first_on_line =
        tokens[..idx].iter().all(|&t| token_is_ws(t)) && prev.iter().all(|&t| token_is_ws(t));
    if is_first_on_line {
        let mut fwd = tokens[idx + 1..].iter().filter(|&&t| !token_is_ws(t));
        let next = fwd.next().copied().or(next_tok);
        if let Some(n) = next
            && (n == "->" || n == "{")
        {
            return true;
        }
    }

    false
}

/// Determines whether the opening brace at `tokens[idx]` introduces a function
/// or method declaration body.
pub(crate) fn is_method_decl_body_brace(prev: &[&str], tokens: &[&str], idx: usize) -> bool {
    let mut depth = 0usize;
    let mut close_paren_idx = None;

    for (offset, &t) in tokens[..idx].iter().enumerate().rev() {
        if token_is_ws(t) {
            continue;
        }
        if t == ")" {
            if depth == 0 {
                close_paren_idx = Some(offset);
                break;
            }
            depth += 1;
        } else if t == "(" {
            depth = depth.saturating_sub(1);
        } else if matches!(t, ";" | "{" | "}") {
            return false;
        }
    }

    if let Some(close_idx) = close_paren_idx {
        return is_method_decl_close_paren(prev, tokens, close_idx, None);
    }

    let mut prev_close_idx = None;
    for (offset, &t) in prev.iter().enumerate().rev() {
        if token_is_ws(t) {
            continue;
        }
        if t == ")" {
            if depth == 0 {
                prev_close_idx = Some(offset);
                break;
            }
            depth += 1;
        } else if t == "(" {
            depth = depth.saturating_sub(1);
        } else if matches!(t, ";" | "{" | "}") {
            return false;
        }
    }

    if let Some(close_idx) = prev_close_idx {
        is_method_decl_close_paren(&[], prev, close_idx, None)
    } else {
        false
    }
}
