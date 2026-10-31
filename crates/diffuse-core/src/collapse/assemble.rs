//! Assembly pass for 1-to-1 Mod rows, multiline subword distribution, and
//! reflow suppression (Pass 2).

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use diffuse_syntax::profile::LangProfile;

use crate::{
    lcs::{word_spans_and_runs_with_profile, word_spans_with_profile},
    merge::RunKind,
    parse::{AddLine, DelLine, Hunk, Row, RowGroup, Spans},
};

use super::{
    PairingContext,
    aux::{
        PendingAux, SlicedMultilineRuns, build_aux_runs, consume_supply, has_closing_affinity,
        has_opening_affinity, is_contiguous_in_del, is_word_char, join_collapse_lines,
        line_delim_delta, normalize_ranges, relabel_moved_runs, slice_multiline_runs,
        supplied_occurrence, trailing_delimiters,
    },
};

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
                if let Some(block_dels) = ctx.collapse_blocks.get(&add_ri) {
                    for &di in block_dels {
                        kept_dels.push(di);
                    }
                } else {
                    kept_dels.push(ri);
                }
                if let Some(block_adds) = ctx.expand_blocks.get(&add_ri) {
                    for &ai in block_adds {
                        pure_adds.push(ai);
                    }
                } else {
                    pure_adds.push(add_ri);
                }
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
            let mut relocated_delimiter_adds = Vec::new();
            if next_adds.is_empty()
                && !ctx.collapse_blocks.contains_key(&add_ri)
                && let Some((_, _, del_delims)) = trailing_delimiters(&del_text, profile)
            {
                let add_delims = trailing_delimiters(add.text(), profile)
                    .map(|(_, _, d)| d)
                    .unwrap_or_default();
                let dropped_delims: &[char] = if del_delims.starts_with(&add_delims) {
                    &del_delims[add_delims.len()..]
                } else if del_delims.ends_with(&add_delims) {
                    &del_delims[..del_delims.len() - add_delims.len()]
                } else {
                    &del_delims[..]
                };
                if !dropped_delims.is_empty() {
                    let mut k = max_scanned + 1;
                    let mut rem_dropped = dropped_delims;
                    let mut candidate_adds = Vec::new();
                    while k < h.rows.len()
                        && matches!(h.rows[k], Row::Add(_))
                        && !ctx.pair_of.contains_key(&k)
                        && !rem_dropped.is_empty()
                    {
                        let nxt_trim = h.rows[k].text().trim();
                        if !diffuse_syntax::profile::is_delimiter_run(nxt_trim, profile) {
                            break;
                        }
                        let row_delims: Vec<char> = nxt_trim
                            .chars()
                            .filter(|&c| diffuse_syntax::profile::is_delimiter_char(c, profile))
                            .collect();
                        if rem_dropped.starts_with(&row_delims) {
                            rem_dropped = &rem_dropped[row_delims.len()..];
                            candidate_adds.push(k);
                            k += 1;
                        } else {
                            break;
                        }
                    }
                    if rem_dropped.is_empty() && !candidate_adds.is_empty() {
                        next_adds.extend(&candidate_adds);
                        relocated_delimiter_adds = candidate_adds;
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
                if !relocated_delimiter_adds.is_empty()
                    && let Some((delim_start, delim_end, _)) =
                        trailing_delimiters(&del_text, profile)
                    && !moved
                        .iter()
                        .any(|&(s, e)| s <= delim_start && delim_end <= e)
                {
                    moved.push((delim_start, delim_end));
                }
                moved.sort_unstable();
                normalize_ranges(&mut moved);
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
                        let is_relocated_delim =
                            diffuse_syntax::profile::is_delimiter_run(nxt_trim, profile)
                                && trailing_delimiters(&del_text, profile).is_some_and(
                                    |(_, _, del_delims)| {
                                        let add_delims = trailing_delimiters(add.text(), profile)
                                            .map(|(_, _, d)| d)
                                            .unwrap_or_default();
                                        let dropped_delims: &[char] =
                                            if del_delims.starts_with(&add_delims) {
                                                &del_delims[add_delims.len()..]
                                            } else if del_delims.ends_with(&add_delims) {
                                                &del_delims[..del_delims.len() - add_delims.len()]
                                            } else {
                                                &del_delims[..]
                                            };
                                        let row_delims: Vec<char> = nxt_trim
                                            .chars()
                                            .filter(|&c| {
                                                diffuse_syntax::profile::is_delimiter_char(
                                                    c, profile,
                                                )
                                            })
                                            .collect();
                                        !row_delims.is_empty()
                                            && (dropped_delims.starts_with(&row_delims)
                                                || dropped_delims.ends_with(&row_delims))
                                    },
                                );

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
                        if is_opening_row
                            && !del_text.trim_end().ends_with(nxt_trim)
                            && !is_relocated_delim
                        {
                            let text_len = add_nxt.text().chars().count();
                            let spans = Spans {
                                del: Vec::new(),
                                add: vec![(0, text_len)],
                            };
                            add_spans.insert(nxt_i, spans);
                            add_runs
                                .insert(nxt_i, vec![(RunKind::Add, add_nxt.text().to_string())]);
                        } else if has_common_tokens
                            || is_closing_row
                            || is_opening_row
                            || is_relocated_delim
                        {
                            let indent_len = add_nxt
                                .text()
                                .chars()
                                .take_while(|c| c.is_whitespace())
                                .count();
                            let text_len = add_nxt.text().chars().count();
                            if is_relocated_delim
                                || (((is_opening_row || is_closing_row)
                                    && del_text.trim_end().ends_with(nxt_trim))
                                    && donor_spans.add == vec![(0, text_len)])
                            {
                                donor_spans.add = if indent_len > 0 {
                                    vec![(0, indent_len)]
                                } else {
                                    Vec::new()
                                };
                            } else {
                                donor_spans.add = donor_spans
                                    .add
                                    .into_iter()
                                    .map(|(as_, ae)| {
                                        let start = as_;
                                        let mut end = ae;
                                        let chars: Vec<char> = add_nxt.text().chars().collect();
                                        if as_ <= indent_len
                                            && end < chars.len()
                                            && chars[end] == ' '
                                        {
                                            end += 1;
                                        }
                                        (start, end)
                                    })
                                    .filter(|(s, e)| s < e)
                                    .collect();
                            }

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

    kept_dels.sort_unstable();
    kept_dels.dedup();
    pure_adds.sort_unstable();
    pure_adds.dedup();

    AssembledMods {
        out,
        kept_dels,
        pure_adds,
        add_spans,
        add_groups,
        add_runs,
    }
}
