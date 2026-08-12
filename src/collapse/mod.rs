//! One-for-one port of `review.ts`'s `collapseHunkRows` (PLAN.md §3) —
//! the pairing (similarity + the brace-anchor), the stretch and
//! reverse-stretch, the reflow rules, the ordered walk, and the final
//! sort. Comments ported verbatim. This is the heart of the engine.
//!
//! The TS mutates its row arrays (splicing `pendingAdds` during the
//! pairing loop); the port tracks rows by their **hunk row index** — the
//! adds' identities survive the consumption, exactly like the TS's object
//! references.

use crate::lcs::{char_slice, word_spans};
use crate::merge::wholesale_pair;
use crate::parse::{Hunk, Row, RowKind};
use crate::similarity::{is_comment_line, line_similarity_parts};
use crate::tail::{fn_of, is_tail_before_closing};
use std::collections::{HashMap, HashSet};

/// Collapse each FIFO del→add pair into a single "mod" row: the new line
/// with its word-diff spans baked in. A modified line renders once (`~`)
/// instead of as a red/green pair against "different" lines.
#[allow(clippy::too_many_arguments)]
pub fn collapse_hunk_rows(
    h: &Hunk,
    hash_comment: bool,
    old_content: Option<&[String]>,
    new_content: Option<&[String]>,
) -> Vec<Row> {
    // Pair each non-moved del with its best-matching add — NOT the FIFO next
    // add: when a declaration is replaced and new code precedes it (e.g. an
    // added method before the modified `pub fn lookup`), the FIFO would pair
    // the old line with the doc comment. The similarity pairing joins the old
    // declaration with the new one.
    let mut pending_adds: Vec<usize> = h
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == RowKind::Add && !r.moved)
        .map(|(i, _)| i)
        .collect();
    // Consecutive-del runs — the REFLOW pairing rule. When several old lines
    // merge onto one new line (`self.resolved_layers` / `.iter()` /
    // `.any(…)` → the one-liner `self.layers.iter().any(…)`), the first del
    // carries the meaningful change (`resolved_`); the later dels' content
    // survives verbatim inside the add — pure reflows. A non-first del whose
    // text is a substring of the candidate add must not steal the pair: it
    // would render the same text struck against itself. The first del's mod
    // shows the actual edit (`self.` + ~~resolved_~~ + `layers…`).
    let mut del_runs: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < h.rows.len() {
        if h.rows[i].kind != RowKind::Del || h.rows[i].moved {
            i += 1;
            continue;
        }
        let mut run = vec![i];
        while i + 1 < h.rows.len() && h.rows[i + 1].kind == RowKind::Del && !h.rows[i + 1].moved {
            i += 1;
            run.push(i);
        }
        del_runs.push(run);
        i += 1;
    }
    let run_of: HashMap<usize, Vec<usize>> = del_runs
        .iter()
        .flat_map(|run| run.iter().map(|&ri| (ri, run.clone())))
        .collect();
    // Consecutive-add runs — the STRETCH candidates: a deleted row whose
    // content is recognisably spread across several added rows pairs with
    // the whole run (`if k < floor {` → `if let Some(floor) = floor` +
    // `&& k > floor` + `{` — one mod, three visual lines, the `<`→`>`
    // flip struck in place).
    let mut runs: Vec<Vec<usize>> = Vec::new(); // windows of pending_adds' row ids
    let mut i = 0;
    while i < pending_adds.len() {
        let mut run = vec![pending_adds[i]];
        // A comment line ends the run — the stretch never crosses a comment
        // boundary (`for …` must not absorb the preceding comment rewrite).
        while i + 1 < pending_adds.len()
            && h.rows[pending_adds[i + 1]].new_line.unwrap_or(0)
                == h.rows[pending_adds[i]].new_line.unwrap_or(0) + 1
            && !is_comment_line(&h.rows[pending_adds[i]].text, hash_comment)
        {
            i += 1;
            run.push(pending_adds[i]);
        }
        // Sliding windows over the run: the del must pair with the part of
        // the run it actually matches (`if k < floor {` matches the trailing
        // `if let …`/`&& …`/`{`, not the `for` line that leads the run).
        for w in 2..=run.len() {
            for k in 0..=(run.len() - w) {
                runs.push(run[k..k + w].to_vec());
            }
        }
        i += 1;
    }
    let mut pair_of: HashMap<usize, usize> = HashMap::new(); // add row id -> del row id
    let mut stretch_of: HashMap<usize, Vec<usize>> = HashMap::new(); // del row id -> add row ids
    // The REVERSE stretch: a del RUN folds into one mod against a single
    // add (the old `find_mount` body — `self.mount_stack` / `.filesystems` /
    // `.iter()` / `.rfind()` / `.map()` — replaced by the one-liner
    // `self.mount_stack.get(guest_path)`). The mod's delText is the run's
    // concatenation — the old body's lines strike across the visual lines,
    // the add fills — instead of the first del pairing while the rest
    // floats as `-` rows after the closed method.
    let mut reverse_stretch_of: HashMap<usize, Vec<usize>> = HashMap::new(); // del row id -> the run

    // The dels' row ids, in order — the TS iterates `h.rows`.
    let del_ids: Vec<usize> = h
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == RowKind::Del)
        .map(|(i, _)| i)
        .collect();

    for &ri in &del_ids {
        let r = &h.rows[ri];
        if r.moved {
            continue;
        }
        // A del whose text survives VERBATIM as a line in the new file is a
        // pure reflow — the similarity pairing must not grab it (the old
        // `find_mount`'s `.rfind(...)` chain reflowed into the new `get`
        // method; a sim-based pair with `format!("no mount captures…")`
        // would render garble). The reflow-drop absorbs it. A TAIL del
        // (the old line after it is the fn's closing brace) is exempt:
        // the brace-anchor pairing (`None` ↔ `Some(result)`) is the
        // del's definitive partner and must run even when the new file
        // happens to contain a verbatim `None` line elsewhere.
        let r_trim = r.text.trim();
        // The brace-anchor exemption: the del is a tail AND some pending
        // add is a tail in the SAME fn (`None` ↔ `Some(result)` — both sit
        // before their fn's closing brace). A tail del with NO such add
        // (the old `find_mount`'s `.map(...)` — its `}` follows, but the
        // new fn's tail is a different fn) stays a reflow: the pairing
        // would only garble.
        let tail_candidate = is_tail_before_closing(r.old_line, true, old_content, new_content)
            && pending_adds.iter().any(|&ai| {
                let a = &h.rows[ai];
                is_tail_before_closing(a.new_line, false, old_content, new_content)
                    && fn_of(old_content, r.old_line) == fn_of(new_content, a.new_line)
            });
        if !r_trim.is_empty()
            && !tail_candidate
            && new_content.is_some_and(|c| c.iter().any(|l| l.trim() == r_trim))
        {
            continue;
        }
        let mut best: Option<usize> = None; // add row id
        let mut best_score = 0.15; // threshold: meaningful replacements only
        let mut best_run: Option<Vec<usize>> = None;
        let mut best_run_score = 0.25; // the stretch threshold: the run must read
        // as "the same line" restructured across the added rows.
        for &ai in &pending_adds {
            let a = &h.rows[ai];
            if let Some(run) = run_of.get(&ri) {
                let trimmed = r.text.trim();
                if run.len() > 1
                    && run[0] != ri
                    && !trimmed.is_empty()
                    && a.text.trim().contains(trimmed)
                {
                    continue; // a reflow del — its text is verbatim inside the add
                }
            }
            // A fragment pair: the add's content is (almost) entirely matched
            // inside the del with weak overall similarity — the one-line `use`
            // statement split into a 7-line block, and this add is one
            // arbitrary fragment of it. Pairing it as "the" replacement
            // renders the mod as the whole old line struck against one new
            // fragment — misleading. Large-overlap pairs (`resolved_layers` →
            // `layers`, a shortened line) keep the add-fraction high but score
            // well above this band; a growth pair (`None` → `return None;`)
            // matches only part of the add and is unaffected. A ONE-TO-ONE
            // rename (`.resolved_layers` → `.layers` — the del has a single
            // candidate add) is never a split: the rejection applies only
            // when the old line's content could be spread across several
            // additions.
            let parts = line_similarity_parts(&r.text, &a.text);
            if parts.add_total > 0
                && parts.add_matched as f64 >= parts.add_total as f64 * 0.8
                && parts.sim < 0.5
                && pending_adds.len() > 1
            {
                continue;
            }
            // A `return` pairs with a `return` — the old `return
            // self.results…` must match the new `return None;` (`return ` +
            // struck expr + fill + `;`), not a nearby pop line that merely
            // shares `self.`/`borrow_mut()`/`key` fragments. Definitive:
            // a `return ` candidate outranks any non-return.
            // A closing-brace-anchored tail pairs with its counterpart: the
            // old `None` before the fn's `}` matches the new `Some(result)`
            // before the `}` (`~~None~~Some(result)`), not a mid-block
            // `return None;` that merely shares the word. When several tails
            // tie, the one anchored in the same surrounding context wins
            // (the pops before both `None` and `Some(result)`).
            let brace_tail = is_tail_before_closing(r.old_line, true, old_content, new_content)
                && is_tail_before_closing(a.new_line, false, old_content, new_content);
            // The tail's fn must match the del's fn — the blank line between
            // fn bodies is the section boundary (`Some(result)` is in
            // `lookup`, the same fn as the old `None`; the `epoch` fn's
            // tail is a different fn).
            let same_fn = fn_of(old_content, r.old_line) == fn_of(new_content, a.new_line);
            let score = (r.text.trim_start().starts_with("return ")
                && a.text.trim_start().starts_with("return ")
                .then_some(1)
                .unwrap_or(0))
                + (brace_tail && same_fn).then_some(1).unwrap_or(0)
                + parts.sim;
            if score > best_score {
                best_score = score;
                best = Some(ai);
            }
        }
        for run in &runs {
            let text = run
                .iter()
                .map(|&ai| h.rows[ai].text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let sim = line_similarity_parts(&r.text, &text).sim;
            // Longest winning window: among the windows that read as "the
            // same line", the longest wins — the `if k < floor {` stretch
            // keeps its `if `/`k `/` floor {` common anchors over the tighter
            // `&& k > floor`+`{` fragment.
            if sim >= 0.25 && (best_run.is_none() || run.len() > best_run.as_ref().unwrap().len()) {
                best_run = Some(run.clone());
                best_run_score = sim;
            }
        }
        // The stretch must read as the same line restructured — the split-word
        // garble (the comment rewrites' `fro~~upper~~zen`) stays the `-`/`+`
        // baseline, not a stretched mod. The del's single best must also be a
        // viable mod (not the wholesale): a comment rewritten across a run is
        // still a comment rewrite — the `-`/`+` block.
        let run_text = best_run
            .as_ref()
            .map(|run| {
                run.iter()
                    .map(|&ai| h.rows[ai].text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        if let (Some(br), Some(b)) = (best_run.clone(), best) {
            let best_span = word_spans(&r.text, &h.rows[b].text);
            if best_run_score >= 0.25
                // The single must be CROSSED to stretch — "if a single line
                // anchors clean against another single line that's a section
                // separator". The if's single (`if`→`&&`, `<`/` {`→`>`) has
                // three del runs; the for's (`lowers`→`below`, `.rev()`) and
                // the mode's (`resolved_`) anchor cleanly — they stay mods.
                && best_span.del.len() >= 3
                && !wholesale_pair(&r.text, &h.rows[b].text, &best_span)
                && !wholesale_pair(&r.text, &run_text, &word_spans(&r.text, &run_text))
            {
                // The stretch wins: the whole run is a better "same line" than
                // any single add. The mod's text is the run's concatenation;
                // the run's adds are consumed (not emitted as + rows).
                stretch_of.insert(ri, br.clone());
                for &aid in &br {
                    pending_adds.retain(|&x| x != aid);
                }
                runs.retain(|run| !run.iter().any(|x| br.contains(x)));
            } else {
                // A pair that would render as the wholesale (garble-shaped
                // spans) reads better as the plain `-`/`+` baseline rows —
                // leave it unpaired.
                let spans = word_spans(&r.text, &h.rows[b].text);
                if !wholesale_pair(&r.text, &h.rows[b].text, &spans) {
                    pair_of.insert(b, ri);
                    pending_adds.retain(|&x| x != b);
                    // The reverse stretch: the del is the FIRST of a
                    // multi-del run and the run's later dels' text is NOT
                    // verbatim inside the add (the old method's body,
                    // deleted — versus the `let up` reflow, whose
                    // `.iter()/.find(…)` lines survive inside the one-liner
                    // and take the reflow-common path instead). The mod's
                    // delText then becomes the run's concatenation; the
                    // run's dels are consumed (not emitted as `-` rows).
                    if let Some(run) = run_of.get(&ri) {
                        if run.len() > 1
                            && run[0] == ri
                            && !run.iter().skip(1).any(|&d| {
                                let t = h.rows[d].text.trim();
                                !t.is_empty() && h.rows[b].text.contains(t)
                            })
                        {
                            reverse_stretch_of.insert(ri, run.clone());
                        }
                    }
                }
            }
        }
    }
    let paired_dels: HashSet<usize> = pair_of.values().copied().collect();
    let add_of: HashMap<usize, usize> = pair_of.iter().map(|(&add, &del)| (del, add)).collect();
    let stretched_dels: HashSet<usize> = stretch_of.keys().copied().collect();
    let stretched_adds: HashSet<usize> = stretch_of.values().flatten().copied().collect();

    // Ordered walk: mods emit at their del's position; each mod's tail
    // absorbs the unpaired adds that follow it *until the next mod's del* —
    // an unpaired add between two mods belongs to the later mod, not the
    // earlier one's section (adjacent changes must not overlap). A blank line
    // OR a comment line (a new comment block — per-language) ends the
    // section: closing braces stay with their change, an unrelated doc block
    // does not get glued onto a deleted line. Unpaired dels are collected
    // and, when their content is already inside a paired add (formatting
    // restructuring), dropped; the rest trail at the end.
    let mut out: Vec<Row> = Vec::new();
    let mut kept_dels: Vec<usize> = Vec::new();
    let mut pure_adds: Vec<usize> = Vec::new();
    let mut absorbed_dels: HashSet<usize> = HashSet::new();
    let mut last_mod: Option<usize> = None; // index into out
    for (ri, r) in h.rows.iter().enumerate() {
        if r.kind == RowKind::Add {
            if pair_of.contains_key(&ri) || stretched_adds.contains(&ri) {
                continue; // the mod appears at its del's position
            }
            let trimmed = r.text.trim();
            // The section absorbs only adds that FOLLOW the mod's new line:
            // when the paired add is the LAST of a replacement block (old
            // `None` ↔ new `return None;`), the block's earlier lines render
            // as + rows before the mod, never as its tail. A trailing BLANK
            // also joins the section (the block reads as a unit — the closing
            // brace and the air after it), then the section ends: the
            // following method stays a separate add.
            if let Some(lm) = last_mod {
                let m = &out[lm];
                if !r.moved
                    && !is_comment_line(&r.text, hash_comment)
                    && r.new_line.unwrap_or(0) > m.new_line.unwrap_or(0)
                {
                    out[lm].tail.push(r.clone());
                    if trimmed.is_empty() {
                        last_mod = None;
                    }
                    continue;
                }
            }
            last_mod = None; // a reflow or a comment block ends the section
            pure_adds.push(ri);
        } else if paired_dels.contains(&ri) {
            let add_ri = add_of[&ri];
            let add = &h.rows[add_ri];
            // The reverse stretch: the del's own text is the RUN's
            // concatenation (the whole old body), not just the first line.
            let stretch_run = reverse_stretch_of.get(&ri);
            let del_text = match stretch_run {
                Some(run) => run
                    .iter()
                    .map(|&d| h.rows[d].text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => r.text.clone(),
            };
            let mut spans = word_spans(&del_text, &add.text);
            // The reflow-common merge: the mod's del is the FIRST of its
            // run — it carries the meaningful change; the run's later dels
            // are reflows whose text survives verbatim inside the add. That
            // text is the same on both sides — it must read as common, not
            // as newly added (`self.~~resolved_~~layers.iter().any(…)`:
            // the `.iter().any(…)` is the reflowed `.iter()` + `.any(…)`
            // lines; the `let up` one-liner's `?;` is the reflowed
            // `.find(…)` tail). A whole add span whose text contains a
            // reflow line's content is that reflow — drop the span.
            let run = run_of.get(&ri);
            let reflows: Vec<String> = match run {
                Some(run) if run.len() > 1 => {
                    run.iter().skip(1).map(|&d| h.rows[d].text.trim().to_string()).collect()
                }
                _ => Vec::new(),
            };
            if !reflows.is_empty() {
                spans.add.retain(|(as_, ae)| {
                    let seg = char_slice(&add.text, *as_, *ae);
                    !reflows.iter().any(|t| !t.is_empty() && seg.contains(t.as_str()))
                });
            }
            let mod_row = Row {
                kind: RowKind::Mod,
                text: add.text.clone(),
                new_line: add.new_line,
                old_line: r.old_line,
                spans: Some(spans),
                moved: false,
                del_text: Some(del_text),
                tail: Vec::new(),
            };
            out.push(mod_row);
            last_mod = Some(out.len() - 1);
            if let Some(run) = stretch_run {
                for &d in run.iter().skip(1) {
                    absorbed_dels.insert(d);
                }
            }
        } else if stretched_dels.contains(&ri) {
            // The stretch: one deleted row over a run of added rows. The
            // mod's text is the run's concatenation — the merged display
            // spans the visual lines, the `~` prefix repeats per line.
            let run = &stretch_of[&ri];
            let text = run
                .iter()
                .map(|&ai| h.rows[ai].text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let mod_row = Row {
                kind: RowKind::Mod,
                text: text.clone(),
                new_line: h.rows[run[0]].new_line,
                old_line: r.old_line,
                spans: Some(word_spans(&r.text, &text)),
                moved: false,
                del_text: Some(r.text.clone()),
                tail: Vec::new(),
            };
            out.push(mod_row);
            last_mod = Some(out.len() - 1);
        } else if !r.moved && !absorbed_dels.contains(&ri) {
            kept_dels.push(ri);
        }
    }
    let add_text = out
        .iter()
        .map(|m| m.text.as_str())
        .chain(pure_adds.iter().map(|&ri| h.rows[ri].text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    let meaningful: Vec<usize> = kept_dels
        .iter()
        .copied()
        .filter(|&di| {
            let t = h.rows[di].text.trim();
            t.is_empty() || !add_text.contains(t)
        })
        .collect();
    // Adds whose content exists verbatim in a del are a pure reflow — drop
    // both sides so the reflow is invisible rather than ≈ noise. Content-
    // based, not move-flag-based: the greedy move matcher can leave a reflow
    // add unmarked when multiple adds share the content (a duplicated pop in
    // a restructured block). The del corpus excludes MOVED and PAIRED dels —
    // those are displayed elsewhere (≈ rows, mod rows), so an add matching
    // them is real content (the second pop pair in a restructured block),
    // not a reflow. A MOVED add checks against the full corpus: a local
    // same-position move (old 4 → new 4) is a pure reshuffle — invisible —
    // while the unmoved adds use the corpus of dels that are actually
    // INVISIBLE — not moved (shown ≈), not paired (shown in the mod), and
    // not meaningful (shown as a - row). An add matching a shown del is real
    // content: the seven lines a one-line import was split into must all
    // survive.
    let del_text_all = h
        .rows
        .iter()
        .filter(|r| r.kind == RowKind::Del)
        .map(|d| d.text.trim())
        .collect::<Vec<_>>()
        .join("\n");
    let shown_dels: HashSet<usize> = meaningful.iter().copied().collect();
    let del_text_live = h
        .rows
        .iter()
        .enumerate()
        .filter(|(i, r)| r.kind == RowKind::Del && !r.moved && !paired_dels.contains(i) && !shown_dels.contains(i))
        .map(|(_, d)| d.text.trim())
        .collect::<Vec<_>>()
        .join("\n");
    let kept_adds: Vec<usize> = pure_adds
        .iter()
        .copied()
        .filter(|&ai| {
            let a = &h.rows[ai];
            let t = a.text.trim();
            let corpus = if a.moved { &del_text_all } else { &del_text_live };
            t.is_empty() || !corpus.contains(t)
        })
        .collect();
    // Unified order within the hunk: mods and adds render at their NEW-side
    // position (a replacement block that precedes the paired line — new lines
    // 134-139 before the mod at 140 — leads with the additions), and a
    // genuinely removed line renders right after the mod whose old position
    // preceded it (the removed old 109 sits after the mod that replaced old
    // 108), never interrupting the new-file sequence.
    let mut anchor_of: HashMap<usize, usize> = HashMap::new(); // del idx -> out idx of the anchor mod
    for &di in &meaningful {
        let Some(dold) = h.rows[di].old_line else { continue };
        let mut anchor: Option<usize> = None;
        for (mi, m) in out.iter().enumerate() {
            if m.kind != RowKind::Mod || m.old_line.is_none() {
                continue;
            }
            let mold = m.old_line.unwrap();
            if mold <= dold && (anchor.is_none() || mold > out[anchor.unwrap()].old_line.unwrap()) {
                anchor = Some(mi);
            }
        }
        if let Some(a) = anchor {
            anchor_of.insert(di, a);
        }
    }
    // The sort key: [position, subkey] — dels render after their anchor's
    // comment-chain, before the next change.
    let key_of = |r: &Row| -> (u32, f64) {
        if r.kind == RowKind::Del {
            let base = match r.old_line {
                Some(ol) => {
                    // find the anchor (out index) by matching old_line — the
                    // anchor map is keyed by h.rows index; here we look up by
                    // the del's old line.
                    let _ = ol;
                    // (recomputed below)
                    None
                }
                None => None,
            };
            let _ = base;
            return (0, r.old_line.unwrap_or(0) as f64);
        }
        (r.new_line.unwrap_or(0), if r.kind == RowKind::Mod { 1.0 } else { 2.0 })
    };
    let _ = key_of;

    // (The anchor-based sort is ported below; the del keys need the
    // meaningful dels' anchors + the comment-chain advance.)
    let mut keyed: Vec<(Row, (u32, f64))> = Vec::new();
    for r in out.into_iter() {
        keyed.push((r, (r.new_line.unwrap_or(0), if r.kind == RowKind::Mod { 1.0 } else { 2.0 })));
    }
    for &di in &meaningful {
        let d = &h.rows[di];
        let key = match anchor_of.get(&di) {
            None => (0, d.old_line.unwrap_or(0) as f64),
            Some(&mi) => {
                let base = keyed[mi].0;
                // A removed line must not interrupt a comment block: it
                // renders after the consecutive comment adds that follow its
                // anchor (the removed `let mut floor` sits after the doc
                // lines, before the new declaration — never mid-comment).
                let mut last = base;
                for &ai in &kept_adds {
                    let kn = h.rows[ai].new_line.unwrap_or(0);
                    if kn == last + 1 && is_comment_line(&h.rows[ai].text, hash_comment) {
                        last = kn;
                    }
                }
                // 2.5: after any add sharing that position (the comment's
                // last line), before the next change.
                (last, 2.5)
            }
        };
        keyed.push((h.rows[di].clone(), key));
    }
    for &ai in &kept_adds {
        let a = &h.rows[ai];
        keyed.push((a.clone(), (a.new_line.unwrap_or(0), 2.0)));
    }
    keyed.sort_by(|a, b| a.1.cmp(&b.1).then(a.1.1.partial_cmp(&b.1.1).unwrap_or(std::cmp::Ordering::Equal)));
    keyed.into_iter().map(|(r, _)| r).collect()
}
