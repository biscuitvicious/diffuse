//! Output Hunk & Invariant Validation Models.
//!
//! Encodes hunk-level change coordinates, collapsed rows, and comprehensive
//! invariant validation (line number monotonicity, span bounds, disk-exactness,
//! and reflow conservation).

use diffuse_syntax::profile::LangProfile;
use serde::{Deserialize, Serialize};

use super::OutputRow;
use crate::{
    counts::FileCounts,
    merge::RunKind,
    parse::{Hunk, RowGroup, RowKind},
};

/// Identifies which side of a moved line an [`OutputRow`] represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum MovSide {
    Src,
    Tgt,
}

/// Identifies which side of an expanded modification line an expanded row
/// represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ModSide {
    /// Pre-image side (deletion half displaying old line/text).
    Del,
    /// Post-image side (addition half displaying new line/text).
    Add,
}

/// Identifies the box border glyph position within an expanded modification
/// cluster.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Hash, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModGlyph {
    /// Top corner of the box ("┌") - first line in an expanded cluster.
    #[default]
    Top,
    /// Vertical edge of the box ("│") - intermediate lines in an expanded
    /// cluster.
    Mid,
    /// Bottom corner of the box ("└") - last line in an expanded cluster.
    Bottom,
}

impl ModGlyph {
    /// The box border glyph character.
    #[inline]
    pub const fn as_char(self) -> char {
        match self {
            Self::Top => '┌',
            Self::Mid => '│',
            Self::Bottom => '└',
        }
    }

    /// The box border glyph string slice.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "┌",
            Self::Mid => "│",
            Self::Bottom => "└",
        }
    }

    /// Returns true if this is the bottom of the box, terminating the cluster.
    #[inline]
    pub const fn is_bottom(self) -> bool {
        matches!(self, Self::Bottom)
    }

    /// Default glyph for an isolated (single-row) modification side.
    #[inline]
    pub const fn for_side(side: ModSide) -> Self {
        Self::for_cluster(side, 0, 1)
    }

    /// Glyph for a modification row at offset `idx` (0..total) within a cluster
    /// of `total` rows on the given side.
    #[inline]
    pub const fn for_cluster(side: ModSide, idx: usize, total: usize) -> Self {
        match side {
            ModSide::Del => {
                if idx == 0 {
                    Self::Top
                } else {
                    Self::Mid
                }
            }
            ModSide::Add => {
                if idx + 1 >= total {
                    Self::Bottom
                } else {
                    Self::Mid
                }
            }
        }
    }
}

/// A hunk in the wire contract: change coordinates and its collapsed rows.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[derive(Default)]
pub struct OutputHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub rows: Vec<OutputRow>,
}

impl OutputHunk {
    pub fn reverse(mut self) -> Self {
        std::mem::swap(&mut self.old_start, &mut self.new_start);
        std::mem::swap(&mut self.old_count, &mut self.new_count);
        self.rows = self.rows.into_iter().map(OutputRow::reverse).collect();
        self
    }

    /// Aggregated line-level change counts for this hunk's rows, mirroring
    /// [`crate::counts::file_counts`] row semantics (relocations count at
    /// target side; source side is absorbed; context and separators
    /// excluded). Notes attach at file level, so `notes` is always 0 — note
    /// counts are never attributed to hunks.
    pub fn counts(&self) -> FileCounts {
        let mut counts = FileCounts::default();
        for r in &self.rows {
            match r.kind {
                RowKind::Del => counts.dels += 1,
                RowKind::Add => counts.adds += 1,
                RowKind::Mod => counts.mods += 1,
                RowKind::Mov if r.mov_side != Some(MovSide::Src) => counts.movs += 1,
                _ => {}
            }
        }
        counts
    }

    pub fn reconstruct(&self) -> Hunk {
        Hunk {
            old_start: self.old_start,
            old_count: self.old_count,
            new_start: self.new_start,
            new_count: self.new_count,
            section: String::new(),
            rows: self.rows.iter().map(OutputRow::reconstruct).collect(),
            above_ctx: 0,
            below_ctx: 0,
        }
    }

    pub fn validate_invariants(&self) -> Result<(), String> {
        self.validate_invariants_with_profile(None)
    }

    pub fn validate_invariants_with_profile(
        &self,
        profile: Option<&LangProfile>,
    ) -> Result<(), String> {
        let mut last_new = None;
        let mut last_old = None;

        for (row_idx, r) in self.rows.iter().enumerate() {
            // New line number monotonicity (strict stepwise unbroken sequence)
            if let Some(nl) = r.new_line {
                if let Some(prev) = last_new
                    && nl != prev + 1
                {
                    return Err(format!(
                        "Hunk row {row_idx}: new_line non-stepwise: expected {}, got {nl}",
                        prev + 1
                    ));
                }
                last_new = Some(nl);
            }

            // Old line number monotonicity (strict stepwise unbroken sequence)
            if let Some(ol) = r.old_line {
                if let Some(prev) = last_old
                    && ol != prev + 1
                {
                    return Err(format!(
                        "Hunk row {row_idx}: old_line non-stepwise: expected {}, got {ol}",
                        prev + 1
                    ));
                }
                last_old = Some(ol);
            }

            // Line number invariants per row kind
            match r.kind {
                RowKind::Add => {
                    if r.new_line.is_none() || r.old_line.is_some() {
                        return Err(format!(
                            "Hunk row {row_idx}: Add row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                }
                RowKind::Del => {
                    if r.old_line.is_none() || r.new_line.is_some() {
                        return Err(format!(
                            "Hunk row {row_idx}: Del row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                    if r.group.is_some() {
                        if r.runs
                            .iter()
                            .any(|run| run.kind != RunKind::Del && run.kind != RunKind::MovedSrc)
                        {
                            return Err(format!(
                                "Hunk row {row_idx}: peer-grouped Del row has non-Del/non-MovedSrc runs"
                            ));
                        }
                    } else if r.runs.iter().any(|run| run.kind != RunKind::Del) {
                        return Err(format!("Hunk row {row_idx}: Del row has non-Del runs"));
                    }
                }
                RowKind::Mod => {
                    if r.new_line.is_none() || r.old_line.is_none() {
                        return Err(format!(
                            "Hunk row {row_idx}: Mod row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                    if r.wholesale {
                        return Err(format!("Hunk row {row_idx}: Mod row must not be wholesale"));
                    }
                }
                RowKind::Ctx => {
                    if r.new_line.is_none() || r.old_line.is_none() {
                        return Err(format!(
                            "Hunk row {row_idx}: Ctx row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                    if r.runs.iter().any(|run| run.kind != RunKind::Ctx) {
                        return Err(format!("Hunk row {row_idx}: Ctx row has non-Ctx runs"));
                    }
                }
                RowKind::Mov => {
                    if r.mov_side == Some(MovSide::Src) {
                        if r.old_line.is_none() || r.new_line.is_some() {
                            return Err(format!(
                                "Hunk row {row_idx}: MovSrc row invalid line numbers (new={:?}, old={:?})",
                                r.new_line, r.old_line
                            ));
                        }
                    } else if r.new_line.is_none() || r.old_line.is_some() {
                        return Err(format!(
                            "Hunk row {row_idx}: MovTgt row invalid line numbers (new={:?}, old={:?})",
                            r.new_line, r.old_line
                        ));
                    }
                }
                _ => {}
            }

            // Spans bounds and ordering
            let spans = r.spans();
            let new_char_len = r.new_text().chars().count();
            let mut prev_add_end = 0;
            for &(s, e) in &spans.add {
                if s > e || e > new_char_len || s < prev_add_end {
                    return Err(format!(
                        "Hunk row {row_idx}: invalid add span [{s}..{e}] for text length {new_char_len}"
                    ));
                }
                prev_add_end = e;
            }

            let old_char_len = r.old_text().chars().count();
            let mut prev_del_end = 0;
            for &(s, e) in &spans.del {
                if s > e || e > old_char_len || s < prev_del_end {
                    return Err(format!(
                        "Hunk row {row_idx}: invalid del span [{s}..{e}] for old text length {old_char_len}"
                    ));
                }
                prev_del_end = e;
            }

            // Disk-exact invariant:
            // Forward: non-del-like runs concatenate to row.new_text()
            if r.kind == RowKind::Mod || (r.kind == RowKind::Add && r.is_peer()) {
                let kept: String = r
                    .runs
                    .iter()
                    .filter(|x| !x.kind.is_del_like())
                    .map(|x| x.text.as_ref())
                    .collect();
                if kept != r.new_text() {
                    return Err(format!(
                        "Hunk row {row_idx}: forward text mismatch: kept {kept:?} != text {:?}",
                        r.new_text()
                    ));
                }
            }
            // Reverse: non-add-like runs concatenate to row.old_text()
            if r.kind == RowKind::Mod || (r.kind == RowKind::Del && r.is_peer()) {
                let rev: String = r
                    .runs
                    .iter()
                    .filter(|x| !x.kind.is_add_like())
                    .map(|x| x.text.as_ref())
                    .collect();
                let old_text = r.old_text();
                if rev != old_text {
                    return Err(format!(
                        "Hunk row {row_idx}: reverse text mismatch: rev {rev:?} != old_text {old_text:?}"
                    ));
                }
            }
        }

        // Reflow groups invariant within the hunk
        self.validate_reflow_groups(profile)?;

        Ok(())
    }

    fn validate_reflow_groups(&self, _profile: Option<&LangProfile>) -> Result<(), String> {
        // Validate Peer Groups:
        // 1. Peer group atomicity: all rows with the same GroupId must form an unbroken
        //    contiguous slice.
        // 2. Peer group slots: slot indices must be strictly monotonic.
        // 3. Peer group lines: old_line and new_line must be strictly monotonic.
        // 4. Closed-economy conservation: sum(MovedSrc chars) == sum(MovedTgt chars).
        let mut peer_groups: rustc_hash::FxHashMap<u32, Vec<(usize, &OutputRow)>> =
            rustc_hash::FxHashMap::default();
        for (i, r) in self.rows.iter().enumerate() {
            if let Some(RowGroup::Peer { id, .. }) = r.group {
                peer_groups.entry(*id).or_default().push((i, r));
            }
        }
        for (id, members) in peer_groups {
            for window in members.windows(2) {
                if window[1].0 != window[0].0 + 1 {
                    return Err(format!(
                        "Peer group {id} atomicity violation: interrupted at row indices {} and {}",
                        window[0].0, window[1].0
                    ));
                }
            }

            let mut last_slot = None;
            let mut last_old = None;
            let mut last_new = None;
            let mut group_moved_src = Vec::new();
            let mut group_moved_tgt = Vec::new();

            for (_, r) in &members {
                if let Some(RowGroup::Peer { slot, .. }) = r.group {
                    if let Some(prev) = last_slot
                        && slot <= prev
                    {
                        return Err(format!(
                            "Peer group {id} slot non-monotonic: {prev} -> {slot}"
                        ));
                    }
                    last_slot = Some(slot);
                }

                if let Some(ol) = r.old_line {
                    if let Some(prev) = last_old
                        && ol <= prev
                    {
                        return Err(format!(
                            "Peer group {id} old line {ol} <= previous old line {prev}"
                        ));
                    }
                    last_old = Some(ol);
                }

                if let Some(nl) = r.new_line {
                    if let Some(prev) = last_new
                        && nl <= prev
                    {
                        return Err(format!(
                            "Peer group {id} new line {nl} <= previous new line {prev}"
                        ));
                    }
                    last_new = Some(nl);
                }

                for run in &r.runs {
                    match run.kind {
                        RunKind::MovedSrc => {
                            group_moved_src.extend(run.text.chars().filter(|c| !c.is_whitespace()))
                        }
                        RunKind::MovedTgt => {
                            group_moved_tgt.extend(run.text.chars().filter(|c| !c.is_whitespace()))
                        }
                        _ => {}
                    }
                }
            }

            group_moved_src.sort_unstable();
            group_moved_tgt.sort_unstable();
            if group_moved_src != group_moved_tgt {
                return Err(format!(
                    "Peer group {id} character conservation mismatch: MovedSrc={:?} vs MovedTgt={:?}",
                    group_moved_src, group_moved_tgt
                ));
            }
        }

        Ok(())
    }
}
