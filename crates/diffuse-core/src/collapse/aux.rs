//! Multiline run slicing, delimiter tracking, and auxiliary row synthesis
//! helpers.

use rustc_hash::FxHashMap;
use unicode_segmentation::UnicodeSegmentation;

use diffuse_algo::dyck::{DelimiterCategory, DelimiterClassifier, UniversalDelimiterClassifier};
use diffuse_syntax::profile::LangProfile;

use crate::{merge::RunKind, parse::Spans};

pub(crate) struct SlicedMultilineRuns {
    pub(crate) sliced: Vec<Vec<(RunKind, String)>>,
    pub(crate) anchor_runs: Vec<(RunKind, String)>,
    pub(crate) aux_moved_ranges: Vec<(usize, usize)>,
}

/// One aux row awaiting global moved-in allocation: its donor spans plus
/// the whole-token claims (aux-char coordinates) competing for supply.
pub(crate) struct PendingAux {
    pub(crate) nxt_i: usize,
    pub(crate) donor: Spans,
    pub(crate) aux_chars: Vec<char>,
    pub(crate) claims: Vec<(usize, usize)>,
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

pub(crate) fn slice_multiline_runs(
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

                current_line += 1;
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
pub(crate) fn build_aux_runs(
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
            let next_add = add_spans
                .iter()
                .map(|&(s, _)| s)
                .filter(|&s| s > pos)
                .min()
                .unwrap_or(aux_len);
            let next_del = dels_by_pos
                .keys()
                .copied()
                .filter(|&k| k > pos && k < next_add)
                .min();
            let next_stop = next_del.unwrap_or(next_add);
            let chunk: String = aux_chars[pos..next_stop].iter().collect();
            push(&mut runs, RunKind::MovedTgt, chunk);
            pos = next_stop;
        }
    }
    runs
}

pub(crate) fn default_join_needs_space(last: Option<char>, first: Option<char>) -> bool {
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

pub(crate) fn join_collapse_lines(lines: &[&str], new_text: &str) -> String {
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

/// Normalize char ranges: sorted, merged, empties dropped.
pub(crate) fn normalize_ranges(ranges: &mut Vec<(usize, usize)>) {
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

/// Extracts the character indices `(start, end)` and sequence of delimiter
/// characters of the trailing delimiter run in `text` (skipping trailing
/// whitespace). `start` is the index of the first delimiter in the run
/// (excluding any leading whitespace).
pub(crate) fn trailing_delimiters(
    text: &str,
    profile: Option<&LangProfile>,
) -> Option<(usize, usize, Vec<char>)> {
    let chars: Vec<char> = text.chars().collect();
    let last_idx = chars.iter().rposition(|c| !c.is_whitespace())?;
    let mut first_delim_idx = None;
    let mut delims = Vec::new();
    let mut i = last_idx;
    loop {
        let c = chars[i];
        if diffuse_syntax::profile::is_delimiter_char(c, profile) {
            first_delim_idx = Some(i);
            delims.push(c);
        } else if c.is_whitespace() {
            // Whitespace between delimiters is allowed.
        } else {
            break;
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    delims.reverse();
    let start = first_delim_idx?;
    Some((start, last_idx + 1, delims))
}

/// First occurrence of `needle` chars in `hay` fully covered by `supply`
/// ranges (all char coordinates). Deterministic: earliest del position.
pub(crate) fn supplied_occurrence(
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
pub(crate) fn consume_supply(supply: &mut Vec<(usize, usize)>, s: usize, e: usize) {
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

#[inline]
pub(crate) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Checks whether `aux_chars[s..e]` occurs contiguously anywhere in
/// `del_chars`.
pub(crate) fn is_contiguous_in_del(
    del_chars: &[char],
    aux_chars: &[char],
    s: usize,
    e: usize,
) -> bool {
    let needle = &aux_chars[s..e];
    if needle.len() > del_chars.len() {
        return false;
    }
    del_chars.windows(needle.len()).any(|w| w == needle)
}

pub(crate) fn has_opening_affinity(text: &str, profile: Option<&LangProfile>) -> bool {
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

pub(crate) fn has_closing_affinity(text: &str, profile: Option<&LangProfile>) -> bool {
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
