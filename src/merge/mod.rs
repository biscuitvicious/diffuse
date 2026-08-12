//! One-for-one port of the merge machinery (PLAN.md §3): `wholesalePair`,
//! `delSpansToShow`, `delPairing`, `mergedRuns`, `displayText`,
//! `isWholesaleMod`, `isPureDeletionMod`. Comments ported verbatim; char
//! offsets throughout (the lcs.rs convention).

use crate::lcs::char_slice;
use crate::parse::{Row, Spans};
use std::collections::{HashMap, HashSet};

/// A merged run of a mod row's display.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub kind: RunKind,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunKind {
    Common,
    Del,
    Add,
}

/// The Unicode White_Space property plus the zero-width space — the
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
 */
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
    let mut del_pos: Vec<usize> = Vec::new();
    let mut cum = 0usize;
    for (ds, de) in &del_spans {
        del_pos.push(ds - cum);
        cum += de - ds;
    }
    let mut pair: HashMap<usize, usize> = HashMap::new();
    let mut used_adds: HashSet<usize> = HashSet::new();
    for (j, (ds, de)) in del_spans.iter().enumerate() {
        let mut best: Option<usize> = None;
        let mut best_gap: usize = 8;
        for (i, (as0, ae)) in add_spans.iter().enumerate() {
            if used_adds.contains(&i) {
                continue;
            }
            // Overlapping non-identical ranges interleave in the merge —
            // they stay separate events (same guard as `delPairing`).
            let del_end = del_pos[j] + (de - ds);
            let same = del_pos[j] == *as0 && del_end == *ae;
            let disjoint = del_end <= *as0 || *ae <= del_pos[j];
            if !same && !disjoint {
                continue;
            }
            // Prefer the CONTENT counterpart over a closer punctuation
            // fragment (`0` ↔ `None`, never the type syntax `>` — the strike
            // must read `= ~~0~~None;`).
            // OPERATOR flips pair operator-to-operator with precedence: a
            // `<`→`>` comparison flip is either a bug or the logical twin of
            // another change — it must stand out (`k ~~<~~> floor`), never
            // bury itself inside a nearer word span. The add's operator
            // anchors at its own offset inside the span.
            let del_op = char_slice(del_text, *ds, *de).trim();
            let mut op_gap = usize::MAX;
            for c in *as0..*ae {
                if let Some(ch) = char_at(add_text, c)
                    && "<>!=+*/-%&|^~".contains(ch)
                {
                    op_gap = del_pos[j].abs_diff(c);
                    break;
                }
            }
            let first = char_at(add_text, *as0);
            let gap = (del_pos[j].abs_diff(*as0)
                + if first.is_some_and(is_word) { 0 } else { 5 })
            .min(op_gap);
            if gap <= best_gap {
                best_gap = gap;
                best = Some(i);
            }
            if del_op.len() == 1 && del_op.starts_with(['<', '>']) {
                let dch = del_op.chars().next().unwrap();
                for c in *as0..*ae {
                    if let Some(ch) = char_at(add_text, c)
                        && (ch == '<' || ch == '>')
                    {
                        let g = del_pos[j].abs_diff(c) + if ch == dch { 0 } else { 2 };
                        if g < best_gap {
                            best_gap = g;
                            best = Some(i);
                        }
                    }
                }
            }
        }
        if let Some(b) = best {
            used_adds.insert(b);
            pair.insert(j, b);
        }
    }
    // A WORD-led standalone del landing mid-word is garble (the comment
    // rewrite's `them.` in `m~~them~~ust`); a punctuation-led fragment
    // (`.rev()`, ` {`) reads fine struck mid-word — `enumer~~.rev()~~ate()`
    // still reconstructs `enumerate()`. Meaningful identity = subword.
    del_spans.iter().enumerate().any(|(j, (ds, de))| {
        if pair.contains_key(&j) {
            return false;
        }
        // A multi-line span (a whole deleted block folded into a
        // reverse-stretch mod) cannot be a split-word garble — the strike
        // spans the visual lines, the new line fills beside it. The
        // reverse-stretch of the old `find_mount` body must stay the
        // subword mod (`self.mount_stack` common + struck chain + filled
        // `.get(guest_path)`), not fall to the `-`/`+` baseline.
        if char_slice(del_text, *ds, *de).contains('\n') {
            return false;
        }
        let ins = del_pos[j].min(add_text.len());
        let left = ins.checked_sub(1).and_then(|i| char_at(add_text, i));
        let right = char_at(add_text, ins);
        if !(ins > 0 && ins < add_text.len() && left.is_some_and(is_word) && right.is_some_and(is_word)) {
            return false;
        }
        let t = char_slice(del_text, *ds, *de).trim_start();
        t.chars().next().map_or(false, is_word)
    })
}

/// The artifact-filtered del spans a mod row shows (see the TS).
pub fn del_spans_to_show(r: &Row, del_spans_in: Option<&[(usize, usize)]>) -> Vec<(usize, usize)> {
    let del_spans: Vec<(usize, usize)> = match del_spans_in {
        Some(s) => s.to_vec(),
        None => r.spans.as_ref().map_or(Vec::new(), |s| s.del.clone()),
    };
    if r.del_text.is_none() {
        return Vec::new();
    }
    let del_text = r.del_text.as_ref().expect("del_text");
    del_spans
        .iter()
        .filter(|(ds, de)| {
            let raw = char_slice(del_text, *ds, *de);
            let t = raw.trim();
            // A whitespace-only span is a real indent change (8 → 12 spaces
            // moves the line into a block): show it struck at the line
            // start. Drop it only when the add's leading run is identical.
            if t.is_empty() {
                let leading = r.text.chars().take_while(|c| c.is_ascii_whitespace()).collect::<String>();
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
            char_slice(&r.text, *ds, *de) != t
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
    let mut del_pos: Vec<usize> = Vec::new();
    let mut cum = 0usize;
    for (ds, de) in del_spans {
        del_pos.push(ds - cum);
        cum += de - ds;
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
            // The aligned replacement occupies the same range; anything
            // else that overlaps the del's range interleaves in the merge
            // (`resolved_` at 13 must not pair with `.iter()` at 19 —
            // that would push the removal past `layers`). Overlapping
            // non-identical spans stay separate events.
            let del_end = del_pos[j] + (de - ds);
            let same = del_pos[j] == *as0 && del_end == *ae;
            let disjoint = del_end <= *as0 || *ae <= del_pos[j];
            // Whitespace spans pair despite the overlap — the indent
            // change is expressed minimally (8 → 12 spaces = 8 common
            // + 4 added), not as two independent whole runs.
            let ws_pair = all_ws(char_slice(del_text, *ds, *de)) && all_ws(char_slice(add_text, *as0, *ae));
            if !same && !disjoint && !ws_pair {
                continue;
            }
            // Prefer the CONTENT counterpart over a closer punctuation
            // fragment (`0` ↔ `None`, never the type syntax `>` — the
            // strike must read `= ~~0~~None;`).
            // OPERATOR flips pair operator-to-operator with precedence: a
            // `<`→`>` comparison flip is either a bug or the logical twin
            // of another change — it must stand out (`k ~~<~~> floor`),
            // never bury itself inside a nearer word span. The add's
            // operator anchors at its own offset inside the span.
            let del_op = char_slice(del_text, *ds, *de).trim();
            let op_chars = del_op.chars().collect::<Vec<char>>();
            let is_compare_op = op_chars.len() >= 1 && op_chars.len() <= 2
                && (op_chars[0] == '<' || op_chars[0] == '>')
                && (op_chars.len() == 1 || op_chars[1] == '=');
            if is_compare_op {
                // A lone comparison operator pairs with a comparison
                // operator wherever it appears — this is the counterpart
                // by definition, so it WINS over the nearest-span rule
                // (`k <` → `k >` strikes the `<` against the `>`; the
                // flip must stand out).
                let mut op_best: Option<usize> = None;
                let mut op_best_gap = usize::MAX;
                for c in *as0..*ae {
                    if let Some(ch) = char_at(add_text, c)
                        && (ch == '<' || ch == '>')
                    {
                        let g = del_pos[j].abs_diff(c) + if ch == op_chars[0] { 0 } else { 2 };
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
            // (The TS computes an opGap here but never reads it in
            // `delPairing` — dead in the reference; the gap formula is the
            // position distance plus the alnum bonus only.)
            let first = char_at(add_text, *as0);
            let gap = del_pos[j].abs_diff(*as0)
                + if first.is_some_and(is_word) { 0 } else { 5 };
            // Whitespace pairs express the MINIMUM wherever they land —
            // the old `floor {`'s space before the brace pairs with the
            // new `floor\n                {`'s newline+indent even across
            // the run (a big gap): the strike/fill must NOT leak into
            // the middle of `&& k > floor` as `&&  k`.
            if ws_pair {
                best_gap = 0;
                best = Some(i);
                continue;
            }
            // `<=` — ties prefer the LATER span (the value `None` over
            // the type syntax `Option<`; `= ~~0~~None;` reads).
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
    if r.del_text.is_none() {
        return false;
    }
    let spans = r.spans.clone().unwrap_or_default();
    wholesale_pair(r.del_text.as_ref().expect("del_text"), &r.text, &spans)
}

/// A mod whose change is purely a removal: every add span is whitespace —
/// the new line is the old line minus text (`pub(super) struct X {` →
/// `pub struct X {`, `resolved_layers` → `layers`). Nothing was added, so
/// the row must not render as an addition: it takes the neutral shade
/// (like a moved line — the line survives) with the red strikes carrying
/// the removal.
pub fn is_pure_deletion_mod(r: &Row) -> bool {
    match &r.spans {
        None => true,
        Some(spans) => spans
            .add
            .iter()
            .all(|(s, e)| char_slice(&r.text, *s, *e).trim().is_empty()),
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
            Run { kind: RunKind::Del, text: r.del_text.clone().unwrap_or_default() },
            Run { kind: RunKind::Add, text: r.text.clone() },
        ];
    }
    // The reverse stretch: the delText is a whole deleted block (the old
    // `find_mount` body) folded against a single add. The shared prefix
    // reads common (`self.mount_stack`), the block's rest strikes across
    // the visual lines, the add's rest fills — the del spans' own
    // positions (the chain's delPos) would interleave the fill into the
    // middle of the struck block (`…iter` + `get` + `…`), garbling it.
    let del_text = r.del_text.clone().unwrap_or_default();
    if del_text.contains('\n') {
        let mut share = 0usize;
        let dl: Vec<char> = del_text.chars().collect();
        let al: Vec<char> = r.text.chars().collect();
        while share < dl.len() && share < al.len() && dl[share] == al[share] {
            share += 1;
        }
        // The fill reads FIRST — the new one-liner (`self.mount_stack`
        // + filled `.get(guest_path)`) — then the struck old block
        // spans the following visual lines (`~~\n.filesystems…~~`).
        return vec![
            Run { kind: RunKind::Common, text: dl[..share].iter().collect() },
            Run { kind: RunKind::Add, text: al[share..].iter().collect() },
            Run { kind: RunKind::Del, text: dl[share..].iter().collect() },
        ];
    }
    let spans = r.spans.clone().unwrap_or_default();
    let add_spans = spans.add.clone();
    let del_spans = del_spans_to_show(r, Some(&spans.del));
    let DelPairing { del_pos, pair, op_anchor } = del_pairing(&del_spans, &add_spans, &r.text, &del_text);
    let mut runs: Vec<Run> = Vec::new();
    let emit_range = |runs: &mut Vec<Run>, from: usize, to: usize| {
        if to > from {
            runs.push(Run { kind: RunKind::Common, text: char_slice(&r.text, from, to).to_string() });
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
            events.push(Ev { pos, del: Some(*dspan), add: Some(add_spans[*ai]) });
        } else {
            // A standalone landing mid-word must not glue the strike
            // inside a surviving token (`enumer~~.rev()~~ate()` is
            // wrong — "rev" stuck to "enumerate"'s "r"); snap forward
            // to the end of the enclosing token run so the removal
            // reads as a unit (`enumerate()~~.rev()~~ {`).
            let mut ins = del_pos[j];
            let left = ins.checked_sub(1).and_then(|i| char_at(&r.text, i));
            let right = char_at(&r.text, ins);
            if ins > 0 && ins < char_len_of(&r.text) && left.is_some_and(is_word) && right.is_some_and(is_word) {
                while ins < char_len_of(&r.text) {
                    if let Some(c) = char_at(&r.text, ins)
                        && c.is_ascii_whitespace()
                    {
                        break;
                    }
                    ins += 1;
                }
            }
            events.push(Ev { pos: ins, del: Some(*dspan), add: None });
        }
    }
    for (i, aspan) in add_spans.iter().enumerate() {
        if !used_adds.contains(&i) {
            events.push(Ev { pos: aspan.0, del: None, add: Some(*aspan) });
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
                let add_text_span = char_slice(&r.text, aspan.0, aspan.1);
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
                        runs.push(Run { kind: RunKind::Common, text: dch[..share].iter().collect() });
                    }
                    if share < dch.len() {
                        runs.push(Run { kind: RunKind::Del, text: dch[share..].iter().collect() });
                    }
                    if share < ach.len() {
                        runs.push(Run { kind: RunKind::Add, text: ach[share..].iter().collect() });
                    }
                    pos = aspan.1;
                    continue;
                }
            }
            runs.push(Run { kind: RunKind::Del, text: del_text_span.to_string() });
            pos = pos.max(ev.pos);
        }
        if let Some(aspan) = ev.add {
            runs.push(Run { kind: RunKind::Add, text: char_slice(&r.text, aspan.0, aspan.1).to_string() });
            pos = aspan.1;
        }
    }
    if pos < char_len_of(&r.text) {
        emit_range(&mut runs, pos, char_len_of(&r.text));
    }
    runs
}

fn char_len_of(s: &str) -> usize {
    s.chars().count()
}

/// The merged display string of a row (the runs joined; a wholesale mod is
/// the whole-old + whole-new pair).
pub fn display_text(r: &Row) -> String {
    if (r.kind != crate::parse::RowKind::Mod && !r.moved) || r.del_text.is_none() {
        return r.text.clone();
    }
    if is_wholesale_mod(r) {
        return format!("{}{}", r.del_text.clone().unwrap_or_default(), r.text);
    }
    merged_runs(r).iter().map(|x| x.text.as_str()).collect()
}
