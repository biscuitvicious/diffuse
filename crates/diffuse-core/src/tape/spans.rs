//! Merged run synthesis, span construction, and whitespace adjustment.

use std::{slice, str};

use crate::{
    lcs::{char_len, char_slice},
    merge::{Run, RunKind},
    parse::Spans,
    tape::{AlignmentTape, TapeChunk},
};

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

impl<'a> AlignmentTape<'a> {
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
}
