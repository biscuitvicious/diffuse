//! File and row count calculation for review summaries.

use serde::{Deserialize, Serialize};

use crate::parse::{DiffFile, Row};

/// Aggregated line-level change counts for a diff file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileCounts {
    pub adds: usize,
    pub dels: usize,
    pub mods: usize,
    pub movs: usize,
}

impl FileCounts {
    pub fn reverse(&self) -> Self {
        let rev = FileCounts {
            adds: self.dels,
            dels: self.adds,
            mods: self.mods,
            movs: self.movs,
        };

        crate::ensure!(
            rev.adds == self.dels
                && rev.dels == self.adds
                && rev.mods == self.mods
                && rev.movs == self.movs,
            "FileCounts::reverse must swap adds and dels while preserving mods and movs"
        );

        rev
    }
}

/// Display counts: moves are neither added nor removed; mods are one row.
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
                Row::Del(del_line) => {
                    if !del_line.moved {
                        dels += 1;
                    }
                }
                Row::Mod { .. } => mods += 1,
                Row::Mov { .. } => movs += 1,
                Row::Ctx(_) | Row::Sep(_) => {}
            }
        }
    }
    // Each moved add represents one relocated line-pair (the matching del is
    // absorbed or displayed muted — either way it is not a change).
    let counts = FileCounts {
        adds,
        dels,
        mods,
        movs,
    };

    crate::ensure!(
        counts.adds == adds && counts.dels == dels && counts.mods == mods && counts.movs == movs,
        "Calculated FileCounts must match row accumulations"
    );

    counts
}

#[cfg(test)]
mod tests;
