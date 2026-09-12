//! One-for-one port of the merge machinery (PLAN.md §3): `wholesalePair`,
//! `delSpansToShow`, `delPairing`, `mergedRuns`, `displayText`,
//! `isWholesaleMod`, `isPureDeletionMod`. Comments ported verbatim; char
//! offsets throughout (the lcs.rs convention).

use serde::{Deserialize, Serialize};

use crate::{
    lcs::char_slice,
    parse::{Row, Spans},
};

/// The semantic classification of a token run in a diff row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunKind {
    Common,
    Del,
    Add,
    Ctx,
    /// Moved text: verbatim relocation (a whole `Mov` row) or a reflowed
    /// fragment (struck-moved on the source side, filled-moved on the target).
    /// Renders muted-connected rather than struck/filled.
    Mov,
}

impl RunKind {
    pub fn reverse(&self) -> Self {
        match self {
            RunKind::Common => RunKind::Common,
            RunKind::Del => RunKind::Add,
            RunKind::Add => RunKind::Del,
            RunKind::Ctx => RunKind::Ctx,
            RunKind::Mov => RunKind::Mov,
        }
    }
}

/// A merged run of a mod row's display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Run<'a> {
    pub kind: RunKind,
    pub text: &'a str,
}

impl<'a> Run<'a> {
    pub fn reverse(&self) -> Self {
        Run {
            kind: self.kind.reverse(),
            text: self.text,
        }
    }
}

/// Project the old side of a run sequence (Common + Del + Ctx).
pub fn project_old_runs(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if matches!(r.kind, RunKind::Common | RunKind::Del | RunKind::Ctx) {
            s.push_str(r.text);
        }
    }
    s
}

/// Project the new side of a run sequence (Common + Add + Ctx).
pub fn project_new_runs(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if matches!(r.kind, RunKind::Common | RunKind::Add | RunKind::Ctx) {
            s.push_str(r.text);
        }
    }
    s
}

/// True when a mod row renders as wholesale replacement.
pub fn is_wholesale_mod(r: &Row) -> bool {
    is_wholesale_replacement(r)
}

/// Returns true when a mod row consists of entirely struck-out spans followed
/// by entirely added spans with zero common anchor or context tokens (100% of
/// old line deleted, 100% of new line added).
pub fn is_wholesale_replacement_spans(del_text: &str, add_text: &str, spans: &Spans) -> bool {
    let old_len = crate::lcs::char_len(del_text);
    let new_len = crate::lcs::char_len(add_text);
    let del_chars: usize = spans.del.iter().map(|(s, e)| e.saturating_sub(*s)).sum();
    let add_chars: usize = spans.add.iter().map(|(s, e)| e.saturating_sub(*s)).sum();
    if del_chars == old_len && add_chars == new_len && (old_len > 0 || new_len > 0) {
        let runs = merged_runs_from_texts(del_text, add_text, spans);
        !runs
            .iter()
            .any(|run| matches!(run.kind, RunKind::Common | RunKind::Mov))
    } else {
        false
    }
}

/// True when a Row::Mod is a wholesale replacement (completely struck out and
/// re-added).
pub fn is_wholesale_replacement(r: &Row) -> bool {
    let Some(del_text) = r.del_text() else {
        return false;
    };
    let computed_spans;
    let spans = match r.spans() {
        Some(s) if !s.is_empty() || del_text == r.text() => s,
        _ => {
            computed_spans = crate::lcs::word_spans(del_text, r.text());
            &computed_spans
        }
    };
    is_wholesale_replacement_spans(del_text, r.text(), spans)
}

/// A mod whose change is purely a removal: every add span is whitespace —
/// the new line is the old line minus text (`pub(super) struct X {` →
/// `pub struct X {`, `resolved_layers` → `layers`). Nothing was added, so
/// the row must not render as an addition: it takes the neutral shade
/// (like a moved line — the line survives) with the red strikes carrying
/// the removal.
pub fn is_pure_deletion_mod(r: &Row) -> bool {
    match r.spans() {
        None => true,
        Some(spans) => spans
            .add
            .iter()
            .all(|(s, e)| char_slice(r.text(), *s, *e).trim().is_empty()),
    }
}

/// The ordered common/del/add runs of a mod's merged view. Each del span
/// pairs with the add span nearest its own add-position (within a small
/// window — the aligned replacement); a del span with no nearby add (a
/// standalone removal like `resolved_`) inserts at its own position, and a
/// The merged runs of a mod row's display.
pub fn merged_runs<'a>(r: &'a Row) -> Vec<Run<'a>> {
    let Some(del_text) = r.del_text() else {
        return Vec::new();
    };
    let computed_spans;
    let spans = match r.spans() {
        Some(s) if !s.is_empty() || del_text == r.text() => s,
        _ => {
            computed_spans = crate::lcs::word_spans(del_text, r.text());
            &computed_spans
        }
    };
    let runs = merged_runs_from_texts(del_text, r.text(), spans);
    if matches!(r, Row::Mod { .. }) {
        let rev = project_reverse(&runs);
        crate::ensure!(
            rev == del_text,
            "1-to-1 Row::Mod project_reverse must perfectly reconstruct old del_text. Expected: {:?}, Got: {:?}",
            del_text,
            rev
        );
    }
    runs
}

/// The ordered common/del/add runs of a mod's merged view from texts and spans
/// directly.
pub fn merged_runs_from_texts<'a>(
    del_text: &'a str,
    add_text: &'a str,
    spans: &Spans,
) -> Vec<Run<'a>> {
    if spans.del.is_empty() {
        let mut runs = Vec::new();
        let mut pos = 0;
        for &(start, end) in &spans.add {
            if start > pos {
                runs.push(Run {
                    kind: RunKind::Common,
                    text: char_slice(add_text, pos, start),
                });
            }
            if end > start {
                runs.push(Run {
                    kind: RunKind::Add,
                    text: char_slice(add_text, start, end),
                });
            }
            pos = end;
        }
        if pos < char_len_of(add_text) {
            runs.push(Run {
                kind: RunKind::Common,
                text: char_slice(add_text, pos, char_len_of(add_text)),
            });
        }
        return coalesce_runs(runs);
    }
    if spans.add.is_empty() {
        let mut runs = Vec::new();
        let mut pos = 0;
        for &(start, end) in &spans.del {
            if start > pos {
                runs.push(Run {
                    kind: RunKind::Common,
                    text: char_slice(del_text, pos, start),
                });
            }
            if end > start {
                runs.push(Run {
                    kind: RunKind::Del,
                    text: char_slice(del_text, start, end),
                });
            }
            pos = end;
        }
        if pos < char_len_of(del_text) {
            runs.push(Run {
                kind: RunKind::Common,
                text: char_slice(del_text, pos, char_len_of(del_text)),
            });
        }
        return coalesce_runs(runs);
    }

    let tape = crate::tape::AlignmentTape::from_texts(del_text, add_text, None);
    tape.to_merged_runs()
}

/// Compress adjacent runs of the same kind into a single run with joined text.
pub fn coalesce_runs<'a>(runs: Vec<Run<'a>>) -> Vec<Run<'a>> {
    let mut out: Vec<Run<'a>> = Vec::new();
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.kind == run.kind
            && last.text.as_ptr().wrapping_add(last.text.len()) == run.text.as_ptr()
        {
            last.text = unsafe {
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                    last.text.as_ptr(),
                    last.text.len() + run.text.len(),
                ))
            };
            continue;
        }
        out.push(run);
    }
    out
}

fn char_len_of(s: &str) -> usize {
    s.chars().count()
}

/// The merged display string of a row (the runs joined; a wholesale mod is
/// the whole-old + whole-new pair).
pub fn display_text(r: &Row) -> String {
    let use_merged = (matches!(r, Row::Mod { .. })
        || r.is_moved()
        || (matches!(r, Row::Add(_)) && r.del_text().is_some()))
        && r.del_text().is_some();
    if !use_merged {
        return r.text().to_string();
    }
    if is_wholesale_mod(r) {
        return format!("{}{}", r.del_text().unwrap_or_default(), r.text());
    }
    merged_runs(r).iter().map(|x| x.text).collect()
}

/// Project the row runs forward to reconstruct the new line (stripping Del
/// runs).
pub fn project_forward(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| r.kind != RunKind::Del)
        .map(|r| r.text)
        .collect()
}

/// Project the row runs backward to reconstruct the old line (stripping Add
/// runs).
pub fn project_reverse(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| r.kind != RunKind::Add)
        .map(|r| r.text)
        .collect()
}

#[cfg(test)]
mod tests;
