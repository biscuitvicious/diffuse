//! Context-delimited change clusters and syntax section partitioning (Phase 2 &
//! 3).

use crate::parse::{Hunk, Row};

/// Classification of syntactic construct for change runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SyntaxSectionKind {
    /// Doc comment (`///`, `//!`, `/** ... */`)
    DocComment,
    /// Line comment (`//`, `#`)
    LineComment,
    /// Block comment (`/* ... */`)
    BlockComment,
    /// Attribute / decorator (`#[...]`, `@...`)
    Attribute,
    /// Top-level declaration (`fn`, `pub fn`, `struct`, `impl`, `class`, `def`,
    /// `trait`)
    Declaration,
    /// General code / expression / statement
    Code,
    /// Blank line
    Blank,
}

impl SyntaxSectionKind {
    /// Whether two syntax kinds can be paired as a direct in-place
    /// modification.
    #[inline]
    pub fn is_compatible(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::DocComment, Self::DocComment)
                | (Self::LineComment, Self::LineComment)
                | (Self::BlockComment, Self::BlockComment)
                | (Self::Attribute, Self::Attribute)
                | (Self::Declaration, Self::Declaration)
                | (Self::Code, Self::Code)
                | (Self::Declaration, Self::Code)
                | (Self::Code, Self::Declaration)
                | (Self::Blank, Self::Blank)
        )
    }
}

/// Classify the syntax section of a line.
pub fn classify_syntax_section(line: &str, hash_comment: bool) -> SyntaxSectionKind {
    let t = line.trim();
    if t.is_empty() {
        return SyntaxSectionKind::Blank;
    }
    if t.starts_with("///") || t.starts_with("//!") {
        return SyntaxSectionKind::DocComment;
    }
    if t.starts_with("//") || (hash_comment && t.starts_with('#')) {
        return SyntaxSectionKind::LineComment;
    }
    if t.starts_with("/*") || t.starts_with('*') {
        return SyntaxSectionKind::BlockComment;
    }
    if t.starts_with("#[") || t.starts_with('@') {
        return SyntaxSectionKind::Attribute;
    }
    if t.starts_with("pub ")
        || t.starts_with("fn ")
        || t.starts_with("async fn ")
        || t.starts_with("impl ")
        || t.starts_with("struct ")
        || t.starts_with("enum ")
        || t.starts_with("trait ")
        || t.starts_with("mod ")
        || t.starts_with("type ")
        || t.starts_with("class ")
        || t.starts_with("def ")
    {
        return SyntaxSectionKind::Declaration;
    }
    SyntaxSectionKind::Code
}

/// An atomic change cluster bounded by context lines or syntax section
/// transitions.
#[derive(Clone, Debug, PartialEq)]
pub struct ChangeCluster {
    /// Preceding context lines immediately before this cluster (for
    /// brace/syntax anchor proximity).
    pub preceding_ctx: Vec<String>,
    /// Old-side rows (dels, in original hunk order).
    pub old_rows: Vec<Row>,
    /// New-side rows (adds, in original hunk order).
    pub new_rows: Vec<Row>,
    /// Following context lines immediately after this cluster (for brace/syntax
    /// anchor proximity).
    pub following_ctx: Vec<String>,
    pub section: SyntaxSectionKind,
}

impl ChangeCluster {
    #[inline]
    pub fn m(&self) -> usize {
        self.old_rows.len()
    }
    #[inline]
    pub fn n(&self) -> usize {
        self.new_rows.len()
    }

    pub fn old_iter(&self) -> impl Iterator<Item = &Row> {
        self.old_rows.iter()
    }
    pub fn new_iter(&self) -> impl Iterator<Item = &Row> {
        self.new_rows.iter()
    }
    pub fn old_texts(&self) -> impl Iterator<Item = &str> {
        self.old_iter().map(super::parse::Row::text)
    }
    pub fn new_texts(&self) -> impl Iterator<Item = &str> {
        self.new_iter().map(super::parse::Row::text)
    }
}

/// Decomposes a hunk into a sequence of isolated change clusters and context
/// rows.
#[derive(Clone, Debug, PartialEq)]
pub enum HunkSegment {
    Context(Row),
    Cluster(ChangeCluster),
}

/// Partitions a hunk into context rows and atomic change clusters.
pub fn partition_hunk_segments(hunk: &Hunk, hash_comment: bool) -> Vec<HunkSegment> {
    let mut segments: Vec<HunkSegment> = Vec::new();
    let mut i = 0;
    while i < hunk.rows.len() {
        if matches!(hunk.rows[i], Row::Ctx(_)) {
            segments.push(HunkSegment::Context(hunk.rows[i].clone()));
            i += 1;
            continue;
        }

        // Collect preceding context lines from immediately preceding Ctx rows (up to 6)
        let mut preceding_ctx = Vec::new();
        let mut ctx_idx = i;
        while ctx_idx > 0
            && matches!(hunk.rows[ctx_idx - 1], Row::Ctx(_))
            && preceding_ctx.len() < 6
        {
            ctx_idx -= 1;
            preceding_ctx.push(hunk.rows[ctx_idx].text().to_string());
        }
        preceding_ctx.reverse();

        let first_kind = classify_syntax_section(hunk.rows[i].text(), hash_comment);
        let mut old_rows = Vec::new();
        let mut new_rows = Vec::new();

        while i < hunk.rows.len() {
            if matches!(hunk.rows[i], Row::Ctx(_)) {
                break;
            }
            let row = &hunk.rows[i];
            let _row_kind = classify_syntax_section(row.text(), hash_comment);

            match row {
                Row::Del(_) => old_rows.push(row.clone()),
                Row::Add(_) => new_rows.push(row.clone()),
                Row::Mod { old_line, new_line } => {
                    old_rows.push(Row::Del(old_line.clone()));
                    new_rows.push(Row::Add(new_line.clone()));
                }
                Row::Mov { new_line, .. } => {
                    new_rows.push(Row::Add(new_line.clone()));
                }
                Row::Sep(_) | Row::Ctx(_) => {}
            }
            i += 1;
        }

        // Collect following context lines from immediately following Ctx rows (up to 6)
        let mut following_ctx = Vec::new();
        let mut f_idx = i;
        while f_idx < hunk.rows.len()
            && matches!(hunk.rows[f_idx], Row::Ctx(_))
            && following_ctx.len() < 6
        {
            following_ctx.push(hunk.rows[f_idx].text().to_string());
            f_idx += 1;
        }

        if !old_rows.is_empty() || !new_rows.is_empty() {
            segments.push(HunkSegment::Cluster(ChangeCluster {
                preceding_ctx,
                old_rows,
                new_rows,
                following_ctx,
                section: first_kind,
            }));
        }
    }
    segments
}

#[cfg(test)]
mod tests;

pub mod dp;
pub mod pairing;
pub mod render;
