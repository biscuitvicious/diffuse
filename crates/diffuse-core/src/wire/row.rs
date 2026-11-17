//! Structured Row & Tuple Serialization Schema.
//!
//! Compact array-tuple encoding for rows (`[kind, old_line, new_line, runs,
//! flags?]`), span derivation, and conversion back to structural AST `Row`
//! nodes.

use std::{fmt, ops::Not};

use serde::{Deserialize, Serialize};

use super::{MovSide, OutputRun, runs_new_text, runs_old_text};
use crate::{
    merge::RunKind,
    parse::{AddLine, CtxLine, DelLine, Row, RowGroup, RowKind, Spans},
};

/// Optional trailing metadata flags on a serialized compact row tuple.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowFlags {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mov_side: Option<MovSide>,
    #[serde(default, skip_serializing_if = "Not::not")]
    pub wholesale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<RowGroup>,
}

impl RowFlags {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.mov_side.is_none() && !self.wholesale && self.group.is_none()
    }
}

/// A row in the wire-contract output: the collapse row + its merged token runs.
///
/// Serialized as a positional tuple: `[kind, old_line, new_line, runs,
/// flags?]`.
#[derive(Clone, PartialEq, Eq, Debug, Hash)]
pub struct OutputRow {
    pub kind: RowKind,
    pub mov_side: Option<MovSide>,
    pub new_line: Option<u32>,
    pub old_line: Option<u32>,
    pub runs: Vec<OutputRun>,
    pub wholesale: bool,
    pub group: Option<RowGroup>,
}

impl OutputRow {
    pub fn reverse(mut self) -> Self {
        self.kind = self.kind.reverse();
        self.mov_side = self.mov_side.map(|s| match s {
            MovSide::Src => MovSide::Tgt,
            MovSide::Tgt => MovSide::Src,
        });
        std::mem::swap(&mut self.new_line, &mut self.old_line);
        self.runs = self.runs.into_iter().map(OutputRun::reverse).collect();
        self
    }

    #[inline]
    pub fn reconstruct(&self) -> Row {
        self.into()
    }

    /// Whether this row is an auxiliary row within a multiline cluster.
    #[inline]
    pub fn is_aux(&self) -> bool {
        matches!(self.group, Some(RowGroup::Aux { .. }))
    }

    /// Computes the new-side line text projected across this row's runs (Ctx +
    /// Add).
    #[inline]
    pub fn new_text(&self) -> String {
        runs_new_text(&self.runs)
    }

    /// Computes the old-side line text projected across this row's runs (Ctx +
    /// Del).
    #[inline]
    pub fn old_text(&self) -> String {
        runs_old_text(&self.runs)
    }

    /// Main line text for display: old text for Del / MovSrc, new text for Ctx
    /// / Add / Mod / MovTgt.
    #[inline]
    pub fn text(&self) -> String {
        match self.kind {
            RowKind::Del => self.old_text(),
            RowKind::Mov if self.mov_side == Some(MovSide::Src) => self.old_text(),
            _ => self.new_text(),
        }
    }

    /// Derives change spans on demand from runs.
    #[inline]
    pub fn spans(&self) -> Spans {
        derive_spans_from_runs(&self.runs)
    }
}

/// Test support across the engine's row-level tests: whether the rendered
/// row stream links old line `old` with new line `new` — as a Mod, or as an
/// adjacent MovSrc/MovTgt pair. Representation is engine policy; the
/// pairing is the contract.
#[cfg(test)]
pub(crate) fn test_rows_pair_with(rows: &[&OutputRow], old: u32, new: u32) -> bool {
    if rows
        .iter()
        .any(|r| r.kind == RowKind::Mod && r.old_line == Some(old) && r.new_line == Some(new))
    {
        return true;
    }
    rows.windows(2).any(|w| {
        w[0].kind == RowKind::Mov
            && w[0].mov_side == Some(MovSide::Src)
            && w[0].old_line == Some(old)
            && w[1].kind == RowKind::Mov
            && w[1].mov_side == Some(MovSide::Tgt)
            && w[1].new_line == Some(new)
    })
}

impl Serialize for OutputRow {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq;
        let flags = RowFlags {
            mov_side: self.mov_side,
            wholesale: self.wholesale,
            group: self.group,
        };
        let len = if flags.is_empty() { 4 } else { 5 };
        let mut seq = serializer.serialize_seq(Some(len))?;
        seq.serialize_element(&self.kind)?;
        seq.serialize_element(&self.old_line)?;
        seq.serialize_element(&self.new_line)?;
        seq.serialize_element(&self.runs)?;
        if !flags.is_empty() {
            seq.serialize_element(&flags)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for OutputRow {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OutputRowVisitor;

        impl<'de> serde::de::Visitor<'de> for OutputRowVisitor {
            type Value = OutputRow;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a compact row array [kind, old_line, new_line, runs, flags?]")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let kind: RowKind = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let old_line: Option<u32> = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                let new_line: Option<u32> = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(2, &self))?;
                let runs: Vec<OutputRun> = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;
                let flags: Option<RowFlags> = seq.next_element()?;

                let (mov_side, wholesale, group) = match flags {
                    Some(f) => (f.mov_side, f.wholesale, f.group),
                    None => (None, false, None),
                };

                Ok(OutputRow {
                    kind,
                    mov_side,
                    new_line,
                    old_line,
                    runs,
                    wholesale,
                    group,
                })
            }
        }

        deserializer.deserialize_seq(OutputRowVisitor)
    }
}

impl From<&OutputRow> for Row {
    fn from(r: &OutputRow) -> Self {
        let cached_runs: Vec<(RunKind, String)> = r
            .runs
            .iter()
            .map(|run| (run.kind, run.text.to_string()))
            .collect();
        let spans = r.spans();
        match r.kind {
            RowKind::Ctx => {
                let old_pos = r.old_line.expect("Ctx row must have old line");
                let new_pos = r.new_line.expect("Ctx row must have new line");
                Row::Ctx(CtxLine::new(old_pos, new_pos, r.new_text()))
            }
            RowKind::Del => {
                let num = r.old_line.expect("Del row must have old line");
                let del = DelLine::new(num, r.old_text());
                if let Some(RowGroup::Aux { anchor }) = r.group {
                    Row::AuxDel {
                        anchor,
                        line: del,
                        spans,
                        cached_runs,
                    }
                } else {
                    Row::Del(del)
                }
            }
            RowKind::Add => {
                let num = r.new_line.expect("Add row must have new line");
                let add = AddLine::new(num, r.new_text());
                if let Some(RowGroup::Aux { anchor }) = r.group {
                    Row::AuxAdd {
                        anchor,
                        line: add,
                        spans,
                        cached_runs,
                    }
                } else {
                    Row::Add(add)
                }
            }
            RowKind::Mod => {
                let old_num = r.old_line.expect("Mod row must have old line");
                let new_num = r.new_line.expect("Mod row must have new line");
                let old_line = DelLine::new(old_num, r.old_text());
                let new_line = AddLine::new(new_num, r.new_text());
                Row::Mod {
                    old_line,
                    new_line,
                    spans,
                    cached_runs,
                    group: r.group,
                }
            }
            RowKind::Mov => match r.mov_side {
                Some(MovSide::Src) => {
                    let old_num = r.old_line.expect("MovSrc row must have old line");
                    let line = DelLine::new(old_num, r.old_text());
                    Row::MovSrc {
                        line,
                        spans,
                        cached_runs,
                    }
                }
                Some(MovSide::Tgt) | None => {
                    let new_num = r.new_line.expect("MovTgt row must have new line");
                    let line = AddLine::new(new_num, r.new_text());
                    Row::MovTgt {
                        line,
                        spans,
                        cached_runs,
                    }
                }
            },
            RowKind::File | RowKind::Sep => Row::Sep(r.new_text()),
        }
    }
}

/// Derives [`Spans`] change intervals from a slice of [`OutputRun`]s.
pub fn derive_spans_from_runs(runs: &[OutputRun]) -> Spans {
    let mut add = Vec::new();
    let mut del = Vec::new();
    let mut new_offset = 0usize;
    let mut old_offset = 0usize;

    for r in runs {
        let char_len = r.text.chars().count();
        match r.kind {
            RunKind::Ctx => {
                new_offset += char_len;
                old_offset += char_len;
            }
            RunKind::Add => {
                let start = new_offset;
                new_offset += char_len;
                add.push((start, new_offset));
            }
            // Moved-out source text left this line exactly like struck
            // text (old-side coverage is identical); only renderers
            // distinguish the two. See `RunKind::is_del_like`.
            RunKind::Del | RunKind::MovedSrc => {
                let start = old_offset;
                old_offset += char_len;
                del.push((start, old_offset));
            }
            // Moved-in target text is new-side-only (like filled text for
            // offset purposes) but lands in no span bucket — it is shared,
            // not novel. See `RunKind::is_add_like`.
            RunKind::MovedTgt => {
                new_offset += char_len;
            }
        }
    }

    let merge_spans = |spans: Vec<(usize, usize)>| -> Vec<(usize, usize)> {
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (s, e) in spans {
            if let Some(last) = merged.last_mut()
                && last.1 == s
            {
                last.1 = e;
            } else {
                merged.push((s, e));
            }
        }
        merged
    };

    Spans {
        del: merge_spans(del),
        add: merge_spans(add),
    }
}
