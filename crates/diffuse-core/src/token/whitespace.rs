//! Whitespace position classification and alignment rewards.

use crate::lcs::token_is_ws;

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
