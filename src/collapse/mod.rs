//! One-for-one port of `review.ts`'s `collapseHunkRows` (PLAN.md §3), refactored in
//! Phase 6 into composable passes over a hunk's diff AST:
//!
//! 1. `pass_pair_and_stretch`: Similarity, brace-anchor, and stretch window pairing.
//! 2. `pass_reverse_stretch`: Del-run folding into single mod rows.
//! 3. `pass_assemble_mods`: Ordered walk creating `Mod` rows, subword spans, and tails.
//! 4. `pass_filter_reflows`: Filtering verbatim reflow dels and adds.
//! 5. `pass_sort_ordered_walk`: Keyed anchor sorting into the final visual row stream.

use crate::lcs::{char_slice, word_spans};
use crate::merge::wholesale_pair;
use crate::parse::{Hunk, Row, RowKind};
use crate::similarity::{
    LineTokens, TokenInterner, content_char_len, is_comment_line, line_similarity_parts_tokens,
    sim_window,
};
use crate::tail::{fn_of, is_tail_before_closing};
use std::collections::{HashMap, HashSet};

/// Results of the initial pairing and stretch pass.
pub struct PairingResults {
    pub pair_of: HashMap<usize, usize>, // add row id -> del row id
    pub stretch_of: HashMap<usize, Vec<usize>>, // del row id -> add row ids
    pub del_runs: Vec<Vec<usize>>,
    pub run_of: HashMap<usize, Vec<usize>>,
    pub del_ids: Vec<usize>,
}

struct AddRun {
    adds: Vec<usize>,
    content_prefix: Vec<usize>,
    ids: Vec<u32>,
    tok_chars: Vec<usize>,
    tok_prefix: Vec<usize>,
    chars_prefix: Vec<usize>,
}

/// Pass 1: Similarity, brace-anchor, and stretch window pairing.
pub fn pass_pair_and_stretch(
    h: &Hunk,
    hash_comment: bool,
    old_content: Option<&[String]>,
    new_content: Option<&[String]>,
) -> PairingResults {
    let mut pending_adds: Vec<usize> = h
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == RowKind::Add && !r.moved)
        .map(|(i, _)| i)
        .collect();

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

    let del_ids: Vec<usize> = h
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.kind == RowKind::Del)
        .map(|(i, _)| i)
        .collect();

    let mut interner = TokenInterner::new();
    let mut del_tokens: HashMap<usize, LineTokens> = HashMap::new();
    let mut add_tokens: HashMap<usize, LineTokens> = HashMap::new();
    for &ri in &del_ids {
        del_tokens.insert(ri, interner.line_tokens(&h.rows[ri].text));
    }
    for &ai in &pending_adds {
        add_tokens.insert(ai, interner.line_tokens(&h.rows[ai].text));
    }

    let mut add_runs: Vec<AddRun> = Vec::new();
    let mut i = 0;
    while i < pending_adds.len() {
        let mut run = vec![pending_adds[i]];
        while i + 1 < pending_adds.len()
            && h.rows[pending_adds[i + 1]].new_line.unwrap_or(0)
                == h.rows[pending_adds[i]].new_line.unwrap_or(0) + 1
            && !is_comment_line(&h.rows[pending_adds[i]].text, hash_comment)
        {
            i += 1;
            run.push(pending_adds[i]);
        }
        let mut content_prefix = Vec::with_capacity(run.len() + 1);
        let mut chars_prefix = Vec::with_capacity(run.len() + 1);
        let mut tok_prefix = Vec::with_capacity(run.len() + 1);
        let mut ids = Vec::new();
        let mut tok_chars = Vec::new();
        content_prefix.push(0);
        chars_prefix.push(0);
        tok_prefix.push(0);
        for &ai in &run {
            let at = add_tokens.get(&ai).unwrap();
            content_prefix
                .push(*content_prefix.last().unwrap() + content_char_len(&h.rows[ai].text));
            chars_prefix.push(*chars_prefix.last().unwrap() + at.chars);
            ids.extend_from_slice(&at.ids);
            tok_chars.extend_from_slice(&at.token_chars);
            tok_prefix.push(ids.len());
        }
        add_runs.push(AddRun {
            adds: run,
            content_prefix,
            ids,
            tok_chars,
            tok_prefix,
            chars_prefix,
        });
        i += 1;
    }

    let mut pair_of: HashMap<usize, usize> = HashMap::new();
    let mut stretch_of: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut consumed_adds: HashSet<usize> = HashSet::new();
    let mut kept_runs: Vec<Vec<usize>> = Vec::new();

    for &ri in &del_ids {
        let r = &h.rows[ri];
        if r.moved {
            continue;
        }
        let r_trim = r.text.trim();
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

        let del_is_tail = is_tail_before_closing(r.old_line, true, old_content, new_content);
        let del_fn = fn_of(old_content, r.old_line);
        let mut best: Option<usize> = None;
        let mut best_score = 0.15;
        let del_set: HashSet<u32> = del_tokens[&ri].ids.iter().copied().collect();
        let mut best_run: Option<Vec<usize>> = None;
        let mut best_run_score = 0.25;

        for &ai in &pending_adds {
            let a = &h.rows[ai];
            if let Some(run) = run_of.get(&ri) {
                let trimmed = r.text.trim();
                if run.len() > 1
                    && run[0] != ri
                    && !trimmed.is_empty()
                    && a.text.trim().contains(trimmed)
                {
                    continue;
                }
            }

            let return_bonus = if r.text.trim_start().starts_with("return ")
                && a.text.trim_start().starts_with("return ")
            {
                1.0
            } else {
                0.0
            };
            let dt = &del_tokens[&ri];
            let at = &add_tokens[&ai];
            let brace_bonus = if del_is_tail
                && is_tail_before_closing(a.new_line, false, old_content, new_content)
                && del_fn == fn_of(new_content, a.new_line)
            {
                1.0
            } else {
                0.0
            };
            let bonus = return_bonus + brace_bonus;
            let sim_max = if dt.tokens.is_empty() && at.tokens.is_empty() {
                1.0
            } else if dt.chars == 0 || at.chars == 0 {
                0.0
            } else {
                let add_common: usize = at
                    .ids
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| del_set.contains(id))
                    .map(|(i, _)| at.token_chars[i])
                    .sum();
                let add_set: HashSet<u32> = at.ids.iter().copied().collect();
                let del_common: usize = dt
                    .ids
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| add_set.contains(id))
                    .map(|(i, _)| dt.token_chars[i])
                    .sum();
                add_common.min(del_common) as f64 / dt.chars.max(at.chars) as f64
            };
            if bonus + sim_max <= best_score {
                continue;
            }
            let parts = line_similarity_parts_tokens(dt, at, &interner.ws, &interner.alnum);
            if parts.add_total > 0
                && parts.add_matched as f64 >= parts.add_total as f64 * 0.8
                && parts.sim < 0.3
                && pending_adds.len() > 1
            {
                continue;
            }

            let score = return_bonus + brace_bonus + parts.sim;
            if score > best_score {
                best_score = score;
                best = Some(ai);
            }
        }

        let ot_chars = content_char_len(&r.text);
        let mut best_run_len = 0;
        let mut lcs_scratch: Vec<u32> = Vec::new();
        for sr in &add_runs {
            let n = sr.adds.len();
            if n < 2 {
                continue;
            }
            let mut add_common_prefix: Vec<usize> = Vec::with_capacity(sr.ids.len() + 1);
            add_common_prefix.push(0);
            for t in 0..sr.ids.len() {
                let add = if del_set.contains(&sr.ids[t]) {
                    sr.tok_chars[t]
                } else {
                    0
                };
                add_common_prefix.push(*add_common_prefix.last().unwrap() + add);
            }
            for k in 0..n {
                for w in 2..=(n - k) {
                    if w <= best_run_len {
                        continue;
                    }
                    if sr.content_prefix[k + w] - sr.content_prefix[k] > ot_chars * 4 {
                        break;
                    }
                    let win = &sr.adds[k..k + w];
                    let live = kept_runs.iter().any(|br| br.as_slice() == win)
                        || win.iter().all(|a| !consumed_adds.contains(a));
                    if !live {
                        continue;
                    }
                    let tok_lo = sr.tok_prefix[k];
                    let tok_hi = sr.tok_prefix[k + w];
                    let nt_ids = &sr.ids[tok_lo..tok_hi];
                    let nt_tok_chars = &sr.tok_chars[tok_lo..tok_hi];
                    let nt_chars = sr.chars_prefix[k + w] - sr.chars_prefix[k];
                    let add_common = add_common_prefix[tok_hi] - add_common_prefix[tok_lo];
                    if (add_common as f64) < 0.25 * (ot_chars.max(nt_chars)) as f64 {
                        continue;
                    }
                    let nt_trimmed = if nt_ids.is_empty() {
                        win.iter()
                            .map(|&ai| h.rows[ai].text.trim())
                            .collect::<Vec<_>>()
                            .join("\n")
                            .trim()
                            .to_string()
                    } else {
                        String::new()
                    };
                    let sim = sim_window(
                        &del_tokens[&ri],
                        nt_ids,
                        nt_tok_chars,
                        nt_chars,
                        &nt_trimmed,
                        &interner.ws,
                        &interner.alnum,
                        &mut lcs_scratch,
                    );
                    if sim >= 0.25 {
                        best_run = Some(win.to_vec());
                        best_run_len = w;
                        best_run_score = sim;
                    }
                }
            }
        }

        let run_text = best_run
            .as_ref()
            .map(|run| {
                run.iter()
                    .map(|&ai| h.rows[ai].text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let is_comment = |t: &str| -> bool {
            let s = t.trim();
            s.starts_with("//") || s.starts_with("#") || s.starts_with("/*") || s.starts_with("*")
        };
        let is_comment_stretch_steal = if is_comment(&r.text)
            && best_run.as_ref().is_some_and(|run| run.len() > 1)
        {
            let mut i = ri + 1;
            let mut del_comments = 1;
            while i < h.rows.len() && h.rows[i].kind == RowKind::Del && is_comment(&h.rows[i].text)
            {
                del_comments += 1;
                i += 1;
            }
            del_comments > 1
        } else {
            false
        };

        let stretch_ok = best_run.is_some()
            && !is_comment_stretch_steal
            && best_run_score >= 0.25
            && best.is_some()
            && word_spans(&r.text, &h.rows[best.unwrap()].text).del.len() >= 3
            && !wholesale_pair(
                &r.text,
                &h.rows[best.unwrap()].text,
                &word_spans(&r.text, &h.rows[best.unwrap()].text),
            )
            && !wholesale_pair(&r.text, &run_text, &word_spans(&r.text, &run_text));

        if stretch_ok {
            let br = best_run.as_ref().unwrap().clone();
            stretch_of.insert(ri, br.clone());
            for &aid in &br {
                if let Some(pos) = pending_adds.iter().position(|&x| x == aid) {
                    pending_adds.remove(pos);
                } else if !pending_adds.is_empty() {
                    pending_adds.pop();
                }
            }
            for &aid in &br {
                consumed_adds.insert(aid);
            }
            kept_runs.push(br.clone());
        } else if let Some(b) = best {
            let spans = word_spans(&r.text, &h.rows[b].text);
            if !wholesale_pair(&r.text, &h.rows[b].text, &spans) {
                pair_of.insert(b, ri);
                pending_adds.retain(|&x| x != b);
            }
        }
    }

    PairingResults {
        pair_of,
        stretch_of,
        del_runs,
        run_of,
        del_ids,
    }
}

/// Pass 2: Reverse stretch identification.
pub fn pass_reverse_stretch(
    h: &Hunk,
    del_ids: &[usize],
    run_of: &HashMap<usize, Vec<usize>>,
    add_of: &HashMap<usize, usize>,
    paired_dels: &HashSet<usize>,
    stretched_dels: &HashSet<usize>,
) -> HashMap<usize, Vec<usize>> {
    let mut reverse_stretch_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for &ri in del_ids {
        let Some(run) = run_of.get(&ri) else {
            continue;
        };
        if run.len() <= 1 || run[0] != ri {
            continue;
        }
        let Some(&b) = add_of.get(&ri) else {
            continue;
        };
        if run.iter().skip(1).any(|&d| {
            let t = h.rows[d].text.trim();
            !t.is_empty() && h.rows[b].text.contains(t)
        }) {
            continue;
        }
        let folded: Vec<usize> = std::iter::once(ri)
            .chain(
                run.iter()
                    .skip(1)
                    .copied()
                    .filter(|d| !paired_dels.contains(d) && !stretched_dels.contains(d)),
            )
            .collect();
        if folded.len() > 1 {
            reverse_stretch_of.insert(ri, folded);
        }
    }
    reverse_stretch_of
}

/// Assembly product of the ordered walk.
pub struct AssembledMods {
    pub out: Vec<Row>,
    pub kept_dels: Vec<usize>,
    pub pure_adds: Vec<usize>,
    pub absorbed_dels: HashSet<usize>,
}

/// Context holding all pairing maps and sets for assembly.
pub struct PairingContext<'a> {
    pub pair_of: &'a HashMap<usize, usize>,
    pub add_of: &'a HashMap<usize, usize>,
    pub stretch_of: &'a HashMap<usize, Vec<usize>>,
    pub reverse_stretch_of: &'a HashMap<usize, Vec<usize>>,
    pub run_of: &'a HashMap<usize, Vec<usize>>,
    pub paired_dels: &'a HashSet<usize>,
    pub stretched_dels: &'a HashSet<usize>,
    pub stretched_adds: &'a HashSet<usize>,
}

/// Pass 3: Assemble Mod rows, subword spans, and section tails.
pub fn pass_assemble_mods(h: &Hunk, hash_comment: bool, ctx: &PairingContext) -> AssembledMods {
    let mut out: Vec<Row> = Vec::new();
    let mut kept_dels: Vec<usize> = Vec::new();
    let mut pure_adds: Vec<usize> = Vec::new();
    let mut absorbed_dels: HashSet<usize> = HashSet::new();
    let mut last_mod: Option<usize> = None;

    for (ri, r) in h.rows.iter().enumerate() {
        if r.kind == RowKind::Add {
            if ctx.pair_of.contains_key(&ri) || ctx.stretched_adds.contains(&ri) {
                continue;
            }
            let trimmed = r.text.trim();
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
            last_mod = None;
            pure_adds.push(ri);
        } else if ctx.paired_dels.contains(&ri) {
            let add_ri = ctx.add_of[&ri];
            let add = &h.rows[add_ri];
            let stretch_run = ctx.reverse_stretch_of.get(&ri);
            let del_text = match stretch_run {
                Some(run) => run
                    .iter()
                    .map(|&d| h.rows[d].text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                None => r.text.clone(),
            };
            let mut spans = word_spans(&del_text, &add.text);
            let run = ctx.run_of.get(&ri);
            let reflows: Vec<String> = match run {
                Some(run) if run.len() > 1 => run
                    .iter()
                    .skip(1)
                    .map(|&d| h.rows[d].text.trim().to_string())
                    .collect(),
                _ => Vec::new(),
            };
            if !reflows.is_empty() {
                spans.add.retain(|(as_, ae)| {
                    let seg = char_slice(&add.text, *as_, *ae);
                    !reflows
                        .iter()
                        .any(|t| !t.is_empty() && seg.contains(t.as_str()))
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
        } else if ctx.stretched_dels.contains(&ri) {
            let run = &ctx.stretch_of[&ri];
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

    AssembledMods {
        out,
        kept_dels,
        pure_adds,
        absorbed_dels: absorbed_dels.clone(),
    }
}

/// Reflow filter product.
pub struct ReflowFilter {
    pub meaningful_dels: Vec<usize>,
    pub kept_adds: Vec<usize>,
}

/// Pass 4: Filter verbatim reflow dels and adds.
pub fn pass_filter_reflows(
    h: &Hunk,
    out: &[Row],
    kept_dels: &[usize],
    pure_adds: &[usize],
    paired_dels: &HashSet<usize>,
) -> ReflowFilter {
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
        .filter(|(i, r)| {
            r.kind == RowKind::Del
                && !r.moved
                && !paired_dels.contains(i)
                && !shown_dels.contains(i)
        })
        .map(|(_, d)| d.text.trim())
        .collect::<Vec<_>>()
        .join("\n");

    let kept_adds: Vec<usize> = pure_adds
        .iter()
        .copied()
        .filter(|&ai| {
            let a = &h.rows[ai];
            if a.moved && a.del_text.as_deref().is_some_and(|dt| dt != a.text) {
                return true;
            }
            let t = a.text.trim();
            let corpus = if a.moved {
                &del_text_all
            } else {
                &del_text_live
            };
            t.is_empty() || !corpus.contains(t)
        })
        .collect();

    ReflowFilter {
        meaningful_dels: meaningful,
        kept_adds,
    }
}

/// Pass 5: Keyed anchor sorting into the final visual row stream.
pub fn pass_sort_ordered_walk(
    h: &Hunk,
    hash_comment: bool,
    out: Vec<Row>,
    meaningful_dels: &[usize],
    kept_adds: &[usize],
) -> Vec<Row> {
    let mut anchor_of: HashMap<usize, usize> = HashMap::new();
    for &di in meaningful_dels {
        let Some(dold) = h.rows[di].old_line else {
            continue;
        };
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

    let mut keyed: Vec<(Row, (u32, f64))> = Vec::new();
    for r in out.into_iter() {
        let key = (
            r.new_line.unwrap_or(0),
            if r.kind == RowKind::Mod { 1.0 } else { 2.0 },
        );
        keyed.push((r, key));
    }
    for &di in meaningful_dels {
        let d = &h.rows[di];
        let key = match anchor_of.get(&di) {
            None => (0, d.old_line.unwrap_or(0) as f64),
            Some(&mi) => {
                let base = keyed[mi].1.0;
                let mut last = base;
                for &ai in kept_adds {
                    let kn = h.rows[ai].new_line.unwrap_or(0);
                    if kn == last + 1 && is_comment_line(&h.rows[ai].text, hash_comment) {
                        last = kn;
                    }
                }
                (last, 2.5)
            }
        };
        keyed.push((h.rows[di].clone(), key));
    }
    for &ai in kept_adds {
        let a = &h.rows[ai];
        keyed.push((a.clone(), (a.new_line.unwrap_or(0), 2.0)));
    }
    keyed.sort_by(|a, b| {
        a.1.0.cmp(&b.1.0).then(
            a.1.1
                .partial_cmp(&b.1.1)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    keyed.into_iter().map(|(r, _)| r).collect()
}

/// Collapse each FIFO del→add pair into a single "mod" row: the new line
/// with its word-diff spans baked in.
///
/// Phase 6 Pipeline: composed of 5 pure, application-preserving passes over
/// the diff AST.
pub fn collapse_hunk_rows(
    h: &Hunk,
    hash_comment: bool,
    old_content: Option<&[String]>,
    new_content: Option<&[String]>,
) -> Vec<Row> {
    // Pass 1: Similarity, brace-anchor, and stretch window pairing
    let p1 = pass_pair_and_stretch(h, hash_comment, old_content, new_content);
    let paired_dels: HashSet<usize> = p1.pair_of.values().copied().collect();
    let add_of: HashMap<usize, usize> = p1.pair_of.iter().map(|(&add, &del)| (del, add)).collect();
    let stretched_dels: HashSet<usize> = p1.stretch_of.keys().copied().collect();
    let stretched_adds: HashSet<usize> = p1.stretch_of.values().flatten().copied().collect();

    // Pass 2: Reverse stretch identification
    let reverse_stretch_of = pass_reverse_stretch(
        h,
        &p1.del_ids,
        &p1.run_of,
        &add_of,
        &paired_dels,
        &stretched_dels,
    );

    // Pass 3: Assemble Mod rows, subword spans, and section tails
    let pairing_ctx = PairingContext {
        pair_of: &p1.pair_of,
        add_of: &add_of,
        stretch_of: &p1.stretch_of,
        reverse_stretch_of: &reverse_stretch_of,
        run_of: &p1.run_of,
        paired_dels: &paired_dels,
        stretched_dels: &stretched_dels,
        stretched_adds: &stretched_adds,
    };
    let p3 = pass_assemble_mods(h, hash_comment, &pairing_ctx);

    // Pass 4: Filter verbatim reflow dels and adds
    let p4 = pass_filter_reflows(h, &p3.out, &p3.kept_dels, &p3.pure_adds, &paired_dels);

    // Pass 5: Keyed anchor sorting into the final visual row stream
    pass_sort_ordered_walk(h, hash_comment, p3.out, &p4.meaningful_dels, &p4.kept_adds)
}

#[cfg(test)]
mod tests;
