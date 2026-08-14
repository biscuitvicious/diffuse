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
use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

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
/// are separate tokens so `->` → `=>` highlights a single character). Zero-copy
/// slices.
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

/// Computes unambiguous unique token anchors (Patience diff pre-pass).
pub fn unique_token_anchors(a: &[&str], b: &[&str]) -> Vec<(usize, usize)> {
    let mut a_counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for (i, &t) in a.iter().enumerate() {
        let entry = a_counts.entry(t).or_insert((0, i));
        entry.0 += 1;
    }
    let mut b_counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for (j, &t) in b.iter().enumerate() {
        let entry = b_counts.entry(t).or_insert((0, j));
        entry.0 += 1;
    }
    let mut common_unique = Vec::new();
    for (&t, &(count_a, idx_a)) in &a_counts {
        if count_a == 1
            && !token_is_ws(t)
            && token_is_alnum(t)
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

/// Core LCS backtrace over string slices.
pub fn raw_lcs_pairs(a: &[&str], b: &[&str]) -> HashMap<usize, usize> {
    raw_lcs_pairs_with_lookahead(a, b, None, None)
}

/// Core LCS backtrace with optional boundary lookahead tokens from parent line.
pub fn raw_lcs_pairs_with_lookahead(
    a: &[&str],
    b: &[&str],
    next_a: Option<&str>,
    next_b: Option<&str>,
) -> HashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return HashMap::new();
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
    let mut pairs = HashMap::new();
    let mut i = 0;
    let mut j = 0;
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
            pairs.insert(i, j);
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
/// Combines Patience unique token anchoring with run-continuation LCS.
pub fn lcs_pairs(a: &[&str], b: &[&str]) -> HashMap<usize, usize> {
    let anchors = unique_token_anchors(a, b);
    if anchors.is_empty() {
        return raw_lcs_pairs(a, b);
    }
    lcs_pairs_anchored(a, b, &anchors)
}

pub fn lcs_pairs_anchored(
    a: &[&str],
    b: &[&str],
    anchors: &[(usize, usize)],
) -> HashMap<usize, usize> {
    let mut pairs = HashMap::new();
    let mut a_prev = 0;
    let mut b_prev = 0;
    for &(ai, bi) in anchors {
        if ai > a_prev && bi > b_prev {
            let sub_pairs = raw_lcs_pairs_with_lookahead(
                &a[a_prev..ai],
                &b[b_prev..bi],
                Some(a[ai]),
                Some(b[bi]),
            );
            for (si, sj) in sub_pairs {
                pairs.insert(a_prev + si, b_prev + sj);
            }
        }
        pairs.insert(ai, bi);
        a_prev = ai + 1;
        b_prev = bi + 1;
    }
    if a_prev < a.len() && b_prev < b.len() {
        let sub_pairs = raw_lcs_pairs_with_lookahead(&a[a_prev..], &b[b_prev..], None, None);
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
) -> HashMap<usize, usize> {
    let n = a.len();
    let m = b.len();
    if n == 0 || m == 0 {
        return HashMap::new();
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
    let mut pairs = HashMap::new();
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
            matched += b_token_chars[j];
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
    matched
}

/// Char-offset spans of the UNMATCHED (changed) tokens in `toks`.
pub fn spans_from(text: &str, toks: &[&str], matched: &HashSet<usize>) -> Vec<(usize, usize)> {
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
    let anchors = unique_token_anchors(&ot, &nt);
    let has_anchors = !anchors.is_empty();
    let pairs = if has_anchors {
        lcs_pairs_anchored(&ot, &nt, &anchors)
    } else {
        raw_lcs_pairs(&ot, &nt)
    };
    let mut matched_old = HashSet::new();
    let mut matched_new = HashSet::new();
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
    if !has_anchors && ratio < 0.25 {
        return WordSpans {
            del: vec![(0, char_len(old_text))],
            add: vec![(0, char_len(new_text))],
        };
    }
    WordSpans {
        del: merge_whitespace_gaps(old_text, spans_from(old_text, &ot, &matched_old)),
        add: merge_whitespace_gaps(new_text, spans_from(new_text, &nt, &matched_new)),
    }
}

fn merge_whitespace_gaps(text: &str, spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if spans.is_empty() {
        return spans;
    }
    let mut merged = Vec::new();
    let mut current = spans[0];
    for &(next_start, next_end) in spans.iter().skip(1) {
        let gap = char_slice(text, current.1, next_start);
        if !gap.is_empty() && gap.chars().all(char::is_whitespace) {
            current = (current.0, next_end);
        } else {
            merged.push(current);
            current = (next_start, next_end);
        }
    }
    merged.push(current);
    merged
}

#[cfg(test)]
mod tests;
