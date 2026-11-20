//! Word spans calculation, tape reconstruction, and subline edit run
//! generation.

use crate::{merge::RunKind, parse::Spans};
use diffuse_syntax::profile::LangProfile;
use rustc_hash::FxHashSet;

use super::{arena::token_product_cap, tokenizer::char_len};

/// Affix overlap: both sides are long enough (>= 8 bytes) and one is a
/// prefix or suffix of the other. Shared by cluster factorization edges
/// (`factor.rs`), slot-alignment scoring (`slot.rs`), and the span fast
/// path below so the three call sites cannot diverge.
#[inline]
pub fn is_affix_pair(a_trim: &str, b_trim: &str) -> bool {
    a_trim.len() >= 8
        && b_trim.len() >= 8
        && (a_trim.starts_with(b_trim)
            || b_trim.starts_with(a_trim)
            || a_trim.ends_with(b_trim)
            || b_trim.ends_with(a_trim))
}

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
 * Word-level change spans plus owned merged runs with active syntax profile
 * and default token product cap (see
 * `word_spans_and_runs_with_profile_and_cap`).
 */
pub fn word_spans_and_runs_with_profile(
    old_text: &str,
    new_text: &str,
    profile: Option<&LangProfile>,
) -> (WordSpans, Vec<(RunKind, String)>) {
    word_spans_and_runs_with_profile_and_cap(old_text, new_text, profile, token_product_cap())
}

#[inline]
fn truncate_for_trace(s: &str, max_len: usize) -> &str {
    match s.char_indices().nth(max_len) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/**
 * Word-level change spans between an old and a new line with explicit token
 * product cap.
 */
#[tracing::instrument(
    level = "debug",
    ret,
    skip_all,
    fields(
        old = %truncate_for_trace(old_text, 40),
        new = %truncate_for_trace(new_text, 40),
        strategy = tracing::field::Empty,
        anchors = tracing::field::Empty,
        matched_chars = tracing::field::Empty,
        ratio = tracing::field::Empty
    )
)]
pub fn word_spans_with_profile_and_cap(
    old_text: &str,
    new_text: &str,
    profile: Option<&LangProfile>,
    cap: usize,
) -> WordSpans {
    word_spans_and_runs_with_profile_and_cap(old_text, new_text, profile, cap).0
}

/**
 * Word-level change spans plus the owned merged runs from a single
 * alignment tape build. Render unification: these runs always derive from
 * the same profiled tape as the spans, so render consuming them can never
 * contradict the spans' tape (see `merge::tests` agreement/divergence
 * pins).
 */
#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(
        old = %truncate_for_trace(old_text, 40),
        new = %truncate_for_trace(new_text, 40),
        strategy = tracing::field::Empty,
        anchors = tracing::field::Empty,
        matched_chars = tracing::field::Empty,
        ratio = tracing::field::Empty
    )
)]
pub fn word_spans_and_runs_with_profile_and_cap(
    old_text: &str,
    new_text: &str,
    profile: Option<&LangProfile>,
    cap: usize,
) -> (WordSpans, Vec<(RunKind, String)>) {
    if !old_text.is_empty() && old_text == new_text {
        return (
            WordSpans::default(),
            vec![(RunKind::Ctx, old_text.to_string())],
        );
    }
    if !old_text.is_empty() && !new_text.is_empty() && new_text.starts_with(old_text) {
        let old_char_len = char_len(old_text);
        let new_char_len = char_len(new_text);
        let spans = WordSpans {
            del: Vec::new(),
            add: vec![(old_char_len, new_char_len)],
        };
        let runs = vec![
            (RunKind::Ctx, old_text.to_string()),
            (RunKind::Add, new_text[old_text.len()..].to_string()),
        ];
        return (spans, runs);
    }
    if !old_text.is_empty() && !new_text.is_empty() && old_text.starts_with(new_text) {
        let new_char_len = char_len(new_text);
        let old_char_len = char_len(old_text);
        let spans = WordSpans {
            del: vec![(new_char_len, old_char_len)],
            add: Vec::new(),
        };
        let runs = vec![
            (RunKind::Ctx, new_text.to_string()),
            (RunKind::Del, old_text[new_text.len()..].to_string()),
        ];
        return (spans, runs);
    }
    if !old_text.is_empty() && !new_text.is_empty() && new_text.ends_with(old_text) {
        let prefix_len = new_text.len() - old_text.len();
        let prefix_chars = char_len(&new_text[..prefix_len]);
        let spans = WordSpans {
            del: Vec::new(),
            add: vec![(0, prefix_chars)],
        };
        let runs = vec![
            (RunKind::Add, new_text[..prefix_len].to_string()),
            (RunKind::Ctx, old_text.to_string()),
        ];
        return (spans, runs);
    }
    if !old_text.is_empty() && !new_text.is_empty() && old_text.ends_with(new_text) {
        let prefix_len = old_text.len() - new_text.len();
        let prefix_chars = char_len(&old_text[..prefix_len]);
        let spans = WordSpans {
            del: vec![(0, prefix_chars)],
            add: Vec::new(),
        };
        let runs = vec![
            (RunKind::Del, old_text[..prefix_len].to_string()),
            (RunKind::Ctx, new_text.to_string()),
        ];
        return (spans, runs);
    }

    let tape = crate::tape::AlignmentTape::from_texts_with_cap(old_text, new_text, profile, cap);
    let res = tape.to_spans();

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

    let runs = tape
        .to_merged_runs()
        .iter()
        .map(|r| (r.kind, r.text.to_string()))
        .collect();
    (res, runs)
}
