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
                                "Hunk row {row_idx}: AuxDel row has non-Del/non-MovedSrc runs"
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
            if r.kind == RowKind::Mod || (r.kind == RowKind::Add && r.is_aux()) {
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
            if r.kind == RowKind::Mod || (r.kind == RowKind::Del && r.is_aux()) {
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

    fn validate_reflow_groups(&self, profile: Option<&LangProfile>) -> Result<(), String> {
        let strip_ws = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();

        let anchor_indices: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.group == Some(RowGroup::Anchor))
            .map(|(i, _)| i)
            .collect();

        for (i, r) in self.rows.iter().enumerate() {
            if let Some(RowGroup::Aux { anchor }) = r.group {
                let has_anchor = anchor_indices
                    .iter()
                    .any(|&ai| self.rows[ai].new_line == Some(anchor));
                if !has_anchor {
                    return Err(format!(
                        "Row {i} is Aux pointing to nonexistent anchor line {anchor}"
                    ));
                }
            }
        }

        // Reflow Group Atomicity: Anchor row and all companion aux rows
        // must form an unbroken contiguous slice in visual row order.
        for &anchor_idx in &anchor_indices {
            let anchor_row = &self.rows[anchor_idx];
            let anchor_nl = anchor_row.new_line;
            let anchor_ol = anchor_row.old_line;

            let mut group_indices: Vec<usize> = vec![anchor_idx];
            for (i, r) in self.rows.iter().enumerate() {
                if i == anchor_idx {
                    continue;
                }
                if let Some(RowGroup::Aux { anchor }) = r.group
                    && Some(anchor) == anchor_nl
                {
                    group_indices.push(i);
                }
            }

            if group_indices.len() > 1 {
                group_indices.sort_unstable();
                for window in group_indices.windows(2) {
                    if window[1] != window[0] + 1 {
                        let anchor_ident = anchor_nl.or(anchor_ol).unwrap_or(0);
                        return Err(format!(
                            "Reflow atomicity violation: Reflow group for anchor line {anchor_ident} is interrupted at row indices {} and {}",
                            window[0], window[1]
                        ));
                    }
                }
            }
        }

        for &anchor_idx in &anchor_indices {
            let anchor_row = &self.rows[anchor_idx];
            if anchor_row.old_line.is_none() {
                continue;
            }
            let anchor_nl = anchor_row.new_line;
            let anchor_ol = anchor_row.old_line;
            let anchor_num = anchor_nl.or(anchor_ol).unwrap_or(0);

            let mut group_rows: Vec<&OutputRow> = vec![anchor_row];
            for (i, r) in self.rows.iter().enumerate() {
                if i == anchor_idx {
                    continue;
                }
                if let Some(RowGroup::Aux { anchor }) = r.group
                    && Some(anchor) == anchor_nl
                {
                    group_rows.push(r);
                }
            }

            let aux_adds: Vec<&&OutputRow> = group_rows
                .iter()
                .filter(|r| r.kind == RowKind::Add)
                .collect();
            let aux_dels: Vec<&&OutputRow> = group_rows
                .iter()
                .filter(|r| r.kind == RowKind::Del)
                .collect();
            let has_aux_dels = group_rows.iter().any(|r| {
                r.kind == RowKind::Del
                    || (r.kind == RowKind::Mov && r.mov_side == Some(MovSide::Src))
            });
            let has_aux_adds = group_rows.iter().any(|r| {
                r.kind == RowKind::Add
                    || (r.kind == RowKind::Mov && r.mov_side == Some(MovSide::Tgt))
            });

            if !aux_adds.is_empty() && !has_aux_dels {
                let is_import =
                    profile.is_some_and(|p| p.is_import_statement(&anchor_row.old_text()));

                let anchor_moved_src: String = anchor_row
                    .runs
                    .iter()
                    .filter(|run| run.kind == RunKind::MovedSrc)
                    .map(|run| run.text.as_ref())
                    .collect();
                let aux_non_add: String = aux_adds
                    .iter()
                    .flat_map(|aux| &aux.runs)
                    .filter(|run| run.kind == RunKind::MovedTgt || run.kind == RunKind::Del)
                    .map(|run| run.text.as_ref())
                    .collect();

                if is_import {
                    let expected_old: String = anchor_row
                        .runs
                        .iter()
                        .filter(|run| run.kind == RunKind::Ctx || run.kind == RunKind::MovedSrc)
                        .map(|run| run.text.as_ref())
                        .collect();
                    let mut reconstructed_old = String::new();
                    for r in &group_rows {
                        if r.kind == RowKind::Mod {
                            reconstructed_old.extend(
                                r.runs
                                    .iter()
                                    .filter(|run| run.kind == RunKind::Ctx)
                                    .map(|run| run.text.as_ref()),
                            );
                        } else if r.kind == RowKind::Add {
                            reconstructed_old.extend(
                                r.runs
                                    .iter()
                                    .filter(|run| {
                                        run.kind == RunKind::MovedTgt || run.kind == RunKind::Del
                                    })
                                    .map(|run| run.text.as_ref()),
                            );
                        }
                    }
                    if strip_ws(&reconstructed_old) != strip_ws(&expected_old) {
                        return Err(format!(
                            "Anchor line {anchor_num} old-text reconstruction mismatch:\n  expected: {expected_old:?}\n  got:      {reconstructed_old:?}"
                        ));
                    }
                    if strip_ws(&anchor_moved_src) != strip_ws(&aux_non_add) {
                        return Err(format!(
                            "Anchor line {anchor_num} moved-text conservation mismatch:\n  anchor MovedSrc: {anchor_moved_src:?}\n  aux non-add:     {aux_non_add:?}"
                        ));
                    }
                } else {
                    let mut s1: Vec<char> = anchor_moved_src
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect();
                    let mut s2: Vec<char> =
                        aux_non_add.chars().filter(|c| !c.is_whitespace()).collect();
                    s1.sort_unstable();
                    s2.sort_unstable();
                    if s1 != s2 {
                        return Err(format!(
                            "Anchor line {anchor_num} moved-text character conservation mismatch:\n  anchor MovedSrc chars: {s1:?}\n  aux non-add chars:     {s2:?}"
                        ));
                    }
                }

                // Suffix aux add line numbers strictly ascending
                let mut last_aux_line = None;
                for aux in group_rows.iter().filter(|r| r.kind == RowKind::Add) {
                    if let Some(nl) = aux.new_line {
                        if let Some(prev) = last_aux_line
                            && nl <= prev
                        {
                            return Err(format!("Aux row line {nl} <= previous aux line {prev}"));
                        }
                        last_aux_line = Some(nl);
                    }
                }
            } else if !aux_dels.is_empty() && !has_aux_adds {
                let anchor_moved_tgt: String = anchor_row
                    .runs
                    .iter()
                    .filter(|run| run.kind == RunKind::MovedTgt)
                    .map(|run| run.text.as_ref())
                    .collect();
                let aux_moved_src: String = aux_dels
                    .iter()
                    .flat_map(|aux| &aux.runs)
                    .filter(|run| run.kind == RunKind::MovedSrc)
                    .map(|run| run.text.as_ref())
                    .collect();

                let mut s1: Vec<char> = anchor_moved_tgt
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                let mut s2: Vec<char> = aux_moved_src
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                s1.sort_unstable();
                s2.sort_unstable();
                if s1 != s2 {
                    return Err(format!(
                        "Anchor line {anchor_num} moved-text character conservation mismatch:\n  anchor MovedTgt chars: {s1:?}\n  aux MovedSrc chars: {s2:?}"
                    ));
                }

                // Suffix aux del line numbers strictly ascending
                let mut last_aux_line = None;
                for aux in &aux_dels {
                    if let Some(ol) = aux.old_line {
                        if let Some(prev) = last_aux_line
                            && ol <= prev
                        {
                            return Err(format!("Aux row line {ol} <= previous aux line {prev}"));
                        }
                        last_aux_line = Some(ol);
                    }
                }
            }
        }

        Ok(())
    }
}
