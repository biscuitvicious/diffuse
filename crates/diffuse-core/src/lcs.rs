//! One-for-one port of `review.ts`'s word-level diff (PLAN.md §3):
//! `tokenizeLine`, `lcsPairs` (the tie-break ladder), `spansFrom`,
//! `wordSpans`. The comments carry the *why* — ported verbatim.
//!
//! Offset convention: the TS uses UTF-16 code-unit offsets (JS string
//! indices). This port uses **char offsets** (Unicode scalar values) —
//! the same tokens are unmatched, the spans index the same text the
//! renderer receives, so behavior is identical; only the raw offset
//! values differ for non-ASCII lines.

pub mod bit_parallel;

use std::cell::RefCell;

use rustc_hash::{FxHashMap, FxHashSet};

use diffuse_syntax::profile::LangProfile;

use crate::{merge::RunKind, parse::Spans, token::WhitespaceKind};

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

/// The salient test: alphanumeric, not a keyword in `profile`, and not a
/// boolean literal.
pub fn token_is_salient(t: &str, profile: Option<&LangProfile>) -> bool {
    token_is_alnum(t) && !profile.is_some_and(|p| p.is_keyword(t)) && !matches!(t, "true" | "false")
}

/// Checks whether `text` contains at least one salient token.
pub fn line_has_salient_token(text: &str, profile: Option<&LangProfile>) -> bool {
    let mut has = false;
    for_each_token(text, |t| {
        if has {
            return;
        }
        if token_is_salient(t, profile) {
            has = true;
        }
    });
    has
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

fn compound_identifier_root_and_stem<'a>(
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

/// Core LCS backtrace over string slices.
pub fn raw_lcs_pairs(a: &[&str], b: &[&str]) -> FxHashMap<usize, usize> {
    raw_lcs_pairs_with_context(a, b, &[], &[], None, None)
}

/// Core LCS backtrace with optional boundary lookahead tokens from parent line.
pub fn raw_lcs_pairs_with_lookahead(
    a: &[&str],
    b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
) -> FxHashMap<usize, usize> {
    raw_lcs_pairs_with_context(a, b, &[], &[], next_a, next_b)
}

/// Whether `t` is a paired syntactic container delimiter.
#[inline]
pub fn is_container_delim(t: &str) -> bool {
    matches!(
        t,
        "(" | ")" | "[" | "]" | "{" | "}" | "<" | ">" | "`" | "\"" | "'"
    )
}

/// Token match reward for Gotoh affine gap sequence alignment.
#[inline]
fn token_match_reward(t: &str) -> i32 {
    if token_is_ws(t) {
        2
    } else if is_container_delim(t) {
        20
    } else if token_is_alnum(t) {
        let len = char_len(t) as i32;
        if len <= 2 { 10 } else { 14 + len.min(10) * 2 }
    } else if char_len(t) <= 1 {
        // Low-entropy isolated punctuation and single-character operators
        // are weighted strictly below affine gap opening + extension (20),
        // preventing isolated punctuation matches across disjoint semantic units.
        4
    } else {
        10
    }
}

const NEG_INF: i32 = -1_000_000;

/// Scoring weights and alignment parameters for Gotoh subline sequence
/// alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weights {
    /// Cost to open a gap in Gotoh affine DP (18).
    pub gap_open: i32,
    /// Incremental cost to extend a gap (2).
    pub gap_extend: i32,
    /// Bonus for consecutive matched tokens in the same container run (6).
    pub contig_bonus: i32,
    /// Subword concatenation bonus when adjacent subwords match (16).
    pub subword_bonus: i32,
    /// Bonus for matching tokens at identical delimiter nesting depth (2).
    pub depth_match_bonus: i32,
    /// Penalty per depth level for matching tokens at differing delimiter
    /// nesting depths (2).
    pub depth_mismatch_penalty: i32,
    /// Bonus for matching tokens with concordant subword boundary context (2).
    pub boundary_match_bonus: i32,
    /// Penalty for matching tokens with discordant subword boundary context
    /// (0).
    pub boundary_mismatch_penalty: i32,
}

impl Default for Weights {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Weights {
    pub const DEFAULT: Self = Self {
        gap_open: 18,
        gap_extend: 2,
        contig_bonus: 6,
        subword_bonus: 16,
        depth_match_bonus: 2,
        depth_mismatch_penalty: 2,
        boundary_match_bonus: 2,
        boundary_mismatch_penalty: 0,
    };
}

/// Reusable scratch memory arena for Gotoh 3-state affine gap dynamic
/// programming matrices, eliminating repeated per-invocation heap allocations.
#[derive(Default)]
pub struct LcsScratchArena {
    m_matrix: Vec<i32>,
    x_matrix: Vec<i32>,
    y_matrix: Vec<i32>,
    run_len: Vec<u16>,
    tb_m: Vec<u8>,
    tb_x: Vec<u8>,
    tb_y: Vec<u8>,
    depth_a: Vec<u16>,
    depth_b: Vec<u16>,
    container_a: Vec<u8>,
    container_b: Vec<u8>,
}

impl LcsScratchArena {
    /// Create a new scratch arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Prepares backing matrices for a DP run of size `total_cells`.
    /// Reallocates only if the existing capacity is insufficient, and
    /// resets initial values without releasing allocated memory.
    pub fn prepare(&mut self, total_cells: usize, n: usize, m: usize) {
        if self.m_matrix.len() < total_cells {
            self.m_matrix.resize(total_cells, NEG_INF);
            self.x_matrix.resize(total_cells, NEG_INF);
            self.y_matrix.resize(total_cells, NEG_INF);
            self.run_len.resize(total_cells, 0);
            self.tb_m.resize(total_cells, 0);
            self.tb_x.resize(total_cells, 0);
            self.tb_y.resize(total_cells, 0);
        }
        if self.depth_a.len() < n {
            self.depth_a.resize(n, 0);
        }
        if self.depth_b.len() < m {
            self.depth_b.resize(m, 0);
        }
        if self.container_a.len() < n {
            self.container_a.resize(n, 0);
        }
        if self.container_b.len() < m {
            self.container_b.resize(m, 0);
        }
        self.m_matrix[..total_cells].fill(NEG_INF);
        self.x_matrix[..total_cells].fill(NEG_INF);
        self.y_matrix[..total_cells].fill(NEG_INF);
        self.run_len[..total_cells].fill(0);
        self.tb_m[..total_cells].fill(0);
        self.tb_x[..total_cells].fill(0);
        self.tb_y[..total_cells].fill(0);
    }
}

thread_local! {
    static SCRATCH_ARENA: RefCell<LcsScratchArena> = RefCell::new(LcsScratchArena::default());
}

/// Core sequence alignment backtrace with Gotoh 3-state affine gap dynamic
/// programming using a reusable scratch arena and default weights.
pub fn raw_lcs_pairs_with_arena(
    arena: &mut LcsScratchArena,
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
) -> FxHashMap<usize, usize> {
    raw_lcs_pairs_with_arena_and_weights(
        arena,
        a,
        b,
        prev_a,
        prev_b,
        next_a,
        next_b,
        &Weights::DEFAULT,
    )
}

/// Core sequence alignment backtrace with Gotoh 3-state affine gap dynamic
/// programming using a reusable scratch arena and custom weights.
#[allow(clippy::too_many_arguments)]
pub fn raw_lcs_pairs_with_arena_and_weights(
    arena: &mut LcsScratchArena,
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
    weights: &Weights,
) -> FxHashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return FxHashMap::default();
    }

    let stride = m + 1;
    let idx = |i: usize, j: usize| i * stride + j;
    let total_cells = (n + 1) * (m + 1);

    arena.prepare(total_cells, n, m);
    diffuse_algo::track_nesting_depths_into(
        prev_a,
        a,
        &diffuse_algo::UniversalDelimiterClassifier,
        &mut arena.depth_a[..n],
    );
    diffuse_algo::track_nesting_depths_into(
        prev_b,
        b,
        &diffuse_algo::UniversalDelimiterClassifier,
        &mut arena.depth_b[..m],
    );
    let depth_a = &arena.depth_a[..n];
    let depth_b = &arena.depth_b[..m];

    diffuse_algo::track_enclosing_containers_into(
        prev_a,
        a,
        &diffuse_algo::UniversalDelimiterClassifier,
        &mut arena.container_a[..n],
    );
    diffuse_algo::track_enclosing_containers_into(
        prev_b,
        b,
        &diffuse_algo::UniversalDelimiterClassifier,
        &mut arena.container_b[..m],
    );
    let container_a = &arena.container_a[..n];
    let container_b = &arena.container_b[..m];

    let m_matrix = &mut arena.m_matrix[..total_cells];
    let x_matrix = &mut arena.x_matrix[..total_cells];
    let y_matrix = &mut arena.y_matrix[..total_cells];
    let run_len = &mut arena.run_len[..total_cells];
    let tb_m = &mut arena.tb_m[..total_cells];
    let tb_x = &mut arena.tb_x[..total_cells];
    let tb_y = &mut arena.tb_y[..total_cells];

    // Backward Gotoh DP over suffixes: cell (i, j) aligns a[i..n] and b[j..m].
    m_matrix[idx(n, m)] = 0;
    for i in (0..n).rev() {
        x_matrix[idx(i, m)] = -weights.gap_open - ((n - i) as i32) * weights.gap_extend;
        tb_x[idx(i, m)] = 1;
    }
    for j in (0..m).rev() {
        y_matrix[idx(n, j)] = -weights.gap_open - ((m - j) as i32) * weights.gap_extend;
        tb_y[idx(n, j)] = 2;
    }

    for i in (0..n).rev() {
        for j in (0..m).rev() {
            // State X: deletion of a[i]
            let cx = x_matrix[idx(i + 1, j)].saturating_sub(weights.gap_extend);
            let cm_x =
                m_matrix[idx(i + 1, j)].saturating_sub(weights.gap_open + weights.gap_extend);
            let cy = y_matrix[idx(i + 1, j)].saturating_sub(weights.gap_extend);
            let (best_x, to_x) = if cx >= cm_x && cx >= cy {
                (cx, 1)
            } else if cy >= cm_x {
                (cy, 2)
            } else {
                (cm_x, 0)
            };
            x_matrix[idx(i, j)] = best_x;
            tb_x[idx(i, j)] = to_x;

            // State Y: insertion of b[j]
            let cy = y_matrix[idx(i, j + 1)].saturating_sub(weights.gap_extend);
            let cm_y =
                m_matrix[idx(i, j + 1)].saturating_sub(weights.gap_open + weights.gap_extend);
            let cx = x_matrix[idx(i, j + 1)].saturating_sub(weights.gap_extend);
            let (best_y, to_y) = if cy >= cm_y && cy >= cx {
                (cy, 2)
            } else if cx >= cm_y {
                (cx, 1)
            } else {
                (cm_y, 0)
            };
            y_matrix[idx(i, j)] = best_y;
            tb_y[idx(i, j)] = to_y;

            // State M: match a[i] with b[j]
            if tokens_can_match(a, b, prev_a, prev_b, next_a, next_b, i, j) {
                let ca = crate::token::ContainerKind::from_id(container_a[i]);
                let cb = crate::token::ContainerKind::from_id(container_b[j]);
                if !crate::token::ContainerKind::is_compatible_with(ca, cb) {
                    continue;
                }

                if is_subword_token(a[i]) {
                    let la = i > 0
                        && is_subword_token(a[i - 1])
                        && are_contiguous_subwords(a[i - 1], a[i]);
                    let ra = i + 1 < n
                        && is_subword_token(a[i + 1])
                        && are_contiguous_subwords(a[i], a[i + 1]);
                    let lb = j > 0
                        && is_subword_token(b[j - 1])
                        && are_contiguous_subwords(b[j - 1], b[j]);
                    let rb = j + 1 < m
                        && is_subword_token(b[j + 1])
                        && are_contiguous_subwords(b[j], b[j + 1]);
                    if (la != lb || ra != rb) && (depth_a[i] != depth_b[j] || ca != cb) {
                        continue;
                    }
                }
                let is_ws = token_is_ws(a[i]);
                let depth_matched = depth_a[i] == depth_b[j];
                let next_rl = if is_ws { 0 } else { run_len[idx(i + 1, j + 1)] };
                let is_subword_pair = i + 1 < n
                    && j + 1 < m
                    && are_contiguous_subwords(a[i], a[i + 1])
                    && are_contiguous_subwords(b[j], b[j + 1]);
                let subword_bonus = if is_subword_pair {
                    weights.subword_bonus
                } else {
                    0
                };
                let same_container =
                    depth_matched && (i + 1 == n || j + 1 == m || depth_a[i + 1] == depth_b[j + 1]);
                let run_bonus = if is_ws || !same_container {
                    0
                } else {
                    weights.contig_bonus + (next_rl.min(8) as i32) * 2 + subword_bonus
                };
                let next_m_val = m_matrix[idx(i + 1, j + 1)].saturating_add(run_bonus);
                let next_x = x_matrix[idx(i + 1, j + 1)];
                let next_y = y_matrix[idx(i + 1, j + 1)];
                let (best_next, to_m, rl) = if next_m_val >= next_x && next_m_val >= next_y {
                    (next_m_val, 0, if same_container { next_rl + 1 } else { 1 })
                } else if next_x >= next_y {
                    (next_x, 1, 1)
                } else {
                    (next_y, 2, 1)
                };
                let base_r = if is_ws {
                    token_ws_kind(a, prev_a, next_a, i).match_reward()
                } else {
                    token_match_reward(a[i])
                };
                let depth_diff = (depth_a[i] as i32 - depth_b[j] as i32).abs();
                let depth_adj = if depth_diff == 0 {
                    weights.depth_match_bonus
                } else {
                    -depth_diff * weights.depth_mismatch_penalty
                };
                let boundary_adj = if is_subword_token(a[i]) {
                    let la = i > 0
                        && is_subword_token(a[i - 1])
                        && are_contiguous_subwords(a[i - 1], a[i]);
                    let ra = i + 1 < n
                        && is_subword_token(a[i + 1])
                        && are_contiguous_subwords(a[i], a[i + 1]);
                    let lb = j > 0
                        && is_subword_token(b[j - 1])
                        && are_contiguous_subwords(b[j - 1], b[j]);
                    let rb = j + 1 < m
                        && is_subword_token(b[j + 1])
                        && are_contiguous_subwords(b[j], b[j + 1]);
                    if la == lb && ra == rb {
                        weights.boundary_match_bonus
                    } else {
                        -weights.boundary_mismatch_penalty
                    }
                } else {
                    0
                };
                let r = (base_r + depth_adj + boundary_adj).max(0);
                m_matrix[idx(i, j)] = r.saturating_add(best_next);
                tb_m[idx(i, j)] = to_m;
                run_len[idx(i, j)] = if is_ws { 0 } else { rl };
            }
        }
    }

    let start_m = m_matrix[idx(0, 0)];
    let start_x = x_matrix[idx(0, 0)];
    let start_y = y_matrix[idx(0, 0)];
    let mut cur_state = if start_m >= start_x && start_m >= start_y {
        0
    } else if start_x >= start_y {
        1
    } else {
        2
    };

    let mut i = 0;
    let mut j = 0;
    let mut pairs = FxHashMap::default();

    while i < n || j < m {
        match cur_state {
            0 => {
                if i < n && j < m {
                    pairs.insert(i, j);
                    cur_state = tb_m[idx(i, j)];
                    i += 1;
                    j += 1;
                } else {
                    break;
                }
            }
            1 => {
                if i < n {
                    cur_state = tb_x[idx(i, j)];
                    i += 1;
                } else {
                    break;
                }
            }
            2 => {
                if j < m {
                    cur_state = tb_y[idx(i, j)];
                    j += 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }

    pairs
}

/// Core sequence alignment backtrace with Gotoh 3-state affine gap dynamic
/// programming using thread-local scratch memory.
pub fn raw_lcs_pairs_with_context(
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
) -> FxHashMap<usize, usize> {
    SCRATCH_ARENA.with_borrow_mut(|arena| {
        raw_lcs_pairs_with_arena(arena, a, b, prev_a, prev_b, next_a, next_b)
    })
}

/// LCS backtrace: map of old-token index → matched new-token index.
/// Combines recursive Patience unique token anchoring with run-continuation
/// LCS.
pub fn lcs_pairs(a: &[&str], b: &[&str]) -> FxHashMap<usize, usize> {
    lcs_pairs_recursive(a, b, &[], &[], None, None, None)
}

/// LCS backtrace with optional syntax profile for operator anchor awareness.
pub fn lcs_pairs_with_profile(
    a: &[&str],
    b: &[&str],
    profile: Option<&LangProfile>,
) -> FxHashMap<usize, usize> {
    lcs_pairs_recursive(a, b, &[], &[], None, None, profile)
}

/// Recursive Patience LCS backtrace with anchor lookahead context.
pub fn lcs_pairs_recursive(
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
    profile: Option<&LangProfile>,
) -> FxHashMap<usize, usize> {
    if a.is_empty() || b.is_empty() {
        return FxHashMap::default();
    }
    let anchors = unique_token_anchors(a, b, profile);
    if anchors.is_empty() {
        return raw_lcs_pairs_with_context(a, b, prev_a, prev_b, next_a, next_b);
    }
    let mut pairs = FxHashMap::default();
    let mut a_prev = 0;
    let mut b_prev = 0;
    for (anchor_idx, &(ai, bi)) in anchors.iter().enumerate() {
        let mut back_a = ai;
        let mut back_b = bi;
        while back_a > a_prev
            && back_b > b_prev
            && is_subword_token(a[back_a - 1])
            && is_subword_token(b[back_b - 1])
            && a[back_a - 1] == b[back_b - 1]
        {
            back_a -= 1;
            back_b -= 1;
        }

        let (next_limit_a, next_limit_b) = if let Some(&(nai, nbi)) = anchors.get(anchor_idx + 1) {
            (nai, nbi)
        } else {
            (a.len(), b.len())
        };

        let mut fwd_a = ai;
        let mut fwd_b = bi;
        while fwd_a + 1 < next_limit_a
            && fwd_b + 1 < next_limit_b
            && are_contiguous_subwords(a[fwd_a], a[fwd_a + 1])
            && are_contiguous_subwords(b[fwd_b], b[fwd_b + 1])
            && a[fwd_a + 1] == b[fwd_b + 1]
        {
            fwd_a += 1;
            fwd_b += 1;
        }

        if back_a > a_prev && back_b > b_prev {
            let sub_prev_a = if a_prev == 0 { prev_a } else { &a[..a_prev] };
            let sub_prev_b = if b_prev == 0 { prev_b } else { &b[..b_prev] };
            let sub_pairs = lcs_pairs_recursive(
                &a[a_prev..back_a],
                &b[b_prev..back_b],
                sub_prev_a,
                sub_prev_b,
                Some(a[back_a]),
                Some(b[back_b]),
                profile,
            );
            for (si, sj) in sub_pairs {
                pairs.insert(a_prev + si, b_prev + sj);
            }
        }

        for (k_a, k_b) in (back_a..=fwd_a).zip(back_b..=fwd_b) {
            pairs.insert(k_a, k_b);
        }
        a_prev = fwd_a + 1;
        b_prev = fwd_b + 1;
    }
    if a_prev < a.len() && b_prev < b.len() {
        let sub_prev_a = if a_prev == 0 { prev_a } else { &a[..a_prev] };
        let sub_prev_b = if b_prev == 0 { prev_b } else { &b[..b_prev] };
        let sub_pairs = lcs_pairs_recursive(
            &a[a_prev..],
            &b[b_prev..],
            sub_prev_a,
            sub_prev_b,
            next_a,
            next_b,
            profile,
        );
        for (si, sj) in sub_pairs {
            pairs.insert(a_prev + si, b_prev + sj);
        }
    }
    pairs
}

pub fn lcs_pairs_anchored(
    a: &[&str],
    b: &[&str],
    anchors: &[(usize, usize)],
) -> FxHashMap<usize, usize> {
    let mut pairs = FxHashMap::default();
    let mut a_prev = 0;
    let mut b_prev = 0;
    for (anchor_idx, &(ai, bi)) in anchors.iter().enumerate() {
        let mut back_a = ai;
        let mut back_b = bi;
        while back_a > a_prev
            && back_b > b_prev
            && is_subword_token(a[back_a - 1])
            && is_subword_token(b[back_b - 1])
            && a[back_a - 1] == b[back_b - 1]
        {
            back_a -= 1;
            back_b -= 1;
        }

        let (next_limit_a, next_limit_b) = if let Some(&(nai, nbi)) = anchors.get(anchor_idx + 1) {
            (nai, nbi)
        } else {
            (a.len(), b.len())
        };

        let mut fwd_a = ai;
        let mut fwd_b = bi;
        while fwd_a + 1 < next_limit_a
            && fwd_b + 1 < next_limit_b
            && are_contiguous_subwords(a[fwd_a], a[fwd_a + 1])
            && are_contiguous_subwords(b[fwd_b], b[fwd_b + 1])
            && a[fwd_a + 1] == b[fwd_b + 1]
        {
            fwd_a += 1;
            fwd_b += 1;
        }

        if back_a > a_prev && back_b > b_prev {
            let sub_pairs = raw_lcs_pairs_with_context(
                &a[a_prev..back_a],
                &b[b_prev..back_b],
                &a[..a_prev],
                &b[..b_prev],
                Some(a[back_a]),
                Some(b[back_b]),
            );
            for (si, sj) in sub_pairs {
                pairs.insert(a_prev + si, b_prev + sj);
            }
        }

        for (k_a, k_b) in (back_a..=fwd_a).zip(back_b..=fwd_b) {
            pairs.insert(k_a, k_b);
        }
        a_prev = fwd_a + 1;
        b_prev = fwd_b + 1;
    }
    if a_prev < a.len() && b_prev < b.len() {
        let sub_pairs = raw_lcs_pairs_with_context(
            &a[a_prev..],
            &b[b_prev..],
            &a[..a_prev],
            &b[..b_prev],
            None,
            None,
        );
        for (si, sj) in sub_pairs {
            pairs.insert(a_prev + si, b_prev + sj);
        }
    }
    pairs
}

/// The LCS over INTERNED token ids — the pairing's hot inner loop (D×A
/// runs per hunk). The String equality becomes a u32 compare; the tie-break
/// ladder's `token_is_ws`/`token_is_alnum` become array lookups (the
/// intern pool precomputed them). Used during hunk-collapse pairing.
pub fn lcs_pairs_ids(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
) -> FxHashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return FxHashMap::default();
    }
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        let row = i * (m + 1);
        let next = row + m + 1;
        let ai = a[i];
        for j in (0..m).rev() {
            dp[row + j] = if ai == b[j] {
                dp[next + j + 1] + 1
            } else {
                dp[next + j].max(dp[row + j + 1])
            };
        }
    }
    let mut pairs = FxHashMap::default();
    let mut i = 0;
    let mut j = 0;
    let mut in_add_run = false;
    while i < n && j < m {
        if a[i] == b[j] {
            if id_ws[a[i] as usize]
                && in_add_run
                && !(i + 1 < n && j + 1 < m && a[i + 1] == b[j + 1] && !id_ws[a[i + 1] as usize])
            {
                j += 1;
                continue;
            }
            if in_add_run
                && dp[i * (m + 1) + j + 1] == dp[i * (m + 1) + j]
                && (j + 1..m).any(|k| {
                    b[k] == a[i] && dp[(i + 1) * (m + 1) + k + 1] + 1 == dp[i * (m + 1) + j]
                })
            {
                j += 1;
                continue;
            }
            pairs.insert(i, j);
            i += 1;
            j += 1;
            in_add_run = false;
        } else if dp[(i + 1) * (m + 1) + j] > dp[i * (m + 1) + j + 1] {
            i += 1;
        } else if dp[(i + 1) * (m + 1) + j] < dp[i * (m + 1) + j + 1] {
            j += 1;
            in_add_run = true;
        } else if id_ws[a[i] as usize] && !id_ws[b[j] as usize] {
            if i + 1 < n && a[i + 1] == b[j] {
                i += 1;
            } else {
                j += 1;
                in_add_run = true;
            }
        } else if id_ws[b[j] as usize] && !id_ws[a[i] as usize] {
            j += 1;
            in_add_run = true;
        } else if !b[j + 1..].iter().any(|t| *t == a[i])
            || (!id_alnum[a[i] as usize] && id_alnum[b[j] as usize])
        {
            i += 1;
        } else {
            j += 1;
            in_add_run = true;
        }
    }
    pairs
}

/// The matched CHAR count of the LCS over interned ids — the window
/// scan's `sim` needs only this (NOT the pairs themselves). Same DP + the
/// SAME tie-break ladder as `lcs_pairs_ids`, but accumulates `b_token_chars[j]`
/// on each match instead of building a `HashMap` — the window scan's 42k
/// per-call `HashMaps` (alloc + per-pair inserts) were the remaining scan
/// cost. Behavior-identical to `lcs_pairs_ids(...).values().map(char).sum`.
pub fn lcs_matched_chars_ids(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
    b_token_chars: &[usize],
    scratch: &mut Vec<u64>,
) -> usize {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return 0;
    }
    if a == b {
        let mut total = 0;
        for (idx, &id) in b.iter().enumerate() {
            if let Some(&is_ws) = id_ws.get(id as usize)
                && !is_ws
            {
                total += b_token_chars.get(idx).copied().unwrap_or(0);
            }
        }
        return total;
    }
    bit_parallel::bit_parallel_suffix_matched_chars_scratch(
        a,
        b,
        id_ws,
        id_alnum,
        b_token_chars,
        scratch,
    )
}

/// Char-offset spans of the UNMATCHED (changed) tokens in `toks`.
pub fn spans_from(text: &str, toks: &[&str], matched: &FxHashSet<usize>) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut pos = 0usize;
    let mut run_start: Option<usize> = None;
    for (i, t) in toks.iter().enumerate() {
        let len = char_len(t);
        if !matched.contains(&i) {
            if run_start.is_none() {
                run_start = Some(pos);
            }
        } else if let Some(start) = run_start.take() {
            spans.push((start, pos));
        }
        pos += len;
    }
    if let Some(start) = run_start {
        spans.push((start, pos));
    }
    let _ = text;
    spans
}

/// A pair of change-span sets: char ranges into the old/new text. The
/// row's span type (`parse::Spans`) — one type for the whole pipeline.
pub type WordSpans = Spans;

/// Default maximum token product cap (old tokens × new tokens) allowed for
/// word-level diffing before falling back.
pub const DEFAULT_TOKEN_PRODUCT_CAP: usize = 250_000;

thread_local! {
    static TOKEN_PRODUCT_CAP: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_TOKEN_PRODUCT_CAP) };
}

/// Set the maximum token product cap (old tokens × new tokens) allowed for
/// word-level diffing before falling back.
pub fn set_token_product_cap(cap: usize) {
    TOKEN_PRODUCT_CAP.with(|c| c.set(cap));
}

/// Get the current token product cap.
pub fn token_product_cap() -> usize {
    TOKEN_PRODUCT_CAP.with(|c| c.get())
}

/**
 * Word-level change spans between an old and a new line. Both sides get the
 * runs of tokens that have no LCS counterpart; a line with no counterpart
 * (pure add/del) yields no spans (the line-level shade covers it).
 */
#[tracing::instrument(level = "trace", ret, skip_all, fields(old_len = old_text.len(), new_len = new_text.len()))]
pub fn word_spans(old_text: &str, new_text: &str) -> WordSpans {
    word_spans_with_profile(old_text, new_text, None)
}

/**
 * Word-level change spans between an old and a new line with active syntax
 * profile.
 */
#[tracing::instrument(level = "trace", ret, skip_all, fields(old_len = old_text.len(), new_len = new_text.len()))]
pub fn word_spans_with_profile(
    old_text: &str,
    new_text: &str,
    profile: Option<&LangProfile>,
) -> WordSpans {
    word_spans_with_profile_and_cap(old_text, new_text, profile, token_product_cap())
}

/**
 * Word-level change spans plus owned merged runs with active syntax profile
 * and default token product cap (see
 * `word_spans_and_runs_with_profile_and_cap`).
 */
pub fn word_spans_and_runs_with_profile(
    old_text: &str,
    new_text: &str,
    profile: Option<&LangProfile>,
) -> (WordSpans, Vec<(RunKind, String)>) {
    word_spans_and_runs_with_profile_and_cap(old_text, new_text, profile, token_product_cap())
}

#[inline]
fn truncate_for_trace(s: &str, max_len: usize) -> &str {
    match s.char_indices().nth(max_len) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/**
 * Word-level change spans between an old and a new line with explicit token
 * product cap.
 */
#[tracing::instrument(
    level = "debug",
    ret,
    skip_all,
    fields(
        old = %truncate_for_trace(old_text, 40),
        new = %truncate_for_trace(new_text, 40),
        strategy = tracing::field::Empty,
        anchors = tracing::field::Empty,
        matched_chars = tracing::field::Empty,
        ratio = tracing::field::Empty
    )
)]
pub fn word_spans_with_profile_and_cap(
    old_text: &str,
    new_text: &str,
    profile: Option<&LangProfile>,
    cap: usize,
) -> WordSpans {
    word_spans_and_runs_with_profile_and_cap(old_text, new_text, profile, cap).0
}

/**
 * Word-level change spans plus the owned merged runs from a single
 * alignment tape build. Render unification: these runs always derive from
 * the same profiled tape as the spans, so render consuming them can never
 * contradict the spans' tape (see `merge::tests` agreement/divergence
 * pins).
 */
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(
        old = %truncate_for_trace(old_text, 40),
        new = %truncate_for_trace(new_text, 40),
        strategy = tracing::field::Empty,
        anchors = tracing::field::Empty,
        matched_chars = tracing::field::Empty,
        ratio = tracing::field::Empty
    )
)]
pub fn word_spans_and_runs_with_profile_and_cap(
    old_text: &str,
    new_text: &str,
    profile: Option<&LangProfile>,
    cap: usize,
) -> (WordSpans, Vec<(RunKind, String)>) {
    let tape = crate::tape::AlignmentTape::from_texts_with_cap(old_text, new_text, profile, cap);
    let res = tape.to_spans();

    crate::ensure!(
        res.del
            .iter()
            .all(|&(start, end)| start <= end && end <= char_len(old_text)),
        "WordSpans del spans must fall within old_text bounds"
    );
    crate::ensure!(
        res.add
            .iter()
            .all(|&(start, end)| start <= end && end <= char_len(new_text)),
        "WordSpans add spans must fall within new_text bounds"
    );
    crate::ensure!(
        res.del.windows(2).all(|w| w[0].1 <= w[1].0),
        "WordSpans del spans must be non-overlapping and sorted"
    );
    crate::ensure!(
        res.add.windows(2).all(|w| w[0].1 <= w[1].0),
        "WordSpans add spans must be non-overlapping and sorted"
    );

    let runs = tape
        .to_merged_runs()
        .iter()
        .map(|r| (r.kind, r.text.to_string()))
        .collect();
    (res, runs)
}

#[cfg(test)]
mod tests;
