//! File and row count calculation for review summaries.

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::parse::{DiffFile, Row};

/// Aggregated line-level change counts for a diff file, including attached
/// review notes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileCounts {
    pub adds: usize,
    pub dels: usize,
    pub mods: usize,
    pub movs: usize,
    /// Number of review notes attached to this file.
    #[serde(default, skip_serializing_if = "usize_is_zero")]
    pub notes: usize,
}

fn usize_is_zero(v: &usize) -> bool {
    *v == 0
}

/// Which mod-count bucket a display segment belongs to. Renderers map this
/// to their own highlight palette; the marker glyphs live on
/// [`CountSegment`] so every counts format shares one source of truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountKind {
    Dels,
    Adds,
    Mods,
    Movs,
    Notes,
}

/// One non-zero mod-count segment for display: its bucket, marker glyph, and
/// value. Markers carry their own spacing (`"💬 "` trails a space) so
/// renderers simply join `marker + value` segments with a single space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountSegment {
    pub kind: CountKind,
    pub marker: &'static str,
    pub value: usize,
}

impl FileCounts {
    /// Returns `true` when all counts are zero (no changes and no notes).
    pub fn is_empty(&self) -> bool {
        self.adds == 0 && self.dels == 0 && self.mods == 0 && self.movs == 0 && self.notes == 0
    }

    /// Non-zero count segments in stable display order (dels, adds, mods,
    /// movs, notes). At most five segments exist, stored inline. Empty when
    /// [`FileCounts::is_empty`].
    pub fn segments(&self) -> SmallVec<[CountSegment; 5]> {
        let mut out = SmallVec::new();
        if self.dels > 0 {
            out.push(CountSegment {
                kind: CountKind::Dels,
                marker: "−",
                value: self.dels,
            });
        }
        if self.adds > 0 {
            out.push(CountSegment {
                kind: CountKind::Adds,
                marker: "+",
                value: self.adds,
            });
        }
        if self.mods > 0 {
            out.push(CountSegment {
                kind: CountKind::Mods,
                marker: "~",
                value: self.mods,
            });
        }
        if self.movs > 0 {
            out.push(CountSegment {
                kind: CountKind::Movs,
                marker: "≈",
                value: self.movs,
            });
        }
        if self.notes > 0 {
            out.push(CountSegment {
                kind: CountKind::Notes,
                marker: "💬 ",
                value: self.notes,
            });
        }
        out
    }

    pub fn reverse(&self) -> Self {
        let rev = FileCounts {
            adds: self.dels,
            dels: self.adds,
            mods: self.mods,
            movs: self.movs,
            notes: self.notes,
        };

        crate::ensure!(
            rev.adds == self.dels
                && rev.dels == self.adds
                && rev.mods == self.mods
                && rev.movs == self.movs
                && rev.notes == self.notes,
            "FileCounts::reverse must swap adds and dels while preserving mods, movs, and notes"
        );

        rev
    }
}

/// Display counts: each visual row contributes exactly once.
/// A mod is one logical change (+1 mod). A moved pair is one logical
/// relocation — only the target side counts (+1 mov); the source side
/// is absorbed like a mod's old line.
///
/// Note: this function cannot know the note count (notes live in the
/// per-repo [`crate::notes::NotesDB`], outside the parsed diff), so it
/// returns `notes: 0`. The pipeline (`process_single_diff_file`) stamps the
/// real count onto `OutputFile::counts.notes` after attaching the file's
/// notes, which feeds the per-file header counts and the
/// [`crate::wire::PipelineOutput::total_counts`] aggregate.
#[tracing::instrument(level = "trace", ret, skip(f), fields(path = %f.path))]
pub fn file_counts(f: &DiffFile) -> FileCounts {
    let mut adds = 0usize;
    let mut dels = 0usize;
    let mut mods = 0usize;
    let mut movs = 0usize;
    for h in &f.hunks {
        for r in &h.rows {
            match r {
                Row::Add(_) => adds += 1,
                Row::Del(_) => dels += 1,
                Row::Mod { .. } => mods += 1,
                Row::MovTgt(_) => movs += 1,
                Row::MovSrc(_) | Row::Ctx(_) | Row::Sep(_) => {}
            }
        }
    }
    let counts = FileCounts {
        adds,
        dels,
        mods,
        movs,
        notes: 0,
    };

    crate::ensure!(
        counts.adds == adds && counts.dels == dels && counts.mods == mods && counts.movs == movs,
        "Calculated FileCounts must match row accumulations"
    );

    counts
}

#[cfg(test)]
mod tests;
