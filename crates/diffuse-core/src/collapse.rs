//! One-for-one port of `review.ts`'s `collapseHunkRows` (PLAN.md §3),
//! refactored in Phase 6 into composable passes over a hunk's diff AST:
//!
//! 1. `pass_pairing`: Similarity and brace-anchor pairing (1-to-1).
//! 2. `pass_assemble_mods`: Assemble 1-to-1 `Mod` rows, multiline subword
//!    distribution, and reflow suppression.
//! 3. `pass_filter_reflows`: Filtering verbatim reflow dels and adds.
//! 4. `pass_sort_ordered_walk`: Keyed anchor sorting into the final visual row
//!    stream.

use std::cmp::Ordering;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    lcs::{char_slice, tokenize_line, word_spans_with_profile},
    parse::{AddLine, DelLine, Hunk, Row, Spans},
    shift::pass_shift_hunk,
    syntax::profile::LangProfile,
};

/// Results of the initial pairing pass.
pub struct PairingResults {
    pub pair_of: FxHashMap<usize, usize>, // add row id -> del row id
    pub del_runs: Vec<Vec<usize>>,
    pub run_of: FxHashMap<usize, Vec<usize>>,
    pub del_ids: Vec<usize>,
    pub collapse_blocks: FxHashMap<usize, Vec<usize>>,
    pub expand_blocks: FxHashMap<usize, Vec<usize>>,
}

/// Products of the assembly pass.
pub struct AssembledMods {
    pub out: Vec<Row>,
    pub kept_dels: Vec<usize>,
    pub pure_adds: Vec<usize>,
    pub add_spans: FxHashMap<usize, (Spans, String)>,
}

/// Context holding all pairing maps and sets for assembly.
pub struct PairingContext<'a> {
    pub pair_of: &'a FxHashMap<usize, usize>,
    pub add_of: &'a FxHashMap<usize, usize>,
    pub run_of: &'a FxHashMap<usize, Vec<usize>>,
    pub paired_dels: &'a FxHashSet<usize>,
    pub collapse_blocks: &'a FxHashMap<usize, Vec<usize>>,
    pub expand_blocks: &'a FxHashMap<usize, Vec<usize>>,
}

/// Collapse each FIFO del→add pair into a single "mod" row: the new line
/// with its word-diff spans baked in.
///
/// Composed of 4 pure passes over the diff AST.
#[tracing::instrument(
    level = "debug",
    ret,
    skip(h, old_content, new_content, profile),
    fields(old_start = h.old_start, new_start = h.new_start, row_count = h.rows.len())
)]
pub fn collapse_hunk_rows(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Vec<Row> {
    let h_norm = pass_shift_hunk(h, profile);
    let h = &h_norm;

    // Pass 1: DP cluster pairing engine
    let p1 = crate::cluster::pairing::dp_pairing(h, old_content, new_content, profile);

    let paired_dels: FxHashSet<usize> = p1.pair_of.values().copied().collect();
    let add_of: FxHashMap<usize, usize> =
        p1.pair_of.iter().map(|(&add, &del)| (del, add)).collect();

    // Pass 2: Assemble Mod rows and subword spans
    let pairing_ctx = PairingContext {
        pair_of: &p1.pair_of,
        add_of: &add_of,
        run_of: &p1.run_of,
        paired_dels: &paired_dels,
        collapse_blocks: &p1.collapse_blocks,
        expand_blocks: &p1.expand_blocks,
    };
    let p2 = pass_assemble_mods(h, &pairing_ctx, profile);

    // Pass 3: Filter verbatim reflow dels and adds
    let p3 = pass_filter_reflows(
        h,
        &p2.out,
        &p2.kept_dels,
        &p2.pure_adds,
        &pairing_ctx,
        profile,
    );

    // Pass 4: Keyed anchor sorting into the final visual row stream
    let res = pass_sort_ordered_walk(
        h,
        p2.out,
        &p3.meaningful_dels,
        &p3.kept_adds,
        &p2.add_spans,
        p3.reflow_ctx,
    );

    crate::ensure!(
        res.iter().all(|r| match r {
            Row::Mod {
                old_line, new_line, ..
            } => old_line.num.get() > 0 && new_line.num.get() > 0,
            Row::Ctx(c) => c.old_pos.get() > 0 && c.new_pos.get() > 0,
            Row::Add(a) => a.num.get() > 0,
            Row::Del(d) => d.num.get() > 0,
            Row::MovSrc(d) => d.num.get() > 0,
            Row::MovTgt(a) => a.num.get() > 0,
            Row::Sep(_) => true,
        }),
        "All collapsed rows must have valid, non-zero line coordinates"
    );

    crate::invariant!(
        res.iter()
            .all(|r| !matches!(r, Row::Mod { .. }) || !crate::merge::is_wholesale_replacement(r)),
        "Collapsed rows must never contain wholesale replacement Mod rows"
    );

    res
}

fn default_join_needs_space(last: Option<char>, first: Option<char>) -> bool {
    let Some(l) = last else {
        return false;
    };
    let Some(f) = first else {
        return false;
    };

    // Never space after open delimiters
    if l == '(' || l == '[' {
        return false;
    }
    // Never space before close delimiters, commas, semicolons, or access dots
    if f == ')' || f == ']' || f == ',' || f == ';' || f == '.' || f == ':' {
        return false;
    }
    // Never space after access dot
    if l == '.' {
        return false;
    }

    true
}

fn join_collapse_lines(lines: &[&str], new_text: &str) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines[0].trim_end().to_string();
    for line in &lines[1..] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let last_char = out.chars().last();
        let first_char = trimmed.chars().next();
        let prev_tok = out.split_whitespace().last().unwrap_or("");
        let next_tok = trimmed.split_whitespace().next().unwrap_or("");

        let needs_space = if !prev_tok.is_empty() && !next_tok.is_empty() {
            let joined_no_space = format!("{}{}", prev_tok, next_tok);
            let joined_with_space = format!("{} {}", prev_tok, next_tok);
            if new_text.contains(&joined_no_space) {
                false
            } else if new_text.contains(&joined_with_space) {
                true
            } else {
                default_join_needs_space(last_char, first_char)
            }
        } else {
            default_join_needs_space(last_char, first_char)
        };

        if needs_space {
            out.push(' ');
        }
        out.push_str(trimmed);
    }
    out
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
    let mut add_spans: FxHashMap<usize, (Spans, String)> = FxHashMap::default();

    let mut collapse_absorbed_dels: FxHashSet<usize> = FxHashSet::default();
    for dels in ctx.collapse_blocks.values() {
        for &d in dels {
            collapse_absorbed_dels.insert(d);
        }
    }

    for (ri, r) in h.rows.iter().enumerate() {
        if matches!(r, Row::MovTgt(_)) {
            out.push(r.clone());
        } else if matches!(r, Row::MovSrc(_)) {
            kept_dels.push(ri);
        } else if matches!(r, Row::Add(_)) {
            if ctx.pair_of.contains_key(&ri) {
                continue;
            }
            pure_adds.push(ri);
        } else if matches!(r, Row::Ctx(_)) {
            out.push(r.clone());
        } else if ctx.paired_dels.contains(&ri) {
            let add_ri = ctx.add_of[&ri];
            let add = &h.rows[add_ri];

            let (del_text, first_old_num) =
                if let Some(block_dels) = ctx.collapse_blocks.get(&add_ri) {
                    let lines: Vec<&str> = block_dels.iter().map(|&di| h.rows[di].text()).collect();
                    let joined = join_collapse_lines(&lines, add.text());
                    let first_num = block_dels
                        .first()
                        .and_then(|&di| h.rows[di].old_line_num())
                        .unwrap_or_else(|| r.old_line_num().unwrap());
                    (joined, first_num)
                } else {
                    (r.text().to_string(), r.old_line_num().unwrap())
                };

            let spans = word_spans_with_profile(&del_text, add.text(), profile);

            if crate::merge::is_wholesale_replacement_spans(&del_text, add.text(), &spans) {
                kept_dels.push(ri);
                pure_adds.push(add_ri);
                continue;
            }

            let mut next_adds = Vec::new();
            let mut max_scanned = add_ri;
            if let Some(block) = ctx.expand_blocks.get(&add_ri) {
                for &ai in block {
                    if ai != add_ri {
                        next_adds.push(ai);
                    }
                    max_scanned = max_scanned.max(ai);
                }
            }

            if !ctx.collapse_blocks.contains_key(&add_ri) {
                let mut k = max_scanned + 1;
                let has_deleted_content = spans.del.iter().any(|&(ds, de)| {
                    let seg = char_slice(&del_text, ds, de);
                    tokenize_line(seg)
                        .into_iter()
                        .any(|tok| !tok.trim().is_empty() && tok.chars().any(char::is_alphanumeric))
                });

                if (has_deleted_content || ctx.expand_blocks.contains_key(&add_ri))
                    && let Some(p) = profile
                {
                    let opens_delim = add.text().trim().ends_with('(')
                        || add.text().trim().ends_with('{')
                        || add.text().trim().ends_with('[')
                        || p.is_import_statement(&del_text)
                        || (ctx.expand_blocks.contains_key(&add_ri)
                            && (del_text.contains('{') || del_text.contains('(')));

                    if opens_delim {
                        while k < h.rows.len()
                            && matches!(h.rows[k], Row::Add(_))
                            && !ctx.pair_of.contains_key(&k)
                        {
                            let nxt_t = h.rows[k].text().trim();
                            if p.is_decl_break(nxt_t) {
                                break;
                            }
                            let is_closing = p.is_expression_close(nxt_t)
                                || p.is_block_close(nxt_t)
                                || del_text.trim_end().ends_with(nxt_t);
                            let is_param_or_item = nxt_t.ends_with(',')
                                || nxt_t.ends_with('(')
                                || nxt_t.ends_with('{')
                                || nxt_t.ends_with('[')
                                || p.is_comment_line(nxt_t);
                            let is_continuation = p.is_continuation_line(nxt_t);

                            if is_closing {
                                next_adds.push(k);
                                break;
                            } else if is_param_or_item || is_continuation {
                                next_adds.push(k);
                                k += 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
            next_adds.sort_unstable();
            next_adds.dedup();

            // The mod row always presents its own word-level truth: strike
            // what left this line, fill what replaced it — even when the
            // struck content reflows into the continuation adds below
            // (where the donor distribution shows where it went).  The
            // source strike is what connects the two; suppressing it made
            // wrapped tails silently vanish from the diff.
            if !next_adds.is_empty() {
                for &nxt_i in &next_adds {
                    let add_nxt = &h.rows[nxt_i];
                    let mut donor_spans =
                        word_spans_with_profile(&del_text, add_nxt.text(), profile);
                    let tok_count = tokenize_line(add_nxt.text())
                        .filter(|t| !t.trim().is_empty())
                        .count();
                    let nxt_trim = add_nxt.text().trim();
                    let is_closing_row = profile.is_some_and(|p| {
                        p.is_expression_close(nxt_trim) || p.is_block_close(nxt_trim)
                    })
                        || crate::syntax::profile::CloseDelimiter::default_classify_exact(nxt_trim)
                            != crate::syntax::profile::CloseDelimiter::None;
                    let is_opening_row = profile.is_some_and(|p| p.is_expression_open(nxt_trim))
                        || nxt_trim == "{"
                        || nxt_trim == "("
                        || nxt_trim == "[";

                    let has_common_tokens = tok_count > 0 && donor_spans.add.len() < tok_count;
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
                    } else if has_common_tokens || is_closing_row {
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

                        // Companion addition rows never carry deletions from
                        // the donor line. The anchor
                        // mod row already displays the struck content.
                        donor_spans.del = Vec::new();

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

            let mod_row = Row::Mod {
                old_line: DelLine::new(first_old_num, del_text),
                new_line: AddLine::with_spans(add.new_line_num().unwrap(), add.text(), spans),
            };
            out.push(mod_row);
        } else if collapse_absorbed_dels.contains(&ri) {
            continue;
        } else {
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
    /// Context rows synthesized from verbatim-reflow del/add pairs: identical
    /// content that the collapse absorbs as unchanged rather than dropping,
    /// keeping the visible row stream contiguous.
    pub reflow_ctx: Vec<Row>,
}

use crate::parse::{CtxLine, LineNum};

/// Pass 3: Filter verbatim reflow dels and adds.
pub fn pass_filter_reflows(
    h: &Hunk,
    _out: &[Row],
    kept_dels: &[usize],
    pure_adds: &[usize],
    ctx: &PairingContext,
    profile: Option<&LangProfile>,
) -> ReflowFilter {
    // 1. Dels absorbed into a Mod row: multiline del run whose anchor del was
    // paired with an add row that incorporates this del's text.
    let mut absorbed_dels: FxHashSet<usize> = FxHashSet::default();
    for (&add_idx, &del_idx) in ctx.pair_of {
        if let Some(run) = ctx.run_of.get(&del_idx)
            && run.first() == Some(&del_idx)
        {
            let add_text = h.rows[add_idx].text();
            for &d in run.iter().skip(1) {
                let t = h.rows[d].text().trim();
                if !t.is_empty() && add_text.contains(t) {
                    absorbed_dels.insert(d);
                }
            }
        }
    }

    // 2. Candidate unabsorbed dels that could pair with verbatim standalone
    //    adds.
    let candidate_dels: Vec<usize> = kept_dels
        .iter()
        .copied()
        .filter(|di| !absorbed_dels.contains(di) && !h.rows[*di].is_moved())
        .collect();

    // Candidate pure adds that are not moved.
    let candidate_adds: Vec<usize> = pure_adds
        .iter()
        .copied()
        .filter(|&ai| !h.rows[ai].is_moved())
        .collect();

    // 3. Pair verbatim (identical text) standalone del/add lines into Ctx rows.
    let mut reflow_ctx: Vec<Row> = Vec::new();
    let mut used_dels: FxHashSet<usize> = FxHashSet::default();
    let mut used_adds: FxHashSet<usize> = FxHashSet::default();

    for &ai in &candidate_adds {
        let a = &h.rows[ai];
        let at = a.text().trim();
        if at.is_empty() {
            continue;
        }
        for &di in &candidate_dels {
            if used_dels.contains(&di) {
                continue;
            }
            if h.rows[di].text() == a.text() {
                used_dels.insert(di);
                used_adds.insert(ai);
                reflow_ctx.push(Row::Ctx(CtxLine::new(
                    h.rows[di].old_line_num().unwrap_or(LineNum::MIN),
                    a.new_line_num().unwrap_or(LineNum::MIN),
                    a.text().to_string(),
                )));
                break;
            } else if h.rows[di].text().trim() == at {
                used_dels.insert(di);
                used_adds.insert(ai);
                let spans =
                    crate::lcs::word_spans_with_profile(h.rows[di].text(), a.text(), profile);
                reflow_ctx.push(Row::Mod {
                    old_line: crate::parse::DelLine::new(
                        h.rows[di].old_line_num().unwrap_or(LineNum::MIN),
                        h.rows[di].text().to_string(),
                    ),
                    new_line: crate::parse::AddLine::with_spans(
                        a.new_line_num().unwrap_or(LineNum::MIN),
                        a.text(),
                        spans,
                    ),
                });
                break;
            }
        }
    }

    // 4. Meaningful dels: candidate dels that were NOT converted to Ctx, plus
    //    moved dels that were not absorbed.
    let meaningful: Vec<usize> = kept_dels
        .iter()
        .copied()
        .filter(|di| {
            if h.rows[*di].is_moved() {
                !absorbed_dels.contains(di)
            } else {
                !absorbed_dels.contains(di) && !used_dels.contains(di)
            }
        })
        .collect();

    // 5. Kept adds: pure_adds that survived (moved adds + adds not converted to
    //    Ctx).
    let kept_adds: Vec<usize> = pure_adds
        .iter()
        .copied()
        .filter(|ai| {
            let a = &h.rows[*ai];
            if a.is_moved() {
                if a.del_text().is_some_and(|dt| dt != a.text()) {
                    return true;
                }
                // A local same-position move (old line == new line) is an
                // invisible reshuffle; true moves across
                // different line numbers survive as Row::Mov.
                if a.old_line().is_some() && a.old_line() == a.new_line() {
                    return false;
                }
                return true;
            }
            !used_adds.contains(ai)
        })
        .collect();

    ReflowFilter {
        meaningful_dels: meaningful,
        kept_adds,
        reflow_ctx,
    }
}

/// Pass 4: Keyed anchor sorting into the final visual row stream.
pub fn pass_sort_ordered_walk(
    h: &Hunk,
    out: Vec<Row>,
    meaningful_dels: &[usize],
    kept_adds: &[usize],
    add_spans: &FxHashMap<usize, (Spans, String)>,
    reflow_ctx: Vec<Row>,
) -> Vec<Row> {
    let mut anchor_of: FxHashMap<usize, usize> = FxHashMap::default();
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
            Row::MovTgt(new_line) => {
                if let Some((spans, _)) = add_spans.get(&ai) {
                    Row::MovTgt(AddLine::with_spans(
                        new_line.num,
                        new_line.text.clone(),
                        spans.clone(),
                    ))
                } else {
                    Row::MovTgt(new_line.clone())
                }
            }
            _ => h.rows[ai].clone(),
        };
        let key = (h.rows[ai].new_line().unwrap_or(0), 1.0);
        keyed.push((a, key));
    }
    for ctx in reflow_ctx {
        let key = (ctx.new_line().unwrap_or(0), 1.0);
        keyed.push((ctx, key));
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

#[cfg(test)]
mod tests;
