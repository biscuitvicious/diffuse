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
    pub old_rows: Vec<Row>,
    pub new_rows: Vec<Row>,
    pub section: SyntaxSectionKind,
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
        if let Row::Ctx(_) = &hunk.rows[i] {
            segments.push(HunkSegment::Context(hunk.rows[i].clone()));
            i += 1;
            continue;
        }

        // Change cluster run
        let mut old_rows = Vec::new();
        let mut new_rows = Vec::new();
        let first_kind = classify_syntax_section(hunk.rows[i].text(), hash_comment);

        while i < hunk.rows.len() {
            if matches!(hunk.rows[i], Row::Ctx(_)) {
                break;
            }
            let row = &hunk.rows[i];
            let row_kind = classify_syntax_section(row.text(), hash_comment);

            // If syntax construct is incompatible with this cluster and we already
            // collected rows, break cluster
            if (!old_rows.is_empty() || !new_rows.is_empty())
                && !first_kind.is_compatible(row_kind)
                && row_kind != SyntaxSectionKind::Blank
            {
                break;
            }

            match row {
                Row::Del(_) => old_rows.push(row.clone()),
                Row::Add(_) => new_rows.push(row.clone()),
                Row::Mod { old_line, new_line } | Row::Mov { old_line, new_line } => {
                    old_rows.push(Row::Del(old_line.clone()));
                    new_rows.push(Row::Add(new_line.clone()));
                }
                Row::Sep(_) => {}
                Row::Ctx(_) => unreachable!(),
            }
            i += 1;
        }

        if !old_rows.is_empty() || !new_rows.is_empty() {
            segments.push(HunkSegment::Cluster(ChangeCluster {
                old_rows,
                new_rows,
                section: first_kind,
            }));
        }
    }
    segments
}

#[cfg(test)]
mod tests;
