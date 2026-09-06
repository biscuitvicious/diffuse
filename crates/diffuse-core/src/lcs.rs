//! One-for-one port of `review.ts`'s word-level diff (PLAN.md §3):
//! `tokenizeLine`, `lcsPairs` (the tie-break ladder), `spansFrom`,
//! `wordSpans`. The comments carry the *why* — ported verbatim.
//!
//! Offset convention: the TS uses UTF-16 code-unit offsets (JS string
//! indices). This port uses **char offsets** (Unicode scalar values) —
//! the same tokens are unmatched, the spans index the same text the
//! renderer receives, so behavior is identical; only the raw offset
//! values differ for non-ASCII lines.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{parse::Spans, syntax::profile::LangProfile};

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
    )
}

const KIND_WORD: u8 = 0;
const KIND_WS: u8 = 1;
const KIND_DELIM: u8 = 2;
const KIND_OP: u8 = 3;

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
                _ => {
                    i += 1;
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_WORD {
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
        } else {
            chars.next();
            while let Some(&(_, next_c)) = chars.peek() {
                if is_ws_char(next_c) || is_delim_char(next_c) || is_op_char(next_c) {
                    break;
                }
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
                _ => {
                    while i < len && ASCII_KIND[bytes[i] as usize] == KIND_WORD {
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
            } else {
                let mut advance = c_len;
                for next_c in chars {
                    if is_ws_char(next_c) || is_delim_char(next_c) || is_op_char(next_c) {
                        break;
                    }
                    advance += next_c.len_utf8();
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

/// The TS `/\s/` test — ASCII whitespace (JS `\s` without the `u` flag),
/// NOT Unicode whitespace. Tokens from `tokenize_line` are single-class, so
/// `any` over the token is equivalent to the JS regex test.
pub fn token_is_ws(t: &str) -> bool {
    t.chars().any(|c| c.is_ascii_whitespace())
}

/// The TS `/[A-Za-z0-9]/` test — any ASCII alphanumeric char.
pub fn token_is_alnum(t: &str) -> bool {
    t.chars().any(|c| c.is_ascii_alphanumeric())
}

/// Computes unambiguous unique token anchors (Patience diff pre-pass).
pub fn unique_token_anchors(
    a: &[&str],
    b: &[&str],
    profile: Option<&LangProfile>,
) -> Vec<(usize, usize)> {
    let mut a_counts: FxHashMap<&str, (usize, usize)> = FxHashMap::default();
    for (i, &t) in a.iter().enumerate() {
        let entry = a_counts.entry(t).or_insert((0, i));
        entry.0 += 1;
    }
    let mut b_counts: FxHashMap<&str, (usize, usize)> = FxHashMap::default();
    for (j, &t) in b.iter().enumerate() {
        let entry = b_counts.entry(t).or_insert((0, j));
        entry.0 += 1;
    }
    let is_anchor = |t: &str| token_is_alnum(t) || profile.is_some_and(|p| p.is_operator_anchor(t));
    let mut common_unique = Vec::new();
    for (&t, &(count_a, idx_a)) in &a_counts {
        if count_a == 1
            && !token_is_ws(t)
            && is_anchor(t)
            && let Some(&(count_b, idx_b)) = b_counts.get(t)
            && count_b == 1
        {
            common_unique.push((idx_a, idx_b));
        }
    }
    if common_unique.is_empty() {
        return Vec::new();
    }
    common_unique.sort_by_key(|&(ai, _)| ai);

    // Compute Longest Increasing Subsequence of b indices
    let mut piles: Vec<usize> = Vec::new();
    let mut pile_indices: Vec<usize> = Vec::new();
    let mut parent = vec![None; common_unique.len()];

    for (idx, &(_, b_idx)) in common_unique.iter().enumerate() {
        let p = piles.partition_point(|&x| x < b_idx);
        if p == piles.len() {
            piles.push(b_idx);
            pile_indices.push(idx);
        } else {
            piles[p] = b_idx;
            pile_indices[p] = idx;
        }
        if p > 0 {
            parent[idx] = Some(pile_indices[p - 1]);
        }
    }

    let mut lis = Vec::new();
    let mut curr = *pile_indices.last().unwrap();
    loop {
        lis.push(common_unique[curr]);
        match parent[curr] {
            Some(prev) => curr = prev,
            None => break,
        }
    }
    lis.reverse();
    lis
}

/// Counts consecutive non-whitespace tokens that match between `a[i..]` and
/// `b[j..]`.  Used to prefer a later occurrence of a repeated token when it
/// starts a longer contiguous shared run.
fn contiguous_non_ws_run(a: &[&str], b: &[&str], mut i: usize, mut j: usize) -> usize {
    let mut len = 0;
    while i < a.len() && j < b.len() && a[i] == b[j] {
        if !token_is_ws(a[i]) {
            len += 1;
        }
        i += 1;
        j += 1;
    }
    len
}

const MAX_AFFINITY_LOOKBACK: usize = 4;

fn contiguous_non_ws_run_backward(
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    mut i: usize,
    mut j: usize,
) -> usize {
    let mut len = 0;
    while i > 0 && j > 0 && a[i - 1] == b[j - 1] && len < MAX_AFFINITY_LOOKBACK {
        if !token_is_ws(a[i - 1]) {
            len += 1;
        }
        i -= 1;
        j -= 1;
    }
    if i == 0 && j == 0 {
        let mut pi = prev_a.len();
        let mut pj = prev_b.len();
        while pi > 0 && pj > 0 && prev_a[pi - 1] == prev_b[pj - 1] && len < MAX_AFFINITY_LOOKBACK {
            if !token_is_ws(prev_a[pi - 1]) {
                len += 1;
            }
            pi -= 1;
            pj -= 1;
        }
    }
    len
}

pub(crate) const MAX_TIE_LOOKAHEAD: usize = 32;

/// Measures the contiguous non-whitespace token cluster length centered at (i,
/// j) along the current backtrace path.
fn contiguous_affinity(
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    i: usize,
    j: usize,
) -> usize {
    let backward = contiguous_non_ws_run_backward(a, b, prev_a, prev_b, i, j);
    let forward = contiguous_non_ws_run(a, b, i, j);
    backward + forward
}

/// Measures the contiguous non-whitespace token cluster length for a candidate
/// alternative match at `(k, j)` where `k > i`, ensuring backward context from
/// `k` does not overlap with tokens already committed prior to `i` or prior to
/// `last_committed_j`.
fn candidate_contiguous_affinity(
    a: &[&str],
    b: &[&str],
    committed_i: usize,
    last_committed_j: usize,
    k: usize,
    j: usize,
) -> usize {
    let mut backward = 0;
    let mut ki = k;
    let mut bj = j;
    while ki > committed_i && bj > last_committed_j && a[ki - 1] == b[bj - 1] {
        if !token_is_ws(a[ki - 1]) {
            backward += 1;
        }
        ki -= 1;
        bj -= 1;
    }
    let forward = contiguous_non_ws_run(a, b, k, j);
    backward + forward
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

/// Core LCS backtrace with preceding and trailing boundary context.
pub fn raw_lcs_pairs_with_context(
    a: &[&str],
    b: &[&str],
    prev_a: &[&str],
    prev_b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
) -> FxHashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return FxHashMap::default();
    }
    let mut dp = vec![0u32; (n + 1) * (m + 1)];
    let idx = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[idx(i, j)] = if a[i] == b[j] {
                dp[idx(i + 1, j + 1)] + 1
            } else {
                dp[idx(i + 1, j)].max(dp[idx(i, j + 1)])
            };
        }
    }
    let mut pairs = FxHashMap::default();
    let mut i = 0;
    let mut j = 0;
    let mut last_committed_i = 0;
    let mut last_committed_j = 0;
    let mut in_add_run = false;
    while i < n && j < m {
        if a[i] == b[j] {
            let next_matches = if i + 1 < n && j + 1 < m {
                a[i + 1] == b[j + 1] && !token_is_ws(a[i + 1])
            } else if i + 1 == n && j + 1 == m && next_a.is_some() && next_b.is_some() {
                next_a == next_b && !token_is_ws(next_a.unwrap())
            } else {
                false
            };
            if token_is_ws(a[i]) && in_add_run && !next_matches {
                j += 1;
                continue;
            }
            if in_add_run
                && dp[idx(i, j + 1)] == dp[idx(i, j)]
                && (j + 1..m).any(|k| b[k] == a[i] && dp[idx(i + 1, k + 1)] + 1 == dp[idx(i, j)])
            {
                j += 1;
                continue;
            }
            // When skipping a[i] is free w.r.t. LCS length, prefer a later
            // occurrence of b[j] in a if it belongs to a strictly longer
            // contiguous run of matching non-ws tokens (taking into
            // account bounded contiguous token affinity along the current
            // path). This prevents a later occurrence with a short
            // forward run from breaking an established, longer
            // ongoing contiguous match, while bounding the
            // candidate search window to MAX_TIE_LOOKAHEAD.
            if !token_is_ws(a[i]) && i + 1 < n && dp[idx(i + 1, j)] == dp[idx(i, j)] {
                let cur_affinity = contiguous_affinity(a, b, prev_a, prev_b, i, j);
                let lookahead_limit = (i + 1 + MAX_TIE_LOOKAHEAD).min(n);
                let has_better = (i + 1..lookahead_limit).any(|k| {
                    a[k] == b[j]
                        && dp[idx(k, j)] == dp[idx(i, j)]
                        && candidate_contiguous_affinity(a, b, i, last_committed_j, k, j)
                            > cur_affinity
                });
                if has_better {
                    i += 1;
                    continue;
                }
            }
            // Symmetrically: when skipping b[j] is free w.r.t. LCS length,
            // prefer a later occurrence of a[i] in b if it belongs to a
            // strictly longer contiguous run of matching non-ws tokens.
            if !token_is_ws(b[j]) && j + 1 < m && dp[idx(i, j + 1)] == dp[idx(i, j)] {
                let cur_affinity = contiguous_affinity(a, b, prev_a, prev_b, i, j);
                let lookahead_limit = (j + 1 + MAX_TIE_LOOKAHEAD).min(m);
                let has_better = (j + 1..lookahead_limit).any(|k| {
                    b[k] == a[i]
                        && dp[idx(i, k)] == dp[idx(i, j)]
                        && candidate_contiguous_affinity(b, a, j, last_committed_i, k, i)
                            > cur_affinity
                });
                if has_better {
                    j += 1;
                    in_add_run = true;
                    continue;
                }
            }
            pairs.insert(i, j);
            last_committed_i = i + 1;
            last_committed_j = j + 1;
            i += 1;
            j += 1;
            in_add_run = false;
        } else if dp[idx(i + 1, j)] > dp[idx(i, j + 1)] {
            i += 1;
        } else if dp[idx(i + 1, j)] < dp[idx(i, j + 1)] {
            j += 1;
            in_add_run = true;
        } else if token_is_ws(a[i]) && !token_is_ws(b[j]) {
            if i + 1 < n && a[i + 1] == b[j] {
                i += 1;
            } else {
                j += 1;
                in_add_run = true;
            }
        } else if token_is_ws(b[j]) && !token_is_ws(a[i]) {
            j += 1;
            in_add_run = true;
        } else if !b[j + 1..].iter().any(|t| *t == a[i])
            || (!token_is_alnum(a[i]) && token_is_alnum(b[j]))
        {
            i += 1;
        } else {
            j += 1;
            in_add_run = true;
        }
    }
    pairs
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

#[inline]
fn is_subword_token(t: &str) -> bool {
    t == "_" || token_is_alnum(t)
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
    for &(ai, bi) in &anchors {
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
        for (k_a, k_b) in (back_a..=ai).zip(back_b..=bi) {
            pairs.insert(k_a, k_b);
        }
        a_prev = ai + 1;
        b_prev = bi + 1;
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
    for &(ai, bi) in anchors {
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
        for (k_a, k_b) in (back_a..=ai).zip(back_b..=bi) {
            pairs.insert(k_a, k_b);
        }
        a_prev = ai + 1;
        b_prev = bi + 1;
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
    scratch: &mut Vec<u32>,
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
    let stride = m + 1;
    let total_len = (n + 1) * stride;
    if scratch.len() < total_len {
        scratch.resize(total_len, 0);
    }
    let dp_ptr = scratch.as_mut_ptr();
    unsafe {
        let mut next_row_ptr = dp_ptr.add(n * stride);
        for col in 0..=m {
            *next_row_ptr.add(col) = 0;
        }
        let mut i = n;
        while i > 0 {
            i -= 1;
            let curr_row_ptr = dp_ptr.add(i * stride);
            *curr_row_ptr.add(m) = 0;
            let ai = *a.get_unchecked(i);
            let mut j = m;
            let mut right = 0;
            while j > 0 {
                j -= 1;
                let bj = *b.get_unchecked(j);
                let val = if ai == bj {
                    *next_row_ptr.add(j + 1) + 1
                } else {
                    let down = *next_row_ptr.add(j);
                    down.max(right)
                };
                *curr_row_ptr.add(j) = val;
                right = val;
            }
            next_row_ptr = curr_row_ptr;
        }
    }
    let dp = &mut scratch[..total_len];
    if dp[0] == 0 {
        return 0;
    }
    let mut matched = 0usize;
    let mut i = 0;
    let mut j = 0;
    let mut in_add_run = false;
    let mut row_curr = 0;
    let mut row_next = stride;
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
                && dp[row_curr + j + 1] == dp[row_curr + j]
                && (j + 1..m).any(|k| b[k] == a[i] && dp[row_next + k + 1] + 1 == dp[row_curr + j])
            {
                j += 1;
                continue;
            }
            matched += b_token_chars[j];
            i += 1;
            j += 1;
            row_curr += stride;
            row_next += stride;
            in_add_run = false;
        } else if dp[row_next + j] > dp[row_curr + j + 1] {
            i += 1;
            row_curr += stride;
            row_next += stride;
        } else if dp[row_next + j] < dp[row_curr + j + 1] {
            j += 1;
            in_add_run = true;
        } else if id_ws[a[i] as usize] && !id_ws[b[j] as usize] {
            if i + 1 < n && a[i + 1] == b[j] {
                i += 1;
                row_curr += stride;
                row_next += stride;
            } else {
                j += 1;
                in_add_run = true;
            }
        } else if id_ws[b[j] as usize] && !id_ws[a[i] as usize] {
            j += 1;
            in_add_run = true;
        } else if !b[j + 1..].contains(&a[i])
            || (!id_alnum[a[i] as usize] && id_alnum[b[j] as usize])
        {
            i += 1;
            row_curr += stride;
            row_next += stride;
        } else {
            j += 1;
            in_add_run = true;
        }
    }
    matched
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
 * Word-level change spans between an old and a new line with explicit token
 * product cap.
 */
#[tracing::instrument(level = "trace", ret, skip_all, fields(old_len = old_text.len(), new_len = new_text.len(), cap = cap))]
pub fn word_spans_with_profile_and_cap(
    old_text: &str,
    new_text: &str,
    profile: Option<&LangProfile>,
    cap: usize,
) -> WordSpans {
    let ot: Vec<&str> = tokenize_line(old_text).collect();
    let nt: Vec<&str> = tokenize_line(new_text).collect();
    // Cap: long lines fall back to whole-line highlighting.
    if ot.len() * nt.len() > cap {
        if old_text == new_text {
            return WordSpans::default();
        }
        return WordSpans {
            del: vec![(0, char_len(old_text))],
            add: vec![(0, char_len(new_text))],
        };
    }
    let anchors = unique_token_anchors(&ot, &nt, profile);
    let has_anchors = !anchors.is_empty();
    let pairs = if has_anchors {
        lcs_pairs_anchored(&ot, &nt, &anchors)
    } else {
        raw_lcs_pairs(&ot, &nt)
    };
    let mut matched_old = FxHashSet::default();
    let mut matched_new = FxHashSet::default();
    for (o, n) in &pairs {
        matched_old.insert(*o);
        matched_new.insert(*n);
    }
    // Mostly-different pair (e.g. one line rewritten as another): the few
    // coincidentally-shared words ("the", "a", "(") would leave gaps in a
    // line that reads as fully changed — treat the whole line as one span.
    // The threshold is low: a rename like `self.resolved_layers` →
    // `self.layers.iter().any(…)` shares the meaningful `self.`+`layers`
    // run (0.28) and must stay INLINE — `self.` + ~~resolved_~~ + `layers`
    // + the addition — not the whole-line blow-up.
    let matched_chars: usize = pairs.values().map(|j| char_len(nt[*j])).sum();

    let ratio = if !new_text.is_empty() {
        matched_chars as f64 / char_len(new_text) as f64
    } else {
        0.0
    };
    let res = if !has_anchors && ratio < 0.25 {
        WordSpans {
            del: vec![(0, char_len(old_text))],
            add: vec![(0, char_len(new_text))],
        }
    } else {
        WordSpans {
            del: spans_from(old_text, &ot, &matched_old),
            add: spans_from(new_text, &nt, &matched_new),
        }
    };

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

    res
}

#[cfg(test)]
mod tests;
