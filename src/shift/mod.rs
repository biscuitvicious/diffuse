//! Diff identity transformations (PLAN.md §3, AGENTS.md §8).
//!
//! A hunk's coverage may shift DOWN or UP — absorbing context from one side
//! while releasing matching lines on the other. Because the released lines have
//! text strictly identical to the absorbed lines (`released.text ==
//! absorbed.text`), the transformation is a pure mathematical identity on the
//! reconstructed file.

use crate::parse::{AddLine, Hunk, LineNum, Row};

/// Helper recognizing statement, declaration, or doc-comment heads.
pub fn is_decl_or_comment(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("///")
        || t.starts_with("//!")
        || t.starts_with("/*")
        || t.starts_with("#[")
        || t.starts_with("pub ")
        || t.starts_with("fn ")
        || t.starts_with("async fn ")
        || t.starts_with("impl ")
        || t.starts_with("struct ")
        || t.starts_with("enum ")
        || t.starts_with("trait ")
        || t.starts_with("mod ")
        || t.starts_with("type ")
        || t.starts_with("const ")
        || t.starts_with("static ")
        || t.starts_with("def ")
        || t.starts_with("class ")
}

/// Helper recognizing closing delimiters (`}`, `};`, `)`).
pub fn is_closing_delimiter(s: &str) -> bool {
    let t = s.trim();
    t == "}" || t == "};" || t == ")"
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
) -> usize {
    if split_idx <= start_idx || split_idx >= end_idx {
        return 0;
    }

    let text = hunk.rows[split_idx].text().trim();
    if !is_closing_delimiter(text) {
        return 0;
    }

    let add_count = end_idx - start_idx;
    let k = split_idx - start_idx;

    // Check if what follows split_idx is a declaration / doc comment boundary
    if k + 1 < add_count && is_decl_or_comment(hunk.rows[split_idx + 1].text()) {
        1 // Closing delimiter only
    } else if k + 2 < add_count
        && hunk.rows[split_idx + 1].text().trim().is_empty()
        && is_decl_or_comment(hunk.rows[split_idx + 2].text())
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
pub fn pass_shift_hunk(h: &Hunk, new_content: Option<&[String]>) -> Hunk {
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

        // Look for an interior closing delimiter inside the additions that precedes a
        // declaration
        for k in 1..add_count {
            let row_idx = start + k;
            let needed = calculate_needed_shift_down(&new_hunk, start, end, row_idx);
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
