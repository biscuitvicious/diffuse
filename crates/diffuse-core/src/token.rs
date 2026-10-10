//! Semantic token classification, information entropy calculation, and
//! multi-token phrase landmark detection for robust alignment anchoring.

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use diffuse_syntax::profile::LangProfile;

use crate::lcs::{char_len, token_is_ws};

#[cfg(test)]
mod tests;

/// Position-based classification of whitespace tokens within a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WhitespaceKind {
    /// Leading whitespace (indentation).
    Leading,
    /// Internal whitespace separating tokens within code or prose.
    Internal,
    /// Trailing whitespace at the line boundary.
    Trailing,
}

impl WhitespaceKind {
    /// Semantic token weight used in subword similarity calculations.
    ///
    /// - `Leading`: Indentation depth weighted at ~1 unit per 4-space indent
    ///   level (minimum 1.0).
    /// - `Internal`: Full character weight as an intra-line structural token
    ///   separator.
    /// - `Trailing`: Zero weight as it carries zero semantic value.
    #[inline]
    pub fn token_weight(&self, char_count: usize) -> f64 {
        match self {
            WhitespaceKind::Leading => ((char_count as f64) * 0.25).max(1.0),
            WhitespaceKind::Internal => char_count as f64,
            WhitespaceKind::Trailing => 0.0,
        }
    }

    /// Gotoh sequence alignment match reward.
    ///
    /// - `Leading`: Standard indentation match reward (2).
    /// - `Internal`: Standard whitespace separator match reward (2).
    /// - `Trailing`: Zero reward so trailing whitespace never latches or
    ///   distorts alignment.
    #[inline]
    pub fn match_reward(&self) -> i32 {
        match self {
            WhitespaceKind::Leading => 2,
            WhitespaceKind::Internal => 2,
            WhitespaceKind::Trailing => 0,
        }
    }
}

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

/// Classifies a whitespace token at `idx` in `tokens` into its position-based
/// role: leading (indentation), internal (token separator), or trailing
/// whitespace.
///
/// Precondition: `idx < tokens.len()` and `token_is_ws(tokens[idx])`.
pub fn classify_whitespace(tokens: &[&str], idx: usize) -> WhitespaceKind {
    assert!(
        idx < tokens.len(),
        "token index out of bounds: {} >= {}",
        idx,
        tokens.len()
    );
    assert!(
        token_is_ws(tokens[idx]),
        "expected whitespace token at index {}, got {:?}",
        idx,
        tokens[idx]
    );

    let is_leading = tokens[..idx].iter().all(|t| token_is_ws(t));
    let has_non_ws_after = tokens[idx + 1..].iter().any(|t| !token_is_ws(t));

    if is_leading {
        WhitespaceKind::Leading
    } else if !has_non_ws_after {
        WhitespaceKind::Trailing
    } else {
        WhitespaceKind::Internal
    }
}

/// Returns the `WhitespaceKind` if the token at `idx` is whitespace, or `None`
/// otherwise.
pub fn classify_token_whitespace(tokens: &[&str], idx: usize) -> Option<WhitespaceKind> {
    if idx < tokens.len() && token_is_ws(tokens[idx]) {
        Some(classify_whitespace(tokens, idx))
    } else {
        None
    }
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

/// Computes parenthesis nesting depth for tokens, ignoring parentheses that
/// occur inside string literals or backtick code spans.
pub fn compute_parenthesis_depth(
    tokens: &[&str],
    containers: &[Option<ContainerKind>],
) -> Vec<usize> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut depth = 0usize;
    for (idx, &t) in tokens.iter().enumerate() {
        if containers[idx].is_none() {
            if t == ")" {
                depth = depth.saturating_sub(1);
                result.push(depth);
            } else if t == "(" {
                result.push(depth);
                depth += 1;
            } else {
                result.push(depth);
            }
        } else {
            result.push(depth);
        }
    }
    result
}

#[inline]
fn trim_to_balanced_boundaries(tokens: &[&str]) -> (usize, usize) {
    diffuse_algo::trim_balanced_boundaries(tokens, &diffuse_algo::AsciiDelimiterClassifier)
}

#[inline]
fn is_dangling_prefix_token(t: &str, profile: Option<&LangProfile>) -> bool {
    if token_is_ws(t) {
        return true;
    }
    if matches!(t, ")" | "]" | "}") {
        return true;
    }
    !is_anchor_content_token(t, profile) && !matches!(t, "(" | "[" | "{" | "\"" | "'" | "`")
}

#[inline]
fn is_dangling_suffix_token(t: &str, profile: Option<&LangProfile>) -> bool {
    if token_is_ws(t) {
        return true;
    }
    if matches!(t, "(" | "[" | "{") {
        return true;
    }
    !is_anchor_content_token(t, profile) && !matches!(t, ")" | "]" | "}" | "\"" | "'" | "`")
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

    let a_containers = compute_token_containers(a);
    let b_containers = compute_token_containers(b);
    let a_depths = compute_parenthesis_depth(a, &a_containers);
    let b_depths = compute_parenthesis_depth(b, &b_containers);

    #[derive(Clone, Copy, Debug)]
    struct PhraseCandidate {
        a_start: usize,
        b_start: usize,
        len: usize,
        entropy: f64,
    }

    let mut candidates = Vec::new();

    // Inverted index of token occurrences in B to prune non-matching (i, j)
    // pairs
    let mut b_pos: FxHashMap<&str, SmallVec<[usize; 4]>> = FxHashMap::default();
    for (j, &tok) in b.iter().enumerate() {
        b_pos.entry(tok).or_default().push(j);
    }

    // Inverted index of token occurrences in A to fast-check phrase uniqueness
    let mut a_pos: FxHashMap<&str, SmallVec<[usize; 4]>> = FxHashMap::default();
    for (i, &tok) in a.iter().enumerate() {
        a_pos.entry(tok).or_default().push(i);
    }

    for i in 0..a.len() {
        let tok = a[i];
        let Some(matching_js) = b_pos.get(tok) else {
            continue;
        };

        for &j in matching_js {
            // Only inspect left-maximal matches
            if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
                continue;
            }

            let mut len = 0;
            while i + len < a.len() && j + len < b.len() && a[i + len] == b[j + len] {
                len += 1;
            }

            let (rel_start, rel_end) = trim_to_balanced_boundaries(&a[i..i + len]);
            let start = rel_start;
            let end = rel_end;

            let trimmed_len = end - start;
            if trimmed_len < 2 {
                continue;
            }

            let has_content = (start..end).any(|idx| is_anchor_content_token(a[i + idx], profile));
            if !has_content {
                continue;
            }

            let mut a_start = i + start;
            let mut b_start = j + start;
            let mut a_end = a_start + trimmed_len;
            let mut b_end = b_start + trimmed_len;

            // Landmark boundary invariant: A phrase landmark must begin and end on
            // valid syntactic phrase boundaries (alphanumerics, balanced container
            // delimiters, or quotes), trimming dangling operators, separators, or
            // whitespace.
            while a_start < a_end && is_dangling_prefix_token(a[a_start], profile) {
                a_start += 1;
                b_start += 1;
            }
            while a_end > a_start && is_dangling_suffix_token(a[a_end - 1], profile) {
                a_end -= 1;
                b_end -= 1;
            }

            let trimmed_len = a_end - a_start;
            if trimmed_len < 2 {
                continue;
            }

            // Invariant 2: A phrase that starts inside a syntactic container
            // (e.g. string literal or backtick code span) must not cross out
            // of that container into the outer scope or adjacent containers.
            if let Some(c_a) = a_containers[a_start]
                && a_containers[a_start..a_end].iter().any(|&c| c != Some(c_a))
            {
                continue;
            }
            if let Some(c_b) = b_containers[b_start]
                && b_containers[b_start..b_end].iter().any(|&c| c != Some(c_b))
            {
                continue;
            }

            let starts_inside_word = (a_start > 0
                && crate::lcs::is_subword_token(a[a_start - 1])
                && crate::lcs::is_subword_token(a[a_start]))
                || (b_start > 0
                    && crate::lcs::is_subword_token(b[b_start - 1])
                    && crate::lcs::is_subword_token(b[b_start]));
            let ends_inside_word = (a_end < a.len()
                && crate::lcs::is_subword_token(a[a_end - 1])
                && crate::lcs::is_subword_token(a[a_end]))
                || (b_end < b.len()
                    && crate::lcs::is_subword_token(b[b_end - 1])
                    && crate::lcs::is_subword_token(b[b_end]));

            if starts_inside_word || ends_inside_word {
                continue;
            }

            if !crate::lcs::tokens_can_match(a, b, &[], &[], None, None, a_start, b_start) {
                continue;
            }
            let phrase_slice = &a[a_start..a_start + trimmed_len];

            // Verify unique occurrence in both token slices.
            // A phrase cannot occur at any window where its least frequent
            // token is not present.
            let min_a_idx = (0..trimmed_len)
                .min_by_key(|&idx| a_pos.get(phrase_slice[idx]).map_or(0, |p| p.len()))
                .unwrap_or(0);
            let count_a = {
                let tok = phrase_slice[min_a_idx];
                let positions = &a_pos[tok];
                if positions.len() == 1 {
                    1
                } else {
                    let mut c = 0;
                    for &pos in positions {
                        if pos >= min_a_idx && pos + trimmed_len - min_a_idx <= a.len() {
                            let start = pos - min_a_idx;
                            if &a[start..start + trimmed_len] == phrase_slice {
                                c += 1;
                                if c > 1 {
                                    break;
                                }
                            }
                        }
                    }
                    c
                }
            };
            if count_a != 1 {
                continue;
            }

            let min_b_idx = (0..trimmed_len)
                .min_by_key(|&idx| b_pos.get(phrase_slice[idx]).map_or(0, |p| p.len()))
                .unwrap_or(0);
            let (total_b, depth_b) = {
                let tok = phrase_slice[min_b_idx];
                let positions = &b_pos[tok];
                let mut total = 0;
                let mut depth_count = 0;
                for &pos in positions {
                    if pos >= min_b_idx && pos + trimmed_len - min_b_idx <= b.len() {
                        let start = pos - min_b_idx;
                        if &b[start..start + trimmed_len] == phrase_slice {
                            total += 1;
                            if b_depths[start] == a_depths[a_start] {
                                depth_count += 1;
                            }
                        }
                    }
                }
                (total, depth_count)
            };

            let is_unique =
                total_b == 1 || (a_depths[a_start] == b_depths[b_start] && depth_b == 1);
            if !is_unique {
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

    let selected = diffuse_algo::find_monotonic_chain(
        &candidates,
        |c| diffuse_algo::MatchInterval::new(c.a_start, c.b_start, c.len),
        |c| c.entropy,
    );

    // Emit all non-whitespace tokens within selected phrases as anchors.
    let mut result = Vec::new();
    for phrase in selected {
        for offset in 0..phrase.len {
            let t = a[phrase.a_start + offset];
            if !token_is_ws(t) {
                result.push((phrase.a_start + offset, phrase.b_start + offset));
            }
        }
    }

    result
}

/// Syntactic container enclosing a token (string literal or code span).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContainerKind {
    #[default]
    Backtick,
    DoubleQuote,
}

/// Computes the active enclosing container (if any) for each token in a
/// sequence.
pub fn compute_token_containers(tokens: &[&str]) -> Vec<Option<ContainerKind>> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut stack: diffuse_algo::SmallStack<ContainerKind, 8> = diffuse_algo::SmallStack::new();

    for &t in tokens {
        match t {
            "`" => {
                if stack.last() == Some(&ContainerKind::Backtick) {
                    let top = stack.pop();
                    result.push(top);
                } else {
                    stack.push(ContainerKind::Backtick);
                    result.push(stack.last().copied());
                }
            }
            "\"" => {
                if stack.last() == Some(&ContainerKind::DoubleQuote) {
                    let top = stack.pop();
                    result.push(top);
                } else {
                    stack.push(ContainerKind::DoubleQuote);
                    result.push(stack.last().copied());
                }
            }
            _ => {
                result.push(stack.last().copied());
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
    let phrase_anchors = multi_token_phrase_anchors(a, b, profile);
    let a_containers = compute_token_containers(a);
    let b_containers = compute_token_containers(b);

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

    let is_compatible_with_phrases = |cand_a: usize, cand_b: usize| -> bool {
        for &(pa, pb) in &phrase_anchors {
            if cand_a == pa || cand_b == pb {
                return false;
            }
            if (cand_a < pa && cand_b > pb) || (cand_a > pa && cand_b < pb) {
                return false;
            }
        }
        true
    };

    let mut candidates = Vec::with_capacity(phrase_anchors.len() + 16);
    candidates.extend_from_slice(&phrase_anchors);

    for (&t, &(count_a, idx_a)) in &a_counts {
        if count_a == 1
            && let Some(&(count_b, idx_b)) = b_counts.get(t)
            && count_b == 1
        {
            let is_op = profile.is_some_and(|p| p.is_operator_anchor(t));
            let is_alnum = crate::lcs::token_is_alnum(t);
            if (is_alnum || is_op)
                && !is_function_word(t)
                && is_compatible_with_phrases(idx_a, idx_b)
                && a_containers[idx_a] == b_containers[idx_b]
            {
                if is_alnum {
                    let left_sub_a = idx_a > 0 && crate::lcs::is_subword_token(a[idx_a - 1]);
                    let left_sub_b = idx_b > 0 && crate::lcs::is_subword_token(b[idx_b - 1]);
                    let right_sub_a =
                        idx_a + 1 < a.len() && crate::lcs::is_subword_token(a[idx_a + 1]);
                    let right_sub_b =
                        idx_b + 1 < b.len() && crate::lcs::is_subword_token(b[idx_b + 1]);
                    if left_sub_a || left_sub_b || right_sub_a || right_sub_b {
                        continue;
                    }
                }
                if crate::lcs::tokens_can_match(a, b, &[], &[], None, None, idx_a, idx_b) {
                    let info = token_self_information(t, profile);
                    if info >= MIN_SINGLE_TOKEN_ENTROPY || is_op {
                        candidates.push((idx_a, idx_b));
                    }
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
