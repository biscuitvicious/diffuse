//! One-for-one port of `review.ts`'s `lineSimilarity`/`lineSimilarityParts`
//! (PLAN.md §3). The comments carry the *why* — ported verbatim.

use std::mem;

use crate::{
    algo::interner::SymbolTable,
    lcs::{
        char_len, lcs_matched_chars_ids, lcs_pairs, lcs_pairs_ids, token_is_alnum, token_is_ws,
        tokenize_line,
    },
};

/// The similarity score plus the add-side fractions.
#[derive(Clone, Copy, Debug, Default)]
pub struct SimilarityParts {
    pub sim: f64,
    pub add_matched: usize,
    pub add_total: usize,
}

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
    let ot: Vec<&str> = tokenize_line(a).filter(|t| !t.trim().is_empty()).collect();
    let nt: Vec<&str> = tokenize_line(b).filter(|t| !t.trim().is_empty()).collect();

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
        matched += char_len(nt[*j]);
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

/// The content-char count of a line: its tokenized tokens minus the
/// whitespace-only ones (the `content` filter of `line_similarity_parts`).
/// The collapse uses it for the stretch-window length bound — see
/// `collapse::collapse_hunk_rows` (the LCS pairs EQUAL tokens, so the
/// matched chars can never exceed the del's own content chars; a window
/// with more than 4× the del's content can never score ≥ 0.25).
pub fn content_char_len(text: &str) -> usize {
    tokenize_line(text)
        .filter(|t| !t.trim().is_empty())
        .map(char_len)
        .sum()
}

/// The content-filtered tokens + the char sum — the PRECOMPUTED form of a
/// line's similarity inputs. The collapse's pairing compares every del
/// against every candidate add; tokenizing each row ONCE per hunk (instead
/// of once per pair) is the difference between O(dels×adds) tokenizations
/// and O(dels+adds). The tokens are INTERNED (the LCS compares u32 ids, not
/// Strings — the pairing's hot inner loop).
pub struct LineTokens<'a> {
    pub tokens: Vec<&'a str>,
    /// The interned ids (one per content token) — the LCS input.
    pub ids: Vec<u32>,
    /// Each token's char length (the token-presence bound's sums).
    pub token_chars: Vec<usize>,
    pub chars: usize,
    /// The whitespace-trimmed original (the empty-token case's tie-break:
    /// two blank-ish lines score 1.0 when their trims match).
    pub trimmed: &'a str,
}

/// The token interner: assigns one u32 id per distinct token + records the
/// tie-break ladder's per-token flags (ws/alnum). Shared across a hunk's
/// rows, so identical tokens in different lines compare by id.
#[derive(Default)]
pub struct TokenInterner {
    symbols: SymbolTable<&'static str>,
    pub ws: Vec<bool>,
    pub alnum: Vec<bool>,
    pub char_lens: Vec<u32>,
}

impl TokenInterner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.symbols.clear();
        self.ws.clear();
        self.alnum.clear();
        self.char_lens.clear();
    }

    pub(crate) fn intern(&mut self, token: &str) -> u32 {
        let static_token: &'static str = unsafe { mem::transmute(token) };
        if let Some(id) = self.symbols.get_id(&static_token) {
            return id.as_u32();
        }
        let id = self.symbols.intern(static_token);
        self.ws.push(token_is_ws(token));
        self.alnum.push(token_is_alnum(token));
        self.char_lens.push(crate::lcs::char_len(token) as u32);
        id.as_u32()
    }

    /// Tokenize + filter + intern a line once.
    pub fn line_tokens<'a>(&mut self, text: &'a str) -> LineTokens<'a> {
        let mut tokens = Vec::new();
        let mut ids = Vec::new();
        let mut token_chars = Vec::new();
        let mut chars = 0;
        crate::lcs::for_each_token(text, |t| {
            if !t.trim().is_empty() {
                tokens.push(t);
                ids.push(self.intern(t));
                let tc = char_len(t);
                token_chars.push(tc);
                chars += tc;
            }
        });
        LineTokens {
            trimmed: text.trim(),
            tokens,
            ids,
            token_chars,
            chars,
        }
    }
}

/// The similarity over PRETOKENIZED lines (`line_similarity` on cached
/// tokens — identical results, no re-tokenization). `ws`/`alnum` are the
/// interner's per-id flags (the LCS tie-break ladder).
pub fn line_similarity_parts_tokens(
    ot: &LineTokens,
    nt: &LineTokens,
    ws: &[bool],
    alnum: &[bool],
) -> SimilarityParts {
    if ot.tokens.is_empty() || nt.tokens.is_empty() {
        return SimilarityParts {
            sim: if ot.trimmed == nt.trimmed { 1.0 } else { 0.0 },
            add_matched: 0,
            add_total: 0,
        };
    }
    let pairs = lcs_pairs_ids(&ot.ids, &nt.ids, ws, alnum);
    let mut matched: usize = 0;
    for j in pairs.values() {
        matched += char_len(nt.tokens[*j]);
    }
    let total = ot.chars.max(nt.chars);
    SimilarityParts {
        sim: if total > 0 {
            matched as f64 / total as f64
        } else {
            0.0
        },
        add_matched: matched,
        add_total: nt.chars,
    }
}

/// The similarity of a del (`ot`) against a stretch WINDOW whose content
/// tokens are a contiguous slice `nt_ids` of some add-run's concatenated
/// ids (with the matching `nt_token_chars` and the window's total `nt_chars`).
/// Behavior-identical to `line_similarity_parts_tokens(…).sim` — the
/// window scan builds NO per-window `LineTokens`: the run's ids/chars are
/// precomputed once and each window is a slice into them (move the data,
/// don't clone it per window).
#[allow(clippy::too_many_arguments)]
pub fn sim_window(
    ot: &LineTokens,
    nt_ids: &[u32],
    nt_token_chars: &[usize],
    nt_chars: usize,
    nt_trimmed: &str,
    ws: &[bool],
    alnum: &[bool],
    scratch: &mut Vec<u64>,
) -> f64 {
    if ot.tokens.is_empty() || nt_ids.is_empty() {
        return if ot.trimmed == nt_trimmed { 1.0 } else { 0.0 };
    }
    let matched = lcs_matched_chars_ids(&ot.ids, nt_ids, ws, alnum, nt_token_chars, scratch);
    let total = ot.chars.max(nt_chars);
    if total > 0 {
        matched as f64 / total as f64
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests;
