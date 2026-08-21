//! Context-delimited change clusters and syntax section partitioning (Phase 2 &
//! 3).

pub mod dp;
pub mod pairing;
pub mod render;

use crate::{
    parse::{Hunk, Row},
    syntax::LangProfile,
};

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
pub fn classify_syntax_section(line: &str, profile: Option<&LangProfile>) -> SyntaxSectionKind {
    let t = line.trim();
    if t.is_empty() {
        return SyntaxSectionKind::Blank;
    }
    let Some(p) = profile else {
        return SyntaxSectionKind::Code;
    };
    if p.line_comment && (t.starts_with("///") || t.starts_with("//!")) {
        return SyntaxSectionKind::DocComment;
    }
    if (p.line_comment && t.starts_with("//")) || (p.hash_comment && t.starts_with('#')) {
        return SyntaxSectionKind::LineComment;
    }
    if p.block_comment && (t.starts_with("/*") || t.starts_with('*')) {
        return SyntaxSectionKind::BlockComment;
    }
    if t.starts_with("#[") || t.starts_with('@') {
        return SyntaxSectionKind::Attribute;
    }
    if p.decl_prefixes.iter().any(|&pf| t.starts_with(pf))
        || p.scope_stop_prefixes.iter().any(|&pf| t.starts_with(pf))
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
        self.old_iter().map(Row::text)
    }
    pub fn new_texts(&self) -> impl Iterator<Item = &str> {
        self.new_iter().map(Row::text)
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
pub fn partition_hunk_segments(
    hunk: &Hunk,
    old_content: Option<&[String]>,
    new_content: Option<&[String]>,
    profile: Option<&LangProfile>,
) -> Vec<HunkSegment> {
    let mut segments = Vec::new();
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

        let first_kind = classify_syntax_section(hunk.rows[i].text(), profile);
        let mut old_rows = Vec::new();
        let mut new_rows = Vec::new();

        while i < hunk.rows.len() {
            if matches!(hunk.rows[i], Row::Ctx(_)) {
                break;
            }
            let row = &hunk.rows[i];
            let _row_kind = classify_syntax_section(row.text(), profile);

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

        if preceding_ctx.is_empty() {
            if let Some(ol) = old_rows
                .first()
                .and_then(Row::old_line)
                .or_else(|| hunk.rows.first().and_then(Row::old_line))
                && let Some(oc) = old_content
                && (ol as usize) > 1
            {
                let end = ((ol as usize) - 1).min(oc.len());
                let start = end.saturating_sub(6).min(oc.len());
                for l in &oc[start..end] {
                    preceding_ctx.push(l.clone());
                }
            } else if let Some(nl) = new_rows
                .first()
                .and_then(Row::new_line)
                .or_else(|| hunk.rows.first().and_then(Row::new_line))
                && let Some(nc) = new_content
                && (nl as usize) > 1
            {
                let end = ((nl as usize) - 1).min(nc.len());
                let start = end.saturating_sub(6).min(nc.len());
                for l in &nc[start..end] {
                    preceding_ctx.push(l.clone());
                }
            }
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

        if following_ctx.is_empty() {
            if let Some(nl) = new_rows
                .last()
                .and_then(Row::new_line)
                .or_else(|| hunk.rows.last().and_then(Row::new_line))
                && let Some(nc) = new_content
                && (nl as usize) < nc.len()
            {
                let start = (nl as usize).min(nc.len());
                let end = (start + 6).min(nc.len());
                for l in &nc[start..end] {
                    following_ctx.push(l.clone());
                }
            } else if let Some(ol) = old_rows
                .last()
                .and_then(Row::old_line)
                .or_else(|| hunk.rows.last().and_then(Row::old_line))
                && let Some(oc) = old_content
                && (ol as usize) < oc.len()
            {
                let start = (ol as usize).min(oc.len());
                let end = (start + 6).min(oc.len());
                for l in &oc[start..end] {
                    following_ctx.push(l.clone());
                }
            }
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
