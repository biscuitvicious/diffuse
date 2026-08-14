use crate::parse::{DiffFile, Row};

/// Display counts: moves are neither added nor removed; mods are one row.
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
                Row::Mov { .. } => movs += 1,
                Row::Ctx(_) | Row::Sep(_) => {}
            }
        }
    }
    // Each moved add represents one relocated line-pair (the matching del is
    // absorbed or displayed muted — either way it is not a change).
    FileCounts {
        adds,
        dels,
        mods,
        movs,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileCounts {
    pub adds: usize,
    pub dels: usize,
    pub mods: usize,
    pub movs: usize,
}

impl FileCounts {
    pub fn reverse(&self) -> Self {
        FileCounts {
            adds: self.dels,
            dels: self.adds,
            mods: self.mods,
            movs: self.movs,
        }
    }
}

#[cfg(test)]
mod tests;
