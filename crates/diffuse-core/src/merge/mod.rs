//! One-for-one port of the merge machinery (PLAN.md §3): `wholesalePair`,
//! `delSpansToShow`, `delPairing`, `mergedRuns`, `displayText`,
//! `isWholesaleMod`, `isPureDeletionMod`. Comments ported verbatim; char
//! offsets throughout (the lcs.rs convention).

use rustc_hash::{FxHashMap, FxHashSet};

use serde::{Deserialize, Serialize};

use crate::{
    lcs::char_slice,
    parse::{Row, Spans},
    syntax::{prelude::is_ascii_word_char, registry::first_ascii_word},
};

/// The semantic classification of a token run in a diff row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunKind {
    Common,
    Del,
    Add,
    Ctx,
    /// Moved text: verbatim relocation (a whole `Mov` row) or a reflowed
    /// fragment (struck-moved on the source side, filled-moved on the target).
    /// Renders muted-connected rather than struck/filled.
    Mov,
}

impl RunKind {
    pub fn reverse(&self) -> Self {
        match self {
            RunKind::Common => RunKind::Common,
            RunKind::Del => RunKind::Add,
            RunKind::Add => RunKind::Del,
            RunKind::Ctx => RunKind::Ctx,
            RunKind::Mov => RunKind::Mov,
        }
    }
}

/// A merged run of a mod row's display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Run<'a> {
    pub kind: RunKind,
    pub text: &'a str,
}

impl<'a> Run<'a> {
    pub fn reverse(&self) -> Self {
        Run {
            kind: self.kind.reverse(),
            text: self.text,
        }
    }
}

/// The result of the del/add span pairing.
pub struct DelPairing {
    pub del_pos: Vec<usize>,
    pub pair: FxHashMap<usize, usize>,
    pub op_anchor: FxHashMap<usize, usize>,
}

/// Project the old side of a run sequence (Common + Del + Ctx).
pub fn project_old_runs(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if matches!(r.kind, RunKind::Common | RunKind::Del | RunKind::Ctx) {
            s.push_str(r.text);
        }
    }
    s
}

/// Project the new side of a run sequence (Common + Add + Ctx).
pub fn project_new_runs(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if matches!(r.kind, RunKind::Common | RunKind::Add | RunKind::Ctx) {
            s.push_str(r.text);
        }
    }
    s
}

/// The Unicode `White_Space` property plus the zero-width space — the
/// tokenizer's whitespace class.
fn is_ws_class(c: char) -> bool {
    c.is_whitespace() || c == '\u{200b}'
}

fn all_ws(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_ws_class)
}

pub(crate) fn char_at(s: &str, i: usize) -> Option<char> {
    s.chars().nth(i)
}

/**
 * The wholesale decision from a raw pair: the artifact-filtered del spans,
 * the mismatched fragment counts, and the split-word insertions (a
 * standalone removal landing mid-ascii-word: `fro~~upper~~zen`). A
 * wholesale pair reads as garble inline — the collapse un-pairs it into the
 * `-`/`+` baseline rows (a `~` row must never carry an embedded "newline" —
 * the whole-old-struck + whole-new-filled blocks look like one, so they get
 * the plain two rows instead). Shared with the collapse so the two never
 * drift apart.
 * */
/// Exact display position of each del span in the merged (new-text) view.
///
/// Position `display_pos[j]` is the new-text character index where del span `j`
/// begins, determined by projecting the deletion offset against the LCS common
/// token boundaries between `del_text` and `add_text`.
pub fn del_display_positions(
    del_spans: &[(usize, usize)],
    _add_spans: &[(usize, usize)],
    del_text: &str,
    add_text: &str,
) -> Vec<usize> {
    if del_text.is_empty() || add_text.is_empty() {
        return vec![0; del_spans.len()];
    }
    let dtoks: Vec<&str> = crate::lcs::tokenize_line(del_text).collect();
    let atoks: Vec<&str> = crate::lcs::tokenize_line(add_text).collect();
    let pairs = crate::lcs::lcs_pairs(&dtoks, &atoks);

    let mut d_spans = Vec::with_capacity(dtoks.len());
    let mut d_pos = 0usize;
    for t in &dtoks {
        let len = crate::lcs::char_len(t);
        d_spans.push((d_pos, d_pos + len));
        d_pos += len;
    }

    let mut a_spans = Vec::with_capacity(atoks.len());
    let mut a_pos = 0usize;
    for t in &atoks {
        let len = crate::lcs::char_len(t);
        a_spans.push((a_pos, a_pos + len));
        a_pos += len;
    }

    let mut positions = Vec::with_capacity(del_spans.len());
    for (ds, _de) in del_spans {
        let mut ins_pos = 0usize;
        for (di, (d_start, d_end)) in d_spans.iter().enumerate() {
            if *d_end <= *ds {
                if let Some(&ai) = pairs.get(&di) {
                    ins_pos = a_spans[ai].1;
                }
            } else if *d_start < *ds {
                if let Some(&ai) = pairs.get(&di) {
                    let offset = ds - d_start;
                    ins_pos = a_spans[ai].0 + offset;
                }
                break;
            } else {
                break;
            }
        }
        positions.push(ins_pos);
    }
    positions
}

pub fn wholesale_pair(del_text: &str, add_text: &str, spans: &Spans) -> bool {
    let del_spans: Vec<(usize, usize)> = spans
        .del
        .iter()
        .filter(|(ds, de)| {
            let t = char_slice(del_text, *ds, *de).trim();
            !t.is_empty() && !add_text.contains(t)
        })
        .copied()
        .collect();
    let add_spans = &spans.add;
    // Meaningful identity = subword: any pair recognisably "the same line"
    // renders as the inline mod — fragment-count mismatches included (`0` →
    // `Option<`/`>`/`None`, `lowers`+`.rev()` → `below`, `k <`+` {` →
    // `let Some(…)`). Only the split-word check below (a standalone del
    // landing mid-ascii_word) is garble — that one stays wholesale.
    let pairing = del_pairing(&del_spans, add_spans, add_text, del_text);
    // An ASCII-word-led standalone del landing mid-ascii_word is garble (e.g.
    // `m~~them~~ust`); a punctuation-led fragment (`.rev()`, ` {`) reads fine
    // struck mid-ascii_word (`enumer~~.rev()~~ate()` still reconstructs
    // `enumerate()`).
    del_spans.iter().enumerate().any(|(j, (ds, de))| {
        if pairing.pair.contains_key(&j) {
            return false;
        }
        let del_span_text = char_slice(del_text, *ds, *de);
        if del_span_text.trim().contains(' ') {
            return false;
        }
        let add_len = char_len_of(add_text);
        let ins = pairing.del_pos[j].min(add_len);
        let left = ins.checked_sub(1).and_then(|i| char_at(add_text, i));
        let right = char_at(add_text, ins);
        if !(ins > 0
            && ins < add_len
            && left.is_some_and(is_ascii_word_char)
            && right.is_some_and(is_ascii_word_char))
        {
            return false;
        }
        first_ascii_word(del_span_text).is_some()
    })
}

/// The artifact-filtered del spans a mod row shows (see the TS).
pub fn del_spans_to_show(r: &Row, del_spans_in: Option<&[(usize, usize)]>) -> Vec<(usize, usize)> {
    let Some(del_text) = r.del_text() else {
        return Vec::new();
    };
    let spans = r.spans();
    let del_spans: &[(usize, usize)] = match del_spans_in {
        Some(s) => s,
        None => spans.map_or(&[], |s| s.del.as_slice()),
    };
    del_spans_to_show_texts(del_text, r.text(), del_spans)
}

/// The artifact-filtered del spans computed directly from texts and spans
/// without Row allocation.
pub fn del_spans_to_show_texts(
    del_text: &str,
    add_text: &str,
    del_spans: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    del_spans
        .iter()
        .filter(|(ds, de)| {
            let raw = char_slice(del_text, *ds, *de);
            let t = raw.trim();
            // A whitespace-only span is a real indent change (8 → 12 spaces
            // moves the line into a block): show it struck at the line
            // start. Drop it only when the add's leading run is identical.
            if t.is_empty() {
                let leading_len = add_text.len()
                    - add_text
                        .trim_start_matches(|c: char| c.is_ascii_whitespace())
                        .len();
                let leading = &add_text[..leading_len];
                return raw != leading;
            }
            // A single-span del stays even when its text is inside the add
            // (`None` → `return None;` is a growth — the mod must have
            // something struck). With MULTIPLE spans, a span is an absorbed
            // fragment only when its text sits in the add AT THE SAME
            // POSITION (`..]` → `..].` — the `]` survives where it was, the
            // `.` is appended). A word that merely appears elsewhere in the
            // add (`lower` in the del's `"Found in lower layer"` vs
            // `?next_lower,` in the add) is a REAL replacement, not
            // absorption — `r.text.includes(t)` would drop it (the
            // `lower` → `layer below the top upper` rewrite reads as a pure
            // fill with no strike).
            if del_spans.len() <= 1 {
                return true;
            }
            char_slice(add_text, *ds, *de) != t
        })
        .copied()
        .collect()
}

/// The add-text position of each del span (the del offset minus the del
/// text removed before it) and the pairing: a del span pairs with the add
/// span nearest its own position (within a small window — the aligned
/// replacement); a del span with no nearby add is a standalone removal.
/// Shared by the wholesale decision and the merged walk — they stay in
/// lockstep by construction.
pub fn del_pairing(
    del_spans: &[(usize, usize)],
    add_spans: &[(usize, usize)],
    add_text: &str,
    del_text: &str,
) -> DelPairing {
    let del_pos = del_display_positions(del_spans, add_spans, del_text, add_text);
    let mut pair: FxHashMap<usize, usize> = FxHashMap::default();
    let mut used_adds: FxHashSet<usize> = FxHashSet::default();
    let mut op_anchor: FxHashMap<usize, usize> = FxHashMap::default();
    for (j, (ds, de)) in del_spans.iter().enumerate() {
        let mut best: Option<usize> = None;
        let mut best_gap: usize = 8;
        for (i, (as0, ae)) in add_spans.iter().enumerate() {
            if used_adds.contains(&i) {
                continue;
            }
            let del_start = del_pos[j];
            let del_end = del_start + (de - ds);
            let add_start = *as0;
            let add_end = *ae;
            let same_start = del_start == add_start;
            let disjoint = del_end <= add_start || add_end <= del_start;
            let ws_pair =
                all_ws(char_slice(del_text, *ds, *de)) && all_ws(char_slice(add_text, *as0, *ae));
            if !same_start && !disjoint && !ws_pair {
                continue;
            }
            let del_op = char_slice(del_text, *ds, *de).trim();
            let is_compare_op = matches!(del_op, "<" | ">" | "<=" | ">=");
            if is_compare_op {
                let first_op_char = del_op.chars().next().unwrap();
                let mut op_best: Option<usize> = None;
                let mut op_best_char: Option<usize> = None;
                let mut op_best_gap = usize::MAX;
                for c in *as0..*ae {
                    if let Some(ch) = char_at(add_text, c)
                        && (ch == '<' || ch == '>')
                    {
                        let g = del_pos[j].abs_diff(c) + if ch == first_op_char { 0 } else { 2 };
                        if g < op_best_gap {
                            op_best_gap = g;
                            op_best = Some(i);
                            op_best_char = Some(c);
                        }
                    }
                }
                // The op match must still compete across add spans — the
                // winner is the smallest op gap, not the last span scanned
                // (an unconditional win crossed the pairing when several
                // spans contain `<`/`>` chars).
                if let Some(ob) = op_best
                    && op_best_gap < best_gap
                {
                    best = Some(ob);
                    best_gap = op_best_gap;
                    op_anchor.insert(j, op_best_char.expect("op char"));
                }
                continue;
            }
            let first = char_at(add_text, *as0);
            let gap = del_start.abs_diff(add_start)
                + if first.is_some_and(is_ascii_word_char) {
                    0
                } else {
                    5
                };
            if ws_pair {
                best_gap = 0;
                best = Some(i);
                continue;
            }
            if gap <= best_gap {
                best_gap = gap;
                best = Some(i);
            }
        }
        if let Some(b) = best {
            used_adds.insert(b);
            pair.insert(j, b);
        }
    }
    DelPairing {
        del_pos,
        pair,
        op_anchor,
    }
}

/// True when a mod row renders as the whole-old + whole-new pair (the
/// split-word garble gate).
pub fn is_wholesale_mod(r: &Row) -> bool {
    let Some(del_text) = r.del_text() else {
        return false;
    };
    let spans = r.spans().cloned().unwrap_or_default();
    wholesale_pair(del_text, r.text(), &spans)
}

/// A mod whose change is purely a removal: every add span is whitespace —
/// the new line is the old line minus text (`pub(super) struct X {` →
/// `pub struct X {`, `resolved_layers` → `layers`). Nothing was added, so
/// the row must not render as an addition: it takes the neutral shade
/// (like a moved line — the line survives) with the red strikes carrying
/// the removal.
pub fn is_pure_deletion_mod(r: &Row) -> bool {
    match r.spans() {
        None => true,
        Some(spans) => spans
            .add
            .iter()
            .all(|(s, e)| char_slice(r.text(), *s, *e).trim().is_empty()),
    }
}

/// The ordered common/del/add runs of a mod's merged view. Each del span
/// pairs with the add span nearest its own add-position (within a small
/// window — the aligned replacement); a del span with no nearby add (a
/// standalone removal like `resolved_`) inserts at its own position, and a
/// The merged runs of a mod row's display.
pub fn merged_runs<'a>(r: &'a Row) -> Vec<Run<'a>> {
    static EMPTY_SPANS: Spans = Spans {
        del: Vec::new(),
        add: Vec::new(),
    };
    let del_text = r.del_text().unwrap_or_default();
    let spans = r.spans().unwrap_or(&EMPTY_SPANS);
    merged_runs_from_texts(del_text, r.text(), spans)
}

/// The ordered common/del/add runs of a mod's merged view from texts and spans
/// directly.
pub fn merged_runs_from_texts<'a>(
    del_text: &'a str,
    add_text: &'a str,
    spans: &Spans,
) -> Vec<Run<'a>> {
    if wholesale_pair(del_text, add_text, spans) {
        return vec![
            Run {
                kind: RunKind::Del,
                text: del_text,
            },
            Run {
                kind: RunKind::Add,
                text: add_text,
            },
        ];
    }
    let add_spans = &spans.add;
    let del_spans = del_spans_to_show_texts(del_text, add_text, &spans.del);
    let DelPairing {
        del_pos,
        pair,
        op_anchor,
    } = del_pairing(&del_spans, add_spans, add_text, del_text);
    let mut runs: Vec<Run<'a>> = Vec::new();
    let emit_range = |runs: &mut Vec<Run<'a>>, from: usize, to: usize| {
        if to > from {
            runs.push(Run {
                kind: RunKind::Common,
                text: char_slice(add_text, from, to),
            });
        }
    };
    // Events in add-text position order: each add span, each standalone
    // del (no nearby add), and each del paired with its nearest add. A
    // standalone removal must not trail the line — a `resolved_` deleted
    // near the start inserts before the appended `.iter()` at the end.
    struct Ev {
        pos: usize,
        del: Option<(usize, usize)>,
        add: Option<(usize, usize)>,
    }
    let mut events: Vec<Ev> = Vec::with_capacity(del_spans.len() + add_spans.len());
    let mut used_adds: FxHashSet<usize> =
        FxHashSet::with_capacity_and_hasher(add_spans.len(), Default::default());
    for (j, dspan) in del_spans.iter().enumerate() {
        if let Some(ai) = pair.get(&j) {
            used_adds.insert(*ai);
            // An operator flip anchors at the operator's own offset
            // (`k ~~<~~> floor` — the `<` strikes against the `>`).
            let pos = op_anchor.get(&j).copied().unwrap_or(add_spans[*ai].0);
            events.push(Ev {
                pos,
                del: Some(*dspan),
                add: Some(add_spans[*ai]),
            });
        } else {
            // del_pos[j] is already the exact display position
            // (del_display_positions applied the add-correction);
            // just clamp to the new-text length.
            let add_len = char_len_of(add_text);
            let ins = del_pos[j].min(add_len);
            events.push(Ev {
                pos: ins,
                del: Some(*dspan),
                add: None,
            });
        }
    }
    for (i, aspan) in add_spans.iter().enumerate() {
        if !used_adds.contains(&i) {
            events.push(Ev {
                pos: aspan.0,
                del: None,
                add: Some(*aspan),
            });
        }
    }
    events.sort_by_key(|e| e.pos);
    let mut pos = 0usize;
    for ev in events {
        if ev.pos > pos {
            emit_range(&mut runs, pos, ev.pos);
        }
        if let Some(dspan) = ev.del {
            // Whitespace pairs express the MINIMUM change: the shared
            // prefix survives as common, only the difference is struck
            // or filled (8 → 12 spaces = 8 common + 4 added, never the
            // whole run on each side).
            let del_text_span = char_slice(del_text, dspan.0, dspan.1);
            if let Some(aspan) = ev.add {
                let add_text_span = char_slice(add_text, aspan.0, aspan.1);
                if all_ws(del_text_span) && all_ws(add_text_span) {
                    let is_leading = dspan.0 == 0 && aspan.0 == 0;
                    if is_leading {
                        // For leading indentation, align changes to the first
                        // possible location
                        // (normally col 0) so whitespace changes line up
                        // cleanly across rows
                        // with different indentation levels without jagged
                        // edges.
                        let d_bytes = del_text_span.as_bytes();
                        let a_bytes = add_text_span.as_bytes();
                        if d_bytes.len() < a_bytes.len() {
                            let diff = a_bytes.len() - d_bytes.len();
                            let mut suffix_match = 0;
                            while suffix_match < d_bytes.len()
                                && d_bytes[d_bytes.len() - 1 - suffix_match]
                                    == a_bytes[a_bytes.len() - 1 - suffix_match]
                            {
                                suffix_match += 1;
                            }
                            if suffix_match == d_bytes.len() {
                                runs.push(Run {
                                    kind: RunKind::Add,
                                    text: &add_text_span[..diff],
                                });
                                runs.push(Run {
                                    kind: RunKind::Common,
                                    text: &add_text_span[diff..],
                                });
                                pos = aspan.1;
                                continue;
                            }
                        } else if d_bytes.len() > a_bytes.len() {
                            let diff = d_bytes.len() - a_bytes.len();
                            let mut suffix_match = 0;
                            while suffix_match < a_bytes.len()
                                && d_bytes[d_bytes.len() - 1 - suffix_match]
                                    == a_bytes[a_bytes.len() - 1 - suffix_match]
                            {
                                suffix_match += 1;
                            }
                            if suffix_match == a_bytes.len() {
                                runs.push(Run {
                                    kind: RunKind::Del,
                                    text: &del_text_span[..diff],
                                });
                                runs.push(Run {
                                    kind: RunKind::Common,
                                    text: add_text_span,
                                });
                                pos = aspan.1;
                                continue;
                            }
                        }
                    }

                    // The shared prefix is the ACTUAL matching chars — a
                    // type change (`\t` vs 4 spaces) shares nothing: the
                    // tab strikes, the spaces fill, both subworded.
                    let mut share = 0usize;
                    let d_bytes = del_text_span.as_bytes();
                    let a_bytes = add_text_span.as_bytes();
                    while share < d_bytes.len()
                        && share < a_bytes.len()
                        && d_bytes[share] == a_bytes[share]
                    {
                        share += 1;
                    }
                    if share > 0 {
                        runs.push(Run {
                            kind: RunKind::Common,
                            text: &del_text_span[..share],
                        });
                    }
                    if share < del_text_span.len() {
                        runs.push(Run {
                            kind: RunKind::Del,
                            text: &del_text_span[share..],
                        });
                    }
                    if share < add_text_span.len() {
                        runs.push(Run {
                            kind: RunKind::Add,
                            text: &add_text_span[share..],
                        });
                    }
                    pos = aspan.1;
                    continue;
                }
            }
            runs.push(Run {
                kind: RunKind::Del,
                text: del_text_span,
            });
            pos = pos.max(ev.pos);
        }
        if let Some(aspan) = ev.add {
            runs.push(Run {
                kind: RunKind::Add,
                text: char_slice(add_text, aspan.0, aspan.1),
            });
            pos = aspan.1;
        }
    }
    if pos < char_len_of(add_text) {
        emit_range(&mut runs, pos, char_len_of(add_text));
    }
    coalesce_runs(runs)
}

/// Compress adjacent runs of the same kind into a single run with joined text.
pub fn coalesce_runs<'a>(runs: Vec<Run<'a>>) -> Vec<Run<'a>> {
    let mut out: Vec<Run<'a>> = Vec::new();
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.kind == run.kind
            && last.text.as_ptr().wrapping_add(last.text.len()) == run.text.as_ptr()
        {
            last.text = unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                    last.text.as_ptr(),
                    last.text.len() + run.text.len(),
                ))
            };
            continue;
        }
        out.push(run);
    }
    out
}

fn char_len_of(s: &str) -> usize {
    s.chars().count()
}

/// The merged display string of a row (the runs joined; a wholesale mod is
/// the whole-old + whole-new pair).
pub fn display_text(r: &Row) -> String {
    let use_merged = (matches!(r, Row::Mod { .. })
        || r.is_moved()
        || (matches!(r, Row::Add(_)) && r.del_text().is_some()))
        && r.del_text().is_some();
    if !use_merged {
        return r.text().to_string();
    }
    if is_wholesale_mod(r) {
        return format!("{}{}", r.del_text().unwrap_or_default(), r.text());
    }
    merged_runs(r).iter().map(|x| x.text).collect()
}

/// Project the row runs forward to reconstruct the new line (stripping Del
/// runs).
pub fn project_forward(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| r.kind != RunKind::Del)
        .map(|r| r.text)
        .collect()
}

/// Project the row runs backward to reconstruct the old line (stripping Add
/// runs).
pub fn project_reverse(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| r.kind != RunKind::Add)
        .map(|r| r.text)
        .collect()
}

#[cfg(test)]
mod tests;
