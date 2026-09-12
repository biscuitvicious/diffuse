//! Semantic token classification, information entropy calculation, and
//! multi-token phrase landmark detection for robust alignment anchoring.

use rustc_hash::FxHashMap;

use crate::{
    lcs::{char_len, token_is_ws},
    syntax::profile::LangProfile,
};

#[cfg(test)]
mod tests;

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

/// Minimum self-information threshold for a single token to be considered an
/// anchor.
pub const MIN_SINGLE_TOKEN_ENTROPY: f64 = 1.0;

/// Minimum cumulative self-information threshold for a multi-token phrase
/// landmark.
pub const MIN_PHRASE_ENTROPY: f64 = 2.5;

#[inline]
fn is_delim(t: &str) -> bool {
    t.len() == 1
        && matches!(
            t.as_bytes()[0],
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';'
        )
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
fn is_function_word(t: &str) -> bool {
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
    let b = t.as_bytes();
    let starts_valid = b[0].is_ascii_alphabetic() || b[0] == b'_';
    if !starts_valid {
        return false;
    }
    let all_alphanumeric_or_underscore = t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !all_alphanumeric_or_underscore {
        return false;
    }
    t.contains('_') || t.chars().any(|c| c.is_ascii_uppercase())
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

/// Computes the Shannon self-information score of a single token:
/// $$I(t) = \text{class\_weight}(t.\text{class}) \times \ln(1 +
/// \text{char\_len}(t))$$
pub fn token_self_information(token: &str, profile: Option<&LangProfile>) -> f64 {
    let class = classify_token(token, profile);
    let len = char_len(token);
    class.class_weight() * ((1 + len) as f64).ln()
}

/// Returns true if a token can serve as an anchor point or phrase boundary.
#[inline]
pub fn is_anchor_content_token(t: &str, profile: Option<&LangProfile>) -> bool {
    if token_is_ws(t) || is_delim(t) || is_function_word(t) {
        return false;
    }
    let is_op = profile.is_some_and(|p| p.is_operator_anchor(t));
    let is_alnum = crate::lcs::token_is_alnum(t);
    is_alnum || is_op
}

/// Identifies maximal multi-token phrase landmarks ($k \ge 2$) shared between
/// token slices `a` and `b`. Returns pairs of matching token indices `(a_idx,
/// b_idx)`.
pub fn multi_token_phrase_anchors(
    a: &[&str],
    b: &[&str],
    profile: Option<&LangProfile>,
) -> Vec<(usize, usize)> {
    if a.len() < 2 || b.len() < 2 {
        return Vec::new();
    }

    #[derive(Clone, Copy, Debug)]
    struct PhraseCandidate {
        a_start: usize,
        b_start: usize,
        len: usize,
        entropy: f64,
    }

    let mut candidates = Vec::new();

    for i in 0..a.len() {
        for j in 0..b.len() {
            // Only inspect left-maximal matches
            if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
                continue;
            }
            if a[i] != b[j] {
                continue;
            }

            let mut len = 0;
            while i + len < a.len() && j + len < b.len() && a[i + len] == b[j + len] {
                len += 1;
            }

            // Phrase boundaries must start and end on content tokens so they do
            // not steal delimiters or whitespace from adjacent
            // deletions or additions.
            let mut start = 0;
            while start < len && !is_anchor_content_token(a[i + start], profile) {
                start += 1;
            }
            let mut end = len;
            while end > start && !is_anchor_content_token(a[i + end - 1], profile) {
                end -= 1;
            }

            let trimmed_len = end - start;
            if trimmed_len < 2 {
                continue;
            }

            let a_start = i + start;
            let b_start = j + start;
            if !crate::lcs::tokens_can_match(a, b, &[], &[], None, None, a_start, b_start) {
                continue;
            }
            let phrase_slice = &a[a_start..a_start + trimmed_len];

            // Verify unique occurrence in both token slices
            let count_a = a
                .windows(trimmed_len)
                .filter(|w| *w == phrase_slice)
                .count();
            if count_a != 1 {
                continue;
            }
            let count_b = b
                .windows(trimmed_len)
                .filter(|w| *w == phrase_slice)
                .count();
            if count_b != 1 {
                continue;
            }

            // Verify cumulative information entropy meets landmark threshold
            let mut entropy = 0.0;
            for &tok in phrase_slice {
                entropy += token_self_information(tok, profile);
            }
            if entropy < MIN_PHRASE_ENTROPY {
                continue;
            }

            candidates.push(PhraseCandidate {
                a_start,
                b_start,
                len: trimmed_len,
                entropy,
            });
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // Sort candidates by a_start
    candidates.sort_by_key(|c| c.a_start);

    // Dynamic programming to select the non-overlapping, strictly monotonic
    // sequence of phrases that maximizes cumulative information entropy.
    let n = candidates.len();
    let mut dp: Vec<(f64, Option<usize>)> = Vec::with_capacity(n);

    for k in 0..n {
        let mut best_score = candidates[k].entropy;
        let mut best_prev = None;
        for prev in 0..k {
            if candidates[prev].a_start + candidates[prev].len <= candidates[k].a_start
                && candidates[prev].b_start + candidates[prev].len <= candidates[k].b_start
            {
                let score = dp[prev].0 + candidates[k].entropy;
                if score > best_score {
                    best_score = score;
                    best_prev = Some(prev);
                }
            }
        }
        dp.push((best_score, best_prev));
    }

    let mut best_end = 0;
    let mut max_total = 0.0;
    for (k, &(score, _)) in dp.iter().enumerate() {
        if score > max_total {
            max_total = score;
            best_end = k;
        }
    }

    let mut selected = Vec::new();
    let mut curr = Some(best_end);
    while let Some(idx) = curr {
        selected.push(candidates[idx]);
        curr = dp[idx].1;
    }
    selected.reverse();

    // Emit only content tokens within selected phrases as anchors. Punctuation
    // and whitespace within phrases are matched by LCS between anchor points.
    let mut result = Vec::new();
    for phrase in selected {
        for offset in 0..phrase.len {
            let t = a[phrase.a_start + offset];
            if is_anchor_content_token(t, profile) {
                result.push((phrase.a_start + offset, phrase.b_start + offset));
            }
        }
    }

    result
}

/// Computes entropy-weighted anchors combining multi-token phrase landmarks
/// and high-information single tokens.
pub fn entropy_anchors(
    a: &[&str],
    b: &[&str],
    profile: Option<&LangProfile>,
) -> Vec<(usize, usize)> {
    let mut a_counts: FxHashMap<&str, (usize, usize)> = FxHashMap::default();
    for (i, &t) in a.iter().enumerate() {
        if !token_is_ws(t) && !is_delim(t) {
            let entry = a_counts.entry(t).or_insert((0, i));
            entry.0 += 1;
        }
    }

    let mut b_counts: FxHashMap<&str, (usize, usize)> = FxHashMap::default();
    for (j, &t) in b.iter().enumerate() {
        if !token_is_ws(t) && !is_delim(t) {
            let entry = b_counts.entry(t).or_insert((0, j));
            entry.0 += 1;
        }
    }

    let mut candidates = Vec::new();
    for (&t, &(count_a, idx_a)) in &a_counts {
        if count_a == 1
            && let Some(&(count_b, idx_b)) = b_counts.get(t)
            && count_b == 1
        {
            let is_op = profile.is_some_and(|p| p.is_operator_anchor(t));
            let is_alnum = crate::lcs::token_is_alnum(t);
            if (is_alnum || is_op)
                && !is_function_word(t)
                && crate::lcs::tokens_can_match(a, b, &[], &[], None, None, idx_a, idx_b)
            {
                let info = token_self_information(t, profile);
                if info >= MIN_SINGLE_TOKEN_ENTROPY || is_op {
                    candidates.push((idx_a, idx_b));
                }
            }
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    candidates.sort_by_key(|&(ai, _)| ai);

    // Compute Longest Increasing Subsequence of b indices
    let mut piles: Vec<usize> = Vec::new();
    let mut pile_indices: Vec<usize> = Vec::new();
    let mut parent = vec![None; candidates.len()];

    for (idx, &(_, b_idx)) in candidates.iter().enumerate() {
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
        lis.push(candidates[curr]);
        match parent[curr] {
            Some(prev) => curr = prev,
            None => break,
        }
    }
    lis.reverse();
    lis
}
