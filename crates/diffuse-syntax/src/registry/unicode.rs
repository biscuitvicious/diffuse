//! Character-class scanning helpers for Unicode text fallback.

use crate::profile::{CharLiteralProfile, LangProfile, SingleQuoteKind};

use super::word::is_word_char;

/// `#[^\n]*` / `//[^\n]*` — to end of line (the char before the next `\n`).
pub(super) fn scan_to_eol(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1 != '\n' {
        j += 1;
    }
    j
}

/// `/*[\s\S]*?*/` — the FIRST `*/` (non-greedy). None if unterminated
/// (the regex requires the closing `*/`; unterminated → no block match).
pub(super) fn scan_block(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 2;
    while j < n {
        if chars[j].1 == '*' && j + 1 < n && chars[j + 1].1 == '/' {
            return Some(j + 2);
        }
        j += 1;
    }
    None
}

/// `r#*"(?:[^"\\]|\\.)*"#*` — the raw string. Opening `#*` is the run before
/// the `"`; trailing `#*` is an independent run after the closing `"`
/// (the regex does NOT tie the counts). None if malformed.
pub(super) fn scan_raw(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1 == '#' {
        j += 1;
    }
    if j >= n || chars[j].1 != '"' {
        return None;
    }
    j += 1;
    while j < n && chars[j].1 != '"' {
        // `\\.` skips one escaped char.
        j += if chars[j].1 == '\\' { 2 } else { 1 };
    }
    if j >= n {
        return None;
    }
    j += 1;
    while j < n && chars[j].1 == '#' {
        j += 1;
    }
    Some(j)
}

/// `"(?:[^"\\n]|\\.)*"` / `'...'` / `` `...` `` — to the closing quote,
/// no raw newline. No close → None (→ caller falls back to `RE_OTHER` for the
/// opening quote char).
pub(super) fn scan_quoted(chars: &[(usize, char)], i: usize, quote: char) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 1;
    while j < n {
        let c = chars[j].1;
        if c == '\\' {
            j += 2;
        } else if c == quote {
            return Some(j + 1);
        } else if c == '\n' {
            return None;
        } else {
            j += 1;
        }
    }
    None
}

pub(super) fn scan_dstr(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '"').unwrap_or(i + 1)
}

pub(super) fn scan_sstr(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '\'').unwrap_or(i + 1)
}

pub(super) fn scan_char_literal(
    text: &str,
    chars: &[(usize, char)],
    i: usize,
    rules: &CharLiteralProfile,
) -> Option<usize> {
    let n = chars.len();
    let mut j = i + 1;
    while j < n {
        let c = chars[j].1;
        if c == '\\' {
            j += 2;
        } else if c == '\'' {
            let start_byte = chars[i + 1].0;
            let end_byte = chars[j].0;
            let inner = &text[start_byte..end_byte];
            if rules.validate_inner(inner) {
                return Some(j + 1);
            }
            return None;
        } else if c == '\n' || c == '\r' {
            return None;
        } else {
            j += 1;
        }
    }
    None
}

pub(super) fn scan_single_quote(
    text: &str,
    chars: &[(usize, char)],
    i: usize,
    profile: &LangProfile,
) -> usize {
    match profile.single_quote {
        SingleQuoteKind::String => scan_sstr(chars, i),
        SingleQuoteKind::Char(rules) => scan_char_literal(text, chars, i, &rules).unwrap_or(i + 1),
        SingleQuoteKind::None => i + 1,
    }
}

pub(super) fn scan_bt(chars: &[(usize, char)], i: usize) -> usize {
    scan_quoted(chars, i, '`').unwrap_or(i + 1)
}

/// `[0-9a-fA-F_]+` then `\b`: trailing word boundary. None if no hex digit
/// or the trailing char is an ascii_word char (the `+` needs ≥1 hex digit, and
/// the `\b` needs an ascii non-word after — a trailing ascii_word char fails
/// the whole hex).
pub(super) fn scan_hex(chars: &[(usize, char)], i: usize) -> Option<usize> {
    let n = chars.len();
    let mut j = i;
    while j < n {
        let c = chars[j].1;
        if c.is_ascii_hexdigit() || c == '_' {
            j += 1;
        } else {
            break;
        }
    }
    if j == i {
        return None; // `+` needs ≥1 hex digit
    }
    // trailing \b: next char non-word (or EOF).
    if j < n && is_word_char(chars[j].1) {
        return None;
    }
    Some(j)
}

/// `\b\d[\d_]*(?:\.[\d_]+)?(?:[eE][+-]?\d+)?\b`. Scan the full greedy num
/// (digits, optional decimal, optional exponent), then verify the trailing
/// `\b`. If it fails, the regex backtracks: drop the exponent, then the
/// decimal, down to the bare digit run — the first drop whose end sits
/// before an ascii non-word char wins. If even the bare digit run fails the
/// trailing `\b` (a word char immediately follows, e.g. `5x`), the whole
/// num fails → the single digit is `.` (`RE_OTHER`).
pub(super) fn scan_num_or_other(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    // bare digit run `\d[\d_]*`
    let mut digits_end = i;
    while digits_end < n {
        let c = chars[digits_end].1;
        if c.is_ascii_digit() || c == '_' {
            digits_end += 1;
        } else {
            break;
        }
    }
    // optional decimal `\.[\d_]+`
    let mut after_dec = digits_end;
    if digits_end < n && chars[digits_end].1 == '.' {
        let mut j = digits_end + 1;
        while j < n && (chars[j].1.is_ascii_digit() || chars[j].1 == '_') {
            j += 1;
        }
        if j > digits_end + 1 {
            after_dec = j;
        }
    }
    // optional exponent `[eE][+-]?\d+`
    let mut after_exp = after_dec;
    if after_dec < n && (chars[after_dec].1 == 'e' || chars[after_dec].1 == 'E') {
        let mut j = after_dec + 1;
        if j < n && (chars[j].1 == '+' || chars[j].1 == '-') {
            j += 1;
        }
        let start_digits = j;
        while j < n && (chars[j].1.is_ascii_digit() || chars[j].1 == '_') {
            j += 1;
        }
        if j > start_digits {
            after_exp = j;
        }
    }
    // Backtrack ladder (longest first): full, without exp, without decimal.
    for &end in &[after_exp, after_dec, digits_end] {
        let tail_ok = end >= n || !is_word_char(chars[end].1);
        if tail_ok {
            return end;
        }
    }
    i + 1 // bare digit run followed by a word char → num fails → RE_OTHER `.`
}

/// Unicode identifier scan: `[\p{Alphabetic}_][\p{Alphabetic}\p{Numeric}_]*`
pub(super) fn scan_ident(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && is_word_char(chars[j].1) {
        j += 1;
    }
    j
}

/// `\s+` (Unicode whitespace, like the regex crate's `\s`).
pub(super) fn scan_ws(chars: &[(usize, char)], i: usize) -> usize {
    let n = chars.len();
    let mut j = i + 1;
    while j < n && chars[j].1.is_whitespace() {
        j += 1;
    }
    j
}
