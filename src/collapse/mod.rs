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
};

use crate::{
    lcs::{char_slice, tokenize_line, word_spans},
    parse::{AddLine, DelLine, Hunk, Row, Spans},
    shift::pass_shift_hunk,
    syntax::LangProfile,
};

/// Results of the initial pairing pass.
pub struct PairingResults {
    pub pair_of: HashMap<usize, usize>, // add row id -> del row id
    pub del_runs: Vec<Vec<usize>>,
    pub run_of: HashMap<usize, Vec<usize>>,
    pub del_ids: Vec<usize>,
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
pub fn pass_assemble_mods(
    h: &Hunk,
    ctx: &PairingContext,
    profile: Option<&LangProfile>,
) -> AssembledMods {
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
        } else if matches!(r, Row::Ctx(_)) {
            out.push(r.clone());
        } else if ctx.paired_dels.contains(&ri) {
            let add_ri = ctx.add_of[&ri];
            let add = &h.rows[add_ri];
            let del_text = r.text().to_string();
            let mut spans = word_spans(&del_text, add.text());

            let mut next_adds = Vec::new();
            let mut k = add_ri + 1;
            let has_deleted_content = spans.del.iter().any(|&(ds, de)| {
                let seg = char_slice(&del_text, ds, de);
                tokenize_line(seg)
                    .into_iter()
                    .any(|tok| !tok.trim().is_empty() && tok.chars().any(char::is_alphanumeric))
            });

            if has_deleted_content && let Some(p) = profile {
                while k < h.rows.len()
                    && matches!(h.rows[k], Row::Add(_))
                    && !ctx.pair_of.contains_key(&k)
                {
                    let nxt_t = h.rows[k].text().trim();
                    if p.is_decl_break(nxt_t) {
                        break;
                    }
                    let is_continuation = p.is_continuation_line(nxt_t);
                    let is_closing = p.is_expression_close(nxt_t);
                    let matches_del = tokenize_line(nxt_t).into_iter().any(|tok| {
                        !tok.trim().is_empty()
                            && tok.chars().any(char::is_alphanumeric)
                            && spans.del.iter().any(|&(ds, de)| {
                                let seg = char_slice(&del_text, ds, de);
                                seg.contains(tok)
                            })
                    });

                    if is_closing || is_continuation || matches_del {
                        next_adds.push(k);
                        k += 1;
                        if is_closing {
                            break;
                        }
                    } else if p.is_import_statement(&del_text) {
                        let has_closing = (k..h.rows.len())
                            .take_while(|&idx| {
                                matches!(h.rows[idx], Row::Add(_))
                                    && !ctx.pair_of.contains_key(&idx)
                            })
                            .any(|idx| {
                                let t = h.rows[idx].text().trim();
                                p.is_block_close(t) || del_text.trim_end().ends_with(t)
                            });
                        if has_closing {
                            next_adds.push(k);
                            k += 1;
                        } else {
                            break;
                        }
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

                let mut moved_token_pos = None;
                for &(ds, de) in &spans.del {
                    let seg = char_slice(&del_text, ds, de);
                    let mut pos = ds;
                    for tok in tokenize_line(seg) {
                        let start = seg[pos - ds..]
                            .find(tok)
                            .map(|off| pos + off)
                            .unwrap_or(pos);
                        pos = start + tok.len();
                        if !tok.trim().is_empty()
                            && tok.chars().any(char::is_alphanumeric)
                            && next_adds_text.contains(tok)
                        {
                            moved_token_pos = Some(start);
                            break;
                        }
                    }
                    if moved_token_pos.is_some() {
                        break;
                    }
                }

                if let Some(sp) = moved_token_pos {
                    let del_head = &del_text[..sp];
                    spans = word_spans(del_head, add.text());
                } else {
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
                }

                for &nxt_i in &next_adds {
                    let add_nxt = &h.rows[nxt_i];
                    let mut donor_spans = word_spans(&del_text, add_nxt.text());
                    let tok_count = tokenize_line(add_nxt.text())
                        .into_iter()
                        .filter(|t| !t.trim().is_empty())
                        .count();
                    let nxt_trim = add_nxt.text().trim();
                    let is_closing_row = nxt_trim == "};" || nxt_trim == "}" || nxt_trim == ");";
                    let is_opening_row = nxt_trim == "{" || nxt_trim == "(";

                    if is_opening_row {
                        let text_len = add_nxt.text().chars().count();
                        let indent_len = add_nxt
                            .text()
                            .chars()
                            .take_while(|c| c.is_whitespace())
                            .count();
                        if del_text.trim_end().ends_with(nxt_trim) {
                            let spans = Spans {
                                del: Vec::new(),
                                add: if indent_len > 0 {
                                    vec![(0, indent_len)]
                                } else {
                                    Vec::new()
                                },
                            };
                            add_spans.insert(nxt_i, (spans, del_text.clone()));
                        } else if text_len > 0 {
                            let spans = Spans {
                                del: Vec::new(),
                                add: vec![(0, text_len)],
                            };
                            add_spans.insert(nxt_i, (spans, del_text.clone()));
                        }
                    } else if (!donor_spans.add.is_empty() || is_closing_row)
                        && donor_spans.add.len() < tok_count
                    {
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
                        donor_spans.del = donor_spans
                            .del
                            .into_iter()
                            .flat_map(|(ds, de)| {
                                let seg = char_slice(&del_text, ds, de);
                                let tokens: Vec<&str> = tokenize_line(seg)
                                    .into_iter()
                                    .filter(|t| !t.trim().is_empty())
                                    .collect();
                                let in_other = tokens.iter().any(|t| other_adds_text.contains(t));
                                if in_other {
                                    let is_trailing_open_delim = de == del_text.len()
                                        && (seg.ends_with('{') || seg.ends_with('('));
                                    if is_trailing_open_delim {
                                        let ws_len =
                                            seg.chars().take_while(|c| c.is_whitespace()).count();
                                        if ws_len > 0 {
                                            vec![(ds, ds + ws_len)]
                                        } else {
                                            Vec::new()
                                        }
                                    } else {
                                        Vec::new()
                                    }
                                } else {
                                    vec![(ds, de)]
                                }
                            })
                            .collect();

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
                    } else {
                        let text_len = add_nxt.text().chars().count();
                        if text_len > 0 {
                            let spans = Spans {
                                del: Vec::new(),
                                add: vec![(0, text_len)],
                            };
                            add_spans.insert(nxt_i, (spans, del_text.clone()));
                        }
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
        } else {
            let is_was_moved = r.is_moved()
                || r.old_line().is_some_and(|ol| {
                    h.rows
                        .iter()
                        .any(|other| other.old_line() == Some(ol) && other.is_moved())
                });
            if !is_was_moved {
                kept_dels.push(ri);
            }
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
        let key = (h.rows[ai].new_line().unwrap_or(0), 1.0);
        keyed.push((a, key));
    }

    keyed.sort_by(|a, b| {
        let (num_a, sub_a) = a.1;
        let (num_b, sub_b) = b.1;
        if num_a != num_b {
            num_a.cmp(&num_b)
        } else {
            sub_a.partial_cmp(&sub_b).unwrap_or(Ordering::Equal)
        }
    });

    keyed.into_iter().map(|(r, _)| r).collect()
}

/// Collapse each FIFO del→add pair into a single "mod" row: the new line
/// with its word-diff spans baked in.
///
/// Composed of 4 pure passes over the diff AST.
pub fn collapse_hunk_rows(
    h: &Hunk,
    old_content: Option<&[String]>,
    new_content: Option<&[String]>,
    profile: Option<&LangProfile>,
) -> Vec<Row> {
    let h_norm = pass_shift_hunk(h, new_content, profile);
    let h = &h_norm;

    // Pass 1: DP cluster pairing engine
    let p1 = crate::cluster::pairing::dp_pairing(h, old_content, new_content, profile);
    let paired_dels: HashSet<usize> = p1.pair_of.values().copied().collect();
    let add_of: HashMap<usize, usize> = p1.pair_of.iter().map(|(&add, &del)| (del, add)).collect();

    // Pass 2: Assemble Mod rows and subword spans
    let pairing_ctx = PairingContext {
        pair_of: &p1.pair_of,
        add_of: &add_of,
        run_of: &p1.run_of,
        paired_dels: &paired_dels,
    };
    let p2 = pass_assemble_mods(h, &pairing_ctx, profile);

    // Pass 3: Filter verbatim reflow dels and adds
    let p3 = pass_filter_reflows(h, &p2.out, &p2.kept_dels, &p2.pure_adds, &paired_dels);

    // Pass 4: Keyed anchor sorting into the final visual row stream
    pass_sort_ordered_walk(h, p2.out, &p3.meaningful_dels, &p3.kept_adds, &p2.add_spans)
}

#[cfg(test)]
mod tests;
