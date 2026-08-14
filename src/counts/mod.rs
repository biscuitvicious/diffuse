//! One-for-one port of `review.ts`'s `fileCounts` (PLAN.md §3) — the
//! header counts. Moves are neither added nor removed; mods are one row.

use crate::parse::{DiffFile, RowKind};

/// Display counts: moves are neither added nor removed; mods are one row.
pub fn file_counts(f: &DiffFile) -> FileCounts {
    let mut adds = 0usize;
    let mut dels = 0usize;
    let mut mods = 0usize;
    let mut moved = 0usize;
    for h in &f.hunks {
        for r in &h.rows {
            match r.kind {
                RowKind::Add if r.moved => moved += 1,
                RowKind::Add => adds += 1,
                RowKind::Del => dels += 1,
                RowKind::Mod => mods += 1,
                _ => {}
            }
            for t in &r.tail {
                if t.moved {
                    moved += 1;
                } else {
                    adds += 1;
                }
            }
        }
    }
    // Each moved add represents one relocated line-pair (the matching del is
    // absorbed or displayed muted — either way it is not a change).
    FileCounts {
        adds,
        dels,
        mods,
        moved,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileCounts {
    pub adds: usize,
    pub dels: usize,
    pub mods: usize,
    pub moved: usize,
}

impl FileCounts {
    pub fn reverse(&self) -> Self {
        FileCounts {
            adds: self.dels,
            dels: self.adds,
            mods: self.mods,
            moved: self.moved,
        }
    }
}

#[cfg(test)]
mod tests;
