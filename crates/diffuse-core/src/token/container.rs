//! Syntactic container boundaries, quotation state, and nesting depths.

use diffuse_algo::DelimiterClassifier;

use crate::lcs::token_is_ws;

/// Syntactic container enclosing a token (paired bracket, string literal, or
/// code span).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContainerKind {
    Paren,
    Bracket,
    Brace,
    DoubleQuote,
    SingleQuote,
    #[default]
    Backtick,
}

impl ContainerKind {
    #[inline]
    pub fn is_quoted(&self) -> bool {
        matches!(self, Self::DoubleQuote | Self::SingleQuote | Self::Backtick)
    }

    /// Whether two container contexts are structurally compatible for token
    /// matching. Containers of different bracket types (e.g. `()` vs `{}`)
    /// or between quoted strings and unquoted code are incompatible. Code
    /// tokens at top-level (`None`) may match code tokens wrapped in an
    /// unquoted code container (e.g. `Paren`, `Bracket`, `Brace`).
    #[inline]
    pub fn is_compatible_with(a: Option<Self>, b: Option<Self>) -> bool {
        match (a, b) {
            (Some(ca), Some(cb)) => ca == cb,
            (Some(c), None) | (None, Some(c)) => !c.is_quoted(),
            (None, None) => true,
        }
    }

    #[inline]
    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            1 => Some(Self::Paren),
            2 => Some(Self::Bracket),
            3 => Some(Self::Brace),
            10 => Some(Self::DoubleQuote),
            11 => Some(Self::SingleQuote),
            12 => Some(Self::Backtick),
            _ => None,
        }
    }
}

/// Computes the active enclosing container (if any) for each token in a
/// sequence.
pub fn compute_token_containers(tokens: &[&str]) -> Vec<Option<ContainerKind>> {
    diffuse_algo::track_enclosing_containers(tokens, &diffuse_algo::UniversalDelimiterClassifier)
        .into_iter()
        .map(|opt| opt.and_then(ContainerKind::from_id))
        .collect()
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
        if !containers[idx].is_some_and(|c| c.is_quoted()) {
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
pub(crate) fn get_enclosing_caller<'a>(tokens: &[&'a str], idx: usize) -> Option<&'a str> {
    let classifier = diffuse_algo::UniversalDelimiterClassifier;
    let mut depth = 0usize;
    for i in (0..idx).rev() {
        if let Some(diffuse_algo::DelimiterCategory::Close(_)) = classifier.classify(&tokens[i]) {
            depth += 1;
        } else if let Some(diffuse_algo::DelimiterCategory::Open(_)) =
            classifier.classify(&tokens[i])
        {
            if depth == 0 {
                return (0..i)
                    .rev()
                    .find(|&k| !token_is_ws(tokens[k]))
                    .map(|k| tokens[k]);
            }
            depth -= 1;
        }
    }
    None
}
