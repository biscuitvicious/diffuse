//! One-for-one port of `review.ts`'s `lineSimilarity`/`lineSimilarityParts`
//! (PLAN.md §3). The comments carry the *why* — ported verbatim.

use crate::lcs::{char_len, lcs_pairs, tokenize_line};

/// Shared-char ratio between two lines (for the mod pairing). Whitespace
/// tokens are excluded — an indent-only reflow must not inflate the match
/// (e.g. `None` vs `        }` share only their leading spaces).
pub fn line_similarity(a: &str, b: &str) -> f64 {
    line_similarity_parts(a, b).sim
}

/// Similarity parts: the overall score plus how much of the NEW line's
/// content is matched. The add-fraction distinguishes a fragment pair (the
/// add's content sits entirely inside the del — an old line split into a
/// block) from a growth pair (the del inside the add — `None` →
/// `return None;`), which both score identically on `sim` alone.
pub fn line_similarity_parts(a: &str, b: &str) -> SimilarityParts {
    let content = |toks: Vec<String>| -> Vec<String> {
        toks.into_iter().filter(|t| !t.trim().is_empty()).collect()
    };
    let ot = content(tokenize_line(a));
    let nt = content(tokenize_line(b));
    if ot.is_empty() || nt.is_empty() {
        return SimilarityParts {
            sim: if a.trim() == b.trim() { 1.0 } else { 0.0 },
            add_matched: 0,
            add_total: 0,
        };
    }
    let pairs = lcs_pairs(&ot, &nt);
    let mut matched: usize = 0;
    for j in pairs.values() {
        matched += char_len(&nt[*j]);
    }
    let total = ot
        .iter()
        .map(|t| char_len(t))
        .sum::<usize>()
        .max(nt.iter().map(|t| char_len(t)).sum());
    SimilarityParts {
        sim: if total > 0 {
            matched as f64 / total as f64
        } else {
            0.0
        },
        add_matched: matched,
        add_total: nt.iter().map(|t| char_len(t)).sum(),
    }
}

/// The similarity score plus the add-side fractions.
#[derive(Clone, Copy, Debug, Default)]
pub struct SimilarityParts {
    pub sim: f64,
    pub add_matched: usize,
    pub add_total: usize,
}

/// The content-char count of a line: its tokenized tokens minus the
/// whitespace-only ones (the `content` filter of `line_similarity_parts`).
/// The collapse uses it for the stretch-window length bound — see
/// `collapse::collapse_hunk_rows` (the LCS pairs EQUAL tokens, so the
/// matched chars can never exceed the del's own content chars; a window
/// with more than 4× the del's content can never score ≥ 0.25).
pub fn content_char_len(text: &str) -> usize {
    tokenize_line(text)
        .into_iter()
        .filter(|t| !t.trim().is_empty())
        .map(|t| char_len(&t))
        .sum()
}

/// A line that starts a comment (per-language): `///`, `//`, `/*`, `*`
/// continuations, and `#` for hash-comment languages. A comment run is its
/// own section — the mod's tail stops at one instead of gluing an unrelated
/// doc block onto the changed lines.
pub fn is_comment_line(text: &str, hash_comment: bool) -> bool {
    let t = text.trim();
    // The `///`/`//`/`/*`/`*` markers are language-agnostic; `#` depends on
    // the language profile (hash-comment languages only — never Rust attrs).
    if t.starts_with("/*") || t.starts_with("*") || t.starts_with("///") || t.starts_with("//") {
        return true;
    }
    if hash_comment && t.starts_with('#') {
        return true;
    }
    false
}
