//! One-for-one port of `review.ts`'s `collapseHunkRows` (PLAN.md §3),
//! refactored in Phase 6 into composable passes over a hunk's diff AST:
//!
//! 1. `pass_pairing`: Similarity and brace-anchor pairing (1-to-1).
//! 2. `pass_assemble_mods`: Assemble 1-to-1 `Mod` rows, multiline subword
//!    distribution, and reflow suppression.
//! 3. `pass_filter_reflows`: Filtering verbatim reflow dels and adds.
//! 4. `pass_sort_ordered_walk`: Keyed anchor sorting into the final visual row
//!    stream.

use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

use regex::Regex;

use crate::{
    lcs::{char_slice, tokenize_line, word_spans},
    merge::wholesale_pair,
    parse::{AddLine, DelLine, Hunk, Row, Spans},
    scope::{fn_of, is_tail_before_closing},
    shift::pass_shift_hunk,
    similarity::{LineTokens, TokenInterner, is_comment_line, line_similarity_parts_tokens},
};

/// Results of the initial pairing pass.
pub struct PairingResults {
    pub pair_of: HashMap<usize, usize>, // add row id -> del row id
    pub del_runs: Vec<Vec<usize>>,
    pub run_of: HashMap<usize, Vec<usize>>,
    pub del_ids: Vec<usize>,
}

/// Pass 1: Similarity and brace-anchor 1-to-1 pairing.
pub fn pass_pairing(
    h: &Hunk,
    hash_comment: bool,
    old_content: Option<&[String]>,
    new_content: Option<&[String]>,
) -> PairingResults {
    let mut pending_adds: Vec<usize> = h
        .rows
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r, Row::Add(_)))
        .map(|(i, _)| i)
        .collect();

    let mut del_runs: Vec<Vec<usize>> = Vec::new();
    let mut i = 0;
    while i < h.rows.len() {
        if !matches!(h.rows[i], Row::Del(_)) {
            i += 1;
            continue;
        }
        let mut run = vec![i];
        while i + 1 < h.rows.len() && matches!(h.rows[i + 1], Row::Del(_)) {
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
        .filter(|(_, r)| matches!(r, Row::Del(_)))
        .map(|(i, _)| i)
        .collect();

    let mut cluster_id: Vec<usize> = Vec::with_capacity(h.rows.len());
    let mut cur_cluster = 0;
    for r in &h.rows {
        if matches!(r, Row::Ctx(_)) {
            cur_cluster += 1;
            cluster_id.push(usize::MAX);
        } else {
            cluster_id.push(cur_cluster);
        }
    }

    let mut interner = TokenInterner::new();
    let mut del_tokens: HashMap<usize, LineTokens> = HashMap::new();
    let mut add_tokens: HashMap<usize, LineTokens> = HashMap::new();
    for &ri in &del_ids {
        del_tokens.insert(ri, interner.line_tokens(h.rows[ri].text()));
    }
    for &ai in &pending_adds {
        add_tokens.insert(ai, interner.line_tokens(h.rows[ai].text()));
    }

    let mut pair_of: HashMap<usize, usize> = HashMap::new();

    for &ri in &del_ids {
        let r = &h.rows[ri];
        if r.is_moved() {
            continue;
        }
        let r_trim = r.text().trim();
        let was_moved = !r_trim.is_empty()
            && h.rows.iter().any(|a| {
                a.is_moved() && (a.old_line() == r.old_line() || a.text().trim() == r_trim)
            });
        if was_moved {
            continue;
        }
        let del_is_tail = is_tail_before_closing(r.old_line(), true, old_content, new_content);
        let tail_candidate = del_is_tail
            && pending_adds.iter().any(|&ai| {
                let a = &h.rows[ai];
                is_tail_before_closing(a.new_line(), false, old_content, new_content)
                    && fn_of(old_content, r.old_line()) == fn_of(new_content, a.new_line())
            });
        let has_same_hunk_add = pending_adds
            .iter()
            .any(|&ai| h.rows[ai].text().trim() == r_trim);
        if !r_trim.is_empty()
            && !tail_candidate
            && !has_same_hunk_add
            && new_content.is_some_and(|c| c.iter().any(|l| l.trim() == r_trim))
        {
            continue;
        }

        let del_fn = fn_of(old_content, r.old_line());
        let mut best: Option<usize> = None;
        let mut best_score = 0.15;
        let del_set: HashSet<u32> = del_tokens[&ri].ids.iter().copied().collect();

        let del_cluster = cluster_id[ri];
        for &ai in &pending_adds {
            if cluster_id[ai] != del_cluster {
                continue;
            }
            let a = &h.rows[ai];
            let add_fn = fn_of(new_content, a.new_line());
            if let (Some(dfn), Some(afn)) = (&del_fn, &add_fn)
                && dfn != afn
                && (fn_exists(new_content, dfn) || fn_exists(old_content, afn))
            {
                continue;
            }
            if let Some(run) = run_of.get(&ri) {
                let trimmed = r.text().trim();
                if run.len() > 1
                    && run[0] != ri
                    && !trimmed.is_empty()
                    && a.text().trim().contains(trimmed)
                {
                    continue;
                }
            }

            let is_del_comment = is_comment_line(r.text(), hash_comment);
            let is_add_comment = is_comment_line(a.text(), hash_comment);
            if is_del_comment != is_add_comment {
                continue;
            }

            let head_bonus = {
                let kw_del = statement_head_keyword(r.text().trim_start());
                let kw_add = statement_head_keyword(a.text().trim_start());
                if let (Some(kd), Some(ka)) = (kw_del, kw_add)
                    && kd == ka
                {
                    1.0
                } else {
                    0.0
                }
            };
            let dt = &del_tokens[&ri];
            let at = &add_tokens[&ai];
            let brace_bonus = if del_is_tail
                && is_tail_before_closing(a.new_line(), false, old_content, new_content)
                && del_fn == fn_of(new_content, a.new_line())
            {
                1.0
            } else {
                0.0
            };
            let bonus = head_bonus + brace_bonus;
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
            let min_sim = if pending_adds.first() == Some(&ai) {
                0.3
            } else {
                0.4
            };
            if parts.add_total > 0
                && parts.add_matched as f64 >= parts.add_total as f64 * 0.8
                && parts.sim < min_sim
                && pending_adds.len() > 1
            {
                continue;
            }

            let spans = word_spans(r.text(), a.text());
            if wholesale_pair(r.text(), a.text(), &spans) {
                continue;
            }

            let score = head_bonus + brace_bonus + parts.sim;
            if score > best_score {
                best_score = score;
                best = Some(ai);
            }
        }

        if let Some(b) = best {
            let spans = word_spans(r.text(), h.rows[b].text());
            if !wholesale_pair(r.text(), h.rows[b].text(), &spans) {
                pair_of.insert(b, ri);
                pending_adds.retain(|&x| x != b);
            }
        }
    }

    PairingResults {
        pair_of,
        del_runs,
        run_of,
        del_ids,
    }
}

fn fn_exists(content: Option<&[String]>, fn_name: &str) -> bool {
    let Some(content) = content else { return false };
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?fn\s+([A-Za-z0-9_]+)").expect("fn regex")
    });
    content
        .iter()
        .any(|l| re.captures(l.trim()).is_some_and(|c| c[1] == *fn_name))
}

/// Helper recognizing statement head keywords for pairing.
fn statement_head_keyword(s: &str) -> Option<&str> {
    const KEYWORDS: &[&str] = &[
        "if", "let", "return", "match", "for", "while", "loop", "fn", "pub", "struct", "enum",
        "impl", "type", "use", "const", "static",
    ];
    let word = s.split_whitespace().next()?;
    let word = word.trim_end_matches('(');
    if KEYWORDS.contains(&word) {
        Some(word)
    } else {
        None
    }
}

/// Products of the assembly pass.
pub struct AssembledMods {
    pub out: Vec<Row>,
    pub kept_dels: Vec<usize>,
    pub pure_adds: Vec<usize>,
    pub add_spans: HashMap<usize, (Spans, String)>,
}

/// Context holding all pairing maps and sets for assembly.
pub struct PairingContext<'a> {
    pub pair_of: &'a HashMap<usize, usize>,
    pub add_of: &'a HashMap<usize, usize>,
    pub run_of: &'a HashMap<usize, Vec<usize>>,
    pub paired_dels: &'a HashSet<usize>,
}

/// Pass 2: Assemble 1-to-1 Mod rows, multiline subword distribution, and reflow
/// suppression.
pub fn pass_assemble_mods(h: &Hunk, _hash_comment: bool, ctx: &PairingContext) -> AssembledMods {
    let mut out: Vec<Row> = Vec::new();
    let mut kept_dels: Vec<usize> = Vec::new();
    let mut pure_adds: Vec<usize> = Vec::new();
    let mut add_spans: HashMap<usize, (Spans, String)> = HashMap::new();

    for (ri, r) in h.rows.iter().enumerate() {
        if matches!(r, Row::Add(_) | Row::Mov { .. }) {
            if ctx.pair_of.contains_key(&ri) {
                continue;
            }
            pure_adds.push(ri);
        } else if ctx.paired_dels.contains(&ri) {
            let add_ri = ctx.add_of[&ri];
            let add = &h.rows[add_ri];
            let del_text = r.text().to_string();
            let mut spans = word_spans(&del_text, add.text());

            let mut next_adds = Vec::new();
            let mut k = add_ri + 1;
            let is_potential_expansion = statement_head_keyword(&del_text).is_some()
                || del_text.trim_end().ends_with('{')
                || del_text.trim_end().ends_with('(');

            if is_potential_expansion {
                while k < h.rows.len()
                    && matches!(h.rows[k], Row::Add(_))
                    && !ctx.pair_of.contains_key(&k)
                {
                    let nxt_t = h.rows[k].text().trim();
                    if nxt_t.starts_with("///")
                        || nxt_t.starts_with("#[")
                        || nxt_t.starts_with("pub ")
                        || nxt_t.starts_with("fn ")
                    {
                        break;
                    }
                    let is_continuation = nxt_t.starts_with("&&")
                        || nxt_t.starts_with("||")
                        || nxt_t.starts_with('.')
                        || nxt_t == "{"
                        || nxt_t == "}"
                        || tokenize_line(nxt_t).into_iter().any(|tok| {
                            !tok.trim().is_empty()
                                && tok.chars().any(char::is_alphanumeric)
                                && del_text.contains(tok)
                        });

                    if is_continuation {
                        next_adds.push(k);
                        k += 1;
                    } else {
                        break;
                    }
                }
            }

            if !next_adds.is_empty() {
                let next_adds_text = next_adds
                    .iter()
                    .map(|&idx| h.rows[idx].text())
                    .collect::<Vec<_>>()
                    .join("\n");

                spans.del.retain(|&(ds, de)| {
                    let seg = char_slice(&del_text, ds, de);
                    let tokens: Vec<&str> = tokenize_line(seg)
                        .into_iter()
                        .filter(|t| !t.trim().is_empty())
                        .collect();
                    let has_alpha = tokens.iter().any(|t| t.chars().any(char::is_alphanumeric));
                    if has_alpha {
                        let moved = tokens.iter().any(|t| next_adds_text.contains(t));
                        !moved
                    } else {
                        true
                    }
                });

                for &nxt_i in &next_adds {
                    let add_nxt = &h.rows[nxt_i];
                    let mut donor_spans = word_spans(&del_text, add_nxt.text());
                    let tok_count = tokenize_line(add_nxt.text())
                        .into_iter()
                        .filter(|t| !t.trim().is_empty())
                        .count();
                    if !donor_spans.add.is_empty() && donor_spans.add.len() < tok_count {
                        let other_adds_text = format!(
                            "{}\n{}",
                            add.text(),
                            next_adds
                                .iter()
                                .filter(|&&i| i != nxt_i)
                                .map(|&idx| h.rows[idx].text())
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                        donor_spans.del.retain(|&(ds, de)| {
                            let seg = char_slice(&del_text, ds, de);
                            let tokens: Vec<&str> = tokenize_line(seg)
                                .into_iter()
                                .filter(|t| !t.trim().is_empty())
                                .collect();
                            let in_other = tokens.iter().any(|t| other_adds_text.contains(t));
                            !in_other
                        });

                        let indent_len = add_nxt
                            .text()
                            .chars()
                            .take_while(|c| c.is_whitespace())
                            .count();
                        donor_spans.add = donor_spans
                            .add
                            .into_iter()
                            .map(|(as_, ae)| {
                                let start = as_;
                                let mut end = ae;
                                let chars: Vec<char> = add_nxt.text().chars().collect();
                                if as_ <= indent_len && end < chars.len() && chars[end] == ' ' {
                                    end += 1;
                                }
                                (start, end)
                            })
                            .filter(|(s, e)| s < e)
                            .collect();

                        add_spans.insert(nxt_i, (donor_spans, del_text.clone()));
                    }
                }
            }

            let run = ctx.run_of.get(&ri);
            let reflows: Vec<String> = match run {
                Some(run) if run.len() > 1 => run
                    .iter()
                    .skip(1)
                    .map(|&d| h.rows[d].text().trim().to_string())
                    .collect(),
                _ => Vec::new(),
            };
            if !reflows.is_empty() {
                spans.add.retain(|(as_, ae)| {
                    let seg = char_slice(add.text(), *as_, *ae);
                    !reflows
                        .iter()
                        .any(|t| !t.is_empty() && seg.contains(t.as_str()))
                });
            }
            let mod_row = Row::Mod {
                old_line: DelLine::new(r.old_line_num().unwrap(), del_text),
                new_line: AddLine::with_spans(add.new_line_num().unwrap(), add.text(), spans),
            };
            out.push(mod_row);
        } else if !r.is_moved() {
            kept_dels.push(ri);
        }
    }

    AssembledMods {
        out,
        kept_dels,
        pure_adds,
        add_spans,
    }
}

/// Reflow filter product.
pub struct ReflowFilter {
    pub meaningful_dels: Vec<usize>,
    pub kept_adds: Vec<usize>,
}

/// Pass 3: Filter verbatim reflow dels and adds.
pub fn pass_filter_reflows(
    h: &Hunk,
    out: &[Row],
    kept_dels: &[usize],
    pure_adds: &[usize],
    paired_dels: &HashSet<usize>,
) -> ReflowFilter {
    let add_text = out
        .iter()
        .map(super::parse::Row::text)
        .chain(pure_adds.iter().map(|&ri| h.rows[ri].text()))
        .collect::<Vec<_>>()
        .join("\n");

    let meaningful: Vec<usize> = kept_dels
        .iter()
        .copied()
        .filter(|&di| {
            let t = h.rows[di].text().trim();
            t.is_empty() || !add_text.contains(t)
        })
        .collect();

    let del_text_all = h
        .rows
        .iter()
        .filter(|r| matches!(r, Row::Del(_)))
        .map(|d| d.text().trim())
        .collect::<Vec<_>>()
        .join("\n");
    let shown_dels: HashSet<usize> = meaningful.iter().copied().collect();
    let del_text_live = h
        .rows
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            let r_trim = r.text().trim();
            matches!(r, Row::Del(_))
                && !r.is_moved()
                && !h.rows.iter().any(|m| {
                    m.is_moved()
                        && (m.old_line() == r.old_line()
                            || (!r_trim.is_empty() && m.text().trim() == r_trim))
                })
                && !paired_dels.contains(i)
                && !shown_dels.contains(i)
        })
        .map(|(_, d)| d.text().trim())
        .collect::<Vec<_>>()
        .join("\n");

    let kept_adds: Vec<usize> = pure_adds
        .iter()
        .copied()
        .filter(|&ai| {
            let a = &h.rows[ai];
            if a.is_moved() && a.del_text().is_some_and(|dt| dt != a.text()) {
                return true;
            }
            let t = a.text().trim();
            let corpus = if a.is_moved() {
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

/// Pass 4: Keyed anchor sorting into the final visual row stream.
pub fn pass_sort_ordered_walk(
    h: &Hunk,
    _hash_comment: bool,
    out: Vec<Row>,
    meaningful_dels: &[usize],
    kept_adds: &[usize],
    add_spans: &HashMap<usize, (Spans, String)>,
) -> Vec<Row> {
    let mut anchor_of: HashMap<usize, usize> = HashMap::new();
    for &di in meaningful_dels {
        let Some(dold) = h.rows[di].old_line() else {
            continue;
        };
        let mut anchor: Option<usize> = None;
        for (mi, m) in out.iter().enumerate() {
            let Some(mold) = m.old_line() else {
                continue;
            };
            if mold <= dold && (anchor.is_none() || mold > out[anchor.unwrap()].old_line().unwrap())
            {
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
            r.new_line().unwrap_or(0),
            if matches!(r, Row::Mod { .. }) || matches!(r, Row::Ctx(_)) {
                1.0
            } else {
                2.0
            },
        );
        keyed.push((r, key));
    }
    for &di in meaningful_dels {
        let d = &h.rows[di];
        let key = match anchor_of.get(&di) {
            None => (0, d.old_line().unwrap_or(0) as f64),
            Some(&mi) => {
                let base = keyed[mi].1.0;
                (base, 1.5)
            }
        };
        keyed.push((h.rows[di].clone(), key));
    }
    for &ai in kept_adds {
        let a = match &h.rows[ai] {
            Row::Add(add_line) => {
                if let Some((spans, del_text)) = add_spans.get(&ai) {
                    Row::Add(AddLine::with_donor(
                        add_line.num,
                        add_line.text.clone(),
                        del_text.clone(),
                        spans.clone(),
                    ))
                } else {
                    Row::Add(add_line.clone())
                }
            }
            Row::Mov { old_line, new_line } => {
                if let Some((spans, _)) = add_spans.get(&ai) {
                    Row::Mov {
                        old_line: old_line.clone(),
                        new_line: AddLine::with_spans(
                            new_line.num,
                            new_line.text.clone(),
                            spans.clone(),
                        ),
                    }
                } else {
                    Row::Mov {
                        old_line: old_line.clone(),
                        new_line: new_line.clone(),
                    }
                }
            }
            _ => h.rows[ai].clone(),
        };
        keyed.push((a, (h.rows[ai].new_line().unwrap_or(0), 2.0)));
    }
    keyed.sort_by(|a, b| {
        a.1.0
            .cmp(&b.1.0)
            .then(a.1.1.partial_cmp(&b.1.1).unwrap_or(Ordering::Equal))
    });
    keyed.into_iter().map(|(r, _)| r).collect()
}

/// Collapse each FIFO del→add pair into a single "mod" row: the new line
/// with its word-diff spans baked in.
///
/// Composed of 4 pure passes over the diff AST.
pub fn collapse_hunk_rows(
    h: &Hunk,
    hash_comment: bool,
    old_content: Option<&[String]>,
    new_content: Option<&[String]>,
) -> Vec<Row> {
    let h_norm = pass_shift_hunk(h, new_content);
    let h = &h_norm;

    // Pass 1: Similarity and brace-anchor pairing
    let p1 = pass_pairing(h, hash_comment, old_content, new_content);
    let paired_dels: HashSet<usize> = p1.pair_of.values().copied().collect();
    let add_of: HashMap<usize, usize> = p1.pair_of.iter().map(|(&add, &del)| (del, add)).collect();

    // Pass 2: Assemble Mod rows and subword spans
    let pairing_ctx = PairingContext {
        pair_of: &p1.pair_of,
        add_of: &add_of,
        run_of: &p1.run_of,
        paired_dels: &paired_dels,
    };
    let p2 = pass_assemble_mods(h, hash_comment, &pairing_ctx);

    // Pass 3: Filter verbatim reflow dels and adds
    let p3 = pass_filter_reflows(h, &p2.out, &p2.kept_dels, &p2.pure_adds, &paired_dels);

    // Pass 4: Keyed anchor sorting into the final visual row stream
    pass_sort_ordered_walk(
        h,
        hash_comment,
        p2.out,
        &p3.meaningful_dels,
        &p3.kept_adds,
        &p2.add_spans,
    )
}

#[cfg(test)]
mod tests;
