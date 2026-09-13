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

use crate::{
    lcs::{
        char_len, char_slice, lcs_pairs_anchored, raw_lcs_pairs, token_product_cap, tokenize_line,
        unique_token_anchors,
    },
    merge::{Run, RunKind},
    parse::Spans,
    syntax::profile::LangProfile,
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

    /// Projects the old text: concatenates `Keep` + `Strike` + `Relocate`.
    pub fn project_old(&self) -> String {
        let mut s = String::new();
        for chunk in &self.chunks {
            match chunk {
                TapeChunk::Keep(text)
                | TapeChunk::Strike(text)
                | TapeChunk::Relocate { text, .. } => {
                    s.push_str(text);
                }
                TapeChunk::Fill(_) => {}
            }
        }
        s
    }

    /// Projects the new text: concatenates `Keep` + `Fill` + `Relocate`.
    pub fn project_new(&self) -> String {
        let mut s = String::new();
        for chunk in &self.chunks {
            match chunk {
                TapeChunk::Keep(text)
                | TapeChunk::Fill(text)
                | TapeChunk::Relocate { text, .. } => {
                    s.push_str(text);
                }
                TapeChunk::Strike(_) => {}
            }
        }
        s
    }

    /// Compute the algebraic inverse alignment tape.
    ///
    /// Swaps `Strike` and `Fill`, inverts `Relocate` spans, and preserves
    /// `Keep`. Satisfies the involution property: `tape.reverse().reverse()
    /// == tape`.
    pub fn reverse(&self) -> AlignmentTape<'a> {
        let mut chunks = Vec::with_capacity(self.chunks.len());
        for chunk in &self.chunks {
            let rev_chunk = match chunk {
                TapeChunk::Keep(s) => TapeChunk::Keep(s),
                TapeChunk::Strike(s) => TapeChunk::Fill(s),
                TapeChunk::Fill(s) => TapeChunk::Strike(s),
                TapeChunk::Relocate {
                    from_span,
                    to_span,
                    text,
                } => TapeChunk::Relocate {
                    from_span: *to_span,
                    to_span: *from_span,
                    text,
                },
            };
            chunks.push(rev_chunk);
        }
        AlignmentTape { chunks }
    }

    /// Convert alignment chunks directly into renderer `Run` items without
    /// heuristic reconstruction.
    pub fn to_merged_runs(&self) -> Vec<Run<'a>> {
        let mut runs: Vec<Run<'a>> = Vec::with_capacity(self.chunks.len() + 4);
        let mut is_leading = true;
        let mut idx = 0;

        while idx < self.chunks.len() {
            // Check for adjacent (Strike, Fill) pair
            if idx + 1 < self.chunks.len()
                && let (TapeChunk::Strike(d), TapeChunk::Fill(a)) =
                    (&self.chunks[idx], &self.chunks[idx + 1])
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
                                kind: RunKind::Common,
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
                                kind: RunKind::Common,
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
                                kind: RunKind::Common,
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
                            kind: RunKind::Common,
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

            let chunk = &self.chunks[idx];
            match chunk {
                TapeChunk::Keep(text) => {
                    if !all_ws_or_empty(text) {
                        is_leading = false;
                    }
                    runs.push(Run {
                        kind: RunKind::Common,
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
                        kind: RunKind::Mov,
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
                            std::slice::from_raw_parts(last.text.as_ptr(), combined_len);
                        last.text = std::str::from_utf8_unchecked(combined_slice);
                    }
                    continue;
                }
            }
            coalesced.push(run);
        }
        coalesced
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

        let anchors = unique_token_anchors(&ot, &nt, profile);
        let has_anchors = !anchors.is_empty();
        let span = tracing::Span::current();
        let pairs = if has_anchors {
            span.record("anchors", tracing::field::debug(&anchors));
            span.record("strategy", "anchored");
            lcs_pairs_anchored(&ot, &nt, &anchors)
        } else {
            span.record("strategy", "raw");
            raw_lcs_pairs(&ot, &nt)
        };

        let matched_chars: usize = pairs.values().map(|j| char_len(nt[*j])).sum();
        let ratio = if !new_text.is_empty() {
            matched_chars as f64 / char_len(new_text) as f64
        } else {
            0.0
        };
        span.record("matched_chars", matched_chars);
        span.record("ratio", ratio);

        if !has_anchors && ratio < 0.25 {
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

        let mut sorted_pairs: Vec<(usize, usize)> = pairs.into_iter().collect();
        sorted_pairs.sort_by_key(|&(o, _)| o);

        let mut old_tok_idx = 0usize;
        let mut new_tok_idx = 0usize;

        let emit_gap = |tape: &mut AlignmentTape<'a>,
                        old_from: usize,
                        old_to: usize,
                        new_from: usize,
                        new_to: usize| {
            let has_old = old_to > old_from;
            let has_new = new_to > new_from;

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

        tape
    }
}

#[cfg(test)]
mod tests;
