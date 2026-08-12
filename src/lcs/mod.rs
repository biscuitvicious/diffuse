//! One-for-one port of `review.ts`'s word-level diff (PLAN.md §3):
//! `tokenizeLine`, `lcsPairs` (the tie-break ladder), `spansFrom`,
//! `wordSpans`. The comments carry the *why* — ported verbatim.
//!
//! Offset convention: the TS uses UTF-16 code-unit offsets (JS string
//! indices). This port uses **char offsets** (Unicode scalar values) —
//! the same tokens are unmatched, the spans index the same text the
//! renderer receives, so behavior is identical; only the raw offset
//! values differ for non-ASCII lines.

use regex::Regex;
use std::sync::OnceLock;

/// Slice `s` by char offsets (the port's span convention).
pub fn char_slice(s: &str, start: usize, end: usize) -> &str {
    let mut it = s.char_indices();
    let start_byte = it.nth(start).map_or(s.len(), |(i, _)| i);
    if end <= start + 1 {
        // A 1-char span: the char at `start` was consumed by `nth(start)`;
        // its UTF-8 length ends the slice.
        let c = s[start_byte..].chars().next().unwrap_or('\0');
        return &s[start_byte..start_byte + c.len_utf8()];
    }
    // After `nth(start)` the iterator is at index `start + 1`; `nth(k)`
    // yields index `start + 1 + k` — want `end - 1`, so k = end - start - 2.
    // The end byte is the char's START byte plus its UTF-8 length (a
    // multi-byte char must not leave a non-boundary end).
    let end_byte = match it.nth(end - start - 2) {
        Some((i, c)) => i + c.len_utf8(),
        None => s.len(),
    };
    &s[start_byte..end_byte]
}

/// Number of chars in `s` (the port's span unit).
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/**
 * Tokenize a diff line into word/operator tokens (whitespace + punctuation
 * are separate tokens so `->` → `=>` highlights a single character).
 */
pub fn tokenize_line(text: &str) -> Vec<String> {
    // Two punctuation classes. OPERATORS glue (`->`, `=>`, `::`, `..`, `==`,
    // `//` …) — an operator change is one span, not per-char noise. The
    // DELIMITERS are single-char tokens: gluing `).` or `),` would hide the
    // shared `)` from the LCS, so `…anchors).` → `…anchors),` would strike
    // the whole `).` instead of just the period. The backtick is a delimiter
    // too — markdown code markers must not glue onto identifiers. The quote
    // `"` is a delimiter as well: `lower layer"` → `layer below the top
    // upper"` must match the del's `layer` to the add's `layer` and its `"`
    // to the add's `"` — glued, the del's `layer"` matches nothing and the
    // string renders as `layer"layer` (the trace wobble).
    // Unicode whitespace by character class: the White_Space property
    // covers every standard whitespace (space separators, tabs, NEL,
    // U+1680, U+2000-200A, U+2028/29, U+202F, U+205F, U+3000), plus the
    // zero-width space (U+200B — a Cf format char, but a real empty-width
    // whitespace difference vs a space, and it must subword, not glue onto
    // the following identifier).
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"([\p{White_Space}\u200b]+|[<>!=+\-*\/\\|&^%~?:._]+|[()\[\]{};,@#`"])"#)
            .expect("tokenizer regex")
    });
    // JS `split` with a single capturing group interleaves the separators as
    // tokens; `regex`'s split ignores captures, so walk `find_iter` instead.
    let mut out = Vec::new();
    let mut last = 0;
    for m in re.find_iter(text) {
        if m.start() > last {
            out.push(text[last..m.start()].to_string());
        }
        out.push(m.as_str().to_string());
        last = m.end();
    }
    if last < text.len() {
        out.push(text[last..].to_string());
    }
    out.retain(|t| !t.is_empty());
    out
}

/// The TS `/\s/` test — ASCII whitespace (JS `\s` without the `u` flag),
/// NOT Unicode whitespace. Tokens from `tokenize_line` are single-class, so
/// `any` over the token is equivalent to the JS regex test.
fn token_is_ws(t: &str) -> bool {
    t.chars().any(|c| c.is_ascii_whitespace())
}

/// The TS `/[A-Za-z0-9]/` test — any ASCII alphanumeric char.
fn token_is_alnum(t: &str) -> bool {
    t.chars().any(|c| c.is_ascii_alphanumeric())
}

/// LCS backtrace: map of old-token index → matched new-token index.
pub fn lcs_pairs(a: &[String], b: &[String]) -> std::collections::HashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return std::collections::HashMap::new();
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
    let mut pairs = std::collections::HashMap::new();
    let mut i = 0;
    let mut j = 0;
    while i < n && j < m {
        if a[i] == b[j] {
            pairs.insert(i, j);
            i += 1;
            j += 1;
        } else if dp[idx(i + 1, j)] > dp[idx(i, j + 1)] {
            i += 1;
        } else if dp[idx(i + 1, j)] < dp[idx(i, j + 1)] {
            j += 1;
        } else if token_is_ws(&a[i]) && !token_is_ws(&b[j]) {
            // A space on the del side: when the del's NEXT token pairs with
            // the add's word, the space yields — the word pair survives
            // (`"Found in lower layer"` → `"Found in layer below …"` keeps
            // the old `layer` matched to the new `layer` instead of the old
            // ` ` pairing ahead of it and the `layer` striking alone).
            if i + 1 < n && a[i + 1] == b[j] {
                i += 1;
            } else {
                j += 1; // otherwise the add may pair later
            }
        } else if token_is_ws(&b[j]) && !token_is_ws(&a[i]) {
            j += 1; // a space on the add side yields: keep the del — a WORD del
        // must survive to pair with its counterpart further along
        // (`k` survives the intervening `let Some(floor) = floor`
        // span to pair with the later `k`), never be skipped for a space
        } else if !b[j + 1..].iter().any(|t| *t == a[i]) {
            i += 1; // the del token has no counterpart anywhere later in the
        // add (`resolved` in `resolved_layers` → `layers`) — dropping it
        // lets the following `layers` pair with the add's `layers`
        // (the meaningful `self.` + `layers` run survives, the
        // coincidental `_`-of-`sub_path` tie-break does not steal it)
        } else if !token_is_alnum(&a[i]) && token_is_alnum(&b[j]) {
            i += 1; // the del's punctuation (`_` after `resolved`) has no word
        // counterpart at this spot — a later word (`layers`) pairs
        // with the add's `layers`, not with a stray underscore
        } else {
            j += 1; // both meaningful: keep the del — it pairs with its own
            // counterpart later, or becomes a standalone del span
        }
    }
    pairs
}

/// Char-offset spans of the UNMATCHED (changed) tokens in `toks`.
pub fn spans_from(
    text: &str,
    toks: &[String],
    matched: &std::collections::HashSet<usize>,
) -> Vec<(usize, usize)> {
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
/// row's span type (parse::Spans) — one type for the whole pipeline.
pub type WordSpans = crate::parse::Spans;

/**
 * Word-level change spans between an old and a new line. Both sides get the
 * runs of tokens that have no LCS counterpart; a line with no counterpart
 * (pure add/del) yields no spans (the line-level shade covers it).
 */
pub fn word_spans(old_text: &str, new_text: &str) -> WordSpans {
    let ot = tokenize_line(old_text);
    let nt = tokenize_line(new_text);
    // Cap: long lines fall back to whole-line highlighting.
    if ot.len() * nt.len() > 40_000 {
        return WordSpans::default();
    }
    let pairs = lcs_pairs(&ot, &nt);
    let mut matched_old = std::collections::HashSet::new();
    let mut matched_new = std::collections::HashSet::new();
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
    let matched_chars: usize = pairs.values().map(|j| char_len(&nt[*j])).sum();
    let ratio = if !new_text.is_empty() {
        matched_chars as f64 / char_len(new_text) as f64
    } else {
        0.0
    };
    if ratio < 0.25 {
        return WordSpans {
            del: vec![(0, char_len(old_text))],
            add: vec![(0, char_len(new_text))],
        };
    }
    WordSpans {
        del: spans_from(old_text, &ot, &matched_old),
        add: spans_from(new_text, &nt, &matched_new),
    }
}

#[cfg(test)]
mod tests;
