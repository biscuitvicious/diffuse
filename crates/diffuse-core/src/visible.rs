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
pub fn hunk_has_adds(h: &Hunk) -> bool {
    h.rows
        .iter()
        .any(|r| matches!(r, Row::Add { .. } | Row::Mod { .. } | Row::MovTgt { .. }))
}

#[cfg(test)]
mod tests;
