//! One-for-one port of `review.ts`'s `collapseHunkRows` (PLAN.md §3),
//! refactored in Phase 6 into composable passes over a hunk's diff AST:
//!
//! 1. `pass_pairing`: Similarity and brace-anchor pairing (1-to-1).
//! 2. `pass_assemble_mods`: Assemble 1-to-1 `Mod` rows, multiline subword
//!    distribution, and reflow suppression.
//! 3. `pass_filter_reflows`: Filtering verbatim reflow dels and adds.
//! 4. `pass_sort_ordered_walk`: Keyed anchor sorting into the final visual row
//!    stream.

use std::{cmp::Ordering, sync::atomic::AtomicU64, time::Instant};

use rustc_hash::{FxHashMap, FxHashSet};

use diffuse_syntax::profile::LangProfile;

use crate::{
    lcs::{tokenize_line, word_spans_and_runs_with_profile, word_spans_with_profile},
    merge::RunKind,
    parse::{AddLine, DelLine, Hunk, Row, RowGroup, Spans},
    shift::pass_shift_hunk,
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

/// Relabel pairwise del runs covered by recorded moves as moved-out:
/// Expand anchors strike only genuinely deleted text. `runs` are
/// `(RunKind, String)` over the anchor old text; `moved` holds char-offset
/// ranges into that same text. Del runs intersecting `moved` split into
/// Del/MovedSrc pieces (same-kind adjacents merged); Ctx/Add runs pass
/// through untouched (Add runs consume new-side text, never old offsets).
/// Out-of-range moved spans clamp; empty ones are ignored.
pub(crate) fn relabel_moved_runs(
    runs: Vec<(RunKind, String)>,
    moved: &[(usize, usize)],
) -> Vec<(RunKind, String)> {
    if moved.is_empty() {
        return runs;
    }
    let push = |out: &mut Vec<(RunKind, String)>, kind: RunKind, text: String| {
        if text.is_empty() {
            return;
        }
        if let Some((last_kind, last_text)) = out.last_mut()
            && *last_kind == kind
        {
            last_text.push_str(&text);
            return;
        }
        out.push((kind, text));
    };
    let mut out = Vec::with_capacity(runs.len());
    let mut old_pos = 0usize;
    for (kind, text) in runs {
        let len = text.chars().count();
        if kind != RunKind::Del || len == 0 {
            if matches!(kind, RunKind::Del | RunKind::Ctx) {
                old_pos += len;
            }
            push(&mut out, kind, text);
            continue;
        }
        let chars: Vec<char> = text.chars().collect();
        let mut spans: Vec<(usize, usize)> = moved
            .iter()
            .filter_map(|&(s, e)| {
                let s = s.saturating_sub(old_pos);
                let e = e.saturating_sub(old_pos).min(len);
                (s < e && s < len).then_some((s.min(len), e))
            })
            .collect();
        spans.sort_unstable();
        let mut p = 0usize;
        for (s, e) in spans {
            let s = s.max(p);
            if s >= e {
                continue;
            }
            if s > p {
                push(&mut out, RunKind::Del, chars[p..s].iter().collect());
            }
            push(&mut out, RunKind::MovedSrc, chars[s..e].iter().collect());
            p = e;
        }
        if p < len {
            push(&mut out, RunKind::Del, chars[p..].iter().collect());
        }
        old_pos += len;
    }
    out
}

/// Products of the assembly pass.
pub struct AssembledMods {
    pub out: Vec<Row>,
    pub kept_dels: Vec<usize>,
    pub pure_adds: Vec<usize>,
    pub add_spans: FxHashMap<usize, (Spans, String)>,
    pub add_groups: FxHashMap<usize, RowGroup>,
}

/// Per paired del row: donor text, first del number, word spans, and the
/// profiled-tape merged runs handed from assemble to render.
type PrecomputedPair = (String, crate::parse::LineNum, Spans, Vec<(RunKind, String)>);

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
pub static T_COLLAPSE: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];

#[tracing::instrument(
    level = "debug",
    skip(h, old_content, new_content, profile),
    fields(old_start = h.old_start, new_start = h.new_start, row_count = h.rows.len())
)]

pub fn collapse_hunk_rows(
    h: &Hunk,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Vec<Row> {
    let t0 = Instant::now();
    let h_norm = pass_shift_hunk(h, profile);
    let h = &h_norm;
    let t1 = Instant::now();
    T_COLLAPSE[0].fetch_add(
        t1.duration_since(t0).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 1: DP cluster pairing engine
    let p1 = crate::cluster::pairing::dp_pairing(h, old_content, new_content, profile);
    let t2 = Instant::now();
    T_COLLAPSE[1].fetch_add(
        t2.duration_since(t1).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

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
    let t3 = Instant::now();
    T_COLLAPSE[2].fetch_add(
        t3.duration_since(t2).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 3: Filter verbatim reflow dels and adds
    let p3 = pass_filter_reflows(h, &p2.out, &p2.kept_dels, &p2.pure_adds, profile);
    let t4 = Instant::now();
    T_COLLAPSE[3].fetch_add(
        t4.duration_since(t3).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    // Pass 4: Keyed anchor sorting into the final visual row stream
    let res = pass_sort_ordered_walk(
        h,
        p2.out,
        &p3.meaningful_dels,
        &p3.kept_adds,
        &p2.add_spans,
        &p2.add_groups,
        p3.reflow_ctx,
    );
    let t5 = Instant::now();
    T_COLLAPSE[4].fetch_add(
        t5.duration_since(t4).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
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
/// Normalize char ranges: sorted, merged, empties dropped.
fn normalize_ranges(ranges: &mut Vec<(usize, usize)>) {
    ranges.retain(|(s, e)| s < e);
    ranges.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (s, e) in ranges.drain(..) {
        if let Some(last) = out.last_mut()
            && s <= last.1
        {
            last.1 = last.1.max(e);
        } else {
            out.push((s, e));
        }
    }
    *ranges = out;
}

/// First occurrence of `needle` chars in `hay` fully covered by `supply`
/// ranges (all char coordinates). Deterministic: earliest del position.
fn supplied_occurrence(
    hay: &[char],
    needle: &[char],
    supply: &[(usize, usize)],
) -> Option<(usize, usize)> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find_map(|s| {
        let e = s + needle.len();
        if hay[s..e] != *needle {
            return None;
        }
        // Covered: every position sits inside some supply range. Supply
        // is normalized, so a single linear scan suffices.
        let mut covered_until = s;
        for &(rs, re) in supply {
            if rs > covered_until {
                break;
            }
            if re > covered_until {
                covered_until = covered_until.max(re);
            }
            if covered_until >= e {
                return Some((s, e));
            }
        }
        None
    })
}

/// Subtract a covered range from normalized supply, splitting around it.
fn consume_supply(supply: &mut Vec<(usize, usize)>, s: usize, e: usize) {
    let mut out = Vec::with_capacity(supply.len() + 1);
    for &(rs, re) in supply.iter() {
        if re <= s || rs >= e {
            out.push((rs, re));
        } else {
            if rs < s {
                out.push((rs, s));
            }
            if re > e {
                out.push((e, re));
            }
        }
    }
    *supply = out;
}

/// One aux row awaiting global moved-in allocation: its donor spans plus
/// the whole-token claims (aux-char coordinates) competing for supply.
struct PendingAux {
    nxt_i: usize,
    donor: Spans,
    aux_chars: Vec<char>,
    claims: Vec<(usize, usize)>,
}

pub fn pass_assemble_mods(
    h: &Hunk,
    ctx: &PairingContext,
    profile: Option<&LangProfile>,
) -> AssembledMods {
    let mut out: Vec<Row> = Vec::new();
    let mut kept_dels: Vec<usize> = Vec::new();
    let mut pure_adds: Vec<usize> = Vec::new();
    let mut add_spans: FxHashMap<usize, (Spans, String)> = FxHashMap::default();
    let mut add_groups: FxHashMap<usize, RowGroup> = FxHashMap::default();

    let mut collapse_absorbed_dels: FxHashSet<usize> = FxHashSet::default();
    for dels in ctx.collapse_blocks.values() {
        for &d in dels {
            collapse_absorbed_dels.insert(d);
        }
    }

    use rayon::prelude::*;

    let mut precomputed_pairs: FxHashMap<usize, PrecomputedPair> = if ctx.paired_dels.len() > 32 {
        ctx.paired_dels
            .par_iter()
            .map(|&ri| {
                let add_ri = ctx.add_of[&ri];
                let add = &h.rows[add_ri];
                let (del_text, first_old_num) = if let Some(block_dels) =
                    ctx.collapse_blocks.get(&add_ri)
                {
                    let lines: Vec<&str> = block_dels.iter().map(|&di| h.rows[di].text()).collect();
                    let joined = join_collapse_lines(&lines, add.text());
                    let first_num = block_dels
                        .first()
                        .and_then(|&di| h.rows[di].old_line_num())
                        .unwrap_or_else(|| h.rows[ri].old_line_num().unwrap());
                    (joined, first_num)
                } else {
                    (
                        h.rows[ri].text().to_string(),
                        h.rows[ri].old_line_num().unwrap(),
                    )
                };
                let (spans, runs) =
                    word_spans_and_runs_with_profile(&del_text, add.text(), profile);
                (ri, (del_text, first_old_num, spans, runs))
            })
            .collect()
    } else {
        FxHashMap::default()
    };

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

            let (del_text, first_old_num, mut spans, mut runs) =
                if let Some(pre) = precomputed_pairs.remove(&ri) {
                    pre
                } else {
                    let (del_text, first_old_num) =
                        if let Some(block_dels) = ctx.collapse_blocks.get(&add_ri) {
                            let lines: Vec<&str> =
                                block_dels.iter().map(|&di| h.rows[di].text()).collect();
                            let joined = join_collapse_lines(&lines, add.text());
                            let first_num = block_dels
                                .first()
                                .and_then(|&di| h.rows[di].old_line_num())
                                .unwrap_or_else(|| r.old_line_num().unwrap());
                            (joined, first_num)
                        } else {
                            (r.text().to_string(), r.old_line_num().unwrap())
                        };

                    let (spans, runs) =
                        word_spans_and_runs_with_profile(&del_text, add.text(), profile);
                    (del_text, first_old_num, spans, runs)
                };

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
                    if del_text.is_ascii() {
                        let bytes = del_text.as_bytes();
                        let start = ds.min(bytes.len());
                        let end = de.min(bytes.len());
                        bytes[start..end].iter().any(|&b| b.is_ascii_alphanumeric())
                    } else {
                        del_text
                            .chars()
                            .skip(ds)
                            .take(de.saturating_sub(ds))
                            .any(char::is_alphanumeric)
                    }
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

            // The mod row presents its word-level truth over the anchor
            // pair: strike what left this line, fill what replaced it.
            // When the struck content reflows into the continuation adds
            // below, old ranges surviving anywhere in those aux rows are
            // marked moved-out instead of struck — the aux rows' context
            // shows where the text went, so the strike would claim a
            // deletion that never happened. This is marking, not
            // suppression: every character stays present in the runs
            // (concat == display still holds), only the kind changes, so
            // wrapped tails cannot silently vanish the way outright
            // suppression once made them do. Stored spans stay pairwise
            // (wholesale coverage semantics are unchanged); runs carry the
            // block-contextualized presentation. The moved set derives
            // from the same word alignment family as everything else
            // here (old text vs joined aux texts), so DP and heuristic
            // anchors render uniformly.
            // Moved-out set: old ranges surviving in the continuation aux
            // rows (complement of the joined-aux alignment's del spans).
            // Empty for 1-to-1 mods (no aux rows): their runs pass through
            // untouched with zero behavior change. Computed before the
            // aux loop: the moved-in supply below derives from it (moved
            // AND displayed-struck — genuinely relocated text).
            let moved = if next_adds.is_empty() {
                Vec::new()
            } else {
                let joined = next_adds
                    .iter()
                    .map(|&ai| h.rows[ai].text())
                    .collect::<Vec<_>>()
                    .join("\n");
                let aux_spans = word_spans_with_profile(&del_text, &joined, profile);
                let mut del = aux_spans.del;
                del.sort_unstable();
                let old_len = del_text.chars().count();
                let mut moved = Vec::new();
                let mut pos = 0usize;
                for (s, e) in del {
                    let s = s.min(old_len);
                    let e = e.min(old_len);
                    if s > pos {
                        moved.push((pos, s));
                    }
                    pos = pos.max(e);
                }
                if pos < old_len {
                    moved.push((pos, old_len));
                }
                moved
            };
            let mut anchor_group = None;
            if !next_adds.is_empty() {
                let anchor_num = add.new_line_num().unwrap().get();
                anchor_group = Some(RowGroup::Anchor);

                let del_chars: Vec<char> = del_text.chars().collect();
                let mut pending: Vec<PendingAux> = Vec::new();
                for &nxt_i in &next_adds {
                    add_groups.insert(nxt_i, RowGroup::Aux { anchor: anchor_num });
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
                        || diffuse_syntax::profile::CloseDelimiter::default_classify_exact(
                            nxt_trim,
                        ) != diffuse_syntax::profile::CloseDelimiter::None;
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

                        // Whole-token claims for global allocation below:
                        // tokens fully inside donor-common ranges compete
                        // for supply; sub-token fragments never claim
                        // (splitting `let` into scattered single letters
                        // would consume unrelated occurrences).
                        let aux_text = add_nxt.text();
                        let aux_chars: Vec<char> = aux_text.chars().collect();
                        let mut fill = donor_spans.add.clone();
                        normalize_ranges(&mut fill);
                        let mut common: Vec<(usize, usize)> = Vec::new();
                        let mut pos = 0usize;
                        for &(s, e) in &fill {
                            if s > pos {
                                common.push((pos, s));
                            }
                            pos = pos.max(e);
                        }
                        if pos < aux_chars.len() {
                            common.push((pos, aux_chars.len()));
                        }
                        let mut toks: Vec<(usize, usize)> = Vec::new();
                        let mut off = 0usize;
                        for tok in crate::lcs::tokenize_line(aux_text) {
                            let len = tok.chars().count();
                            toks.push((off, off + len));
                            off += len;
                        }
                        debug_assert_eq!(off, aux_chars.len());
                        let mut claims = Vec::new();
                        for (ts, te) in toks {
                            if common.iter().any(|&(cs, ce)| cs <= ts && te <= ce) {
                                claims.push((ts, te));
                            }
                        }
                        pending.push(PendingAux {
                            nxt_i,
                            donor: donor_spans,
                            aux_chars,
                            claims,
                        });
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

                // Verbatim-block demotion (profiled display only — the
                // demoted pairing survives to display through stored
                // spans, which unprofiled display recomputes away; without
                // a profile the anchor keeps its LCS pairing and aux rows
                // keep supply-based marking): an aux donor-common range
                // that is long, contentful, and contiguous in del reclaims
                // its occurrence. Masking that occurrence out of del forces
                // the anchor to re-derive without it (context twin turns
                // to fill, struck stays struck), and the aux range flips
                // fill to moved-in. Only MIXED blocks qualify (covering
                // both struck and context): purely calm blocks are shared
                // content, not relocations; purely struck blocks need no
                // demotion. This keeps verbatim relocations atomic instead
                // of splitting their delimiters across narratives.
                const VERBATIM_MIN: usize = 8;
                if profile.is_some() {
                    // Displayed-struck vs context del ranges (pre-relabel runs
                    // tile del_text through the del side).
                    let mut struck_runs: Vec<(usize, usize)> = Vec::new();
                    let mut ctx_runs: Vec<(usize, usize)> = Vec::new();
                    {
                        let mut off = 0usize;
                        for (kind, text) in &runs {
                            if kind.is_add_like() {
                                continue;
                            }
                            let len = text.chars().count();
                            if *kind == RunKind::Del {
                                struck_runs.push((off, off + len));
                            } else {
                                ctx_runs.push((off, off + len));
                            }
                            off += len;
                        }
                    }
                    let overlaps = |ranges: &[(usize, usize)], ds: usize, de: usize| {
                        ranges.iter().any(|&(s, e)| s < de && ds < e)
                    };
                    // Candidate blocks: (len, nxt_i, aux_s, aux_e, del_s, del_e).
                    let mut blocks: Vec<(usize, usize, usize, usize, usize, usize)> = Vec::new();
                    for &nxt_i in &next_adds {
                        let aux_text = h.rows[nxt_i].text();
                        let aux_chars: Vec<char> = aux_text.chars().collect();
                        let donor = word_spans_with_profile(&del_text, aux_text, profile);
                        let mut fill = donor.add.clone();
                        normalize_ranges(&mut fill);
                        let mut pos = 0usize;
                        let mut common: Vec<(usize, usize)> = Vec::new();
                        for &(a, b) in &fill {
                            if a > pos {
                                common.push((pos, a));
                            }
                            pos = pos.max(b);
                        }
                        if pos < aux_chars.len() {
                            common.push((pos, aux_chars.len()));
                        }
                        for (cs, ce) in common {
                            // Never demote leading whitespace (indents stay).
                            let s = aux_chars
                                .iter()
                                .enumerate()
                                .skip(cs)
                                .take_while(|(i, c)| *i < ce && c.is_whitespace())
                                .count()
                                + cs;
                            if ce.saturating_sub(s) < VERBATIM_MIN {
                                continue;
                            }
                            if !aux_chars[s..ce].iter().any(|c| c.is_alphanumeric()) {
                                continue;
                            }
                            let needle: Vec<char> = aux_chars[s..ce].to_vec();
                            // Contiguous verbatim occurrences, preferring the
                            // one overlapping struck ranges most (then first).
                            let mut best: Option<(usize, usize)> = None;
                            let mut best_hits: usize = 0;
                            let nlen = needle.len();
                            if nlen <= del_chars.len() {
                                for ds in 0..=del_chars.len() - nlen {
                                    let de = ds + nlen;
                                    if del_chars[ds..de] != needle[..] {
                                        continue;
                                    }
                                    let struck_hits: usize = struck_runs
                                        .iter()
                                        .map(|&(a, b)| {
                                            a.max(ds).min(de).saturating_sub(b.min(de).max(ds))
                                        })
                                        .sum();
                                    let better = match best {
                                        None => true,
                                        Some((bds, _)) => {
                                            struck_hits > best_hits
                                                || (struck_hits == best_hits && ds < bds)
                                        }
                                    };
                                    if better {
                                        best = Some((ds, de));
                                        best_hits = struck_hits;
                                    }
                                }
                            }
                            if let Some((ds, de)) = best {
                                let mixed =
                                    overlaps(&struck_runs, ds, de) && overlaps(&ctx_runs, ds, de);
                                if mixed {
                                    blocks.push((ce - s, nxt_i, s, ce, ds, de));
                                }
                            }
                        }
                    }
                    // Longest first; del occurrences must not overlap.
                    blocks.sort_by(|a, b| b.0.cmp(&a.0));
                    let mut used: Vec<(usize, usize)> = Vec::new();
                    let mut flips: Vec<(usize, usize, usize)> = Vec::new();
                    for (_, nxt_i, s, e, ds, de) in std::mem::take(&mut blocks) {
                        if used.iter().any(|&(a, b)| a < de && ds < b) {
                            continue;
                        }
                        used.push((ds, de));
                        flips.push((nxt_i, s, e));
                    }
                    if !flips.is_empty() {
                        const SENTINEL: char = '\u{E000}';
                        let mut masked = del_chars.clone();
                        for &(ds, de) in &used {
                            for c in &mut masked[ds..de] {
                                *c = SENTINEL;
                            }
                        }
                        let masked_str: String = masked.iter().collect();
                        let (spans2, runs2) = crate::lcs::word_spans_and_runs_with_profile(
                            &masked_str,
                            add.text(),
                            profile,
                        );
                        // Apply demotion only when the display path can honor
                        // the recomputed spans (same well-formedness gate it
                        // uses): otherwise the wire runs and the display would
                        // disagree and the anchor keeps its LCS pairing.
                        if profile.is_some()
                            && crate::tape::AlignmentTape::chunks_from_spans(
                                &del_text,
                                add.text(),
                                &spans2,
                            )
                            .is_some()
                        {
                            // Restore original chars in del-side runs (1:1 offsets).
                            let mut runs2 = runs2;
                            let mut off = 0usize;
                            for (kind, text) in &mut runs2 {
                                if kind.is_add_like() {
                                    continue;
                                }
                                let mut chars: Vec<char> = text.chars().collect();
                                for c in &mut chars {
                                    if *c == SENTINEL {
                                        *c = del_chars[off];
                                    }
                                    off += 1;
                                }
                                *text = chars.into_iter().collect();
                            }
                            spans = spans2;
                            runs = runs2;
                        }
                    }
                } // end profile-gated verbatim demotion
                // Moved-in supply from the FINAL runs (post-demotion if
                // accepted, else the LCS runs): displayed-struck del ranges
                // that also survive in aux, consumed once each below.
                // Demoted ranges join supply here, so their aux twins render
                // moved-in instead of fill — relocation display stays atomic.
                let mut supply: Vec<(usize, usize)> = Vec::new();
                let mut off = 0usize;
                for (kind, text) in &runs {
                    if kind.is_add_like() {
                        continue;
                    }
                    let len = text.chars().count();
                    if *kind == RunKind::Del {
                        let mut pos = off;
                        for &(ms, me) in &moved {
                            if ms >= off + len {
                                break;
                            }
                            let a = pos.max(ms);
                            let e = (off + len).min(me);
                            if a < e {
                                supply.push((a, e));
                            }
                            pos = pos.max(me);
                        }
                    }
                    off += len;
                }
                normalize_ranges(&mut supply);
                // Global allocation, longest claim first (ties: row order,
                // then position — fully deterministic): each whole-token
                // claim consumes its occurrence from supply or renders as
                // fill. Longest-first maximizes total moved-in display, so
                // a verbatim row outranks a partial rival's fragments.
                let mut order: Vec<(usize, usize, usize, usize)> = Vec::new();
                for (i, p) in pending.iter().enumerate() {
                    for &(s, e) in &p.claims {
                        order.push((e - s, i, s, e));
                    }
                }
                order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
                let mut extra: FxHashMap<usize, Vec<(usize, usize)>> = FxHashMap::default();
                for (_, i, s, e) in order {
                    let p = &pending[i];
                    if let Some((ds, de)) =
                        supplied_occurrence(&del_chars, &p.aux_chars[s..e], &supply)
                    {
                        consume_supply(&mut supply, ds, de);
                    } else {
                        extra.entry(p.nxt_i).or_default().push((s, e));
                    }
                }
                for mut p in pending {
                    if let Some(mut xf) = extra.remove(&p.nxt_i) {
                        p.donor.add.append(&mut xf);
                        normalize_ranges(&mut p.donor.add);
                    }
                    add_spans.insert(p.nxt_i, (p.donor, del_text.clone()));
                }
            }

            let mut old_line = DelLine::new(first_old_num, del_text);
            old_line.group = anchor_group;
            let mut new_line = AddLine::with_spans(add.new_line_num().unwrap(), add.text(), spans)
                .with_cached_runs(relabel_moved_runs(runs, &moved));
            new_line.group = anchor_group;
            let mod_row = Row::Mod { old_line, new_line };
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
        add_groups,
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
    profile: Option<&LangProfile>,
) -> ReflowFilter {
    // Del rows render unless a Collapse block member (skipped in
    // assembly — member text joins the anchor's del_text) or a verbatim
    // reflow pair below claims them. (Containment-absorbing run members
    // into a 1-to-1 anchor would drop old lines silently: the anchor's
    // del_text never displays them. There is deliberately no absorption
    // step here.)
    // Candidate dels that could pair with verbatim standalone adds.
    let candidate_dels: Vec<usize> = kept_dels
        .iter()
        .copied()
        .filter(|di| !h.rows[*di].is_moved())
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

    // 4. Meaningful dels: candidate dels that were NOT converted to Ctx,
    // plus moved dels (relocations always render).
    let meaningful: Vec<usize> = kept_dels
        .iter()
        .copied()
        .filter(|di| h.rows[*di].is_moved() || !used_dels.contains(di))
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
    add_groups: &FxHashMap<usize, RowGroup>,
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
        let (key, anchor_group) = match anchor_of.get(&di) {
            None => ((0, d.old_line().unwrap_or(0) as f64), None),
            Some(&mi) => {
                let base = keyed[mi].1.0;
                let grp = if keyed[mi].0.group() == Some(RowGroup::Anchor) {
                    keyed[mi]
                        .0
                        .new_line()
                        .map(|nl| RowGroup::Aux { anchor: nl })
                } else {
                    None
                };
                ((base, 1.5), grp)
            }
        };
        let row = match d {
            Row::Del(del_line) => {
                let mut line = del_line.clone();
                if line.group.is_none()
                    && let Some(grp) = anchor_group
                {
                    line.group = Some(grp);
                }
                Row::Del(line)
            }
            other => other.clone(),
        };
        keyed.push((row, key));
    }
    for &ai in kept_adds {
        let a = match &h.rows[ai] {
            Row::Add(add_line) => {
                let mut line = if let Some((spans, del_text)) = add_spans.get(&ai) {
                    AddLine::with_donor(
                        add_line.num,
                        add_line.text.clone(),
                        del_text.clone(),
                        spans.clone(),
                    )
                } else {
                    add_line.clone()
                };
                if let Some(&grp) = add_groups.get(&ai) {
                    line.group = Some(grp);
                }
                Row::Add(line)
            }
            Row::MovTgt(new_line) => {
                let mut line = if let Some((spans, _)) = add_spans.get(&ai) {
                    AddLine::with_spans(new_line.num, new_line.text.clone(), spans.clone())
                } else {
                    new_line.clone()
                };
                if let Some(&grp) = add_groups.get(&ai) {
                    line.group = Some(grp);
                }
                Row::MovTgt(line)
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
