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

use diffuse_algo::dyck::{DelimiterCategory, DelimiterClassifier, UniversalDelimiterClassifier};
use diffuse_syntax::profile::LangProfile;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    lcs::{word_spans_and_runs_with_profile, word_spans_with_profile},
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
            if matches!(kind, RunKind::Del | RunKind::Ctx | RunKind::MovedSrc) {
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

struct SlicedMultilineRuns {
    sliced: Vec<Vec<(RunKind, String)>>,
    anchor_runs: Vec<(RunKind, String)>,
    aux_moved_ranges: Vec<(usize, usize)>,
}

fn slice_multiline_runs(
    full_runs: &[(RunKind, String)],
    line_texts: &[&str],
) -> SlicedMultilineRuns {
    let line_count = line_texts.len();
    let push = |runs: &mut Vec<(RunKind, String)>, kind: RunKind, text: String| {
        if text.is_empty() {
            return;
        }
        if let Some((last_kind, last_text)) = runs.last_mut()
            && *last_kind == kind
        {
            last_text.push_str(&text);
            return;
        }
        runs.push((kind, text));
    };

    let mut out: Vec<Vec<(RunKind, String)>> = vec![Vec::new(); line_count];
    let mut anchor_runs: Vec<(RunKind, String)> = Vec::new();
    let mut current_line = 0usize;
    let mut aux_moved_ranges: Vec<(usize, usize)> = Vec::new();
    let mut del_pos = 0usize;

    for (kind, text) in full_runs {
        let len = text.chars().count();
        if *kind == RunKind::Del {
            let start = del_pos;
            let end = del_pos + len;
            del_pos = end;

            let target_line = current_line;

            if target_line < line_count {
                push(&mut out[target_line], RunKind::Del, text.clone());
            }
            if target_line == 0 {
                push(&mut anchor_runs, RunKind::Del, text.clone());
            } else {
                push(&mut anchor_runs, RunKind::MovedSrc, text.clone());
                aux_moved_ranges.push((start, end));
            }
        } else {
            let mut remaining = text.as_str();
            let is_ctx = *kind == RunKind::Ctx;
            let mut chunk_start = del_pos;

            while let Some(nl_pos) = remaining.find('\n') {
                let before = &remaining[..nl_pos];
                let before_len = before.chars().count();
                if !before.is_empty() {
                    if current_line < line_count {
                        push(&mut out[current_line], *kind, before.to_string());
                    }
                    if current_line == 0 {
                        push(&mut anchor_runs, *kind, before.to_string());
                    } else if is_ctx {
                        push(&mut anchor_runs, RunKind::MovedSrc, before.to_string());
                    }
                }
                if is_ctx && before_len > 0 {
                    if current_line > 0 {
                        aux_moved_ranges.push((chunk_start, chunk_start + before_len));
                    }
                    chunk_start += before_len;
                }
                current_line = (current_line + 1).min(line_count.saturating_sub(1));
                remaining = &remaining[nl_pos + 1..];
            }
            let rem_len = remaining.chars().count();
            if !remaining.is_empty() {
                if current_line < line_count {
                    push(&mut out[current_line], *kind, remaining.to_string());
                }
                if current_line == 0 {
                    push(&mut anchor_runs, *kind, remaining.to_string());
                } else if is_ctx {
                    push(&mut anchor_runs, RunKind::MovedSrc, remaining.to_string());
                }
            }
            if is_ctx && rem_len > 0 {
                if current_line > 0 {
                    aux_moved_ranges.push((chunk_start, chunk_start + rem_len));
                }
                chunk_start += rem_len;
            }
            if is_ctx {
                del_pos = chunk_start;
            }
        }
    }

    normalize_ranges(&mut aux_moved_ranges);

    SlicedMultilineRuns {
        sliced: out,
        anchor_runs,
        aux_moved_ranges,
    }
}

/// Assemble display runs for an auxiliary reflow addition row:
/// Non-fill slices of `aux_chars` render as `MovedTgt` (claimed from the donor
/// line); novel additions (e.g. indentation) render as `Add`; any inter-chunk
/// or trailing deletions transferred from the donor line render as `Del`.
fn build_aux_runs(
    aux_chars: &[char],
    add_spans: &[(usize, usize)],
    attached_dels: &[(usize, String)],
) -> Vec<(RunKind, String)> {
    let mut runs: Vec<(RunKind, String)> = Vec::new();
    let push = |runs: &mut Vec<(RunKind, String)>, kind: RunKind, text: String| {
        if text.is_empty() {
            return;
        }
        if let Some((last_kind, last_text)) = runs.last_mut()
            && *last_kind == kind
        {
            last_text.push_str(&text);
            return;
        }
        runs.push((kind, text));
    };

    let mut dels_by_pos: FxHashMap<usize, Vec<&str>> = FxHashMap::default();
    for (pos, text) in attached_dels {
        dels_by_pos.entry(*pos).or_default().push(text.as_str());
    }

    let mut pos = 0usize;
    let aux_len = aux_chars.len();
    while pos < aux_len || !dels_by_pos.is_empty() {
        if let Some(dels) = dels_by_pos.remove(&pos) {
            for d in dels {
                push(&mut runs, RunKind::Del, d.to_string());
            }
        }
        if pos >= aux_len {
            for (_, dels) in dels_by_pos {
                for d in dels {
                    push(&mut runs, RunKind::Del, d.to_string());
                }
            }
            break;
        }
        if let Some(&(_as, ae)) = add_spans.iter().find(|&&(s, e)| s <= pos && pos < e) {
            let next_del = dels_by_pos
                .keys()
                .copied()
                .filter(|&k| k > pos && k < ae)
                .min();
            let next_stop = next_del.unwrap_or(ae);
            let chunk: String = aux_chars[pos..next_stop].iter().collect();
            push(&mut runs, RunKind::Add, chunk);
            pos = next_stop;
        } else {
            let next_add = add_spans.iter().map(|&(s, _)| s).filter(|&s| s > pos).min();
            let next_del = dels_by_pos.keys().copied().filter(|&k| k > pos).min();
            let mut next_stop = aux_len;
            if let Some(na) = next_add {
                next_stop = next_stop.min(na);
            }
            if let Some(nd) = next_del {
                next_stop = next_stop.min(nd);
            }
            let chunk: String = aux_chars[pos..next_stop].iter().collect();
            push(&mut runs, RunKind::MovedTgt, chunk);
            pos = next_stop;
        }
    }
    runs
}

/// Products of the assembly pass.
pub struct AssembledMods {
    pub out: Vec<Row>,
    pub kept_dels: Vec<usize>,
    pub pure_adds: Vec<usize>,
    pub add_spans: FxHashMap<usize, Spans>,
    pub add_groups: FxHashMap<usize, RowGroup>,
    pub add_runs: FxHashMap<usize, Vec<(RunKind, String)>>,
}

/// Per paired del row: donor text, first del number, word spans, and the
/// profiled-tape merged runs handed from assemble to render.
type PrecomputedPair = (String, u32, Spans, Vec<(RunKind, String)>);

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
    let res = pass_sort_ordered_walk(h, p2.out, p3, &p2.add_spans, &p2.add_groups, &p2.add_runs);
    let t5 = Instant::now();
    T_COLLAPSE[4].fetch_add(
        t5.duration_since(t4).as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );

    crate::ensure!(
        res.iter().all(|r| match r {
            Row::Mod {
                old_line, new_line, ..
            } => old_line.num > 0 && new_line.num > 0,
            Row::Ctx(c) => c.old_pos > 0 && c.new_pos > 0,
            Row::Add(a) => a.num > 0,
            Row::Del(d) => d.num > 0,
            Row::MovSrc { line, .. } => line.num > 0,
            Row::MovTgt { line, .. } => line.num > 0,
            Row::AuxDel { line, .. } => line.num > 0,
            Row::AuxAdd { line, .. } => line.num > 0,
            Row::Sep(_) => true,
        }),
        "All collapsed rows must have valid, non-zero line coordinates"
    );

    crate::invariant!(
        res.iter()
            .all(|r| !matches!(r, Row::Mod { .. }) || !crate::merge::is_wholesale_replacement(r)),
        "Collapsed rows must never contain wholesale replacement Mod rows"
    );

    crate::invariant!(
        assert_collapsed_reflow_invariants(&res, profile).is_ok(),
        "Collapsed rows must satisfy reflow group invariants: {:?}",
        assert_collapsed_reflow_invariants(&res, profile).err()
    );

    crate::invariant!(
        check_no_orphaned_continuation_dels(h, &res).is_ok(),
        "{}",
        check_no_orphaned_continuation_dels(h, &res)
            .err()
            .unwrap_or_default()
    );

    crate::invariant!(
        check_context_old_line_monotonicity(&res).is_ok(),
        "{}",
        check_context_old_line_monotonicity(&res)
            .err()
            .unwrap_or_default()
    );

    crate::invariant!(
        check_del_add_topological_order(h, &res).is_ok(),
        "{}",
        check_del_add_topological_order(h, &res)
            .err()
            .unwrap_or_default()
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
        // Word boundary check: needle must not be an interior subword of a larger
        // alphanumeric token in hay
        if needle
            .first()
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            && s > 0
            && (hay[s - 1].is_alphanumeric() || hay[s - 1] == '_')
        {
            return None;
        }
        if needle
            .last()
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
            && e < hay.len()
            && (hay[e].is_alphanumeric() || hay[e] == '_')
        {
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

#[inline]
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn has_opening_affinity(text: &str, profile: Option<&LangProfile>) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if let Some(p) = profile {
        if p.is_opening_delimiter(t) || p.is_expression_open(t) || p.is_block_open(t) {
            return true;
        }
        if let Some(first_g) = t.graphemes(true).next()
            && (p.is_opening_delimiter(first_g)
                || p.is_expression_open(first_g)
                || p.is_block_open(first_g))
        {
            return true;
        }
    } else if let Some(first_g) = t.graphemes(true).next()
        && matches!(
            UniversalDelimiterClassifier.classify(&first_g),
            Some(DelimiterCategory::Open(_))
        )
    {
        return true;
    }
    false
}

fn has_closing_affinity(text: &str, profile: Option<&LangProfile>) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    if let Some(p) = profile {
        if p.is_closing_delimiter(t)
            || p.is_statement_terminator(t)
            || p.is_expression_close(t)
            || p.is_block_close(t)
        {
            return true;
        }
        if let Some(last_g) = t.graphemes(true).next_back()
            && (p.is_closing_delimiter(last_g)
                || p.is_statement_terminator(last_g)
                || p.is_expression_close(last_g)
                || p.is_block_close(last_g))
        {
            return true;
        }
    } else if let Some(last_g) = t.graphemes(true).next_back()
        && matches!(
            UniversalDelimiterClassifier.classify(&last_g),
            Some(
                DelimiterCategory::Close(_)
                    | DelimiterCategory::LeadingCloser
                    | DelimiterCategory::Separator
            )
        )
    {
        return true;
    }
    false
}

pub fn pass_assemble_mods(
    h: &Hunk,
    ctx: &PairingContext,
    profile: Option<&LangProfile>,
) -> AssembledMods {
    let mut out: Vec<Row> = Vec::new();
    let mut kept_dels: Vec<usize> = Vec::new();
    let mut pure_adds: Vec<usize> = Vec::new();
    let mut add_spans: FxHashMap<usize, Spans> = FxHashMap::default();
    let mut add_groups: FxHashMap<usize, RowGroup> = FxHashMap::default();
    let mut add_runs: FxHashMap<usize, Vec<(RunKind, String)>> = FxHashMap::default();

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
        if matches!(r, Row::MovTgt { .. }) {
            out.push(r.clone());
        } else if matches!(r, Row::MovSrc { .. }) {
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
                        let mut depth = line_delim_delta(add.text());
                        let mut already_closed = false;
                        for &ai in &next_adds {
                            let nxt_t = h.rows[ai].text().trim();
                            let delta = line_delim_delta(nxt_t);
                            depth += delta;
                            let is_exact_del_close = del_text.trim_end().ends_with(nxt_t);
                            let is_closing = p.is_expression_close(nxt_t)
                                || p.is_block_close(nxt_t)
                                || is_exact_del_close;
                            if is_closing && depth <= 0 {
                                already_closed = true;
                                break;
                            }
                        }

                        if !already_closed {
                            while k < h.rows.len()
                                && matches!(h.rows[k], Row::Add(_))
                                && !ctx.pair_of.contains_key(&k)
                            {
                                let nxt_t = h.rows[k].text().trim();
                                if p.is_decl_break(nxt_t) || p.is_import_statement(nxt_t) {
                                    break;
                                }
                                let delta = line_delim_delta(nxt_t);
                                let is_exact_del_close = del_text.trim_end().ends_with(nxt_t);
                                let is_closing = p.is_expression_close(nxt_t)
                                    || p.is_block_close(nxt_t)
                                    || is_exact_del_close;
                                let is_param_or_item = nxt_t.ends_with(',')
                                    || nxt_t.ends_with('(')
                                    || nxt_t.ends_with('{')
                                    || nxt_t.ends_with('[')
                                    || p.is_comment_line(nxt_t);
                                let is_continuation = p.is_continuation_line(nxt_t);

                                depth += delta;

                                if is_closing && depth <= 0 {
                                    next_adds.push(k);
                                    break;
                                } else if is_param_or_item || is_continuation || depth > 0 {
                                    next_adds.push(k);
                                    k += 1;
                                } else {
                                    break;
                                }
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
            let is_import = profile.is_some_and(|p| p.is_import_statement(&del_text));
            if is_import
                && !next_adds.is_empty()
                && let (Some(add_brace), Some(del_brace)) =
                    (add.text().rfind('{'), del_text.find('{'))
            {
                let add_pfx = add.text()[..add_brace].trim_end();
                let del_pfx = del_text[..del_brace].trim_end();
                if add_pfx != del_pfx && del_pfx.starts_with(add_pfx) {
                    let add_char_idx = add.text()[..add_brace].chars().count();
                    spans.add.push((add_char_idx, add_char_idx + 1));
                    normalize_ranges(&mut spans.add);

                    let mut new_runs = Vec::new();
                    for (kind, text) in runs {
                        if kind == RunKind::Ctx && text == "{" {
                            new_runs.push((RunKind::Del, "{".to_string()));
                            new_runs.push((RunKind::Add, "{".to_string()));
                        } else if kind == RunKind::Ctx && text.ends_with('{') {
                            let pfx = &text[..text.len() - 1];
                            if !pfx.is_empty() {
                                new_runs.push((RunKind::Ctx, pfx.to_string()));
                            }
                            new_runs.push((RunKind::Del, "{".to_string()));
                            new_runs.push((RunKind::Add, "{".to_string()));
                        } else {
                            new_runs.push((kind, text));
                        }
                    }
                    runs = new_runs;
                }
            }

            let (moved, full_adds, full_runs) = if next_adds.is_empty() {
                (Vec::new(), None, Vec::new())
            } else {
                let anchor_opens_delim = {
                    let t = add.text().trim();
                    line_delim_delta(add.text()) > 0
                        && (del_text.contains(t) || del_text.trim_start().starts_with(t) || {
                            if let Some(last_delim) = t.rfind(['(', '{', '[']) {
                                let head = t[..=last_delim].trim_start();
                                let last_token = head
                                    .rsplit(|c: char| {
                                        c.is_whitespace() || matches!(c, '=' | ':' | '.' | ',')
                                    })
                                    .next()
                                    .unwrap_or("");
                                !last_token.is_empty() && del_text.contains(last_token)
                            } else {
                                false
                            }
                        })
                };
                let (joined, full_adds) = if anchor_opens_delim {
                    let mut full_adds = next_adds.clone();
                    full_adds.push(add_ri);
                    full_adds.sort_unstable();
                    let j = full_adds
                        .iter()
                        .map(|&ai| h.rows[ai].text())
                        .collect::<Vec<_>>()
                        .join("\n");
                    (j, Some(full_adds))
                } else {
                    let j = next_adds
                        .iter()
                        .map(|&ai| h.rows[ai].text())
                        .collect::<Vec<_>>()
                        .join("\n");
                    (j, None)
                };
                let (aux_spans, full_runs) =
                    word_spans_and_runs_with_profile(&del_text, &joined, profile);
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
                (moved, full_adds, full_runs)
            };
            let mut anchor_group = None;
            let actually_moved = if !next_adds.is_empty() {
                let anchor_num = add.new_line_num().unwrap();
                anchor_group = Some(RowGroup::Anchor);

                if let Some(full_adds) = full_adds {
                    let full_add_texts: Vec<&str> =
                        full_adds.iter().map(|&ai| h.rows[ai].text()).collect();
                    let SlicedMultilineRuns {
                        sliced,
                        anchor_runs,
                        aux_moved_ranges,
                    } = slice_multiline_runs(&full_runs, &full_add_texts);
                    for (k, &nxt_i) in full_adds.iter().enumerate().skip(1) {
                        add_groups.insert(nxt_i, RowGroup::Aux { anchor: anchor_num });
                        let mut line_runs = sliced[k].clone();
                        for (kind, _) in &mut line_runs {
                            if *kind == RunKind::Ctx {
                                *kind = RunKind::MovedTgt;
                            }
                        }
                        let mut add_ranges = Vec::new();
                        let mut pos = 0usize;
                        for (kind, text) in &line_runs {
                            let len = text.chars().count();
                            if *kind == RunKind::Add {
                                if len > 0 {
                                    add_ranges.push((pos, pos + len));
                                }
                                pos += len;
                            } else if *kind == RunKind::MovedTgt {
                                pos += len;
                            }
                        }
                        let donor = Spans {
                            del: Vec::new(),
                            add: add_ranges,
                        };
                        add_runs.insert(nxt_i, line_runs);
                        add_spans.insert(nxt_i, donor);
                    }
                    runs = anchor_runs;
                    aux_moved_ranges
                } else {
                    let del_chars: Vec<char> = del_text.chars().collect();
                    let mut pending: Vec<PendingAux> = Vec::new();
                    for &nxt_i in &next_adds {
                        add_groups.insert(nxt_i, RowGroup::Aux { anchor: anchor_num });
                        let add_nxt = &h.rows[nxt_i];
                        let mut donor_spans =
                            word_spans_with_profile(&del_text, add_nxt.text(), profile);
                        let nxt_trim = add_nxt.text().trim();
                        let is_closing_row = profile.is_some_and(|p| {
                            p.is_expression_close(nxt_trim)
                                || p.is_block_close(nxt_trim)
                                || p.is_closing_delimiter(nxt_trim)
                        }) || (!nxt_trim.is_empty()
                            && del_text.trim_end().ends_with(nxt_trim));
                        let is_opening_row =
                            profile.is_some_and(|p| p.is_opening_delimiter(nxt_trim));

                        let has_common_tokens = {
                            let aux_chars: Vec<char> = add_nxt.text().chars().collect();
                            let mut fill = donor_spans.add.clone();
                            normalize_ranges(&mut fill);
                            let mut pos = 0usize;
                            let mut has_content = false;
                            let is_glue = |c: char| {
                                c.is_whitespace()
                                    || matches!(
                                        c,
                                        '{' | '}'
                                            | '('
                                            | ')'
                                            | '['
                                            | ']'
                                            | ','
                                            | ';'
                                            | ':'
                                            | '.'
                                            | '-'
                                            | '>'
                                            | '='
                                            | '|'
                                            | '&'
                                    )
                            };
                            for &(s, e) in &fill {
                                if s > pos
                                    && aux_chars[pos..s.min(aux_chars.len())]
                                        .iter()
                                        .any(|&c| !is_glue(c))
                                {
                                    has_content = true;
                                    break;
                                }
                                pos = pos.max(e);
                            }
                            if !has_content
                                && pos < aux_chars.len()
                                && aux_chars[pos..].iter().any(|&c| !is_glue(c))
                            {
                                has_content = true;
                            }
                            has_content
                        };
                        if is_opening_row && !del_text.trim_end().ends_with(nxt_trim) {
                            let text_len = add_nxt.text().chars().count();
                            let spans = Spans {
                                del: Vec::new(),
                                add: vec![(0, text_len)],
                            };
                            add_spans.insert(nxt_i, spans);
                            add_runs
                                .insert(nxt_i, vec![(RunKind::Add, add_nxt.text().to_string())]);
                        } else if has_common_tokens || is_closing_row || is_opening_row {
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
                            let is_word_subword = |t1: &[char], t2: &[char]| {
                                t1.last().is_some_and(|c| c.is_alphanumeric() || *c == '_')
                                    && t2.first().is_some_and(|c| c.is_alphanumeric() || *c == '_')
                            };
                            let is_closing_punct = |t1: &[char], t2: &[char]| {
                                let is_close = |c: char| {
                                    c != ','
                                        && c != '，'
                                        && c != '、'
                                        && (matches!(c, '}' | ')' | ']' | ';')
                                            || diffuse_syntax::profile::is_statement_terminator_char(
                                                c,
                                            )
                                            || diffuse_syntax::profile::is_closing_delimiter_char(
                                                c,
                                            ))
                                };
                                t1.iter().all(|&c| is_close(c)) && t2.iter().all(|&c| is_close(c))
                            };
                            let is_continuation_phrase = |slice: &[char]| {
                                let s: String = slice.iter().collect();
                                if let Some(p) = profile {
                                    p.is_continuation_phrase(&s)
                                } else {
                                    diffuse_syntax::profile::default_is_continuation_phrase(&s)
                                }
                            };
                            let mut claims: Vec<(usize, usize)> = Vec::new();
                            for (ts, te) in toks {
                                if common.iter().any(|&(cs, ce)| cs <= ts && te <= ce) {
                                    if let Some(last) = claims.last_mut()
                                        && last.1 == ts
                                        && (is_word_subword(
                                            &aux_chars[last.0..last.1],
                                            &aux_chars[ts..te],
                                        ) || is_closing_punct(
                                            &aux_chars[last.0..last.1],
                                            &aux_chars[ts..te],
                                        ) || is_continuation_phrase(&aux_chars[last.0..te]))
                                        && is_contiguous_in_del(&del_chars, &aux_chars, last.0, te)
                                    {
                                        last.1 = te;
                                        continue;
                                    }
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
                            let spans = Spans {
                                del: Vec::new(),
                                add: vec![(0, text_len)],
                            };
                            add_spans.insert(nxt_i, spans);
                            add_runs
                                .insert(nxt_i, vec![(RunKind::Add, add_nxt.text().to_string())]);
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
                        let mut blocks: Vec<(usize, usize, usize, usize, usize, usize)> =
                            Vec::new();
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
                                let len = ce.saturating_sub(s);
                                let min_len = if aux_chars[s..ce]
                                    .last()
                                    .is_some_and(|&c| matches!(c, '{' | '(' | '['))
                                {
                                    4
                                } else {
                                    VERBATIM_MIN
                                };
                                if len < min_len {
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
                                    let mixed = overlaps(&struck_runs, ds, de)
                                        && overlaps(&ctx_runs, ds, de);
                                    if mixed {
                                        blocks.push((ce - s, nxt_i, s, ce, ds, de));
                                    }
                                }
                            }
                        }
                        // Longest first; del occurrences must not overlap.
                        blocks.sort_by_key(|a| std::cmp::Reverse(a.0));
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
                    let anchor_has_add_open = profile.is_some_and(|p| {
                        runs.iter().any(|(kind, text)| {
                            kind.is_add_like()
                                && text.chars().any(|c| {
                                    p.is_trailing_delimiter(c) && matches!(c, '{' | '(' | '[')
                                })
                        })
                    }) || runs.iter().any(|(kind, text)| {
                        kind.is_add_like() && text.chars().any(|c| matches!(c, '{' | '(' | '['))
                    });
                    if anchor_has_add_open && let Some(last_p) = pending.last_mut() {
                        let trim = last_p.aux_chars.iter().collect::<String>();
                        let trim = trim.trim();
                        let is_close_line = profile
                            .is_some_and(|p| p.is_expression_close(trim) || p.is_block_close(trim))
                            || diffuse_syntax::profile::CloseDelimiter::default_classify_exact(
                                trim,
                            ) != diffuse_syntax::profile::CloseDelimiter::None;
                        if is_close_line && trim.ends_with(';') {
                            let mut new_claims = Vec::new();
                            for &(s, e) in &last_p.claims {
                                for idx in s..e {
                                    if last_p.aux_chars[idx] == ';' {
                                        new_claims.push((idx, idx + 1));
                                    } else {
                                        last_p.donor.add.push((idx, idx + 1));
                                    }
                                }
                            }
                            normalize_ranges(&mut last_p.donor.add);
                            last_p.claims = new_claims;
                        }
                    }

                    let mut order: Vec<(usize, usize, usize, usize)> = Vec::new();
                    for (i, p) in pending.iter().enumerate() {
                        for &(s, e) in &p.claims {
                            order.push((e - s, i, s, e));
                        }
                    }
                    order.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
                    let mut extra: FxHashMap<usize, Vec<(usize, usize)>> = FxHashMap::default();
                    let mut actually_consumed: Vec<(usize, usize)> = Vec::new();
                    type MatchedClaim = ((usize, usize), (usize, usize));
                    let mut aux_matched_claims: Vec<Vec<MatchedClaim>> =
                        vec![Vec::new(); pending.len()];
                    for (_, i, s, e) in order {
                        let p = &pending[i];
                        if let Some((ds, de)) =
                            supplied_occurrence(&del_chars, &p.aux_chars[s..e], &supply)
                        {
                            consume_supply(&mut supply, ds, de);
                            actually_consumed.push((ds, de));
                            aux_matched_claims[i].push(((s, e), (ds, de)));
                        } else {
                            extra.entry(p.nxt_i).or_default().push((s, e));
                        }
                    }

                    let mut struck_ranges: Vec<(usize, usize)> = Vec::new();
                    {
                        let mut off = 0usize;
                        for (kind, text) in &runs {
                            if kind.is_add_like() {
                                continue;
                            }
                            let len = text.chars().count();
                            if *kind == RunKind::Del {
                                struck_ranges.push((off, off + len));
                            }
                            off += len;
                        }
                    }

                    let mut consumed_sorted = actually_consumed.clone();
                    normalize_ranges(&mut consumed_sorted);

                    let mut unclaimed_residues: Vec<(usize, usize)> = Vec::new();
                    for &(mut bs, be) in &struck_ranges {
                        for &(ss, se) in &consumed_sorted {
                            if ss >= be || se <= bs {
                                continue;
                            }
                            if ss > bs {
                                unclaimed_residues.push((bs, ss));
                            }
                            bs = bs.max(se);
                            if bs >= be {
                                break;
                            }
                        }
                        if bs < be {
                            unclaimed_residues.push((bs, be));
                        }
                    }

                    for claims in &mut aux_matched_claims {
                        claims.sort_by_key(|&((aux_s, _), _)| aux_s);
                    }

                    let mut anchor_claims: Vec<(usize, usize)> = Vec::new();
                    {
                        let mut off = 0usize;
                        for (kind, text) in &runs {
                            if kind.is_add_like() {
                                continue;
                            }
                            let len = text.chars().count();
                            if *kind == RunKind::Ctx {
                                anchor_claims.push((off, off + len));
                            }
                            off += len;
                        }
                    }

                    let mut attached_dels: FxHashMap<usize, Vec<(usize, String)>> =
                        FxHashMap::default();
                    for &(rs, re) in &unclaimed_residues {
                        let residue_text: String = del_chars[rs..re].iter().collect();
                        let residue_chars = &del_chars[rs..re];

                        let has_open = has_opening_affinity(&residue_text, profile);
                        let has_close = has_closing_affinity(&residue_text, profile);
                        let is_closing_residue = has_close && !has_open;

                        #[derive(Clone, Copy)]
                        struct ClaimCandidate {
                            row_idx: usize,
                            insert_pos: usize,
                            score: usize,
                            del_coord: usize,
                        }

                        let mut best_left: Option<ClaimCandidate> = None;
                        let mut best_right: Option<ClaimCandidate> = None;

                        for (i, claims) in aux_matched_claims.iter().enumerate() {
                            let p = &pending[i];
                            let is_prefix = p.nxt_i < add_ri;

                            // Prefix / Suffix insulation:
                            // Prefix companion rows represent content strictly before the anchor,
                            // so they may never claim residues that occur at or after the anchor's
                            // retained context. Suffix companion rows
                            // represent content strictly after the anchor,
                            // so they may never claim residues that occur before or inside the
                            // anchor's retained context. Prefix row
                            // insulation: prefix companion rows represent content
                            // strictly before the anchor, so they may never claim residues that
                            // occur at or after the anchor's retained context.
                            if is_prefix
                                && anchor_claims
                                    .first()
                                    .is_some_and(|&(anchor_s, _)| rs >= anchor_s || re > anchor_s)
                            {
                                continue;
                            }

                            // Closing delimiter insulation: closing punctuation residues may only
                            // attach to an explicit closing row or a row at net
                            // zero container depth, never to intermediate items.
                            let nxt_trim = p.aux_chars.iter().collect::<String>();
                            let nxt_trim = nxt_trim.trim();
                            let is_closing_row = profile.is_some_and(|pr| {
                                pr.is_expression_close(nxt_trim)
                                    || pr.is_block_close(nxt_trim)
                                    || pr.is_closing_delimiter(nxt_trim)
                            }) || (!nxt_trim.is_empty()
                                && del_text.trim_end().ends_with(nxt_trim));

                            for &((aux_s, aux_e), (del_s, del_e)) in claims {
                                // Left Candidate: residue occurs after claim in del_text
                                if del_e <= rs && (!is_closing_residue || is_closing_row) {
                                    let anchor_intervenes = anchor_claims
                                        .iter()
                                        .any(|&(acs, ace)| acs < rs && ace > del_e);
                                    if !anchor_intervenes {
                                        let gap = rs - del_e;
                                        let flush_bonus = if gap == 0 { 200 } else { 0 };
                                        let proximity = 100usize.saturating_sub(gap);
                                        let fusion_bonus = if gap == 0
                                            && del_e > 0
                                            && is_word_char(del_chars[del_e - 1])
                                            && is_word_char(residue_chars[0])
                                        {
                                            1000
                                        } else {
                                            0
                                        };
                                        let delim_bonus =
                                            if has_close && !has_open { 500 } else { 0 };
                                        let score =
                                            flush_bonus + proximity + fusion_bonus + delim_bonus;

                                        let is_better = match best_left {
                                            None => true,
                                            Some(prev) => {
                                                score > prev.score
                                                    || (score == prev.score
                                                        && del_e > prev.del_coord)
                                            }
                                        };
                                        if is_better {
                                            best_left = Some(ClaimCandidate {
                                                row_idx: i,
                                                insert_pos: aux_e,
                                                score,
                                                del_coord: del_e,
                                            });
                                        }
                                    }
                                }

                                // Right Candidate: residue occurs before claim in del_text.
                                // A residue may only attach to the left of claim C (at C.aux_s)
                                // if it has positive structural affinity to C (opening delimiter
                                // or subword fusion).
                                if re <= del_s {
                                    let gap = del_s - re;
                                    let fusion_bonus = if gap == 0
                                        && del_s < del_chars.len()
                                        && is_word_char(residue_chars[residue_chars.len() - 1])
                                        && is_word_char(del_chars[del_s])
                                    {
                                        1000
                                    } else {
                                        0
                                    };
                                    let delim_bonus = if has_open && !has_close { 500 } else { 0 };

                                    if delim_bonus > 0 || fusion_bonus > 0 {
                                        let anchor_intervenes = anchor_claims
                                            .iter()
                                            .any(|&(acs, ace)| acs < del_s && ace > re);
                                        if !anchor_intervenes {
                                            let flush_bonus = if gap == 0 { 200 } else { 0 };
                                            let proximity = 100usize.saturating_sub(gap);
                                            let score = flush_bonus
                                                + proximity
                                                + fusion_bonus
                                                + delim_bonus;

                                            let is_better = match best_right {
                                                None => true,
                                                Some(prev) => {
                                                    score > prev.score
                                                        || (score == prev.score
                                                            && del_s < prev.del_coord)
                                                }
                                            };
                                            if is_better {
                                                best_right = Some(ClaimCandidate {
                                                    row_idx: i,
                                                    insert_pos: aux_s,
                                                    score,
                                                    del_coord: del_s,
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        let winner = match (best_left, best_right) {
                            (Some(left), Some(right)) => {
                                if right.score > left.score {
                                    Some((right.row_idx, right.insert_pos))
                                } else {
                                    let trailing_delim_idx = profile.and_then(|p| {
                                        pending[left.row_idx]
                                            .aux_chars
                                            .iter()
                                            .rposition(|&c| p.is_trailing_delimiter(c))
                                    });
                                    let insert_pos = if let Some(delim_pos) = trailing_delim_idx
                                        && left.insert_pos > delim_pos
                                        && !residue_text.trim().is_empty()
                                    {
                                        delim_pos
                                    } else {
                                        left.insert_pos
                                    };
                                    Some((left.row_idx, insert_pos))
                                }
                            }
                            (Some(left), None) => {
                                let trailing_delim_idx = profile.and_then(|p| {
                                    pending[left.row_idx]
                                        .aux_chars
                                        .iter()
                                        .rposition(|&c| p.is_trailing_delimiter(c))
                                });
                                let insert_pos = if let Some(delim_pos) = trailing_delim_idx
                                    && left.insert_pos > delim_pos
                                    && !residue_text.trim().is_empty()
                                {
                                    delim_pos
                                } else {
                                    left.insert_pos
                                };
                                Some((left.row_idx, insert_pos))
                            }
                            (None, Some(right)) => Some((right.row_idx, right.insert_pos)),
                            (None, None) => None,
                        };

                        if let Some((i, insert_pos)) = winner {
                            attached_dels
                                .entry(pending[i].nxt_i)
                                .or_default()
                                .push((insert_pos, residue_text));
                            actually_consumed.push((rs, re));
                        }
                    }

                    normalize_ranges(&mut actually_consumed);
                    for mut p in pending {
                        if let Some(mut xf) = extra.remove(&p.nxt_i) {
                            p.donor.add.append(&mut xf);
                            normalize_ranges(&mut p.donor.add);
                        }
                        let dels = attached_dels.remove(&p.nxt_i).unwrap_or_default();
                        let aux_runs = build_aux_runs(&p.aux_chars, &p.donor.add, &dels);
                        add_runs.insert(p.nxt_i, aux_runs);
                        add_spans.insert(p.nxt_i, p.donor);
                    }
                    actually_consumed
                }
            } else {
                moved
            };

            let old_line = DelLine::new(first_old_num, del_text);
            let new_line = AddLine::new(add.new_line_num().unwrap(), add.text());
            let mod_row = Row::Mod {
                old_line,
                new_line,
                spans,
                cached_runs: relabel_moved_runs(runs, &actually_moved),
                is_anchor: anchor_group == Some(RowGroup::Anchor),
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
        add_groups,
        add_runs,
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

use crate::parse::CtxLine;

/// Pass 3: Filter verbatim reflow dels and adds.
pub fn pass_filter_reflows(
    h: &Hunk,
    out: &[Row],
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

    // Precompute cluster identifier per row in `h.rows`. Rows separated by
    // `Row::Ctx` belong to different clusters; reflow pairs must never cross
    // context boundaries.
    let mut cluster_id = 0usize;
    let mut row_cluster: Vec<usize> = Vec::with_capacity(h.rows.len());
    for r in &h.rows {
        if matches!(r, Row::Ctx(_)) {
            cluster_id += 1;
            row_cluster.push(usize::MAX);
        } else {
            row_cluster.push(cluster_id);
        }
    }

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
            if row_cluster[ai] != row_cluster[di] || row_cluster[ai] == usize::MAX {
                continue;
            }
            let (Some(dold), Some(anew)) = (h.rows[di].old_line(), a.new_line()) else {
                continue;
            };
            if h.rows[di].text() == a.text() {
                // Synthesizing a Ctx row requires strict monotonicity against all
                // rows with old and new lines in `out` and prior `reflow_ctx`.
                let crosses_any = out.iter().chain(reflow_ctx.iter()).any(|m| {
                    if let (Some(m_old), Some(m_new)) = (m.old_line(), m.new_line()) {
                        (m_new < anew && m_old >= dold) || (m_new > anew && m_old <= dold)
                    } else {
                        false
                    }
                });
                if crosses_any {
                    continue;
                }
                used_dels.insert(di);
                used_adds.insert(ai);
                reflow_ctx.push(Row::Ctx(CtxLine::new(dold, anew, a.text().to_string())));
                break;
            } else if h.rows[di].text().trim() == at {
                // Synthesizing a Mod row cannot invert order relative to existing Ctx rows.
                let crosses_ctx = out.iter().chain(reflow_ctx.iter()).any(|m| {
                    if matches!(m, Row::Ctx(_))
                        && let (Some(c_old), Some(c_new)) = (m.old_line(), m.new_line())
                    {
                        (c_new < anew && c_old >= dold) || (c_new > anew && c_old <= dold)
                    } else {
                        false
                    }
                });
                if crosses_ctx {
                    continue;
                }
                used_dels.insert(di);
                used_adds.insert(ai);
                let (spans, runs) = crate::lcs::word_spans_and_runs_with_profile(
                    h.rows[di].text(),
                    a.text(),
                    profile,
                );
                reflow_ctx.push(Row::Mod {
                    old_line: crate::parse::DelLine::new(
                        h.rows[di].old_line().expect("old line must be present"),
                        h.rows[di].text().to_string(),
                    ),
                    new_line: crate::parse::AddLine::new(
                        a.new_line().expect("new line must be present"),
                        a.text(),
                    ),
                    spans,
                    cached_runs: runs,
                    is_anchor: false,
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
                return true;
            }
            !used_adds.contains(ai)
        })
        .collect();

    let result = ReflowFilter {
        meaningful_dels: meaningful,
        kept_adds,
        reflow_ctx,
    };

    crate::ensure!(
        check_reflow_cluster_confinement(h, &result).is_ok(),
        "{}",
        check_reflow_cluster_confinement(h, &result)
            .err()
            .unwrap_or_default()
    );

    result
}

/// Pass 4: Keyed anchor sorting into the final visual row stream.
pub fn pass_sort_ordered_walk(
    h: &Hunk,
    out: Vec<Row>,
    reflow: ReflowFilter,
    add_spans: &FxHashMap<usize, Spans>,
    add_groups: &FxHashMap<usize, RowGroup>,
    add_runs: &FxHashMap<usize, Vec<(RunKind, String)>>,
) -> Vec<Row> {
    let mut anchor_of: FxHashMap<usize, usize> = FxHashMap::default();
    for &di in &reflow.meaningful_dels {
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

    let mut anchor_max_aux: FxHashMap<u32, u32> = FxHashMap::default();
    for &ai in &reflow.kept_adds {
        if let Some(RowGroup::Aux { anchor }) = add_groups.get(&ai) {
            let nl = h.rows[ai].new_line().unwrap_or(0);
            anchor_max_aux
                .entry(*anchor)
                .and_modify(|m| *m = (*m).max(nl))
                .or_insert(nl);
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
    for &di in &reflow.meaningful_dels {
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
                let sort_base = if let Some(RowGroup::Aux { anchor }) = grp {
                    base.max(anchor_max_aux.get(&anchor).copied().unwrap_or(base))
                } else {
                    base
                };
                ((sort_base, 1.5), grp)
            }
        };
        let row = match d {
            Row::Del(del_line) => {
                if let Some(RowGroup::Aux { anchor }) = anchor_group {
                    Row::AuxDel {
                        anchor,
                        line: del_line.clone(),
                        spans: Spans::default(),
                        cached_runs: vec![(RunKind::Del, del_line.text.clone())],
                    }
                } else {
                    Row::Del(del_line.clone())
                }
            }
            other => other.clone(),
        };
        keyed.push((row, key));
    }
    for &ai in &reflow.kept_adds {
        let a = match &h.rows[ai] {
            Row::Add(add_line) => {
                if let Some(&RowGroup::Aux { anchor }) = add_groups.get(&ai) {
                    let spans = add_spans.get(&ai).cloned().unwrap_or_else(|| {
                        if add_line.text.is_empty() {
                            Spans {
                                del: Vec::new(),
                                add: vec![(0, 0)],
                            }
                        } else {
                            Spans::default()
                        }
                    });
                    let runs = add_runs
                        .get(&ai)
                        .cloned()
                        .unwrap_or_else(|| vec![(RunKind::Add, add_line.text.clone())]);
                    Row::AuxAdd {
                        anchor,
                        line: add_line.clone(),
                        spans,
                        cached_runs: runs,
                    }
                } else {
                    Row::Add(add_line.clone())
                }
            }
            _ => h.rows[ai].clone(),
        };
        let key = (h.rows[ai].new_line().unwrap_or(0), 1.0);
        keyed.push((a, key));
    }
    for ctx in reflow.reflow_ctx {
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

/// Calculate net delimiter delta for bracket pairs `{}`, `()`, `[]`, ignoring
/// delimiters within string literals.
pub(crate) fn line_delim_delta(text: &str) -> i32 {
    let mut delta = 0i32;
    let mut in_str = false;
    let mut str_char = '"';
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if in_str {
            if ch == str_char {
                in_str = false;
            }
            continue;
        }
        if ch == '"' || ch == '\'' || ch == '`' {
            in_str = true;
            str_char = ch;
            continue;
        }
        match ch {
            '{' | '(' | '[' => delta += 1,
            '}' | ')' | ']' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// Checks whether `aux_chars[s..e]` occurs contiguously anywhere in
/// `del_chars`.
fn is_contiguous_in_del(del_chars: &[char], aux_chars: &[char], s: usize, e: usize) -> bool {
    let needle = &aux_chars[s..e];
    if needle.len() > del_chars.len() {
        return false;
    }
    del_chars.windows(needle.len()).any(|w| w == needle)
}

/// Asserts reflow group invariants on collapsed rows:
/// 1. Old-side reconstruction: removing novel additions across the group in
///    visual order reproduces the surviving old-side text.
/// 2. Moved-text conservation: anchor MovedSrc == sum(aux MovedTgt).
/// 3. Aux line numbers strictly ascending.
/// 4. No orphan aux rows.
fn assert_collapsed_reflow_invariants(
    rows: &[Row],
    profile: Option<&LangProfile>,
) -> Result<(), String> {
    use crate::merge::merged_runs;

    let strip_ws = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();

    let mut anchors: FxHashMap<u32, usize> = FxHashMap::default();
    for (i, r) in rows.iter().enumerate() {
        if r.group() == Some(RowGroup::Anchor) {
            let anchor_num = r.new_line().or_else(|| r.old_line()).unwrap_or(0);
            anchors.insert(anchor_num, i);
        }
    }

    for (&anchor_num, &anchor_idx) in &anchors {
        let anchor_row = &rows[anchor_idx];
        if anchor_row.old_text().is_none() {
            continue;
        }

        let mut group_rows: Vec<&Row> = Vec::new();
        for r in rows {
            if r.group() == Some(RowGroup::Anchor) {
                let num = r.new_line().or_else(|| r.old_line()).unwrap_or(0);
                if num == anchor_num {
                    group_rows.push(r);
                }
            } else if let Some(RowGroup::Aux { anchor }) = r.group()
                && anchor == anchor_num
            {
                group_rows.push(r);
            }
        }

        let aux_adds: Vec<&&Row> = group_rows
            .iter()
            .filter(|r| matches!(r, Row::AuxAdd { .. }))
            .collect();
        let has_aux_dels = group_rows
            .iter()
            .any(|r| matches!(r, Row::AuxDel { .. } | Row::MovSrc { .. }));

        if !aux_adds.is_empty() && !has_aux_dels {
            let is_import = profile.is_some_and(|p| {
                anchor_row
                    .old_text()
                    .is_some_and(|dt| p.is_import_statement(dt))
            });

            let anchor_moved_src: String = merged_runs(anchor_row)
                .into_iter()
                .filter(|run| run.kind == RunKind::MovedSrc)
                .map(|run| run.text)
                .collect();
            let aux_non_add: String = aux_adds
                .iter()
                .flat_map(|aux| merged_runs(aux))
                .filter(|run| run.kind == RunKind::MovedTgt || run.kind == RunKind::Del)
                .map(|run| run.text)
                .collect();

            // 1. Old-side reconstruction invariant (mandatory for import statements)
            if is_import {
                let expected_old: String = merged_runs(anchor_row)
                    .into_iter()
                    .filter(|run| run.kind == RunKind::Ctx || run.kind == RunKind::MovedSrc)
                    .map(|run| run.text)
                    .collect();
                let mut reconstructed_old = String::new();
                for r in &group_rows {
                    if matches!(r, Row::Mod { .. }) {
                        reconstructed_old.extend(
                            merged_runs(r)
                                .into_iter()
                                .filter(|run| run.kind == RunKind::Ctx)
                                .map(|run| run.text),
                        );
                    } else if matches!(r, Row::AuxAdd { .. }) {
                        reconstructed_old.extend(
                            merged_runs(r)
                                .into_iter()
                                .filter(|run| {
                                    run.kind == RunKind::MovedTgt || run.kind == RunKind::Del
                                })
                                .map(|run| run.text),
                        );
                    }
                }
                if strip_ws(&reconstructed_old) != strip_ws(&expected_old) {
                    return Err(format!(
                        "Anchor line {anchor_num} old-text reconstruction mismatch:\n  expected: {expected_old:?}\n  got:      {reconstructed_old:?}"
                    ));
                }
                if strip_ws(&anchor_moved_src) != strip_ws(&aux_non_add) {
                    return Err(format!(
                        "Anchor line {anchor_num} moved-text conservation mismatch:\n  anchor MovedSrc: {anchor_moved_src:?}\n  aux non-add:     {aux_non_add:?}"
                    ));
                }
            } else {
                // 2. Character conservation invariant across all reflow groups
                let mut s1: Vec<char> = anchor_moved_src
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                let mut s2: Vec<char> =
                    aux_non_add.chars().filter(|c| !c.is_whitespace()).collect();
                s1.sort_unstable();
                s2.sort_unstable();
                if s1 != s2 {
                    return Err(format!(
                        "Anchor line {anchor_num} moved-text character conservation mismatch:\n  anchor MovedSrc chars: {s1:?}\n  aux non-add chars:     {s2:?}"
                    ));
                }
            }

            // 4. Suffix aux add line numbers strictly ascending
            let mut last_aux_line = None;
            for aux in group_rows
                .iter()
                .filter(|r| matches!(r, Row::AuxAdd { .. }))
            {
                if let Some(nl) = aux.new_line() {
                    if let Some(prev) = last_aux_line
                        && nl <= prev
                    {
                        return Err(format!("Aux row line {nl} <= previous aux line {prev}"));
                    }
                    last_aux_line = Some(nl);
                }
            }
        }
    }

    // 4. Reflow Group Atomicity: Anchor row and all its companion aux additions
    // (both prefix and suffix) must form an unbroken contiguous slice in visual row
    // order.
    for (&anchor_num, &anchor_idx) in &anchors {
        let mut group_indices: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                matches!(r.group(), Some(RowGroup::Aux { anchor }) if anchor == anchor_num)
                    && matches!(r, Row::AuxAdd { .. } | Row::MovTgt { .. })
            })
            .map(|(i, _)| i)
            .collect();

        if !group_indices.is_empty() {
            group_indices.push(anchor_idx);
            group_indices.sort_unstable();
            for window in group_indices.windows(2) {
                if window[1] != window[0] + 1 {
                    return Err(format!(
                        "Reflow atomicity violation: Reflow group for anchor line {anchor_num} is interrupted at row indices {} and {}",
                        window[0], window[1]
                    ));
                }
            }
        }
    }

    // Verify no orphan aux rows
    for (i, r) in rows.iter().enumerate() {
        if let Some(RowGroup::Aux { anchor }) = r.group()
            && !anchors.contains_key(&anchor)
        {
            return Err(format!(
                "Orphan aux row {i} (line {:?}) pointing to non-existent anchor {anchor}",
                r.new_line().or_else(|| r.old_line())
            ));
        }
    }
    Ok(())
}

/// Contract Invariant 1: Reflow Cluster Confinement.
///
/// Standalone del/add pairs synthesized into `reflow_ctx` must come from the
/// same change cluster in `h`: there must be no intervening `Row::Ctx` in
/// `h.rows` between the original deletion and addition.
pub fn check_reflow_cluster_confinement(h: &Hunk, reflow: &ReflowFilter) -> Result<(), String> {
    for r in &reflow.reflow_ctx {
        let (Some(d_num), Some(a_num)) = (r.old_line(), r.new_line()) else {
            continue;
        };
        let di = h.rows.iter().position(|row| row.old_line() == Some(d_num));
        let ai = h.rows.iter().position(|row| row.new_line() == Some(a_num));
        if let (Some(d_idx), Some(a_idx)) = (di, ai) {
            let (min_idx, max_idx) = (d_idx.min(a_idx), d_idx.max(a_idx));
            for i in (min_idx + 1)..max_idx {
                if matches!(h.rows[i], Row::Ctx(_)) {
                    return Err(format!(
                        "Reflow pair (old line {d_num}, new line {a_num}) crosses intervening context line at raw hunk row {i} (old {:?}, new {:?}): {:?}",
                        h.rows[i].old_line(),
                        h.rows[i].new_line(),
                        h.rows[i].text()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Contract Invariant 2: Syntactic Continuation Continuity.
///
/// A standalone or orphaned deletion sequence in the visual row stream must
/// not start with a syntactic continuation operator (e.g. `&&`, `||`, access
/// `.`, `,`, `)`, `}`) if its syntactic parent line in the old file (`d_old -
/// 1`) was also deleted in the diff.
pub fn check_no_orphaned_continuation_dels(h: &Hunk, res: &[Row]) -> Result<(), String> {
    for (i, r) in res.iter().enumerate() {
        let Row::Del(d) = r else {
            continue;
        };
        // Only inspect the head of a deletion run
        if i > 0 && matches!(res[i - 1], Row::Del(_) | Row::AuxDel { .. }) {
            continue;
        }
        let t = d.text.trim_start();
        let is_continuation = t.starts_with("&&")
            || t.starts_with("||")
            || (t.starts_with('.') && !t.starts_with(".."));
        if is_continuation {
            let prev_old = d.num.saturating_sub(1);
            // Walk backwards in res to see if a context line intervened between prev_old
            // and d.
            let mut ctx_intervened = false;
            for prev in res[..i].iter().rev() {
                if matches!(prev, Row::Ctx(_)) {
                    ctx_intervened = true;
                    break;
                }
                if let Some(ol) = prev.old_line()
                    && ol <= prev_old
                {
                    // Found a non-context row at or before prev_old in the same change cluster.
                    break;
                }
            }
            if ctx_intervened {
                let prev_was_del_in_hunk = h
                    .rows
                    .iter()
                    .any(|row| matches!(row, Row::Del(d0) if d0.num == prev_old));
                if prev_was_del_in_hunk {
                    return Err(format!(
                        "Orphaned continuation deletion at visual row {i} (old line {}): {:?} (syntactic parent old line {prev_old} was deleted in the hunk but separated across context lines from its continuation)",
                        d.num,
                        d.text.trim()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Contract Invariant 3: Context-Relative Old-Line Monotonicity.
///
/// Context lines represent immutable checkpoints present in both old and new
/// files. In the visual stream, for every context row C with old line C_old:
/// 1. Every row before C with an old line must satisfy old < C_old.
/// 2. Every row after C with an old line must satisfy old > C_old.
pub fn check_context_old_line_monotonicity(res: &[Row]) -> Result<(), String> {
    for (i, r) in res.iter().enumerate() {
        let Row::Ctx(c) = r else {
            continue;
        };
        let c_old = c.old_pos;
        for (j, prev) in res[..i].iter().enumerate() {
            if let Some(old) = prev.old_line()
                && old >= c_old
            {
                return Err(format!(
                    "Monotonicity violation: row at visual index {j} has old line {old} >= context line at visual index {i} with old line {c_old} (row: {:?}, context: {:?})",
                    prev.text().trim(),
                    c.text.trim()
                ));
            }
        }
        for (j, next) in res[i + 1..].iter().enumerate() {
            let actual_idx = i + 1 + j;
            if let Some(old) = next.old_line()
                && old <= c_old
            {
                return Err(format!(
                    "Monotonicity violation: row at visual index {actual_idx} has old line {old} <= context line at visual index {i} with old line {c_old} (row: {:?}, context: {:?})",
                    next.text().trim(),
                    c.text.trim()
                ));
            }
        }
    }
    Ok(())
}

/// Contract Invariant 4: Addition Block Structural Non-Penetration (Topological
/// Del/Add Invariant).
///
/// In a raw diff cluster where deletions precede additions, an unpaired
/// deletion from that cluster must not be placed after the additions of that
/// cluster (or inside compound addition statements after opening delimiters).
pub fn check_del_add_topological_order(h: &Hunk, res: &[Row]) -> Result<(), String> {
    let segments = crate::cluster::partition_hunk_segments(h, None, None, None);
    for seg in segments {
        let crate::cluster::HunkSegment::Cluster(cluster) = seg else {
            continue;
        };
        if cluster.old_rows.is_empty() || cluster.new_rows.is_empty() {
            continue;
        }
        let raw_del_indices: Vec<usize> = cluster
            .old_rows
            .iter()
            .filter_map(|r| r.old_line())
            .filter_map(|num| h.rows.iter().position(|r| r.old_line() == Some(num)))
            .collect();
        let raw_add_indices: Vec<usize> = cluster
            .new_rows
            .iter()
            .filter_map(|r| r.new_line())
            .filter_map(|num| h.rows.iter().position(|r| r.new_line() == Some(num)))
            .collect();

        if raw_del_indices.is_empty() || raw_add_indices.is_empty() {
            continue;
        }
        let max_raw_del = *raw_del_indices.iter().max().unwrap();
        let min_raw_add = *raw_add_indices.iter().min().unwrap();

        if max_raw_del < min_raw_add {
            let cluster_dels: FxHashSet<u32> =
                cluster.old_rows.iter().filter_map(Row::old_line).collect();
            let cluster_adds: FxHashSet<u32> =
                cluster.new_rows.iter().filter_map(Row::new_line).collect();

            // Find all Mod rows in res that belong to this cluster
            let cluster_mods: Vec<(u32, u32)> = res
                .iter()
                .filter_map(|r| match r {
                    Row::Mod {
                        old_line, new_line, ..
                    } if cluster_dels.contains(&old_line.num)
                        && cluster_adds.contains(&new_line.num) =>
                    {
                        Some((old_line.num, new_line.num))
                    }
                    _ => None,
                })
                .collect();

            for (vi, r) in res.iter().enumerate() {
                if let Row::Del(d) = r
                    && cluster_dels.contains(&d.num)
                {
                    // Find the anchor new line for deletion d: the new line of the
                    // latest Mod row in the cluster with old_line <= d.num.
                    let d_anchor_new = cluster_mods
                        .iter()
                        .filter(|&&(old_num, _)| old_num <= d.num)
                        .map(|&(_, new_num)| new_num)
                        .max()
                        .unwrap_or(0);

                    for (vj, prev) in res[..vi].iter().enumerate() {
                        if let Some(nl) = prev.new_line()
                            && cluster_adds.contains(&nl)
                            && matches!(prev, Row::Add(_))
                            && nl > d_anchor_new
                        {
                            return Err(format!(
                                "Topological del/add inversion: unpaired deletion at visual row {vi} (old line {}, anchor new {d_anchor_new}) appears after pure addition at visual row {vj} (new line {nl}): deletion {:?} after addition {:?}",
                                d.num,
                                d.text.trim(),
                                prev.text().trim()
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
