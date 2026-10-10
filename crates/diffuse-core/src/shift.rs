//! Diff identity transformations (PLAN.md §3, AGENTS.md §8).
//!
//! A hunk's coverage may shift DOWN or UP — absorbing context from one side
//! while releasing matching lines on the other. Because the released lines have
//! text strictly identical to the absorbed lines (`released.text ==
//! absorbed.text`), the transformation is a pure mathematical identity on the
//! reconstructed file.

use std::mem;

use diffuse_syntax::profile::LangProfile;

use crate::parse::{AddLine, CtxLine, DiffFile, Hunk, LineNum, Row};

/// Evaluates the semantic quality of a boundary between `prev_line` and
/// `next_line`.
///
/// Higher scores indicate natural code boundary splits (e.g. function starts,
/// doc comments, scope closing braces, blank lines).
pub fn semantic_boundary_score(
    prev_line: Option<&str>,
    next_line: Option<&str>,
    profile: Option<&LangProfile>,
) -> i32 {
    let mut score = 0;

    // Blank line boundary
    if prev_line.is_some_and(|l| l.trim().is_empty())
        || next_line.is_some_and(|l| l.trim().is_empty())
    {
        score += 15;
    }

    if let Some(p) = profile {
        // Function/type/doc declaration head
        if let Some(next) = next_line
            && (p.is_decl_break(next) || p.is_comment_line(next))
        {
            score += 25;
        }

        // Scope closing delimiter
        if let Some(prev) = prev_line
            && (p.is_block_close(prev) || p.is_expression_close(prev))
        {
            score += 20;
        }

        // Indented continuation penalty (mid-expression)
        if let (Some(prev), Some(next)) = (prev_line, next_line) {
            let pt = prev.trim();
            let nt = next.trim();
            if !pt.is_empty()
                && !nt.is_empty()
                && !pt.ends_with(';')
                && !pt.ends_with('}')
                && !pt.ends_with('{')
            {
                score -= 10;
            }
        }
    }

    score
}

/// Normalizes context lines to the target `budget` (e.g. standard $N=6$) and
/// merges adjacent hunks whose expanded context windows touch or overlap
/// (gap <= 2 * budget).
pub fn normalize_and_merge_hunks(file: &mut DiffFile, budget: usize) {
    if file.hunks.is_empty() {
        return;
    }
    normalize_file_context_budget(file, budget);
    merge_adjacent_hunks(file, budget);
    normalize_file_context_budget(file, budget);
}

/// Normalizes each hunk's *carried* context to the requested visual width:
/// trims the excess (visual < engine band), expands upward by synthesizing from
/// file content (visual > engine band), then merges hunks whose windows
/// overlap.
pub fn normalize_context(file: &mut DiffFile, visual: usize) {
    if file.hunks.is_empty() {
        return;
    }
    for h in &mut file.hunks {
        let (above, below) = count_carried_ctx(h);
        h.above_ctx = above;
        h.below_ctx = below;
    }
    trim_context(file, visual);
    normalize_file_context_budget(file, visual);
    merge_adjacent_hunks(file, visual);
}

/// Counts leading and trailing `Ctx` rows carried on a hunk (interior context
/// between change rows is neither).
fn count_carried_ctx(h: &Hunk) -> (u32, u32) {
    let mut above = 0u32;
    for r in &h.rows {
        if matches!(r, Row::Ctx(_)) {
            above += 1;
        } else {
            break;
        }
    }
    let mut below = 0u32;
    for r in h.rows.iter().rev() {
        if matches!(r, Row::Ctx(_)) {
            below += 1;
        } else {
            break;
        }
    }
    (above, below)
}

/// Removes leading/trailing `Ctx` rows beyond the requested visual width.
fn trim_context(file: &mut DiffFile, visual: usize) {
    for h in &mut file.hunks {
        while (h.above_ctx as usize) > visual {
            let Some(pos) = h.rows.iter().position(|r| matches!(r, Row::Ctx(_))) else {
                break;
            };
            h.rows.remove(pos);
            h.above_ctx -= 1;
            h.old_start += 1;
            h.new_start += 1;
            h.old_count = h.old_count.saturating_sub(1);
            h.new_count = h.new_count.saturating_sub(1);
        }
        while (h.below_ctx as usize) > visual {
            let Some(pos) = h.rows.iter().rposition(|r| matches!(r, Row::Ctx(_))) else {
                break;
            };
            h.rows.remove(pos);
            h.below_ctx -= 1;
            h.old_count = h.old_count.saturating_sub(1);
            h.new_count = h.new_count.saturating_sub(1);
        }
    }
}

/// Merges adjacent hunks within a `DiffFile` if their context windows touch or
/// overlap (`next_old_start <= cur_old_end + 1`).
pub fn merge_adjacent_hunks(file: &mut DiffFile, _context_budget: usize) {
    merge_adjacent_hunks_list(&mut file.hunks);
}

/// Merges adjacent hunks in a hunk list if their context windows touch or
/// overlap (`next_old_start <= cur_old_end + 1`).
pub fn merge_adjacent_hunks_list(hunks: &mut Vec<Hunk>) {
    if hunks.len() <= 1 {
        return;
    }

    let mut merged: Vec<Hunk> = Vec::with_capacity(hunks.len());
    let mut cur = hunks[0].clone();

    for next in hunks.iter().skip(1) {
        let cur_old_end = cur.old_start + cur.old_count.saturating_sub(1);
        let next_old_start = next.old_start;

        if next_old_start <= cur_old_end + 1 {
            // Merge `next` into `cur`.
            if next_old_start <= cur_old_end {
                // Overlap: the expanded context windows touch. Drop `cur`'s
                // trailing rows that duplicate `next`'s leading rows.
                let mut keep = cur.rows.len();
                for (i, r) in cur.rows.iter().enumerate().rev() {
                    if r.old_line().is_some_and(|l| l >= next_old_start) {
                        keep = i;
                    } else {
                        break;
                    }
                }
                let removed = cur.rows.len() - keep;
                cur.rows.truncate(keep);
                cur.old_count = cur.old_count.saturating_sub(removed as u32);
                cur.new_count = cur.new_count.saturating_sub(removed as u32);
                cur.below_ctx = cur.below_ctx.saturating_sub(removed as u32);
            }

            // Append `next` rows
            cur.rows.extend(next.rows.iter().cloned());
            let new_old_end = next.old_start + next.old_count.saturating_sub(1);
            let new_new_end = next.new_start + next.new_count.saturating_sub(1);
            cur.old_count = new_old_end.saturating_sub(cur.old_start) + 1;
            cur.new_count = new_new_end.saturating_sub(cur.new_start) + 1;
            cur.below_ctx = next.below_ctx;
        } else {
            merged.push(cur);
            cur = next.clone();
        }
    }
    merged.push(cur);
    *hunks = merged;
}

/// Normalizes leading and trailing context lines for all hunks in a `DiffFile`
/// to the target `budget` (e.g. standard $N=6$).
pub fn normalize_file_context_budget(file: &mut DiffFile, budget: usize) {
    for h in &mut file.hunks {
        let new_side = h
            .rows
            .iter()
            .any(|r| matches!(r, Row::Add(_) | Row::MovTgt(_) | Row::Mod { .. }));

        // Expand leading context if needed
        if (h.above_ctx as usize) < budget && h.old_start > 1 && h.new_start > 1 {
            let needed = (budget - (h.above_ctx as usize))
                .min((h.old_start as usize) - 1)
                .min((h.new_start as usize) - 1);
            if needed > 0 {
                let mut leading = Vec::with_capacity(needed);
                for offset in (1..=needed).rev() {
                    let old_ln = h.old_start - (offset as u32);
                    let new_ln = h.new_start - (offset as u32);
                    let text = if new_side {
                        file.new_content
                            .get((new_ln as usize).saturating_sub(1))
                            .cloned()
                            .unwrap_or_default()
                    } else {
                        file.old_content
                            .get((old_ln as usize).saturating_sub(1))
                            .cloned()
                            .unwrap_or_default()
                    };
                    leading.push(Row::Ctx(CtxLine::new(
                        LineNum::new(old_ln).unwrap_or(LineNum::MIN),
                        LineNum::new(new_ln).unwrap_or(LineNum::MIN),
                        text,
                    )));
                }
                leading.extend(mem::take(&mut h.rows));
                h.rows = leading;
                h.old_start -= needed as u32;
                h.new_start -= needed as u32;
                h.old_count += needed as u32;
                h.new_count += needed as u32;
                h.above_ctx += needed as u32;
            }
        }

        // Expand trailing context if needed
        if (h.below_ctx as usize) < budget {
            let cur_old_end = h.old_start + h.old_count.saturating_sub(1);
            let cur_new_end = h.new_start + h.new_count.saturating_sub(1);
            let old_total = file.old_line_count;
            let new_total = file.new_line_count;
            let avail_old = old_total.saturating_sub(cur_old_end as usize);
            let avail_new = new_total.saturating_sub(cur_new_end as usize);
            let needed = (budget - (h.below_ctx as usize))
                .min(avail_old)
                .min(avail_new);
            if needed > 0 {
                for offset in 1..=needed {
                    let old_ln = cur_old_end + (offset as u32);
                    let new_ln = cur_new_end + (offset as u32);
                    let text = if new_side {
                        file.new_content
                            .get((new_ln as usize).saturating_sub(1))
                            .cloned()
                            .unwrap_or_default()
                    } else {
                        file.old_content
                            .get((old_ln as usize).saturating_sub(1))
                            .cloned()
                            .unwrap_or_default()
                    };
                    h.rows.push(Row::Ctx(CtxLine::new(
                        LineNum::new(old_ln).unwrap_or(LineNum::MIN),
                        LineNum::new(new_ln).unwrap_or(LineNum::MIN),
                        text,
                    )));
                }
                h.old_count += needed as u32;
                h.new_count += needed as u32;
                h.below_ctx += needed as u32;
            }
        }
    }
}

/// Calculates the maximum number of rows that an addition block can shift down
/// as an exact content-neutral identity transformation: the released adds at
/// the top of the block must match the carried context rows immediately below
/// it.
pub fn max_shift_down_rows(hunk: &Hunk, split_idx: usize, end_idx: usize) -> usize {
    if split_idx >= end_idx || end_idx > hunk.rows.len() {
        return 0;
    }
    let available = end_idx - split_idx;
    let mut count = 0;
    for offset in 0..available {
        let absorbed_idx = end_idx + offset;
        if absorbed_idx >= hunk.rows.len() {
            break;
        }
        if !matches!(hunk.rows[absorbed_idx], Row::Ctx(_)) {
            break;
        }
        if hunk.rows[split_idx + offset].text() != hunk.rows[absorbed_idx].text() {
            break;
        }
        count += 1;
    }
    count
}

/// Determines the number of rows needed to shift down at `split_idx` to release
/// the closing delimiter, any trailing blank lines, and preceding absorbed
/// statements.
pub fn calculate_needed_shift_down(
    hunk: &Hunk,
    start_idx: usize,
    end_idx: usize,
    split_idx: usize,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) -> usize {
    let Some(p) = profile else {
        return 0;
    };
    if split_idx < start_idx || split_idx >= end_idx {
        return 0;
    }

    let text = hunk.rows[split_idx].text().trim();
    let is_closing_delim = p.classify_exact_close(text)
        != diffuse_syntax::profile::CloseDelimiter::None
        || p.is_expression_close(text)
        || p.is_block_close(text);
    if !is_closing_delim {
        return 0;
    }

    let add_count = end_idx - start_idx;
    let k = split_idx - start_idx;

    let is_boundary = |s: &str| {
        let t = s.trim();
        p.is_comment_line(t)
            || p.is_attribute_line(t)
            || p.is_expression_open(t)
            || p.is_block_open(t)
            || p.decl_prefixes.iter().any(|&pf| t.starts_with(pf))
            || p.scope_stop_prefixes.iter().any(|&pf| t.starts_with(pf))
            || p.import_prefixes.iter().any(|&pf| t.starts_with(pf))
    };

    // Check if what follows split_idx is a declaration / doc comment boundary
    if k + 1 < add_count && is_boundary(hunk.rows[split_idx + 1].text()) {
        1 // Closing delimiter only
    } else if k + 2 < add_count
        && hunk.rows[split_idx + 1].text().trim().is_empty()
        && is_boundary(hunk.rows[split_idx + 2].text())
    {
        2 // Closing delimiter + trailing blank line
    } else {
        0
    }
}

/// Applies a content-neutral downward shift identity transformation to an
/// addition block.
///
/// Reclassifies `count` released adds at the top of the block into context and
/// the `count` matching carried-context rows below the block into adds — a pure
/// classification swap, preserving every row's line numbers and the
/// reconstructed file contents.
pub fn apply_shift_down_identity(hunk: &mut Hunk, split_idx: usize, end_idx: usize, count: usize) {
    if count == 0 || split_idx >= end_idx || split_idx + count > end_idx {
        return;
    }
    if end_idx + count > hunk.rows.len() {
        return;
    }
    for offset in 0..count {
        if !matches!(hunk.rows[end_idx + offset], Row::Ctx(_))
            || hunk.rows[split_idx + offset].text() != hunk.rows[end_idx + offset].text()
        {
            return;
        }
    }

    let mut swaps = Vec::with_capacity(count * 2);
    for offset in 0..count {
        let released = &hunk.rows[split_idx + offset];
        let absorbed = &hunk.rows[end_idx + offset];
        let add_nl = released.new_line_num().unwrap_or(LineNum::MIN);
        let ctx_old = absorbed.old_line_num().unwrap_or(LineNum::MIN);
        let ctx_new = absorbed.new_line_num().unwrap_or(LineNum::MIN);
        swaps.push((
            split_idx + offset,
            Row::Ctx(CtxLine::new(ctx_old, add_nl, released.text().to_string())),
        ));
        swaps.push((
            end_idx + offset,
            Row::Add(AddLine::new(ctx_new, absorbed.text().to_string())),
        ));
    }
    for (idx, row) in swaps {
        hunk.rows[idx] = row;
    }
}

/// Content-neutral diff shift pass.
///
/// Scans addition blocks for interior delimiters/blank lines preceding
/// declarations, checks the available identity-shift capacity against the
/// carried context, and executes the shift.
pub fn pass_shift_hunk(h: &Hunk, profile: Option<&diffuse_syntax::profile::LangProfile>) -> Hunk {
    let mut new_hunk = h.clone();
    let mut i = 0;
    while i < new_hunk.rows.len() {
        if !matches!(new_hunk.rows[i], Row::Add(_)) {
            i += 1;
            continue;
        }
        let start = i;
        while i < new_hunk.rows.len() && matches!(new_hunk.rows[i], Row::Add(_)) {
            i += 1;
        }
        let end = i;
        let add_count = end - start;
        if add_count < 3 {
            continue;
        }

        // Look for an interior or leading closing delimiter inside the
        // additions that precedes a declaration
        for k in 0..add_count {
            let row_idx = start + k;
            let needed = calculate_needed_shift_down(&new_hunk, start, end, row_idx, profile);
            if needed > 0 {
                let possible = max_shift_down_rows(&new_hunk, row_idx, end);
                if possible >= needed {
                    apply_shift_down_identity(&mut new_hunk, row_idx, end, needed);
                    break;
                }
            }
        }
    }
    new_hunk
}

#[cfg(test)]
mod tests;
