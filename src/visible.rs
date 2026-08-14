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

fn del_count(h: &Hunk) -> usize {
    h.rows
        .iter()
        .filter(|r| r.kind == RowKind::Del || r.kind == RowKind::Mod)
        .count()
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
        let a_end = (if a_new { a.new_start } else { a.old_start }) as usize
            + (if a_new { add_count(a) } else { del_count(a) })
            - 1;
        let b_start = (if b_new { b.new_start } else { b.old_start }) as usize;
        return b_start.saturating_sub(a_end + 1);
    }

    let a_new_end = (a.new_start as usize) + add_count(a);
    let new_gap = (b.new_start as usize).saturating_sub(a_new_end + 1);

    let a_old_end = (a.old_start as usize) + del_count(a);
    let old_gap = (b.old_start as usize).saturating_sub(a_old_end + 1);

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
        let start = (if new_side { h.new_start } else { h.old_start }) as usize;
        let count = if new_side { add_count(h) } else { del_count(h) };
        let total = if new_side {
            new_line_count
        } else {
            old_line_count
        };

        if total == 0 {
            alloc.push(ContextAlloc { above: 0, below: 0 });
            continue;
        }

        let mut free_above = start.saturating_sub(1);
        if i > 0 {
            let p = &hunks[i - 1];
            let p_new = add_count(p) > 0;
            if p_new == new_side {
                let p_start = if p_new { p.new_start } else { p.old_start } as usize;
                let p_count = if p_new { add_count(p) } else { del_count(p) };
                let p_end = p_start + p_count - 1;
                let space = start.saturating_sub(p_end + 1);
                free_above = free_above.min(space);
            }
        }

        let mut free_below = total.saturating_sub(start + count) + 1;
        if i < hunks.len() - 1 {
            let n = &hunks[i + 1];
            let n_new = add_count(n) > 0;
            if n_new == new_side {
                let n_start = (if n_new { n.new_start } else { n.old_start }) as usize;
                let space = n_start.saturating_sub(start + count);
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
            tail: Vec::new(),
        });
    }
    rows
}

fn comment_lead(r: &Row) -> bool {
    let t = r.text.trim();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') || t.starts_with('#')
}

pub fn absorb_leading_context(
    side: Side,
    start: usize,
    above: usize,
    h: &Hunk,
    src: &[String],
) -> (Vec<Row>, Vec<Row>) {
    let ctx = context_rows(side, start.saturating_sub(above), above, src);
    let rows = h.rows.clone();
    if ctx.is_empty() {
        return (ctx, rows);
    }
    let limit = 3.min(rows.len()).min(ctx.len());
    for k in (1..=limit).rev() {
        let released = &rows[rows.len() - k..];
        let absorbed = &ctx[ctx.len() - k..];
        if comment_lead(&absorbed[0])
            && released
                .iter()
                .zip(absorbed.iter())
                .all(|(r, a)| r.text == a.text)
        {
            let ab = absorbed
                .iter()
                .map(|r| Row {
                    kind: RowKind::Ctx,
                    text: r.text.clone(),
                    new_line: r.new_line,
                    old_line: None,
                    del_text: None,
                    spans: None,
                    moved: false,
                    tail: Vec::new(),
                })
                .collect();
            let ro = rows[..rows.len() - k].to_vec();
            return (ab, ro);
        }
    }
    (ctx, rows)
}

pub fn absorb_closing_context(
    side: Side,
    start: usize,
    count: usize,
    below: usize,
    h: &Hunk,
    src: &[String],
) -> Vec<Row> {
    let ctx = context_rows(side, start + count, below, src);
    if ctx.is_empty() {
        return ctx;
    }
    let Some(last_row) = h.rows.last() else {
        return ctx;
    };
    if last_row.kind != RowKind::Add {
        return ctx;
    }
    let released = h.rows.first().map(|r| &r.tail[..]).unwrap_or(&[]);
    if released.is_empty() || released.len() > ctx.len() {
        return ctx;
    }
    if released.iter().all(|r| r.text.trim().is_empty()) {
        return ctx;
    }
    for (i, r) in released.iter().enumerate() {
        if r.text != ctx[i].text {
            return ctx;
        }
    }
    let mut absorbed: Vec<Row> = ctx[..released.len()]
        .iter()
        .map(|r| Row {
            kind: RowKind::Add,
            text: r.text.clone(),
            new_line: r.new_line,
            old_line: None,
            del_text: None,
            spans: None,
            moved: false,
            tail: Vec::new(),
        })
        .collect();
    absorbed.extend_from_slice(&ctx[released.len()..]);
    absorbed
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
                    tail: Vec::new(),
                });
            }
        }

        let a = alloc[i];
        let new_side = add_count(h) > 0;
        let start = if new_side { h.new_start } else { h.old_start } as usize;
        let count = if new_side { add_count(h) } else { del_count(h) };
        let side = if new_side { Side::New } else { Side::Old };
        let (lead_above, lead_rows) = absorb_leading_context(
            side,
            start,
            a.above,
            h,
            if new_side { new_content } else { old_content },
        );
        rows.extend(lead_above);

        let src = if new_side { new_content } else { old_content };
        let mut cursor = start.saturating_sub(1);
        for r in lead_rows {
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
                        tail: Vec::new(),
                    });
                }
                cursor = target - 1;
            }
            if let Some(nl) = r.new_line {
                cursor = cursor.max(nl as usize);
            }
            rows.push(r);
        }

        let cov_end = start + count - 1;
        for g in (cursor + 1)..=cov_end {
            let text = src.get(g.saturating_sub(1)).cloned().unwrap_or_default();
            rows.push(Row {
                kind: RowKind::Ctx,
                text,
                new_line: if new_side { Some(g as u32) } else { None },
                old_line: if !new_side { Some(g as u32) } else { None },
                del_text: None,
                spans: None,
                moved: false,
                tail: Vec::new(),
            });
        }

        rows.extend(absorb_closing_context(
            side,
            start,
            count,
            a.below,
            h,
            if new_side { new_content } else { old_content },
        ));
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
                tail: vec![],
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
}
