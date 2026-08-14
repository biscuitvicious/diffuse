//! One-for-one port of the merge machinery (PLAN.md §3): `wholesalePair`,
//! `delSpansToShow`, `delPairing`, `mergedRuns`, `displayText`,
//! `isWholesaleMod`, `isPureDeletionMod`. Comments ported verbatim; char
//! offsets throughout (the lcs.rs convention).

use crate::{
    lcs::char_slice,
    parse::{Row, Spans},
};
use std::collections::{HashMap, HashSet};

/// A merged run of a mod row's display.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Run {
    pub kind: RunKind,
    pub text: String,
}

impl Run {
    pub fn reverse(&self) -> Self {
        Run {
            kind: self.kind.reverse(),
            text: self.text.clone(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunKind {
    Common,
    Del,
    Add,
    Ctx,
}

impl RunKind {
    pub fn reverse(&self) -> RunKind {
        match self {
            RunKind::Add => RunKind::Del,
            RunKind::Del => RunKind::Add,
            RunKind::Common => RunKind::Common,
            RunKind::Ctx => RunKind::Ctx,
        }
    }
}

/// The Unicode `White_Space` property plus the zero-width space — the
/// tokenizer's whitespace class.
fn is_ws_class(c: char) -> bool {
    c.is_whitespace() || c == '\u{200b}'
}

fn all_ws(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_ws_class)
}

/// The TS `/[A-Za-z0-9_]/` test.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn char_at(s: &str, i: usize) -> Option<char> {
    s.chars().nth(i)
}

/**
 * The wholesale decision from a raw pair: the artifact-filtered del spans,
 * the mismatched fragment counts, and the split-word insertions (a
 * standalone removal landing mid-word: `fro~~upper~~zen`). A wholesale pair
 * reads as garble inline — the collapse un-pairs it into the `-`/`+`
 * baseline rows (a `~` row must never carry an embedded "newline" — the
 * whole-old-struck + whole-new-filled blocks look like one, so they get the
 * plain two rows instead). Shared with the collapse so the two never drift
 * apart.
 * */
/// Exact display position of each del span in the merged (new-text) view.
///
/// The merged display is the new text with del runs interspersed. Position
/// `display_pos[j]` is the new-text character index where del span `j` begins:
///
/// ```text
/// raw_pos[j]     = del_span[j].start − Σ(de−ds for del spans k < j)
/// as0_common[i]  = add_span[i].start − Σ(ae−as for add spans m < i)
/// display_pos[j] = raw_pos[j] + Σ(ae−as for add spans where as0_common <= raw_pos[j])
/// ```
///
/// `raw_pos` and `as0_common` project both deletion and addition offsets into
/// the shared common-subsequence coordinate space. Comparing `as0_common <=
/// raw` precisely counts all additions that occur at or before this deletion in
/// the stream, correctly placing trailing and inline deletions.
pub fn del_display_positions(
    del_spans: &[(usize, usize)],
    add_spans: &[(usize, usize)],
) -> Vec<usize> {
    let mut positions = Vec::with_capacity(del_spans.len());
    let mut cum_dels = 0usize;
    for (ds, de) in del_spans {
        let raw = ds - cum_dels;
        let mut cum_adds = 0usize;
        let mut add_common_offset = 0usize;
        for (as0, ae) in add_spans {
            let as0_common = as0 - add_common_offset;
            if as0_common <= raw {
                cum_adds += ae - as0;
            }
            add_common_offset += ae - as0;
        }
        positions.push(raw + cum_adds);
        cum_dels += de - ds;
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
    // landing mid-word) is garble — that one stays wholesale.
    let pairing = del_pairing(&del_spans, add_spans, add_text, del_text);
    // A WORD-led standalone del landing mid-word is garble (e.g. `m~~them~~ust`);
    // a punctuation-led fragment (`.rev()`, ` {`) reads fine struck mid-word
    // (`enumer~~.rev()~~ate()` still reconstructs `enumerate()`).
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
        if !(ins > 0 && ins < add_len && left.is_some_and(is_word) && right.is_some_and(is_word)) {
            return false;
        }
        let t = del_span_text.trim_start();
        t.chars().next().is_some_and(is_word)
    })
}

/// The artifact-filtered del spans a mod row shows (see the TS).
pub fn del_spans_to_show(r: &Row, del_spans_in: Option<&[(usize, usize)]>) -> Vec<(usize, usize)> {
    let del_spans: Vec<(usize, usize)> = match del_spans_in {
        Some(s) => s.to_vec(),
        None => r.spans().map_or(Vec::new(), |s| s.del.clone()),
    };
    if r.del_text().is_none() {
        return Vec::new();
    }
    let del_text = r.del_text().expect("del_text");
    del_spans
        .iter()
        .filter(|(ds, de)| {
            let raw = char_slice(del_text, *ds, *de);
            let t = raw.trim();
            // A whitespace-only span is a real indent change (8 → 12 spaces
            // moves the line into a block): show it struck at the line
            // start. Drop it only when the add's leading run is identical.
            if t.is_empty() {
                let leading = r
                    .text()
                    .chars()
                    .take_while(char::is_ascii_whitespace)
                    .collect::<String>();
                return raw != leading.as_str();
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
            char_slice(r.text(), *ds, *de) != t
        })
        .copied()
        .collect()
}

/// The result of the del/add span pairing.
pub struct DelPairing {
    pub del_pos: Vec<usize>,
    pub pair: HashMap<usize, usize>,
    pub op_anchor: HashMap<usize, usize>,
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
    let mut raw_pos: Vec<usize> = Vec::new();
    let mut cum = 0usize;
    for (ds, de) in del_spans {
        raw_pos.push(ds - cum);
        cum += de - ds;
    }
    let mut add_common_pos: Vec<usize> = Vec::new();
    let mut add_cum = 0usize;
    for (as0, ae) in add_spans {
        add_common_pos.push(as0 - add_cum);
        add_cum += ae - as0;
    }
    let mut pair: HashMap<usize, usize> = HashMap::new();
    let mut used_adds: HashSet<usize> = HashSet::new();
    let mut op_anchor: HashMap<usize, usize> = HashMap::new();
    for (j, (ds, de)) in del_spans.iter().enumerate() {
        let mut best: Option<usize> = None;
        let mut best_gap: usize = 8;
        for (i, (as0, ae)) in add_spans.iter().enumerate() {
            if used_adds.contains(&i) {
                continue;
            }
            let del_start = raw_pos[j];
            let del_end = del_start + (de - ds);
            let add_start = add_common_pos[i];
            let add_end = add_start + (ae - as0);
            let same_start = del_start == add_start;
            let disjoint = del_end <= add_start || add_end <= del_start;
            let ws_pair =
                all_ws(char_slice(del_text, *ds, *de)) && all_ws(char_slice(add_text, *as0, *ae));
            if !same_start && !disjoint && !ws_pair {
                continue;
            }
            let del_op = char_slice(del_text, *ds, *de).trim();
            let op_chars = del_op.chars().collect::<Vec<char>>();
            let is_compare_op = !op_chars.is_empty()
                && op_chars.len() <= 2
                && (op_chars[0] == '<' || op_chars[0] == '>')
                && (op_chars.len() == 1 || op_chars[1] == '=');
            if is_compare_op {
                let mut op_best: Option<usize> = None;
                let mut op_best_gap = usize::MAX;
                for c in *as0..*ae {
                    if let Some(ch) = char_at(add_text, c)
                        && (ch == '<' || ch == '>')
                    {
                        let g = raw_pos[j].abs_diff(c) + if ch == op_chars[0] { 0 } else { 2 };
                        if g < op_best_gap {
                            op_best_gap = g;
                            op_best = Some(i);
                            op_anchor.insert(j, c);
                        }
                    }
                }
                if let Some(ob) = op_best {
                    best = Some(ob);
                    best_gap = 0; // op wins over any gap rule
                    continue;
                }
            }
            let first = char_at(add_text, *as0);
            let gap =
                del_start.abs_diff(add_start) + if first.is_some_and(is_word) { 0 } else { 5 };
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
        del_pos: del_display_positions(del_spans, add_spans),
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
/// standalone add fills at its own position.
pub fn merged_runs(r: &Row) -> Vec<Run> {
    if is_wholesale_mod(r) {
        return vec![
            Run {
                kind: RunKind::Del,
                text: r.del_text().unwrap_or_default().to_string(),
            },
            Run {
                kind: RunKind::Add,
                text: r.text().to_string(),
            },
        ];
    }
    let del_text = r.del_text().unwrap_or_default().to_string();
    let spans = r.spans().cloned().unwrap_or_default();
    let add_spans = spans.add.clone();
    let del_spans = del_spans_to_show(r, Some(&spans.del));
    let DelPairing {
        del_pos,
        pair,
        op_anchor,
    } = del_pairing(&del_spans, &add_spans, r.text(), &del_text);
    let mut runs: Vec<Run> = Vec::new();
    let emit_range = |runs: &mut Vec<Run>, from: usize, to: usize| {
        if to > from {
            runs.push(Run {
                kind: RunKind::Common,
                text: char_slice(r.text(), from, to).to_string(),
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
    let mut events: Vec<Ev> = Vec::new();
    let mut used_adds: HashSet<usize> = HashSet::new();
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
            // del_pos[j] is already the exact display position (del_display_positions
            // applied the add-correction); just clamp to the new-text length.
            let add_len = char_len_of(r.text());
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
            let del_text_span = char_slice(&del_text, dspan.0, dspan.1);
            if let Some(aspan) = ev.add {
                let add_text_span = char_slice(r.text(), aspan.0, aspan.1);
                if all_ws(del_text_span) && all_ws(add_text_span) {
                    // The shared prefix is the ACTUAL matching chars — a
                    // type change (`\t` vs 4 spaces) shares nothing: the
                    // tab strikes, the spaces fill, both subworded.
                    let dch: Vec<char> = del_text_span.chars().collect();
                    let ach: Vec<char> = add_text_span.chars().collect();
                    let mut share = 0usize;
                    while share < dch.len() && share < ach.len() && dch[share] == ach[share] {
                        share += 1;
                    }
                    if share > 0 {
                        runs.push(Run {
                            kind: RunKind::Common,
                            text: dch[..share].iter().collect(),
                        });
                    }
                    if share < dch.len() {
                        runs.push(Run {
                            kind: RunKind::Del,
                            text: dch[share..].iter().collect(),
                        });
                    }
                    if share < ach.len() {
                        runs.push(Run {
                            kind: RunKind::Add,
                            text: ach[share..].iter().collect(),
                        });
                    }
                    pos = aspan.1;
                    continue;
                }
            }
            runs.push(Run {
                kind: RunKind::Del,
                text: del_text_span.to_string(),
            });
            pos = pos.max(ev.pos);
        }
        if let Some(aspan) = ev.add {
            runs.push(Run {
                kind: RunKind::Add,
                text: char_slice(r.text(), aspan.0, aspan.1).to_string(),
            });
            pos = aspan.1;
        }
    }
    if pos < char_len_of(r.text()) {
        emit_range(&mut runs, pos, char_len_of(r.text()));
    }
    coalesce_runs(runs)
}

/// Compress adjacent runs of the same kind into a single run with joined text.
pub fn coalesce_runs(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.kind == run.kind
        {
            last.text.push_str(&run.text);
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
    merged_runs(r).iter().map(|x| x.text.as_str()).collect()
}

/// Project the row runs forward to reconstruct the new line (stripping Del
/// runs).
pub fn project_forward(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| r.kind != RunKind::Del)
        .map(|r| r.text.as_str())
        .collect()
}

/// Project the row runs backward to reconstruct the old line (stripping Add
/// runs).
pub fn project_reverse(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| r.kind != RunKind::Add)
        .map(|r| r.text.as_str())
        .collect()
}

#[cfg(test)]
mod tests;
