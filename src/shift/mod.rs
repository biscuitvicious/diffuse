//! Diff identity transformations (PLAN.md §3, AGENTS.md §8).
//!
//! A hunk's coverage may shift DOWN or UP — absorbing context from one side
//! while releasing matching lines on the other. Because the released lines have
//! text strictly identical to the absorbed lines (`released.text ==
//! absorbed.text`), the transformation is a pure mathematical identity on the
//! reconstructed file.

use crate::parse::{AddLine, CtxLine, DiffFile, Hunk, LineNum, Row};

/// Evaluates the semantic quality of a boundary between `prev_line` and
/// `next_line`.
///
/// Higher scores indicate natural code boundary splits (e.g. function starts,
/// doc comments, scope closing braces, blank lines).
pub fn semantic_boundary_score(
    prev_line: Option<&str>,
    next_line: Option<&str>,
    profile: Option<&crate::syntax::LangProfile>,
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

/// Merges adjacent hunks within a `DiffFile` if their line ranges overlap or if
/// the gap between them is smaller than `2 * context_budget`.
///
/// Synthesizes intermediate `Row::Ctx` rows from `old_content` and
/// `new_content`.
pub fn merge_adjacent_hunks(file: &mut DiffFile, context_budget: usize) {
    if file.hunks.len() <= 1 {
        return;
    }

    let mut merged: Vec<Hunk> = Vec::with_capacity(file.hunks.len());
    let mut cur = file.hunks[0].clone();

    for next in file.hunks.iter().skip(1) {
        let cur_old_end = cur.old_start + cur.old_count.saturating_sub(1);
        let next_old_start = next.old_start;

        if next_old_start <= cur_old_end + (2 * context_budget as u32) + 1 {
            // Merge `next` into `cur`
            // If there is a gap between `cur_old_end` and `next_old_start`, fill with Ctx
            // lines
            if next_old_start > cur_old_end + 1 {
                for line_idx in (cur_old_end + 1)..next_old_start {
                    let old_text = file
                        .old_content
                        .get((line_idx as usize).saturating_sub(1))
                        .cloned()
                        .unwrap_or_default();
                    let new_line_idx = (cur.new_start + cur.new_count.saturating_sub(1) + 1)
                        + (line_idx - cur_old_end - 1);
                    cur.rows.push(Row::Ctx(CtxLine::new(
                        LineNum::new(line_idx).unwrap_or(LineNum::MIN),
                        LineNum::new(new_line_idx).unwrap_or(LineNum::MIN),
                        old_text,
                    )));
                }
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
    file.hunks = merged;
}

/// Normalizes leading and trailing context lines for all hunks in a `DiffFile`
/// to the target `budget` (e.g. standard $N=6$).
pub fn normalize_file_context_budget(file: &mut DiffFile, budget: usize) {
    for h in &mut file.hunks {
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
                    let text = file
                        .old_content
                        .get((old_ln as usize).saturating_sub(1))
                        .cloned()
                        .unwrap_or_default();
                    leading.push(Row::Ctx(CtxLine::new(
                        LineNum::new(old_ln).unwrap_or(LineNum::MIN),
                        LineNum::new(new_ln).unwrap_or(LineNum::MIN),
                        text,
                    )));
                }
                leading.extend(std::mem::take(&mut h.rows));
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
                    let text = file
                        .old_content
                        .get((old_ln as usize).saturating_sub(1))
                        .cloned()
                        .unwrap_or_default();
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
/// as an exact content-neutral identity transformation against `new_content`.
pub fn max_shift_down_rows(
    hunk: &Hunk,
    split_idx: usize,
    end_idx: usize,
    new_content: Option<&[String]>,
) -> usize {
    let Some(content) = new_content else {
        return 0;
    };
    if split_idx >= end_idx || end_idx > hunk.rows.len() {
        return 0;
    }
    let last_add_nl = hunk.rows[end_idx - 1].new_line().unwrap_or(0);
    let target_start = (last_add_nl as usize).saturating_sub(1) + 1;

    let available = end_idx - split_idx;
    let mut count = 0;
    for offset in 0..available {
        let Some(candidate_text) = content.get(target_start + offset) else {
            break;
        };
        let hunk_text = hunk.rows[split_idx + offset].text();
        if candidate_text == hunk_text {
            count += 1;
        } else {
            break;
        }
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
    profile: Option<&crate::syntax::LangProfile>,
) -> usize {
    let Some(p) = profile else {
        return 0;
    };
    if split_idx < start_idx || split_idx >= end_idx {
        return 0;
    }

    let text = hunk.rows[split_idx].text().trim();
    if text != "}" && text != "};" && text != ")" {
        return 0;
    }

    let add_count = end_idx - start_idx;
    let k = split_idx - start_idx;

    let is_boundary = |s: &str| {
        let t = s.trim();
        p.is_comment_line(t)
            || t.starts_with("#[")
            || t.starts_with('@')
            || p.decl_prefixes.iter().any(|&pf| t.starts_with(pf))
            || p.scope_stop_prefixes.iter().any(|&pf| t.starts_with(pf))
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
/// Releases `count` lines starting at `split_idx` into context and absorbs
/// `count` matching lines from below the addition block into trailing added
/// rows.
///
/// Because `released_lines.text == absorbed_lines.text`, this transformation is
/// a pure identity operation on the reconstructed file contents.
pub fn apply_shift_down_identity(hunk: &mut Hunk, split_idx: usize, end_idx: usize, count: usize) {
    if count == 0 || split_idx >= end_idx || split_idx + count > end_idx {
        return;
    }
    let last_add_nl = hunk.rows[end_idx - 1].new_line().unwrap_or(0);
    let mut absorbed_adds = Vec::with_capacity(count);
    for offset in 0..count {
        let orig_text = hunk.rows[split_idx + offset].text().to_string();
        let new_nl = LineNum::new(last_add_nl + 1 + (offset as u32)).unwrap_or(LineNum::MIN);
        absorbed_adds.push(Row::Add(AddLine::new(new_nl, orig_text)));
    }

    // Release the top `count` lines from the addition block
    hunk.rows.drain(split_idx..split_idx + count);
    // Absorb the `count` lines into the bottom of the addition block
    for (idx, r) in absorbed_adds.into_iter().enumerate() {
        hunk.rows.insert(end_idx - count + idx, r);
    }
}

/// Content-neutral diff shift pass.
///
/// Scans addition blocks for interior delimiters/blank lines preceding
/// declarations, checks the available identity-shift capacity against
/// `new_content`, and executes the shift.
pub fn pass_shift_hunk(
    h: &Hunk,
    new_content: Option<&[String]>,
    profile: Option<&crate::syntax::LangProfile>,
) -> Hunk {
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

        // Look for an interior or leading closing delimiter inside the additions that
        // precedes a declaration
        for k in 0..add_count {
            let row_idx = start + k;
            let needed = calculate_needed_shift_down(&new_hunk, start, end, row_idx, profile);
            if needed > 0 {
                let possible = max_shift_down_rows(&new_hunk, row_idx, end, new_content);
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
