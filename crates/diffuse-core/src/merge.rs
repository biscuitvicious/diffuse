//! One-for-one port of the merge machinery (PLAN.md §3): `wholesalePair`,
//! `delSpansToShow`, `delPairing`, `mergedRuns`, `displayText`,
//! `isWholesaleMod`, `isPureDeletionMod`. Comments ported verbatim; char
//! offsets throughout (the lcs.rs convention).

use std::{slice, str};

use crate::{
    lcs::char_slice,
    parse::{Row, Spans},
};

pub use diffuse_parse::RunKind;

/// A merged run of a mod row's display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run<'a> {
    pub kind: RunKind,
    pub text: &'a str,
}

impl<'a> Run<'a> {
    pub const fn reverse(self) -> Self {
        Run {
            kind: self.kind.reverse(),
            text: self.text,
        }
    }
}

/// Project the old side of a run sequence (excluding new-side-only
/// runs: filled-novel and moved-in target text).
pub fn project_old_runs(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if !r.kind.is_add_like() {
            s.push_str(r.text);
        }
    }
    s
}

/// Project the new side of a run sequence (non-del-like, i.e. Ctx + Add;
/// moved-out source text is old-side only).
pub fn project_new_runs(runs: &[Run]) -> String {
    let mut s = String::new();
    for r in runs {
        if !r.kind.is_del_like() {
            s.push_str(r.text);
        }
    }
    s
}

#[inline]
pub fn old_text_from_runs(runs: &[Run]) -> String {
    project_old_runs(runs)
}

#[inline]
pub fn new_text_from_runs(runs: &[Run]) -> String {
    project_new_runs(runs)
}

/// True when a mod row renders as wholesale replacement.
pub fn is_wholesale_mod(r: &Row) -> bool {
    is_wholesale_replacement(r)
}

/// Returns true when a mod row consists of entirely struck-out spans followed
/// by entirely added spans with zero non-whitespace common anchor or context
/// tokens (100% of non-whitespace characters in old line deleted, 100% of
/// non-whitespace characters in new line added).
pub fn is_wholesale_replacement_spans(del_text: &str, add_text: &str, spans: &Spans) -> bool {
    let old_len = crate::lcs::char_len(del_text);
    let new_len = crate::lcs::char_len(add_text);
    if old_len == 0 && new_len == 0 {
        return false;
    }
    let runs = merged_runs_from_texts(del_text, add_text, spans);
    let has_non_ws_ctx = runs
        .iter()
        .any(|run| run.kind == RunKind::Ctx && !run.text.trim().is_empty());
    if !has_non_ws_ctx {
        return !del_text.trim().is_empty() || !add_text.trim().is_empty();
    }
    false
}

/// True when a Row::Mod is a wholesale replacement (completely struck out and
/// re-added).
pub fn is_wholesale_replacement(r: &Row) -> bool {
    if !matches!(r, Row::Mod { .. }) {
        return false;
    }
    if r.group() == Some(crate::parse::RowGroup::Anchor) {
        return false;
    }
    let Some(del_text) = r.old_text() else {
        return false;
    };
    let computed_spans;
    let spans = match r.spans() {
        Some(s)
            if !s.is_empty()
                || del_text == r.text()
                || matches!(r, Row::Add(_) | Row::MovTgt { .. } | Row::AuxAdd { .. }) =>
        {
            s
        }
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
    merged_runs_with_profile(r, None)
}

/// Profile-aware `merged_runs`: prefers the profiled-tape runs handed from
/// assemble (`Row::cached_runs`); otherwise recomputes with the given
/// profile (callers that know the row's language pass it so the recompute
/// matches the spans' tape — see `display_text_with_profile`).
pub fn merged_runs_with_profile<'a>(
    r: &'a Row,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) -> Vec<Run<'a>> {
    let cached = r.cached_runs();
    if !cached.is_empty() {
        return cached
            .iter()
            .map(|(kind, text)| Run {
                kind: *kind,
                text: text.as_str(),
            })
            .collect();
    }
    if matches!(r, Row::AuxAdd { .. }) {
        return vec![Run {
            kind: RunKind::Add,
            text: r.text(),
        }];
    }
    if matches!(r, Row::AuxDel { .. }) {
        return vec![Run {
            kind: RunKind::Del,
            text: r.text(),
        }];
    }
    let Some(del_text) = r.old_text() else {
        return Vec::new();
    };
    let computed_spans;
    let spans = match r.spans() {
        Some(s)
            if !s.is_empty()
                || del_text == r.text()
                || matches!(r, Row::Add(_) | Row::MovTgt { .. } | Row::AuxAdd { .. }) =>
        {
            s
        }
        _ => {
            computed_spans = crate::lcs::word_spans_with_profile(del_text, r.text(), profile);
            &computed_spans
        }
    };
    let runs = merged_runs_from_texts_with_profile(del_text, r.text(), spans, profile);
    if matches!(r, Row::Mod { .. }) {
        crate::ensure!(
            project_reverse_matches(&runs, del_text),
            "1-to-1 Row::Mod project_reverse must perfectly reconstruct old del_text. Expected: {:?}, Got: {:?}",
            del_text,
            project_reverse(&runs)
        );
    }
    // Aux-grouped additions are reflow destinations: text they share with
    // the anchor donor moved in — mark it target. (Plain context would be
    // equally truthful; the mark lets renderers tell moved-in from merely
    // common, and completes the wire's account of the move whose source
    // half the anchor marks moved-out.) Genuine fills keep Add.
    if matches!(r, Row::AuxAdd { .. }) {
        return runs
            .into_iter()
            .map(|run| {
                if run.kind == RunKind::Ctx {
                    Run {
                        kind: RunKind::MovedTgt,
                        ..run
                    }
                } else {
                    run
                }
            })
            .collect();
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
    merged_runs_from_texts_with_profile(del_text, add_text, spans, None)
}

/// Profile-aware `merged_runs_from_texts`: when both span sides are present
/// the runs come from an alignment tape built with the given profile (the
/// same tape that produced the spans, under render unification).
pub fn merged_runs_from_texts_with_profile<'a>(
    del_text: &'a str,
    add_text: &'a str,
    spans: &Spans,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) -> Vec<Run<'a>> {
    if spans.del.is_empty() {
        let mut runs = Vec::new();
        let mut pos = 0;
        for &(start, end) in &spans.add {
            if start > pos {
                runs.push(Run {
                    kind: RunKind::Ctx,
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
                kind: RunKind::Ctx,
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
                    kind: RunKind::Ctx,
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
                kind: RunKind::Ctx,
                text: char_slice(del_text, pos, char_len_of(del_text)),
            });
        }
        return coalesce_runs(runs);
    }

    // Honor stored spans when they rebuild the same pairing: a customized
    // pairing (verbatim-block demotion) survives to display instead of
    // being re-derived. Profiled display only — unprofiled display keeps
    // the legacy recompute (see the `colored_runs` wholesale test).
    // Falls back to the fresh tape on any malformed span shape, so
    // ordinary inputs render exactly as before.
    if profile.is_some()
        && let Some(chunks) =
            crate::tape::AlignmentTape::chunks_from_spans(del_text, add_text, spans)
    {
        return crate::tape::AlignmentTape::runs_from_chunks(&chunks);
    }
    let tape = crate::tape::AlignmentTape::from_texts(del_text, add_text, profile);
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
                str::from_utf8_unchecked(slice::from_raw_parts(
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
    display_text_with_profile(r, None)
}

/// Profile-aware `display_text` (see `merged_runs_with_profile`).
pub fn display_text_with_profile(
    r: &Row,
    profile: Option<&diffuse_syntax::profile::LangProfile>,
) -> String {
    let use_merged = matches!(r, Row::Mod { .. }) || !r.cached_runs().is_empty();
    if !use_merged {
        return r.text().to_string();
    }
    if is_wholesale_mod(r) {
        return format!("{}{}", r.old_text().unwrap_or_default(), r.text());
    }
    merged_runs_with_profile(r, profile)
        .iter()
        .map(|x| x.text)
        .collect()
}

/// Project the row runs forward to reconstruct the new line (stripping
/// del-like runs: struck-deleted and struck-moved alike are old-side).
pub fn project_forward(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| !r.kind.is_del_like())
        .map(|r| r.text)
        .collect()
}

/// Project the row runs backward to reconstruct the old line (stripping Add
/// runs; moved-out source text reconstructs like struck text).
pub fn project_reverse(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| !r.kind.is_add_like())
        .map(|r| r.text)
        .collect()
}

/// Checks if backward projection of runs matches `del_text` without allocating.
pub fn project_reverse_matches(runs: &[Run], del_text: &str) -> bool {
    let mut remainder = del_text;
    for r in runs {
        if !r.kind.is_add_like() {
            if !remainder.starts_with(r.text) {
                return false;
            }
            remainder = &remainder[r.text.len()..];
        }
    }
    remainder.is_empty()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod moved_src_tests {
    use super::*;

    #[test]
    fn test_moved_src_sigil_round_trip() {
        assert_eq!(RunKind::MovedSrc.sign_char(), '<');
        assert_eq!(RunKind::from_sign_char('<'), Some(RunKind::MovedSrc));
        assert_eq!(RunKind::MovedTgt.sign_char(), '>');
        assert_eq!(RunKind::from_sign_char('>'), Some(RunKind::MovedTgt));
        // Existing sigils unaffected.
        assert_eq!(RunKind::from_sign_char('_'), Some(RunKind::Ctx));
        assert_eq!(RunKind::from_sign_char('-'), Some(RunKind::Del));
        assert_eq!(RunKind::from_sign_char('+'), Some(RunKind::Add));
    }

    #[test]
    fn test_moved_src_is_del_like_but_not_del() {
        assert!(RunKind::MovedSrc.is_del_like());
        assert!(RunKind::Del.is_del_like());
        assert!(!RunKind::Ctx.is_del_like());
        assert!(!RunKind::Add.is_del_like());
        // Del-like is not Del: renderers must distinguish struck-moved
        // (skipped by default) from struck-deleted.
        assert_ne!(RunKind::MovedSrc, RunKind::Del);
    }

    #[test]
    fn test_moved_kinds_reverse_into_each_other() {
        // Moved kinds reverse into each other, preserving direction across
        // the old/new swap — this is what keeps wire reversal an exact
        // involution *and* direction-correct (unlike mapping to Add/Del,
        // which breaks one or the other).
        assert_eq!(RunKind::MovedSrc.reverse(), RunKind::MovedTgt);
        assert_eq!(RunKind::MovedTgt.reverse(), RunKind::MovedSrc);
        assert_eq!(RunKind::Del.reverse(), RunKind::Add);
        assert_eq!(RunKind::Ctx.reverse(), RunKind::Ctx);
    }

    #[test]
    fn test_moved_tgt_is_add_like_but_not_add() {
        assert!(RunKind::MovedTgt.is_add_like());
        assert!(RunKind::Add.is_add_like());
        assert!(!RunKind::Ctx.is_add_like());
        assert!(!RunKind::Del.is_add_like());
        assert!(!RunKind::MovedSrc.is_add_like());
        assert_ne!(RunKind::MovedTgt, RunKind::Add);
    }
}
