//! Universal subword token stream & multi-line markup (Phase 4).
//!
//! [`render_solution`] turns a [`DpSolution`] into a [`RenderedRow`] per
//! emitted row — the row (with whatever spans/delText it can carry), the
//! wire spans, and the merged run stream — for EVERY row kind:
//!
//! - `Mov` rows: a whole-line `Mov` run (verbatim relocation); an indentation
//!   reflow subwords via `word_spans` with the common parts rendered moved (8 →
//!   12 spaces = 8 moved + 4 filled).
//! - `Mod` rows: `word_spans` + the standard merged runs (whitespace
//!   contraction and delimiter snapping ride the existing machinery).
//! - Block transitions (1-to-K expansions, K-to-1 collapses): the anchor
//!   `Mod`'s tail that reflowed onto another line renders struck-moved /
//!   filled-moved (`Mov` runs), the absorbed `Add`/`Del` rows carry the
//!   reflowed fragments as `Mov` runs with their filler as plain `Add`/`Del` —
//!   the multi-line highlight distribution of plan §4.2.
//!
//! Deliberately pipeline-inert until the switchover: nothing here is
//! reachable from `run_pipeline` yet; the wire contract only gains the
//! additive `Mov` run kind when the DP engine becomes the default.

use super::dp::{DpOp, DpSolution, SublineMove};
use crate::{
    lcs::{char_len, char_slice, word_spans},
    merge::{Run, RunKind, merged_runs_from_texts},
    parse::{AddLine, DelLine, Row, Spans},
};

/// One rendered row: the row itself, the wire spans, and the run stream.
pub struct RenderedRow<'a> {
    pub row: Row,
    pub spans: Spans,
    pub runs: Vec<Run<'a>>,
}

/// Render a solution's rows into universal run streams.
pub fn render_solution<'a>(sol: &'a DpSolution) -> Vec<RenderedRow<'a>> {
    let mut out: Vec<RenderedRow<'a>> = Vec::new();
    let mut ri = 0usize;
    for t in &sol.transitions {
        match t {
            DpOp::Del => {
                let r = &sol.rows[ri];
                ri += 1;
                out.push(RenderedRow {
                    row: r.clone(),
                    spans: Spans::default(),
                    runs: vec![run(RunKind::Del, r.text())],
                });
            }
            DpOp::Add => {
                let r = &sol.rows[ri];
                ri += 1;
                out.push(RenderedRow {
                    row: r.clone(),
                    spans: Spans::default(),
                    runs: vec![run(RunKind::Add, r.text())],
                });
            }
            DpOp::Mov => {
                let r = &sol.rows[ri];
                ri += 1;
                out.push(render_mov_row(r));
            }
            DpOp::Mod { .. } => {
                let r = &sol.rows[ri];
                ri += 1;
                let (old_line, new_line) = match r {
                    Row::Mod { old_line, new_line } => (old_line, new_line),
                    other => panic!("expected Mod row: {other:?}"),
                };
                out.push(render_mod_row(
                    old_line,
                    new_line,
                    &old_line.text,
                    &new_line.text,
                    Side::Old,
                    &[],
                ));
            }
            DpOp::Expand {
                k, anchor, spans, ..
            } => {
                let block = &sol.rows[ri..ri + k];
                ri += k;
                render_expand(*anchor, spans, block, &mut out);
            }
            DpOp::Collapse {
                k, anchor, spans, ..
            } => {
                let block = &sol.rows[ri..ri + k];
                ri += k;
                render_collapse(*anchor, spans, block, &mut out);
            }
        }
    }
    out
}

/// A `Mov` row: whole-line moved run, or (indentation reflow) the
/// merged subword runs with the common parts rendered moved.
fn render_mov_row<'a>(r: &'a Row) -> RenderedRow<'a> {
    match r {
        Row::MovSrc(del_line) => RenderedRow {
            row: r.clone(),
            spans: Spans::default(),
            runs: vec![run(RunKind::Del, &del_line.text)],
        },
        Row::MovTgt(new_line) => {
            if let Some(ref dt) = new_line.del_text {
                if dt == &new_line.text {
                    return RenderedRow {
                        row: r.clone(),
                        spans: Spans::default(),
                        runs: vec![run(RunKind::Mov, &new_line.text)],
                    };
                }
                // Indentation reflow: subword via word_spans, common → moved.
                let ws = word_spans(dt, &new_line.text);
                let spans = Spans {
                    del: ws.del.clone(),
                    add: ws.add.clone(),
                };
                let mut moved_line = new_line.clone();
                moved_line.spans = spans.clone();
                let runs = subword_runs_texts(dt, &new_line.text, &spans)
                    .into_iter()
                    .map(movify_common)
                    .collect();
                RenderedRow {
                    row: Row::MovTgt(moved_line),
                    spans,
                    runs,
                }
            } else {
                RenderedRow {
                    row: r.clone(),
                    spans: Spans::default(),
                    runs: vec![run(RunKind::Mov, &new_line.text)],
                }
            }
        }
        other => panic!("render_mov_row on a non-Mov row: {other:?}"),
    }
}

/// A plain 1-to-1 `Mod` row: word spans + the standard merged runs (with
/// whitespace contraction and delimiter snapping — the legacy machinery).
/// `moved` ranges (when present) mark reflowed fragments of a block
/// transition on the given side — those runs become `Mov`.
fn render_mod_row<'a>(
    old_line: &DelLine,
    new_line: &AddLine,
    old_text: &'a str,
    new_text: &'a str,
    side: Side,
    moved: &[(usize, usize)],
) -> RenderedRow<'a> {
    let ws = word_spans(old_text, new_text);
    let spans = Spans {
        del: ws.del.clone(),
        add: ws.add.clone(),
    };
    let mut nl = new_line.clone();
    nl.spans = spans.clone();
    let runs = subword_runs_texts(old_text, new_text, &spans);
    let runs = if moved.is_empty() {
        runs
    } else {
        mark_side_runs(runs, side, moved)
    };
    RenderedRow {
        row: Row::Mod {
            old_line: old_line.clone(),
            new_line: nl,
        },
        spans,
        runs,
    }
}

#[inline]
fn run<'a>(kind: RunKind, text: &'a str) -> Run<'a> {
    Run { kind, text }
}

/// The merged runs of a span-carrying mod probe — or a wholesale
/// whole-line pair when `word_spans` produced nothing (huge lines past the
/// token cap: an all-common render would silently hide the change).
fn subword_runs_texts<'a>(del_text: &'a str, add_text: &'a str, spans: &Spans) -> Vec<Run<'a>> {
    let changed = !spans.del.is_empty() || !spans.add.is_empty();
    if !changed && del_text != add_text {
        return vec![run(RunKind::Del, del_text), run(RunKind::Add, add_text)];
    }
    merged_runs_from_texts(del_text, add_text, spans)
}

/// Which side of a mod's run stream a conversion applies to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Old,
    New,
}

/// Rewrite del (old side) or add (new side) runs that overlap any of the
/// given char ranges into `Mov` runs (struck-moved / filled-moved) — the
/// reflowed fragments of a block transition.
fn mark_side_runs<'a>(runs: Vec<Run<'a>>, side: Side, ranges: &[(usize, usize)]) -> Vec<Run<'a>> {
    let overlaps = |a: (usize, usize), b: (usize, usize)| a.0 < b.1 && b.0 < a.1;
    let mut out = Vec::with_capacity(runs.len());
    let mut old_pos = 0usize;
    let mut new_pos = 0usize;
    for r in runs {
        let len = char_len(r.text);
        let mut r = r;
        match r.kind {
            RunKind::Common => {
                old_pos += len;
                new_pos += len;
            }
            RunKind::Del => {
                let span = (old_pos, old_pos + len);
                old_pos += len;
                if side == Side::Old && ranges.iter().any(|&rg| overlaps(span, rg)) {
                    r.kind = RunKind::Mov;
                }
            }
            RunKind::Add => {
                let span = (new_pos, new_pos + len);
                new_pos += len;
                if side == Side::New && ranges.iter().any(|&rg| overlaps(span, rg)) {
                    r.kind = RunKind::Mov;
                }
            }
            _ => {}
        }
        out.push(r);
    }
    out
}

/// A 1-to-K expansion: the anchor `Mod` (its old-side tail that reflowed
/// onto later lines struck-moved) + the absorbed `Add` rows (reflowed
/// fragments filled-moved, delText connecting them to the source line).
fn render_expand<'a>(
    anchor: usize,
    spans: &[SublineMove],
    block: &'a [Row],
    out: &mut Vec<RenderedRow<'a>>,
) {
    // The anchor row is the block's Mod; its old text is everyone's source.
    let (a_old, a_new) = block
        .iter()
        .find_map(|r| match r {
            Row::Mod { old_line, new_line } => Some((old_line, new_line)),
            _ => None,
        })
        .expect("expand block carries an anchor Mod");
    // Old-side char ranges that reflowed onto other lines.
    let moved_old: Vec<(usize, usize)> = spans
        .iter()
        .filter(|s| s.line != anchor)
        .map(|s| s.anchor_span)
        .collect();
    out.push(render_mod_row(
        a_old,
        a_new,
        &a_old.text,
        &a_new.text,
        Side::Old,
        &moved_old,
    ));

    for (li, r) in block.iter().enumerate() {
        if li == anchor {
            continue;
        }
        let a = match r {
            Row::Add(a) => a,
            other => panic!("expand absorbed row is not an Add: {other:?}"),
        };
        out.push(absorbed_add_row(&a_old.text, &a.text, a.num, spans, li));
    }
}

/// A K-to-1 collapse: the anchor `Mod` (new-side fragments absorbed from
/// other old lines filled-moved) + the absorbed `Del` rows (reflowed
/// fragments struck-moved).
fn render_collapse<'a>(
    anchor: usize,
    spans: &[SublineMove],
    block: &'a [Row],
    out: &mut Vec<RenderedRow<'a>>,
) {
    let (a_old, a_new) = block
        .iter()
        .find_map(|r| match r {
            Row::Mod { old_line, new_line } => Some((old_line, new_line)),
            _ => None,
        })
        .expect("collapse block carries an anchor Mod");
    // New-side char ranges absorbed from other old lines.
    let moved_new: Vec<(usize, usize)> = spans
        .iter()
        .filter(|s| s.line != anchor)
        .map(|s| s.anchor_span)
        .collect();
    out.push(render_mod_row(
        a_old,
        a_new,
        &a_old.text,
        &a_new.text,
        Side::New,
        &moved_new,
    ));

    for (li, r) in block.iter().enumerate() {
        if li == anchor {
            continue;
        }
        let d = match r {
            Row::Del(d) => d,
            other => panic!("collapse absorbed row is not a Del: {other:?}"),
        };
        out.push(absorbed_del_row(d, spans, li));
    }
}

/// An absorbed `Add` row of an expansion: the reflowed fragments render
/// filled-moved, the rest fills. The row carries `delText` (the source
/// line) + spans connecting the moved fragments to their origin.
fn absorbed_add_row<'a>(
    old_text: &str,
    add_text: &'a str,
    num: crate::parse::LineNum,
    spans: &[SublineMove],
    li: usize,
) -> RenderedRow<'a> {
    let mut frags: Vec<(usize, usize)> = spans
        .iter()
        .filter(|s| s.line == li)
        .map(|s| s.line_span)
        .collect();
    frags.sort_unstable();
    let runs = fragment_runs(add_text, &frags, RunKind::Add, RunKind::Mov);
    let spans_out = Spans {
        del: spans
            .iter()
            .filter(|s| s.line == li)
            .map(|s| s.anchor_span)
            .collect(),
        add: frags.clone(),
    };
    let add_line = AddLine {
        num,
        text: add_text.to_string(),
        del_text: Some(old_text.to_string()),
        spans: spans_out.clone(),
        group: None,
        cached_runs: Vec::new(),
    };
    RenderedRow {
        row: Row::Add(add_line),
        spans: spans_out,
        runs,
    }
}

/// An absorbed `Del` row of a collapse: the reflowed fragments render
/// struck-moved, the rest strikes. (`DelLine` cannot carry spans — the
/// wire spans ride the [`RenderedRow`].)
fn absorbed_del_row<'a>(d: &'a DelLine, spans: &[SublineMove], li: usize) -> RenderedRow<'a> {
    let mut frags: Vec<(usize, usize)> = spans
        .iter()
        .filter(|s| s.line == li)
        .map(|s| s.line_span)
        .collect();
    frags.sort_unstable();
    let runs = fragment_runs(&d.text, &frags, RunKind::Del, RunKind::Mov);
    let spans_out = Spans {
        del: frags.clone(),
        add: spans
            .iter()
            .filter(|s| s.line == li)
            .map(|s| s.anchor_span)
            .collect(),
    };
    RenderedRow {
        row: Row::Del(d.clone()),
        spans: spans_out,
        runs,
    }
}

/// Partition a line's text into filler runs and fragment runs: the chars
/// inside each fragment become `frag_kind` (moved), everything between
/// them `filler_kind`.
fn fragment_runs<'a>(
    text: &'a str,
    frags: &[(usize, usize)],
    filler_kind: RunKind,
    frag_kind: RunKind,
) -> Vec<Run<'a>> {
    let frags = merge_ws_frags(text, frags);
    let mut runs = Vec::new();
    let mut pos = 0usize;
    let total = char_len(text);
    for &(fs, fe) in &frags {
        if fs > pos {
            runs.push(run(filler_kind, char_slice(text, pos, fs)));
        }
        runs.push(run(frag_kind, char_slice(text, fs, fe)));
        pos = fe.max(pos);
    }
    if pos < total {
        runs.push(run(filler_kind, char_slice(text, pos, total)));
    }
    runs
}

/// Merge fragments whose gap is all-whitespace into one contiguous
/// fragment (per-token spans read as confetti — `return`·`self`·`.` is one
/// moved phrase, the spaces between are part of it). Same rule as
/// `word_spans`' whitespace-gap merging.
fn merge_ws_frags(text: &str, frags: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if frags.is_empty() {
        return frags.to_vec();
    }
    let mut merged = vec![frags[0]];
    for &(fs, fe) in frags.iter().skip(1) {
        let last = merged.last_mut().expect("non-empty");
        let gap = char_slice(text, last.1, fs);
        if gap.chars().all(char::is_whitespace) {
            last.1 = fe;
        } else {
            merged.push((fs, fe));
        }
    }
    merged
}

/// Common runs → moved (a `Mov` row's subword: the common text is by
/// definition the relocated content).
fn movify_common<'a>(r: Run<'a>) -> Run<'a> {
    if r.kind == RunKind::Common {
        Run {
            kind: RunKind::Mov,
            text: r.text,
        }
    } else {
        r
    }
}
