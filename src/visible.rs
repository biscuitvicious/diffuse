//! Context allocation and visible row construction.
//!
//! Port of TypeScript's `allocateContext`, `gapBetween`, `hiddenGap`,
//! `absorbLeadingContext`, `absorbClosingContext`, and `buildVisible`.

use crate::parse::{CtxLine, Hunk, LineNum, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextAlloc {
    pub above: usize,
    pub below: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    New,
    Old,
}

fn add_count(h: &Hunk) -> usize {
    h.rows
        .iter()
        .filter(|r| matches!(r, Row::Add(_) | Row::Mod { .. } | Row::Mov { .. }))
        .count()
}

pub fn hunk_change_bounds(h: &Hunk, side: Side) -> (usize, usize) {
    let mut min_line = usize::MAX;
    let mut max_line = 0;
    for r in &h.rows {
        if matches!(r, Row::Ctx(_)) {
            continue;
        }
        let line = match side {
            Side::New => r.new_line().map(|x| x as usize),
            Side::Old => r.old_line().map(|x| x as usize),
        };
        if let Some(l) = line {
            min_line = min_line.min(l);
            max_line = max_line.max(l);
        }
    }
    if min_line == usize::MAX {
        let start = match side {
            Side::New => h.new_start as usize,
            Side::Old => h.old_start as usize,
        };
        (start, start)
    } else {
        (min_line, max_line)
    }
}

pub fn gap_between(
    hunks: &[Hunk],
    i: usize,
    new_line_count: usize,
    old_line_count: usize,
) -> usize {
    let a = &hunks[i];
    let b = &hunks[i + 1];
    let a_new = add_count(a) > 0;
    let b_new = add_count(b) > 0;

    if a_new == b_new {
        let a_side = if a_new { Side::New } else { Side::Old };
        let (_, a_end) = hunk_change_bounds(a, a_side);
        let b_side = if b_new { Side::New } else { Side::Old };
        let (b_start, _) = hunk_change_bounds(b, b_side);
        return b_start.saturating_sub(a_end + 1);
    }

    let (_, a_new_end) = hunk_change_bounds(a, Side::New);
    let (b_new_start, _) = hunk_change_bounds(b, Side::New);
    let new_gap = b_new_start.saturating_sub(a_new_end + 1);

    let (_, a_old_end) = hunk_change_bounds(a, Side::Old);
    let (b_old_start, _) = hunk_change_bounds(b, Side::Old);
    let old_gap = b_old_start.saturating_sub(a_old_end + 1);

    let new_c = if new_line_count > 0 {
        new_line_count
    } else {
        usize::MAX
    };
    let old_c = if old_line_count > 0 {
        old_line_count
    } else {
        usize::MAX
    };

    new_gap.min(new_c).max(old_gap.min(old_c))
}

pub fn allocate_context(
    hunks: &[Hunk],
    new_line_count: usize,
    old_line_count: usize,
) -> Vec<ContextAlloc> {
    let mut alloc: Vec<ContextAlloc> = Vec::with_capacity(hunks.len());

    for (i, h) in hunks.iter().enumerate() {
        let new_side = add_count(h) > 0;
        let side = if new_side { Side::New } else { Side::Old };
        let (change_start, change_end) = hunk_change_bounds(h, side);
        let total = if new_side {
            new_line_count
        } else {
            old_line_count
        };

        if total == 0 {
            alloc.push(ContextAlloc { above: 0, below: 0 });
            continue;
        }

        let mut free_above = change_start.saturating_sub(1);
        if i > 0 {
            let p = &hunks[i - 1];
            let p_new = add_count(p) > 0;
            if new_side == p_new {
                let (_, p_end) = hunk_change_bounds(p, side);
                free_above = change_start.saturating_sub(p_end + 1);
            }
        }

        let mut free_below = total.saturating_sub(change_end);
        if i + 1 < hunks.len() {
            let n = &hunks[i + 1];
            let n_new = add_count(n) > 0;
            if new_side == n_new {
                let (n_start, _) = hunk_change_bounds(n, side);
                free_below = n_start.saturating_sub(change_end + 1);
            }
        }

        let above = free_above.min(h.above_ctx as usize);
        let below = free_below.min(h.below_ctx as usize);
        alloc.push(ContextAlloc { above, below });
    }

    if hunks.len() > 1 {
        for i in 0..hunks.len() - 1 {
            let a = &hunks[i];
            let b = &hunks[i + 1];
            if (add_count(a) > 0) == (add_count(b) > 0) {
                let gap = gap_between(hunks, i, new_line_count, old_line_count);
                alloc[i + 1].above = alloc[i + 1].above.min(gap.saturating_sub(alloc[i].below));
            }
        }
    }

    alloc
}

pub fn hidden_gap(
    hunks: &[Hunk],
    i: usize,
    alloc: &[ContextAlloc],
    new_line_count: usize,
    old_line_count: usize,
) -> usize {
    let raw = gap_between(hunks, i, new_line_count, old_line_count);
    let consumed = alloc[i].below + alloc[i + 1].above;
    raw.saturating_sub(consumed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleRow {
    pub row: Row,
    pub side: Option<Side>,
}

fn context_rows(side: Side, start_line: usize, count: usize, src: &[String]) -> Vec<VisibleRow> {
    let mut rows = Vec::new();
    for i in 0..count {
        let line_no = start_line + i;
        let text = src
            .get(line_no.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        let num = LineNum::new(line_no as u32).unwrap_or(LineNum::MIN);
        let ctx_line = CtxLine::new(num, num, text);
        rows.push(VisibleRow {
            row: Row::Ctx(ctx_line),
            side: Some(side),
        });
    }
    rows
}

pub fn build_visible(
    hunks: &[Hunk],
    new_line_count: usize,
    old_line_count: usize,
    new_content: &[String],
    old_content: &[String],
    no_content: bool,
) -> Vec<VisibleRow> {
    let alloc = allocate_context(hunks, new_line_count, old_line_count);
    let mut rows = Vec::new();

    for (i, h) in hunks.iter().enumerate() {
        if i > 0 {
            let hidden = hidden_gap(hunks, i - 1, &alloc, new_line_count, old_line_count);
            if hidden > 0 {
                let collapsed = if alloc[i - 1].below == 0 && alloc[i].above == 0 {
                    " (e/E to expand, r to reset)"
                } else {
                    ""
                };
                let unavailable = if no_content {
                    " (content unavailable — expand in the editor)"
                } else {
                    ""
                };
                let hidden_s = if hidden == 1 { "" } else { "s" };
                rows.push(VisibleRow {
                    row: Row::Sep(format!(
                        "⋯ {hidden} line{hidden_s} hidden{collapsed}{unavailable}"
                    )),
                    side: None,
                });
            }
        }

        let a = alloc[i];
        let new_side = add_count(h) > 0;
        let side = if new_side { Side::New } else { Side::Old };
        let (change_start, change_end) = hunk_change_bounds(h, side);
        let src = if new_side { new_content } else { old_content };

        let lead_above = context_rows(side, change_start.saturating_sub(a.above), a.above, src);
        rows.extend(lead_above);

        let mut cursor = change_start.saturating_sub(1);
        for r in &h.rows {
            let target_line = if new_side {
                r.new_line().map(|nl| nl as usize)
            } else {
                r.old_line().map(|ol| ol as usize)
            };
            if let Some(target) = target_line
                && target > cursor + 1
            {
                for g in (cursor + 1)..target {
                    let text = src.get(g.saturating_sub(1)).cloned().unwrap_or_default();
                    let num = LineNum::new(g as u32).unwrap_or(LineNum::MIN);
                    let ctx_line = CtxLine::new(num, num, text);
                    rows.push(VisibleRow {
                        row: Row::Ctx(ctx_line),
                        side: Some(side),
                    });
                }
                cursor = target - 1;
            }
            if let Some(l) = match side {
                Side::New => r.new_line(),
                Side::Old => r.old_line(),
            } {
                cursor = cursor.max(l as usize);
            }
            rows.push(VisibleRow {
                row: r.clone(),
                side: if matches!(r, Row::Ctx(_)) {
                    Some(side)
                } else {
                    None
                },
            });
        }

        let below_rows = context_rows(side, change_end + 1, a.below, src);
        rows.extend(below_rows);
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{AddLine, DelLine};

    #[test]
    fn test_allocate_context_basic() {
        let hunk1 = Hunk {
            old_start: 1,
            old_count: 5,
            new_start: 1,
            new_count: 5,
            section: String::new(),
            rows: vec![Row::Add(AddLine::new(LineNum::new(1).unwrap(), "hello"))],
            above_ctx: 6,
            below_ctx: 6,
        };

        let hunks = vec![hunk1];
        let alloc = allocate_context(&hunks, 20, 20);
        assert_eq!(alloc.len(), 1);
        assert_eq!(alloc[0].above, 0);
        assert_eq!(alloc[0].below, 6);
    }

    #[test]
    fn test_change_bounds_context_allocation() {
        let hunk = Hunk {
            old_start: 60,
            old_count: 10,
            new_start: 60,
            new_count: 20,
            section: String::new(),
            rows: vec![
                Row::Add(AddLine::new(LineNum::new(63).unwrap(), "line 63")),
                Row::Mod {
                    old_line: DelLine::new(LineNum::new(66).unwrap(), "line 66"),
                    new_line: AddLine::new(LineNum::new(76).unwrap(), "line 76"),
                },
            ],
            above_ctx: 6,
            below_ctx: 6,
        };
        let (min_l, max_l) = hunk_change_bounds(&hunk, Side::New);
        assert_eq!(min_l, 63);
        assert_eq!(max_l, 76);

        let hunks = vec![hunk];
        let alloc = allocate_context(&hunks, 100, 100);
        assert_eq!(alloc[0].above, 6);
        assert_eq!(alloc[0].below, 6);

        let new_content: Vec<String> = (1..=100).map(|i| format!("line {}", i)).collect();
        let old_content: Vec<String> = (1..=100).map(|i| format!("line {}", i)).collect();
        let vis = build_visible(&hunks, 100, 100, &new_content, &old_content, false);
        // Above: 6 rows (lines 57..62)
        // Body: 14 rows (line 63, context 64..75, line 76) -> note context between 63
        // and 76 is synthesized Below: 6 rows (lines 77..82)
        assert_eq!(vis.first().unwrap().row.new_line(), Some(57));
        assert_eq!(vis.last().unwrap().row.new_line(), Some(82));
    }
}
