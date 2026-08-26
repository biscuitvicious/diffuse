//! Visible row construction.
//!
//! Context lines are carried through the pipeline (never stripped at parse,
//! never re-synthesized from a preset), so the visible sequence is a straight
//! walk of each hunk's rows — context + changes in order.

use crate::parse::{Hunk, Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    New,
    Old,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleRow {
    pub row: Row,
    pub side: Option<Side>,
}

pub fn build_visible(hunks: &[Hunk], _expand_lines: usize) -> Vec<VisibleRow> {
    let mut rows = Vec::new();
    for h in hunks {
        let side = if hunk_has_adds(h) {
            Side::New
        } else {
            Side::Old
        };
        for r in &h.rows {
            rows.push(VisibleRow {
                row: r.clone(),
                side: if matches!(r, Row::Ctx(_)) {
                    Some(side)
                } else {
                    None
                },
            });
        }
    }
    rows
}

/// Whether the hunk carries any added (new-side) rows — decides which side's
/// line numbers the context rows display against.
fn hunk_has_adds(h: &Hunk) -> bool {
    h.rows
        .iter()
        .any(|r| matches!(r, Row::Add(_) | Row::Mod { .. } | Row::Mov { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{AddLine, CtxLine, DelLine, LineNum};

    fn hunk(
        rows: Vec<Row>,
        old_start: u32,
        old_count: u32,
        new_start: u32,
        new_count: u32,
    ) -> Hunk {
        Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            section: String::new(),
            rows,
            above_ctx: 0,
            below_ctx: 0,
        }
    }

    #[test]
    fn test_build_visible_walks_carried_context() {
        // A single hunk whose rows already carry leading + trailing context.
        let h = hunk(
            vec![
                Row::Ctx(CtxLine::new(
                    LineNum::new(1).unwrap(),
                    LineNum::new(1).unwrap(),
                    "fn foo() {",
                )),
                Row::Mod {
                    old_line: DelLine::new(LineNum::new(2).unwrap(), "let x = 1;"),
                    new_line: AddLine::new(LineNum::new(2).unwrap(), "let x = 2;"),
                },
                Row::Ctx(CtxLine::new(
                    LineNum::new(3).unwrap(),
                    LineNum::new(3).unwrap(),
                    "}",
                )),
            ],
            1,
            3,
            1,
            3,
        );
        let vis = build_visible(&[h], crate::DEFAULT_EXPAND_LINES);
        assert_eq!(vis.len(), 3);
        assert_eq!(vis[0].row.text(), "fn foo() {");
        assert!(vis[0].side.is_some());
        assert!(vis[1].side.is_none());
        assert_eq!(vis[2].row.text(), "}");
    }
}
