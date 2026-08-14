//! One-for-one port of `review.ts`'s word-level diff (PLAN.md §3):
//! `tokenizeLine`, `lcsPairs` (the tie-break ladder), `spansFrom`,
//! `wordSpans`. The comments carry the *why* — ported verbatim.
//!
//! Offset convention: the TS uses UTF-16 code-unit offsets (JS string
//! indices). This port uses **char offsets** (Unicode scalar values) —
//! the same tokens are unmatched, the spans index the same text the
//! renderer receives, so behavior is identical; only the raw offset
//! values differ for non-ASCII lines.

use crate::parse::Spans;
use regex::Regex;
use std::sync::OnceLock;

/// Slice `s` by char offsets (the port's span convention). The JS
/// `String.slice(start, end)` clamps out-of-range indices and returns an
/// empty slice for `start >= end` — the port must be total too: a
/// whole-line gate or an empty del/add line yields empty spans `(0,0)`,
/// and `wholesalePair` slices them (an empty del line is a valid row).
pub fn char_slice(s: &str, start: usize, end: usize) -> &str {
    let len = s.chars().count();
    let start = start.min(len);
    let end = end.min(len).max(start);
    let start_byte = s.char_indices().nth(start).map_or(s.len(), |(i, _)| i);
    if end == start {
        return &s[start_byte..start_byte];
    }
    // The end byte is the char at `end - 1`'s START byte plus its UTF-8
    // length (a multi-byte char must not leave a non-boundary end).
    let end_byte = match s.char_indices().nth(end - 1) {
        Some((i, c)) => i + c.len_utf8(),
        None => s.len(),
    };
    &s[start_byte..end_byte]
}

/// Number of chars in `s` (the port's span unit).
pub fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Tokenize a diff line into word/operator tokens (whitespace + punctuation
/// are separate tokens so `->` → `=>` highlights a single character). Zero-copy slices.
pub fn tokenize_line(text: &str) -> Vec<&str> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"([\p{White_Space}\u200b]+|[<>!=+\-*\/\\|&^%~?:._]+|[()\[\]{};,@#`"])"#)
            .expect("tokenizer regex")
    });
    let mut out = Vec::new();
    let mut last = 0;
    for m in re.find_iter(text) {
        if m.start() > last {
            let seg = &text[last..m.start()];
            if !seg.is_empty() {
                out.push(seg);
            }
        }
        let seg = m.as_str();
        if !seg.is_empty() {
            out.push(seg);
        }
        last = m.end();
    }
    if last < text.len() {
        let seg = &text[last..];
        if !seg.is_empty() {
            out.push(seg);
        }
    }
    out
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

/// LCS backtrace: map of old-token index → matched new-token index.
pub fn lcs_pairs(a: &[&str], b: &[&str]) -> std::collections::HashMap<usize, usize> {
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
        } else if token_is_ws(a[i]) && !token_is_ws(b[j]) {
            if i + 1 < n && a[i + 1] == b[j] {
                i += 1;
            } else {
                j += 1;
            }
        } else if token_is_ws(b[j]) && !token_is_ws(a[i]) {
            j += 1;
        } else if !b[j + 1..].iter().any(|t| *t == a[i])
            || (!token_is_alnum(a[i]) && token_is_alnum(b[j]))
        {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

/// The LCS over INTERNED token ids — the pairing's hot inner loop (D×A
/// runs per hunk). The String equality becomes a u32 compare; the tie-break
/// ladder's `token_is_ws`/`token_is_alnum` become array lookups (the
/// interner assigns one flag per id). Behavior-identical to `lcs_pairs`.
pub fn lcs_pairs_ids(
    a: &[u32],
    b: &[u32],
    id_ws: &[bool],
    id_alnum: &[bool],
) -> std::collections::HashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return std::collections::HashMap::new();
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
    let mut pairs = std::collections::HashMap::new();
    let mut i = 0;
    let mut j = 0;
    while i < n && j < m {
        if a[i] == b[j] {
            pairs.insert(i, j);
            i += 1;
            j += 1;
        } else if dp[(i + 1) * (m + 1) + j] > dp[i * (m + 1) + j + 1] {
            i += 1;
        } else if dp[(i + 1) * (m + 1) + j] < dp[i * (m + 1) + j + 1] {
            j += 1;
        } else if id_ws[a[i] as usize] && !id_ws[b[j] as usize] {
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
        } else if id_ws[b[j] as usize] && !id_ws[a[i] as usize] {
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
        } else if !id_alnum[a[i] as usize] && id_alnum[b[j] as usize] {
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

/// The matched CHAR count of the LCS over interned ids — the window
/// scan's `sim` needs only this (NOT the pairs themselves). Same DP + the
/// SAME tie-break ladder as `lcs_pairs_ids`, but accumulates `b_token_chars[j]`
/// on each match instead of building a HashMap — the window scan's 42k
/// per-call HashMaps (alloc + per-pair inserts) were the remaining scan
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
    // Reuse a single caller-supplied buffer across ALL window scans — the
    // 42k per-call `vec![(n+1)*(m+1)]` allocs (132MB zeroed) were the
    // scan's remaining cost. clear+resize reuses the backing allocation
    // when it's already big enough (no realloc after the first big window).
    scratch.clear();
    scratch.resize((n + 1) * (m + 1), 0);
    let dp = scratch.as_mut_slice();
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
    let mut matched = 0usize;
    let mut i = 0;
    let mut j = 0;
    while i < n && j < m {
        if a[i] == b[j] {
            matched += b_token_chars[j];
            i += 1;
            j += 1;
        } else if dp[(i + 1) * (m + 1) + j] > dp[i * (m + 1) + j + 1] {
            i += 1;
        } else if dp[(i + 1) * (m + 1) + j] < dp[i * (m + 1) + j + 1] {
            j += 1;
        } else if id_ws[a[i] as usize] && !id_ws[b[j] as usize] {
            if i + 1 < n && a[i + 1] == b[j] {
                i += 1;
            } else {
                j += 1;
            }
        } else if id_ws[b[j] as usize] && !id_ws[a[i] as usize] {
            j += 1;
        } else if !b[j + 1..].iter().any(|t| *t == a[i])
            || (!id_alnum[a[i] as usize] && id_alnum[b[j] as usize])
        {
            i += 1;
        } else {
            j += 1;
        }
    }
    matched
}

/// Char-offset spans of the UNMATCHED (changed) tokens in `toks`.
pub fn spans_from(
    text: &str,
    toks: &[&str],
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
pub type WordSpans = Spans;

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
    let matched_chars: usize = pairs.values().map(|j| char_len(nt[*j])).sum();

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
        del: merge_whitespace_gaps(
            old_text,
            spans_from(old_text, &ot, &matched_old),
            Some(new_text),
        ),
        add: merge_whitespace_gaps(
            new_text,
            spans_from(new_text, &nt, &matched_new),
            Some(old_text),
        ),
    }
}

fn merge_whitespace_gaps(
    text: &str,
    spans: Vec<(usize, usize)>,
    other_text: Option<&str>,
) -> Vec<(usize, usize)> {
    if spans.is_empty() {
        return spans;
    }
    let mut merged = Vec::new();
    let mut current = spans[0];
    let mut merged_any = false;

    for &(next_start, next_end) in spans.iter().skip(1) {
        let gap = char_slice(text, current.1, next_start);
        if !gap.is_empty() && gap.chars().all(|c| c.is_whitespace()) {
            current = (current.0, next_end);
            merged_any = true;
        } else {
            merged.push(current);
            current = (next_start, next_end);
        }
    }
    merged.push(current);

    if merged_any {
        for span in &mut merged {
            let raw = char_slice(text, span.0, span.1);
            if raw.ends_with(' ') && !raw.trim().is_empty() {
                let should_trim = match other_text {
                    Some(other) => {
                        let rem = char_slice(text, span.1, char_len(text));
                        let next_word = rem.trim_start();
                        if let Some(first_char) = next_word.chars().next() {
                            other.chars().enumerate().any(|(idx, ch)| {
                                ch == first_char
                                    && idx > 0
                                    && other.chars().nth(idx - 1) == Some(' ')
                            })
                        } else {
                            true
                        }
                    }
                    None => true,
                };
                if should_trim {
                    span.1 -= 1;
                }
            }
        }
    }

    let total_chars = char_len(text);
    for span in &mut merged {
        if span.1 == total_chars && span.0 > 0 {
            let chars: Vec<char> = text.chars().collect();
            if chars[span.0 - 1] == ' ' && (span.0 == 1 || !chars[span.0 - 2].is_whitespace()) {
                span.0 -= 1;
            }
        }
    }
    merged
}

#[cfg(test)]
mod tests;
