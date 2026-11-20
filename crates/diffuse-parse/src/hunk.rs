//! Unified diff hunk models and line-count computations.

use serde::Serialize;

use crate::row::Row;

/// A hunk: the `@@ -old +new @@` region of a diff.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Default)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub section: String,
    /// Base rows (add/del only, parsed from `git diff -U0`).
    pub rows: Vec<Row>,
    pub above_ctx: u32,
    pub below_ctx: u32,
}

impl Hunk {
    pub fn reverse(mut self) -> Self {
        std::mem::swap(&mut self.old_start, &mut self.new_start);
        std::mem::swap(&mut self.old_count, &mut self.new_count);
        self.rows = self.rows.into_iter().map(Row::reverse).collect();
        self
    }
}

/// Row-derived new-side count: the *extent* from the hunk start to the last
/// new-side line, not a row count. Accurate for the @ header counts too.
pub fn add_count(h: &Hunk) -> u32 {
    let mut max_new = h.new_start;
    let mut has_new = false;
    for r in &h.rows {
        match r {
            Row::Add { line: add_line, .. } | Row::MovTgt { line: add_line, .. } => {
                let lines = add_line.text.matches('\n').count() as u32;
                max_new = max_new.max(add_line.num + lines);
                has_new = true;
            }
            Row::Mod { new_line, .. } => {
                let lines = new_line.text.matches('\n').count() as u32;
                max_new = max_new.max(new_line.num + lines);
                has_new = true;
            }
            _ => {}
        }
    }
    if has_new {
        (max_new - h.new_start + 1).max(1)
    } else {
        0
    }
}

pub fn del_count(h: &Hunk) -> u32 {
    h.rows
        .iter()
        .filter(|r| matches!(r, Row::Del { .. }))
        .count() as u32
}

pub fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32, &str)> {
    let rest = line.strip_prefix("@@ -")?;
    let space = rest.find(' ')?;
    let old_part = &rest[..space];
    let rest_after_space = &rest[space + 1..];
    let new_part_full = rest_after_space.strip_prefix('+')?;
    let at_at = new_part_full.find(" @@")?;
    let new_part = &new_part_full[..at_at];
    let section = &new_part_full[at_at + 3..];

    let (old_start, old_count) = if let Some(comma) = old_part.find(',') {
        (
            old_part[..comma].parse().ok()?,
            old_part[comma + 1..].parse().unwrap_or(1),
        )
    } else {
        (old_part.parse().ok()?, 1)
    };

    let (new_start, new_count) = if let Some(comma) = new_part.find(',') {
        (
            new_part[..comma].parse().ok()?,
            new_part[comma + 1..].parse().unwrap_or(1),
        )
    } else {
        (new_part.parse().ok()?, 1)
    };

    Some((old_start, old_count, new_start, new_count, section))
}
