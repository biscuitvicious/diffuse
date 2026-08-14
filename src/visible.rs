//! Context allocation and visible row construction.
//!
//! Port of TypeScript's `allocateContext`, `gapBetween`, `hiddenGap`,
//! `absorbLeadingContext`, `absorbClosingContext`, and `buildVisible`.

use crate::parse::{Hunk, Row, RowKind};

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
        .filter(|r| r.kind == RowKind::Add || r.kind == RowKind::Mod)
        .count()
}

pub fn hunk_change_bounds(h: &Hunk, side: Side) -> (usize, usize) {
    let mut min_line = usize::MAX;
    let mut max_line = 0;
    for r in &h.rows {
        if r.kind == RowKind::Ctx {
            continue;
        }
        let line = match side {
            Side::New => r.new_line.map(|x| x as usize),
            Side::Old => r.old_line.map(|x| x as usize),
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
            if p_new == new_side {
                let p_side = if p_new { Side::New } else { Side::Old };
                let (_, p_end) = hunk_change_bounds(p, p_side);
                let space = change_start.saturating_sub(p_end + 1);
                free_above = free_above.min(space);
            }
        }

        let mut free_below = total.saturating_sub(change_end);
        if i < hunks.len() - 1 {
            let n = &hunks[i + 1];
            let n_new = add_count(n) > 0;
            if n_new == new_side {
                let n_side = if n_new { Side::New } else { Side::Old };
                let (n_start, _) = hunk_change_bounds(n, n_side);
                let space = n_start.saturating_sub(change_end + 1);
                free_below = free_below.min(space);
            }
        }

        alloc.push(ContextAlloc {
            above: (h.above_ctx as usize).min(free_above),
            below: (h.below_ctx as usize).min(free_below),
        });
    }

    for i in 0..hunks.len().saturating_sub(1) {
        let a = &hunks[i];
        let b = &hunks[i + 1];
        if (add_count(a) > 0) == (add_count(b) > 0) {
            let gap = gap_between(hunks, i, new_line_count, old_line_count);
            alloc[i + 1].above = alloc[i + 1].above.min(gap.saturating_sub(alloc[i].below));
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
    let gap = gap_between(hunks, i, new_line_count, old_line_count);
    gap.saturating_sub(alloc[i].below + alloc[i + 1].above)
}

pub fn context_rows(side: Side, start: usize, count: usize, src: &[String]) -> Vec<Row> {
    let mut rows = Vec::with_capacity(count);
    for i in 0..count {
        let line_no = start + i;
        let text = src
            .get(line_no.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        let (new_line, old_line) = match side {
            Side::New => (Some(line_no as u32), None),
            Side::Old => (None, Some(line_no as u32)),
        };
        rows.push(Row {
            kind: RowKind::Ctx,
            text,
            old_line,
            new_line,
            del_text: None,
            spans: None,
            moved: false,
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
) -> Vec<Row> {
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
                rows.push(Row {
                    kind: RowKind::Sep,
                    text: format!("⋯ {hidden} line{hidden_s} hidden{collapsed}{unavailable}"),
                    old_line: None,
                    new_line: None,
                    del_text: None,
                    spans: None,
                    moved: false,
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
                r.new_line.map(|nl| nl as usize).or_else(|| {
                    r.old_line.map(|ol| {
                        ((ol as isize) + ((h.new_start as isize) - (h.old_start as isize))) as usize
                    })
                })
            } else {
                r.old_line.map(|ol| ol as usize)
            };
            if let Some(target) = target_line
                && target > cursor + 1
                && r.kind != RowKind::Del
            {
                for g in (cursor + 1)..target {
                    let text = src.get(g.saturating_sub(1)).cloned().unwrap_or_default();
                    rows.push(Row {
                        kind: RowKind::Ctx,
                        text,
                        new_line: if new_side { Some(g as u32) } else { None },
                        old_line: if !new_side { Some(g as u32) } else { None },
                        del_text: None,
                        spans: None,
                        moved: false,
                    });
                }
                cursor = target - 1;
            }
            if let Some(nl) = r.new_line {
                cursor = cursor.max(nl as usize);
            }
            rows.push(r.clone());
        }

        let below_rows = context_rows(side, change_end + 1, a.below, src);
        rows.extend(below_rows);
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Hunk, Row, RowKind};

    #[test]
    fn test_allocate_context_basic() {
        let hunk1 = Hunk {
            old_start: 1,
            old_count: 5,
            new_start: 1,
            new_count: 5,
            section: String::new(),
            rows: vec![Row {
                kind: RowKind::Add,
                text: "hello".into(),
                old_line: None,
                new_line: Some(1),
                del_text: None,
                spans: None,
                moved: false,
            }],
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
                Row::new(RowKind::Add, "line 63", Some(63), None),
                Row::new(RowKind::Mod, "line 76", Some(76), Some(66)),
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
        // Body: 14 rows (line 63, context 64..75, line 76) -> note context between 63 and 76 is synthesized
        // Below: 6 rows (lines 77..82)
        assert_eq!(vis.first().unwrap().new_line, Some(57));
        assert_eq!(vis.last().unwrap().new_line, Some(82));
    }
}
