//! Hand-rolled token-span scanner (performance-critical path).

use crate::profile::{CharLiteralProfile, LangProfile, SingleQuoteKind};

use super::unicode::{
    scan_block, scan_bt, scan_dstr, scan_hex, scan_ident, scan_num_or_other, scan_raw,
    scan_single_quote, scan_to_eol, scan_ws,
};

/// The token spans of the syntax regex's `find_iter` — a hand-rolled
/// char-class scanner that produces the SAME byte-offset spans as
/// `token_re_for(profile).find_iter(text)`, in the same alternation order
/// (first match wins). The regex crate takes the SLOW NFA path on this
/// alternation (the `\b` word boundaries + non-greedy block comments +
/// ~12 alternatives defeat the DFA) — ~47µs/call, the whole classify cost.
/// This scanner is ~ns/char: a ~50× cut. The color logic in
/// `classify_tokens` runs unchanged on these spans (behavior-identical,
/// golden-gated).
/// Run the hand-rolled scanner producing token spans for syntax coloring.
pub fn for_each_syntax_token_span(
    text: &str,
    profile: &LangProfile,
    mut f: impl FnMut(usize, usize),
) {
    if text.is_ascii() {
        let bytes = text.as_bytes();
        let n = bytes.len();
        let mut i = 0;
        while i < n {
            let start_off = i;
            let b = bytes[i];
            let next = if (profile.hash_comment && b == b'#')
                || (profile.line_comment && b == b'/' && i + 1 < n && bytes[i + 1] == b'/')
            {
                scan_to_eol_bytes(bytes, i)
            } else if profile.block_comment && b == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
                scan_block_bytes(bytes, i).unwrap_or(i + 1)
            } else if b == b'r' {
                scan_raw_bytes(bytes, i).unwrap_or_else(|| scan_ident_bytes(bytes, i))
            } else if b == b'"' {
                scan_dstr_bytes(bytes, i)
            } else if b == b'\'' {
                scan_single_quote_bytes(bytes, i, profile)
            } else if b == b'`' {
                scan_bt_bytes(bytes, i)
            } else if b == b'0' && i + 1 < n && bytes[i + 1] == b'x' {
                scan_hex_bytes(bytes, i + 2).unwrap_or_else(|| scan_num_or_other_bytes(bytes, i))
            } else if b.is_ascii_digit() {
                scan_num_or_other_bytes(bytes, i)
            } else if b.is_ascii_alphabetic() || b == b'_' {
                scan_ident_bytes(bytes, i)
            } else if b.is_ascii_whitespace() {
                scan_ws_bytes(bytes, i)
            } else {
                i + 1
            };
            f(start_off, next);
            i = next;
        }
    } else {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let n = chars.len();
        let end_off = |i: usize| if i < n { chars[i].0 } else { text.len() };
        let mut i = 0;
        while i < n {
            let start_off = chars[i].0;
            let c = chars[i].1;
            let next = if (profile.hash_comment && c == '#')
                || (profile.line_comment && c == '/' && i + 1 < n && chars[i + 1].1 == '/')
            {
                scan_to_eol(&chars, i)
            } else if profile.block_comment && c == '/' && i + 1 < n && chars[i + 1].1 == '*' {
                scan_block(&chars, i).unwrap_or(i + 1)
            } else if c == 'r' {
                scan_raw(&chars, i).unwrap_or_else(|| scan_ident(&chars, i))
            } else if c == '"' {
                scan_dstr(&chars, i)
            } else if c == '\'' {
                scan_single_quote(text, &chars, i, profile)
            } else if c == '`' {
                scan_bt(&chars, i)
            } else if c == '0' && i + 1 < n && chars[i + 1].1 == 'x' {
                scan_hex(&chars, i + 2).unwrap_or_else(|| scan_num_or_other(&chars, i))
            } else if c.is_ascii_digit() {
                scan_num_or_other(&chars, i)
            } else if c.is_alphabetic() || c == '_' {
                scan_ident(&chars, i)
            } else if c.is_whitespace() {
                scan_ws(&chars, i)
            } else {
                i + 1
            };
            f(start_off, end_off(next));
            i = next;
        }
    }
}

#[allow(dead_code)]
pub fn syntax_token_spans(text: &str, profile: &LangProfile) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for_each_syntax_token_span(text, profile, |s, e| out.push((s, e)));
    out
}

#[inline(always)]
fn scan_to_eol_bytes(bytes: &[u8], i: usize) -> usize {
    let n = bytes.len();
    let mut j = i + 1;
    while j < n && bytes[j] != b'\n' {
        j += 1;
    }
    j
}

#[inline(always)]
fn scan_block_bytes(bytes: &[u8], i: usize) -> Option<usize> {
    let n = bytes.len();
    let mut j = i + 2;
    while j < n {
        if bytes[j] == b'*' && j + 1 < n && bytes[j + 1] == b'/' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

#[inline(always)]
fn scan_raw_bytes(bytes: &[u8], i: usize) -> Option<usize> {
    let n = bytes.len();
    let mut j = i + 1;
    while j < n && bytes[j] == b'#' {
        j += 1;
    }
    if j >= n || bytes[j] != b'"' {
        return None;
    }
    j += 1;
    while j < n && bytes[j] != b'"' {
        j += if bytes[j] == b'\\' { 2 } else { 1 };
    }
    if j >= n {
        return None;
    }
    j += 1;
    while j < n && bytes[j] == b'#' {
        j += 1;
    }
    Some(j)
}

#[inline(always)]
fn scan_quoted_bytes(bytes: &[u8], i: usize, quote: u8) -> Option<usize> {
    let n = bytes.len();
    let mut j = i + 1;
    while j < n {
        let b = bytes[j];
        if b == b'\\' {
            j += 2;
        } else if b == quote {
            return Some(j + 1);
        } else if b == b'\n' {
            return None;
        } else {
            j += 1;
        }
    }
    None
}

#[inline(always)]
fn scan_dstr_bytes(bytes: &[u8], i: usize) -> usize {
    scan_quoted_bytes(bytes, i, b'"').unwrap_or(i + 1)
}

#[inline(always)]
fn scan_sstr_bytes(bytes: &[u8], i: usize) -> usize {
    scan_quoted_bytes(bytes, i, b'\'').unwrap_or(i + 1)
}

#[inline(always)]
fn scan_char_literal_bytes(bytes: &[u8], i: usize, rules: &CharLiteralProfile) -> Option<usize> {
    let n = bytes.len();
    let mut j = i + 1;
    while j < n {
        let b = bytes[j];
        if b == b'\\' {
            j += 2;
        } else if b == b'\'' {
            let inner_bytes = &bytes[i + 1..j];
            if let Ok(inner) = std::str::from_utf8(inner_bytes)
                && rules.validate_inner(inner)
            {
                return Some(j + 1);
            }
            return None;
        } else if b == b'\n' || b == b'\r' {
            return None;
        } else {
            j += 1;
        }
    }
    None
}

#[inline(always)]
fn scan_single_quote_bytes(bytes: &[u8], i: usize, profile: &LangProfile) -> usize {
    match profile.single_quote {
        SingleQuoteKind::String => scan_sstr_bytes(bytes, i),
        SingleQuoteKind::Char(rules) => scan_char_literal_bytes(bytes, i, &rules).unwrap_or(i + 1),
        SingleQuoteKind::None => i + 1,
    }
}

#[inline(always)]
fn scan_bt_bytes(bytes: &[u8], i: usize) -> usize {
    scan_quoted_bytes(bytes, i, b'`').unwrap_or(i + 1)
}

#[inline(always)]
fn scan_hex_bytes(bytes: &[u8], i: usize) -> Option<usize> {
    let n = bytes.len();
    let mut j = i;
    while j < n {
        let b = bytes[j];
        if b.is_ascii_hexdigit() || b == b'_' {
            j += 1;
        } else {
            break;
        }
    }
    if j == i {
        return None;
    }
    if j < n && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        return None;
    }
    Some(j)
}

#[inline(always)]
fn scan_num_or_other_bytes(bytes: &[u8], i: usize) -> usize {
    let n = bytes.len();
    let mut digits_end = i;
    while digits_end < n {
        let b = bytes[digits_end];
        if b.is_ascii_digit() || b == b'_' {
            digits_end += 1;
        } else {
            break;
        }
    }
    let mut after_dec = digits_end;
    if digits_end < n && bytes[digits_end] == b'.' {
        let mut j = digits_end + 1;
        while j < n && (bytes[j].is_ascii_digit() || bytes[j] == b'_') {
            j += 1;
        }
        if j > digits_end + 1 {
            after_dec = j;
        }
    }
    let mut after_exp = after_dec;
    if after_dec < n && (bytes[after_dec] == b'e' || bytes[after_dec] == b'E') {
        let mut j = after_dec + 1;
        if j < n && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let start_digits = j;
        while j < n && (bytes[j].is_ascii_digit() || bytes[j] == b'_') {
            j += 1;
        }
        if j > start_digits {
            after_exp = j;
        }
    }
    for &end in &[after_exp, after_dec, digits_end] {
        let tail_ok = end >= n || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if tail_ok {
            return end;
        }
    }
    i + 1
}

#[inline(always)]
fn scan_ident_bytes(bytes: &[u8], i: usize) -> usize {
    let n = bytes.len();
    let mut j = i + 1;
    while j < n && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        j += 1;
    }
    j
}

#[inline(always)]
fn scan_ws_bytes(bytes: &[u8], i: usize) -> usize {
    let n = bytes.len();
    let mut j = i + 1;
    while j < n && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    j
}
