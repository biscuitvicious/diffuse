//! Invertible Alignment Tape & Reversible Run Algebra.
//!
//! Represents a token-level alignment between two lines or blocks as a
//! strongly typed, monoidal tape of atomic operations (`Keep`, `Strike`,
//! `Fill`, `Relocate`).
//!
//! Guarantees by construction:
//! - `project_old(&tape) == old_text`
//! - `project_new(&tape) == new_text`
//! - Invertibility: reverse of a tape is symmetric
//! - No lossy coordinate guessing

use std::{slice, str};

use diffuse_syntax::profile::LangProfile;

use crate::{
    lcs::{
        char_len, char_slice, lcs_pairs_anchored, raw_lcs_pairs, token_is_ws, token_product_cap,
        tokenize_line, unique_token_anchors,
    },
    merge::{Run, RunKind},
    parse::Spans,
    token::classify_whitespace,
};

/// An atomic operation along the alignment tape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapeChunk<'a> {
    /// Identical token run surviving unmodified across both sides.
    Keep(&'a str),
    /// Struck deletion from old text, anchored directly at its common context.
    Strike(&'a str),
    /// Filled addition to new text, anchored directly at its common context.
    Fill(&'a str),
    /// Relocated / moved segment (subline reflow or cross-line transfer).
    Relocate {
        from_span: (usize, usize),
        to_span: (usize, usize),
        text: &'a str,
    },
}

impl<'a> TapeChunk<'a> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Keep(s) | Self::Strike(s) | Self::Fill(s) | Self::Relocate { text: s, .. } => {
                s.is_empty()
            }
        }
    }

    #[inline]
    pub fn text(&self) -> &'a str {
        match self {
            Self::Keep(s) | Self::Strike(s) | Self::Fill(s) | Self::Relocate { text: s, .. } => s,
        }
    }

    /// Number of bytes consumed by this chunk in old text.
    #[inline]
    pub fn old_len(&self) -> usize {
        match self {
            Self::Keep(s) | Self::Strike(s) | Self::Relocate { text: s, .. } => s.len(),
            Self::Fill(_) => 0,
        }
    }

    /// Number of bytes consumed by this chunk in new text.
    #[inline]
    pub fn new_len(&self) -> usize {
        match self {
            Self::Keep(s) | Self::Fill(s) | Self::Relocate { text: s, .. } => s.len(),
            Self::Strike(_) => 0,
        }
    }
}

#[inline]
fn is_ws_char(c: char) -> bool {
    c.is_whitespace() || c == '\u{200b}'
}

#[inline]
fn all_ws(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_ws_char)
}

#[inline]
fn all_ws_or_empty(s: &str) -> bool {
    s.chars().all(is_ws_char)
}

#[inline]
fn truncate_for_trace(s: &str, max_len: usize) -> &str {
    match s.char_indices().nth(max_len) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// An immutable, monoidal sequence of alignment operations between old and
/// new text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlignmentTape<'a> {
    chunks: Vec<TapeChunk<'a>>,
}

impl<'a> AlignmentTape<'a> {
    /// Create an empty alignment tape.
    #[inline]
    pub fn empty() -> Self {
        Self { chunks: Vec::new() }
    }

    /// Append a chunk to the tape, skipping empty slices.
    pub fn push(&mut self, chunk: TapeChunk<'a>) {
        if chunk.is_empty() {
            return;
        }
        self.chunks.push(chunk);
    }

    /// Monoidally compose another tape into this tape.
    #[inline]
    pub fn compose(&mut self, other: AlignmentTape<'a>) {
        self.chunks.extend(other.chunks);
    }

    /// Borrow the underlying chunks.
    #[inline]
    pub fn chunks(&self) -> &[TapeChunk<'a>] {
        &self.chunks
    }

    /// Returns a streaming projection iterator over the old text sequence.
    pub fn project_old_iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.chunks.iter().filter_map(|chunk| match chunk {
            TapeChunk::Keep(text) | TapeChunk::Strike(text) | TapeChunk::Relocate { text, .. } => {
                Some(*text)
            }
            TapeChunk::Fill(_) => None,
        })
    }

    /// Returns a streaming projection iterator over the new text sequence.
    pub fn project_new_iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.chunks.iter().filter_map(|chunk| match chunk {
            TapeChunk::Keep(text) | TapeChunk::Fill(text) | TapeChunk::Relocate { text, .. } => {
                Some(*text)
            }
            TapeChunk::Strike(_) => None,
        })
    }

    /// Projects the old text: concatenates `Keep` + `Strike` + `Relocate`.
    pub fn project_old(&self) -> String {
        let mut s = String::new();
        for part in self.project_old_iter() {
            s.push_str(part);
        }
        s
    }

    /// Projects the new text: concatenates `Keep` + `Fill` + `Relocate`.
    pub fn project_new(&self) -> String {
        let mut s = String::new();
        for part in self.project_new_iter() {
            s.push_str(part);
        }
        s
    }

    /// Compute the algebraic inverse alignment tape.
    ///
    /// Swaps `Strike` and `Fill`, inverts `Relocate` spans, and preserves
    /// `Keep`. Satisfies the involution property: `tape.reverse().reverse()
    /// == tape`.
    pub fn reverse(&self) -> AlignmentTape<'a> {
        Self::from_algebraic_tape(self.to_algebraic_tape().invert())
    }

    /// Converts this alignment tape into an algebraic `Tape<&'a str>`.
    pub fn to_algebraic_tape(&self) -> diffuse_algo::tape::Tape<&'a str> {
        self.chunks
            .iter()
            .map(|c| match c {
                TapeChunk::Keep(s) => diffuse_algo::tape::TapeChunk::Keep(*s),
                TapeChunk::Strike(s) => diffuse_algo::tape::TapeChunk::Strike(*s),
                TapeChunk::Fill(s) => diffuse_algo::tape::TapeChunk::Fill(*s),
                TapeChunk::Relocate {
                    from_span,
                    to_span,
                    text,
                } => diffuse_algo::tape::TapeChunk::Relocate {
                    from_span: *from_span,
                    to_span: *to_span,
                    item: *text,
                },
            })
            .collect()
    }

    /// Creates an `AlignmentTape` from an algebraic `Tape<&'a str>`.
    pub fn from_algebraic_tape(tape: diffuse_algo::tape::Tape<&'a str>) -> Self {
        let chunks = tape
            .into_chunks()
            .into_iter()
            .map(|c| match c {
                diffuse_algo::tape::TapeChunk::Keep(s) => TapeChunk::Keep(s),
                diffuse_algo::tape::TapeChunk::Strike(s) => TapeChunk::Strike(s),
                diffuse_algo::tape::TapeChunk::Fill(s) => TapeChunk::Fill(s),
                diffuse_algo::tape::TapeChunk::Relocate {
                    from_span,
                    to_span,
                    item,
                } => TapeChunk::Relocate {
                    from_span,
                    to_span,
                    text: item,
                },
            })
            .collect();
        Self { chunks }
    }

    /// Convert alignment chunks directly into renderer `Run` items without
    /// heuristic reconstruction.
    pub fn to_merged_runs(&self) -> Vec<Run<'a>> {
        Self::runs_from_chunks(&self.chunks)
    }

    /// Render merged runs from an explicit chunk stream (same presentation
    /// logic as [`Self::to_merged_runs`], decoupled from fresh alignment).
    /// Used to honor stored spans that record a customized pairing.
    pub(crate) fn runs_from_chunks(chunks: &[TapeChunk<'a>]) -> Vec<Run<'a>> {
        let mut runs: Vec<Run<'a>> = Vec::with_capacity(chunks.len() + 4);
        let mut is_leading = true;
        let mut idx = 0;

        while idx < chunks.len() {
            // Check for adjacent (Strike, Fill) pair
            if idx + 1 < chunks.len()
                && let (TapeChunk::Strike(d), TapeChunk::Fill(a)) = (&chunks[idx], &chunks[idx + 1])
            {
                if is_leading {
                    let d_ws_len = d.len() - d.trim_start().len();
                    let a_ws_len = a.len() - a.trim_start().len();
                    if d_ws_len > 0 && a_ws_len > 0 {
                        let d_ws = &d[..d_ws_len];
                        let a_ws = &a[..a_ws_len];
                        if d_ws.len() < a_ws.len()
                            && let Some(add_prefix) = a_ws.strip_suffix(d_ws)
                        {
                            runs.push(Run {
                                kind: RunKind::Add,
                                text: add_prefix,
                            });
                            runs.push(Run {
                                kind: RunKind::Ctx,
                                text: &a_ws[add_prefix.len()..],
                            });
                            if d_ws_len < d.len() {
                                runs.push(Run {
                                    kind: RunKind::Del,
                                    text: &d[d_ws_len..],
                                });
                            }
                            if a_ws_len < a.len() {
                                runs.push(Run {
                                    kind: RunKind::Add,
                                    text: &a[a_ws_len..],
                                });
                            }
                            is_leading = false;
                            idx += 2;
                            continue;
                        } else if d_ws.len() > a_ws.len()
                            && let Some(del_prefix) = d_ws.strip_suffix(a_ws)
                        {
                            runs.push(Run {
                                kind: RunKind::Del,
                                text: del_prefix,
                            });
                            runs.push(Run {
                                kind: RunKind::Ctx,
                                text: a_ws,
                            });
                            if d_ws_len < d.len() {
                                runs.push(Run {
                                    kind: RunKind::Del,
                                    text: &d[d_ws_len..],
                                });
                            }
                            if a_ws_len < a.len() {
                                runs.push(Run {
                                    kind: RunKind::Add,
                                    text: &a[a_ws_len..],
                                });
                            }
                            is_leading = false;
                            idx += 2;
                            continue;
                        } else if d_ws == a_ws {
                            runs.push(Run {
                                kind: RunKind::Ctx,
                                text: a_ws,
                            });
                            if d_ws_len < d.len() {
                                runs.push(Run {
                                    kind: RunKind::Del,
                                    text: &d[d_ws_len..],
                                });
                            }
                            if a_ws_len < a.len() {
                                runs.push(Run {
                                    kind: RunKind::Add,
                                    text: &a[a_ws_len..],
                                });
                            }
                            is_leading = false;
                            idx += 2;
                            continue;
                        }
                    }
                } else if all_ws(d) && all_ws(a) {
                    // Non-leading whitespace: shared prefix is common
                    let mut share = 0usize;
                    let d_bytes = d.as_bytes();
                    let a_bytes = a.as_bytes();
                    while share < d_bytes.len()
                        && share < a_bytes.len()
                        && d_bytes[share] == a_bytes[share]
                    {
                        share += 1;
                    }
                    if share > 0 {
                        runs.push(Run {
                            kind: RunKind::Ctx,
                            text: &d[..share],
                        });
                    }
                    if share < d.len() {
                        runs.push(Run {
                            kind: RunKind::Del,
                            text: &d[share..],
                        });
                    }
                    if share < a.len() {
                        runs.push(Run {
                            kind: RunKind::Add,
                            text: &a[share..],
                        });
                    }
                    idx += 2;
                    continue;
                }
            }

            let chunk = &chunks[idx];
            match chunk {
                TapeChunk::Keep(text) => {
                    if !all_ws_or_empty(text) {
                        is_leading = false;
                    }
                    runs.push(Run {
                        kind: RunKind::Ctx,
                        text,
                    });
                }
                TapeChunk::Strike(text) => {
                    if !all_ws_or_empty(text) {
                        is_leading = false;
                    }
                    runs.push(Run {
                        kind: RunKind::Del,
                        text,
                    });
                }
                TapeChunk::Fill(text) => {
                    if !all_ws_or_empty(text) {
                        is_leading = false;
                    }
                    runs.push(Run {
                        kind: RunKind::Add,
                        text,
                    });
                }
                TapeChunk::Relocate { text, .. } => {
                    if !all_ws_or_empty(text) {
                        is_leading = false;
                    }
                    runs.push(Run {
                        kind: RunKind::Ctx,
                        text,
                    });
                }
            }
            idx += 1;
        }

        // Coalesce adjacent contiguous runs of the same kind
        let mut coalesced: Vec<Run<'a>> = Vec::with_capacity(runs.len());
        for run in runs {
            if let Some(last) = coalesced.last_mut()
                && last.kind == run.kind
            {
                let last_ptr = last.text.as_ptr() as usize;
                let run_ptr = run.text.as_ptr() as usize;
                if last_ptr + last.text.len() == run_ptr {
                    let combined_len = last.text.len() + run.text.len();
                    // SAFETY: both last.text and run.text are valid UTF-8
                    // contiguous slices of the same parent string.
                    unsafe {
                        let combined_slice =
                            slice::from_raw_parts(last.text.as_ptr(), combined_len);
                        last.text = str::from_utf8_unchecked(combined_slice);
                    }
                    continue;
                }
            }
            coalesced.push(run);
        }
        coalesced
    }

    /// Build tape chunks directly from stored spans (both sides non-empty).
    /// Returns `None` unless spans are pairwise well-formed: sorted,
    /// non-overlapping, in-bounds, equal del/add counts, and every common
    /// gap byte-identical on both sides. Callers fall back to a fresh tape
    /// then, so behavior on ordinary inputs is unchanged.
    pub(crate) fn chunks_from_spans(
        del_text: &'a str,
        add_text: &'a str,
        spans: &Spans,
    ) -> Option<Vec<TapeChunk<'a>>> {
        let ok_side = |ranges: &[(usize, usize)], len: usize| {
            let mut pos = 0usize;
            for &(s, e) in ranges {
                if s < pos || e < s || e > len {
                    return false;
                }
                pos = e;
            }
            true
        };
        let del_len = char_len(del_text);
        let add_len = char_len(add_text);
        if !ok_side(&spans.del, del_len) || !ok_side(&spans.add, add_len) {
            return None;
        }
        if spans.del.len() != spans.add.len() {
            return None;
        }
        let mut chunks: Vec<TapeChunk<'a>> = Vec::new();
        let mut pd = 0usize;
        let mut pa = 0usize;
        for (&(ds, de), &(as_, ae)) in spans.del.iter().zip(spans.add.iter()) {
            let gap_del = char_slice(del_text, pd, ds);
            let gap_add = char_slice(add_text, pa, as_);
            if gap_del != gap_add {
                return None;
            }
            if ds > pd {
                // Gap text from the ADD side: downstream whitespace
                // splitting slices the Fill chunk, and coalescing needs
                // a shared parent.
                chunks.push(TapeChunk::Keep(gap_add));
            }
            if de > ds {
                chunks.push(TapeChunk::Strike(char_slice(del_text, ds, de)));
            }
            if ae > as_ {
                chunks.push(TapeChunk::Fill(char_slice(add_text, as_, ae)));
            }
            pd = de;
            pa = ae;
        }
        if pd < del_len || pa < add_len {
            let tail_del = char_slice(del_text, pd, del_len);
            let tail_add = char_slice(add_text, pa, add_len);
            if tail_del != tail_add {
                return None;
            }
            if !tail_add.is_empty() {
                chunks.push(TapeChunk::Keep(tail_add));
            }
        }
        Some(chunks)
    }
    /// Computes canonical `Spans { del, add }` from the alignment tape.
    pub fn to_spans(&self) -> Spans {
        let mut del: Vec<(usize, usize)> = Vec::new();
        let mut add: Vec<(usize, usize)> = Vec::new();

        let mut old_pos = 0usize;
        let mut new_pos = 0usize;

        for chunk in &self.chunks {
            match chunk {
                TapeChunk::Keep(text) => {
                    let len = char_len(text);
                    old_pos += len;
                    new_pos += len;
                }
                TapeChunk::Strike(text) => {
                    let len = char_len(text);
                    let start = old_pos;
                    old_pos += len;
                    if let Some(last) = del.last_mut()
                        && last.1 == start
                    {
                        last.1 = old_pos;
                    } else {
                        del.push((start, old_pos));
                    }
                }
                TapeChunk::Fill(text) => {
                    let len = char_len(text);
                    let start = new_pos;
                    new_pos += len;
                    if let Some(last) = add.last_mut()
                        && last.1 == start
                    {
                        last.1 = new_pos;
                    } else {
                        add.push((start, new_pos));
                    }
                }
                TapeChunk::Relocate { text, .. } => {
                    let len = char_len(text);
                    old_pos += len;
                    new_pos += len;
                }
            }
        }

        if old_pos == 0 && del.is_empty() {
            del.push((0, 0));
        }
        if new_pos == 0 && add.is_empty() {
            add.push((0, 0));
        }

        Spans { del, add }
    }

    /// Construct an alignment tape from old and new line text.
    pub fn from_texts(old_text: &'a str, new_text: &'a str, profile: Option<&LangProfile>) -> Self {
        Self::from_texts_with_cap(old_text, new_text, profile, token_product_cap())
    }

    /// Construct an alignment tape from old and new line text with explicit
    /// token cap.
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
    pub fn from_texts_with_cap(
        old_text: &'a str,
        new_text: &'a str,
        profile: Option<&LangProfile>,
        cap: usize,
    ) -> Self {
        let mut tape = Self::default();

        if old_text.is_empty() && new_text.is_empty() {
            return tape;
        }
        if old_text == new_text {
            tape.push(TapeChunk::Keep(new_text));
            return tape;
        }

        let ot: Vec<&str> = tokenize_line(old_text).collect();
        let nt: Vec<&str> = tokenize_line(new_text).collect();

        if ot.len() * nt.len() > cap {
            if !old_text.is_empty() {
                tape.push(TapeChunk::Strike(old_text));
            }
            if !new_text.is_empty() {
                tape.push(TapeChunk::Fill(new_text));
            }
            return tape;
        }

        let mut prefix = 0usize;
        while prefix < ot.len() && prefix < nt.len() && ot[prefix] == nt[prefix] {
            prefix += 1;
        }
        if prefix > 0
            && (ot[prefix..].iter().all(|t| token_is_ws(t))
                != nt[prefix..].iter().all(|t| token_is_ws(t)))
        {
            while prefix > 0 && token_is_ws(ot[prefix - 1]) {
                prefix -= 1;
            }
        }

        let mut suffix = 0usize;
        while suffix < (ot.len() - prefix)
            && suffix < (nt.len() - prefix)
            && ot[ot.len() - 1 - suffix] == nt[nt.len() - 1 - suffix]
        {
            suffix += 1;
        }
        if suffix > 0
            && (ot[..ot.len() - suffix].iter().all(|t| token_is_ws(t))
                != nt[..nt.len() - suffix].iter().all(|t| token_is_ws(t)))
        {
            while suffix > 0 && token_is_ws(ot[ot.len() - suffix]) {
                suffix -= 1;
            }
        }

        let mut o_pos = 0usize;
        let mut o_spans = Vec::with_capacity(ot.len());
        for t in &ot {
            let len = char_len(t);
            o_spans.push((o_pos, o_pos + len));
            o_pos += len;
        }

        let mut n_pos = 0usize;
        let mut n_spans = Vec::with_capacity(nt.len());
        for t in &nt {
            let len = char_len(t);
            n_spans.push((n_pos, n_pos + len));
            n_pos += len;
        }

        let ot_last_eq_nt_last = !ot.is_empty() && !nt.is_empty() && ot.last() == nt.last();
        let ot_first_eq_nt_first = !ot.is_empty() && !nt.is_empty() && ot.first() == nt.first();

        let is_unambiguous_prefix =
            (prefix == ot.len() || prefix == nt.len()) && !ot_last_eq_nt_last;
        let is_unambiguous_suffix =
            (suffix == ot.len() || suffix == nt.len()) && !ot_first_eq_nt_first;

        // Anchors are computed lazily (see below): the pure prefix/suffix
        // branches never consult them, and the low-ratio fallback only needs
        // `has_anchors` when the ratio actually drops below threshold (rare).
        // Same decisions, none of the entropy/phrase/container work on the
        // common path.
        let mut anchors: Option<Vec<(usize, usize)>> = None;

        let span = tracing::Span::current();
        let sorted_pairs = if is_unambiguous_prefix {
            span.record("strategy", "pure_prefix");
            let mut pairs = Vec::with_capacity(prefix);
            for i in 0..prefix {
                pairs.push((i, i));
            }
            pairs
        } else if is_unambiguous_suffix {
            span.record("strategy", "pure_suffix");
            let mut pairs = Vec::with_capacity(suffix);
            for k in 0..suffix {
                pairs.push((ot.len() - suffix + k, nt.len() - suffix + k));
            }
            pairs
        } else {
            let computed = unique_token_anchors(&ot, &nt, profile);
            let has_anchors = !computed.is_empty();
            let p_map = if has_anchors {
                span.record("anchors", tracing::field::debug(&computed));
                span.record("strategy", "anchored");
                lcs_pairs_anchored(&ot, &nt, &computed)
            } else {
                span.record("strategy", "raw");
                raw_lcs_pairs(&ot, &nt)
            };
            anchors = Some(computed);
            let mut pairs: Vec<(usize, usize)> = p_map.into_iter().collect();
            pairs.sort_by_key(|&(o, _)| o);
            pairs
        };

        let token_weight = |tokens: &[&str], idx: usize| -> f64 {
            let t = tokens[idx];
            if !token_is_ws(t) {
                char_len(t) as f64
            } else {
                classify_whitespace(tokens, idx).token_weight(char_len(t))
            }
        };
        let matched_chars: f64 = sorted_pairs
            .iter()
            .map(|&(i, j)| {
                let w_old = token_weight(&ot, i);
                let w_new = token_weight(&nt, j);
                w_old.min(w_new)
            })
            .sum();
        let new_weight: f64 = (0..nt.len()).map(|j| token_weight(&nt, j)).sum();
        let ratio = if new_weight > 0.0 {
            matched_chars / new_weight
        } else {
            0.0
        };
        span.record("matched_chars", matched_chars);
        span.record("ratio", ratio);

        if ratio < 0.25 && old_text.trim() != new_text.trim() {
            // Anchors were skipped on the pure-prefix/suffix fast paths;
            // compute them only now that the fallback test actually needs
            // `has_anchors` (reordered short-circuit, identical outcome).
            let has_anchors = match anchors {
                Some(ref computed) => !computed.is_empty(),
                None => !unique_token_anchors(&ot, &nt, profile).is_empty(),
            };
            if !has_anchors {
                tracing::debug!(
                    ratio,
                    "Subword match ratio below threshold, falling back to whole-line diff"
                );
                if !old_text.is_empty() {
                    tape.push(TapeChunk::Strike(old_text));
                }
                if !new_text.is_empty() {
                    tape.push(TapeChunk::Fill(new_text));
                }
                return tape;
            }
        }

        let mut old_tok_idx = 0usize;
        let mut new_tok_idx = 0usize;

        let emit_gap = |tape: &mut AlignmentTape<'a>,
                        old_from: usize,
                        old_to: usize,
                        new_from: usize,
                        new_to: usize| {
            let has_old = old_to > old_from;
            let has_new = new_to > new_from;

            if has_old
                && has_new
                && (new_to == nt.len() || nt[new_to..].iter().all(|t| token_is_ws(t)))
            {
                let new_tail_opt = (new_from..new_to).rev().find(|&idx| !token_is_ws(nt[idx]));
                if let Some(new_tail_idx) = new_tail_opt {
                    let tail_token = nt[new_tail_idx];
                    let old_match_opt = (old_from..old_to).find(|&idx| ot[idx] == tail_token);
                    if let Some(old_match_idx) = old_match_opt {
                        let mut o_m_start = old_match_idx;
                        let mut n_m_start = new_tail_idx;
                        if o_m_start > old_from
                            && n_m_start > new_from
                            && token_is_ws(ot[o_m_start - 1])
                            && ot[o_m_start - 1] == nt[n_m_start - 1]
                        {
                            o_m_start -= 1;
                            n_m_start -= 1;
                        }
                        let o_m_end = old_match_idx + 1;
                        let n_m_end = new_tail_idx + 1;

                        let old_keep =
                            char_slice(old_text, o_spans[o_m_start].0, o_spans[o_m_end - 1].1);
                        let new_keep =
                            char_slice(new_text, n_spans[n_m_start].0, n_spans[n_m_end - 1].1);

                        if old_keep == new_keep && !old_keep.is_empty() {
                            if o_m_start > old_from {
                                let strike_before = char_slice(
                                    old_text,
                                    o_spans[old_from].0,
                                    o_spans[o_m_start - 1].1,
                                );
                                if !strike_before.is_empty() {
                                    tape.push(TapeChunk::Strike(strike_before));
                                }
                            }
                            if n_m_start > new_from {
                                let fill_before = char_slice(
                                    new_text,
                                    n_spans[new_from].0,
                                    n_spans[n_m_start - 1].1,
                                );
                                if !fill_before.is_empty() {
                                    tape.push(TapeChunk::Fill(fill_before));
                                }
                            }

                            tape.push(TapeChunk::Keep(new_keep));

                            if old_to > o_m_end {
                                let strike_after =
                                    char_slice(old_text, o_spans[o_m_end].0, o_spans[old_to - 1].1);
                                if !strike_after.is_empty() {
                                    tape.push(TapeChunk::Strike(strike_after));
                                }
                            }
                            if new_to > n_m_end {
                                let fill_after =
                                    char_slice(new_text, n_spans[n_m_end].0, n_spans[new_to - 1].1);
                                if !fill_after.is_empty() {
                                    tape.push(TapeChunk::Fill(fill_after));
                                }
                            }
                            return;
                        }
                    }
                }
            }

            if has_old {
                let o_slice = char_slice(old_text, o_spans[old_from].0, o_spans[old_to - 1].1);
                tape.push(TapeChunk::Strike(o_slice));
            }
            if has_new {
                let n_slice = char_slice(new_text, n_spans[new_from].0, n_spans[new_to - 1].1);
                tape.push(TapeChunk::Fill(n_slice));
            }
        };

        for &(oi, ni) in &sorted_pairs {
            if old_tok_idx < oi || new_tok_idx < ni {
                emit_gap(&mut tape, old_tok_idx, oi, new_tok_idx, ni);
            }
            tape.push(TapeChunk::Keep(nt[ni]));
            old_tok_idx = oi + 1;
            new_tok_idx = ni + 1;
        }

        if old_tok_idx < ot.len() || new_tok_idx < nt.len() {
            emit_gap(&mut tape, old_tok_idx, ot.len(), new_tok_idx, nt.len());
        }

        crate::ensure!(
            tape.project_old() == old_text,
            "AlignmentTape project_old must equal old_text: expected {:?}, got {:?}",
            old_text,
            tape.project_old()
        );
        crate::ensure!(
            tape.project_new() == new_text,
            "AlignmentTape project_new must equal new_text: expected {:?}, got {:?}",
            new_text,
            tape.project_new()
        );

        tape
    }
}

#[cfg(test)]
mod tests;
