//! Stage 2: non-monotone residual cross-slot token and frame relocation.
//!
//! Runs strictly after Stage 1's monotone 2D slot projection over the remaining
//! `Del` and `Add` runs, recovering transposed tokens and stranded expression
//! frames (such as an opener `Some(` crossing an inner expression) while
//! preserving closed-economy conservation.

use std::cmp::{Ordering, Reverse};

use diffuse_algo::dyck::{DelimiterCategory, DelimiterClassifier, UniversalDelimiterClassifier};
use diffuse_syntax::profile::LangProfile;

use crate::{
    collapse::{slot::Runs, tokens::is_salient_word_match},
    lcs::{
        DEFAULT_MAX_SLOT_DISTANCE, line_has_salient_token, merged_runs_for_texts_with_profile,
        token_is_salient, tokenize_line, tokenizer::COMMON_KEYWORDS,
    },
    merge::RunKind,
};

/// Relocates matching leftover `Del` / `Add` token spans across slots within
/// [`DEFAULT_MAX_SLOT_DISTANCE`], enforcing contextual affinity, salient-link
/// gating for bare punctuation, single-donor discipline, and an anti-scavenging
/// coverage floor on pure `Add` rows.
pub(crate) fn relocate_cross_slot_tokens(
    slot_runs: &mut [Runs],
    is_moved_slot: &[bool],
    is_pure_add: &[bool],
    profile: Option<&LangProfile>,
) {
    let num_slots = slot_runs.len();
    let mut candidates = Vec::new();

    // 1. Gather Del runs across slots
    let mut del_runs = Vec::new();
    for (slot_idx, runs) in slot_runs.iter().enumerate() {
        if is_moved_slot.get(slot_idx).copied().unwrap_or(false) {
            continue;
        }
        for (kind, text) in runs {
            if *kind == RunKind::Del && !text.trim().is_empty() {
                del_runs.push((slot_idx, text.clone()));
            }
        }
    }

    // 2. Gather Add runs across slots
    let mut add_runs = Vec::new();
    for (slot_idx, runs) in slot_runs.iter().enumerate() {
        if is_moved_slot.get(slot_idx).copied().unwrap_or(false) {
            continue;
        }
        for (kind, text) in runs {
            if *kind == RunKind::Add && !text.trim().is_empty() {
                add_runs.push((slot_idx, text.clone()));
            }
        }
    }

    // 3. For each pair (src, tgt) with src != tgt, find common Ctx spans
    for (src, del_text) in &del_runs {
        for (tgt, add_text) in &add_runs {
            if src == tgt || src.abs_diff(*tgt) > DEFAULT_MAX_SLOT_DISTANCE {
                continue;
            }
            let d_trim = del_text.trim();
            let a_trim = add_text.trim();
            if d_trim.len() >= 3 && add_text.contains(d_trim) {
                candidates.push(d_trim.to_string());
            }
            if a_trim.len() >= 3 && del_text.contains(a_trim) {
                candidates.push(a_trim.to_string());
            }

            if d_trim
                .split_whitespace()
                .any(|w| w.len() >= 3 && a_trim.contains(w))
            {
                let runs = merged_runs_for_texts_with_profile(del_text, add_text, profile);
                let has_non_ws_ctx = runs
                    .iter()
                    .any(|r| r.kind == RunKind::Ctx && !r.text.trim().is_empty());
                if has_non_ws_ctx {
                    for r in runs {
                        if r.kind == RunKind::Ctx {
                            let t = r.text.trim();
                            if t.len() >= 3
                                || (t.len() >= 2 && !t.chars().any(char::is_alphanumeric))
                            {
                                candidates.push(t.to_string());
                            }
                        }
                    }
                }
            }
            // Check container and statement closing delimiters
            for delim in ["};", "{", "}", "(", ")", "[", "]", ";"] {
                if del_text.contains(delim) && add_text.contains(delim) {
                    candidates.push(delim.to_string());
                }
            }
        }
    }

    candidates.sort_by_key(|a| Reverse(a.len()));
    candidates.dedup();

    // Salient candidates relocate freely within the window; delimiter-only
    // candidates (bare punctuation such as `(`, `{`, `;`) travel only on
    // related pairs: either the pair already exchanged salient content
    // (the punctuation rides with its statement's move), or one side's
    // whole line is bare punctuation (a reflowed `)` landing on its own
    // `);` line). A bare bracket shared between two full, unrelated lines
    // is coincidental punctuation reuse, not a move.
    let (salient, bare): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|t| line_has_salient_token(t, profile));
    let mut linked: Vec<(usize, usize)> = Vec::new();
    let mut donors_for_tgt = vec![Vec::<usize>::new(); num_slots];
    let mut exchanges = Vec::<(usize, usize, String)>::new();

    for token in &salient {
        apply_relocation_candidate(
            slot_runs,
            is_moved_slot,
            is_pure_add,
            &mut donors_for_tgt,
            &mut exchanges,
            token,
            None,
            false,
            &mut linked,
            profile,
        );
    }
    for token in &bare {
        apply_relocation_candidate(
            slot_runs,
            is_moved_slot,
            is_pure_add,
            &mut donors_for_tgt,
            &mut exchanges,
            token,
            Some(&linked),
            true,
            &mut Vec::new(),
            profile,
        );
    }

    // 4. Anti-scavenging coverage floor on pure Add rows:
    // Pure Add rows must not scavenge minor scraps (< 35% coverage) that turn
    // clean addition lines into pseudo-modified rows.
    for tgt in 0..num_slots {
        if !is_pure_add.get(tgt).copied().unwrap_or(false) {
            continue;
        }
        let total_new_len: usize = slot_new_text(&slot_runs[tgt]).trim().len();
        if total_new_len == 0 {
            continue;
        }
        let moved_tgt_len: usize = slot_runs[tgt]
            .iter()
            .filter(|(k, _)| *k == RunKind::MovedTgt)
            .map(|(_, t)| t.len())
            .sum();
        let coverage = moved_tgt_len as f64 / total_new_len as f64;
        if coverage < 0.35 && moved_tgt_len > 0 {
            let mut affected_donors = Vec::new();
            for (src_idx, tgt_idx, text) in exchanges.iter() {
                if *tgt_idx == tgt {
                    replace_run_kind(&mut slot_runs[tgt], RunKind::MovedTgt, RunKind::Add, text);
                    replace_run_kind(
                        &mut slot_runs[*src_idx],
                        RunKind::MovedSrc,
                        RunKind::Del,
                        text,
                    );
                    affected_donors.push(*src_idx);
                }
            }
            coalesce_runs(&mut slot_runs[tgt]);
            for src_idx in affected_donors {
                coalesce_runs(&mut slot_runs[src_idx]);
            }
            linked.retain(|&(_, t)| t != tgt);
        }
    }
}

/// Completes stranded expression frames from the leftovers of the earlier
/// passes.
///
/// `relocate_cross_slot_tokens` only proposes verbatim whole-run matches
/// within [`DEFAULT_MAX_SLOT_DISTANCE`], so two kinds of frame part survive it:
/// openers or closers whose partner lives beyond the window (`Some(` at
/// distance 5), and glue whose counterpart run ends in a different
/// terminator (` as usize,` vs ` as usize;`). Both are recovered here by
/// matching leftover tokens by exact identity: `,` and `;` simply never
/// enter a common block, so they stay `Del`/`Add` with no terminator
/// normalization and no language-specific rule.
///
/// Runs strictly last, over the runs the earlier passes stranded, and is
/// gated so earlier adjudications are never second-guessed:
/// - *generic frame vocabulary only*: blocks that carry a domain identifier
///   (`AddLine::with_spans(`, `spans.clone`) stay with affinity/coverage;
/// - *bracket structure on both sides*: the block holds a bracket, or its
///   nearest non-whitespace neighbor in the slot projection does (` as usize`
///   follows `)`);
/// - *jurisdiction*: a bracketed block within relocation's window is
///   relocation's call, declined deliberately;
/// - *distance exemption*: beyond the window only a bracket-unbalanced block
///   (an opener whose closer must live elsewhere) may travel;
/// - *stranded donor*: a donor whose old side already donated belongs to that
///   exchange, not here;
/// - *donor exhaustion*: the `Del` fragment carries the frame alone, with no
///   leftover words beside it;
/// - *uniqueness*: the block occurs exactly once per side, and exactly one slot
///   on each side holds it;
/// - *Mod-anchored recipient*: pure `Add` rows never enter the target pool, so
///   an addition row cannot be scavenged into a faux modification.
pub(crate) fn relocate_leftover_frames(
    slot_runs: &mut [Runs],
    is_moved_slot: &[bool],
    is_pure_add: &[bool],
    profile: Option<&LangProfile>,
) {
    // Per-slot projections of each side, with each run's byte offset into
    // its slot projection. Retyping never alters slot texts, so these are
    // computed once; they locate a frame block's neighbors exactly.
    let dyck_classifier = UniversalDelimiterClassifier;
    let is_bracket = |c: char| {
        matches!(
            dyck_classifier.classify(&c),
            Some(DelimiterCategory::Open(_)) | Some(DelimiterCategory::Close(_))
        )
    };
    let project = |side: [RunKind; 3]| -> (Vec<String>, Vec<Vec<Option<usize>>>) {
        let mut projs = Vec::with_capacity(slot_runs.len());
        let mut offs = Vec::with_capacity(slot_runs.len());
        for runs in slot_runs.iter() {
            let mut proj = String::new();
            let mut run_off = vec![None; runs.len()];
            for (r, (k, t)) in runs.iter().enumerate() {
                if side.contains(k) {
                    run_off[r] = Some(proj.len());
                    proj.push_str(t);
                }
            }
            projs.push(proj);
            offs.push(run_off);
        }
        (projs, offs)
    };
    let (old_proj, old_run_off) = project([RunKind::Del, RunKind::Ctx, RunKind::MovedSrc]);
    let (new_proj, new_run_off) = project([RunKind::Add, RunKind::Ctx, RunKind::MovedTgt]);
    // Iterative longest-common-substring: each accepted block leaves the
    // pools (by retyping) before the next search, so crossed orders (opener
    // early on one side, terminator early on the other) settle one block per
    // round.
    for _ in 0..16 {
        // Rebuilt every round because retyping shifts byte offsets.
        let src = leftover_pool(slot_runs, is_moved_slot, is_pure_add, RunKind::Del);
        let tgt = leftover_pool(slot_runs, is_moved_slot, is_pure_add, RunKind::Add);
        if src.is_empty() || tgt.is_empty() {
            return;
        }
        // Concatenated leftover texts for block-level uniqueness: the
        // joined block must occur exactly once per side. Per-token
        // frequency would reject ` as usize` because `as` also occurs in
        // ` as f64`; the block as a unit is still unique.
        let src_all: String = src.iter().map(|t| t.text.as_str()).collect();
        let tgt_all: String = tgt.iter().map(|t| t.text.as_str()).collect();
        // Candidate longest common contiguous runs sharing one src slot
        // and one tgt slot. Contiguity is slot-level, not run-level: the
        // word diff splits ` as usize;` into ` `, `as `, `usize`, `;`
        // runs (color boundaries), and the frame block spans them.
        // Same-slot pairs are skipped — intra-slot leftovers were already
        // aligned by the word diff. Longest first so the frame block wins
        // over glyph reuse; failed candidates are skipped, not fatal, so
        // a rejected long block (e.g. non-unique) still lets a shorter
        // unique frame (e.g. `Some(`) apply in a later round.
        let mut candidates: Vec<(usize, usize, usize)> = Vec::new();
        for i in 0..src.len() {
            for j in 0..tgt.len() {
                if src[i].text != tgt[j].text || src[i].slot == tgt[j].slot {
                    continue;
                }
                // Extend leftward so every candidate is maximal: a block
                // that can grow backward by one shared token (`Some(` →
                // ` Some(`) would otherwise be recorded only as its
                // shorter suffix and duplicate its parent.
                let mut si = i;
                let mut sj = j;
                while si > 0
                    && sj > 0
                    && src[si - 1].text == tgt[sj - 1].text
                    && src[si - 1].slot == src[i].slot
                    && tgt[sj - 1].slot == tgt[j].slot
                {
                    si -= 1;
                    sj -= 1;
                }
                let mut k = i - si;
                while si + k < src.len()
                    && sj + k < tgt.len()
                    && src[si + k].text == tgt[sj + k].text
                    && src[si + k].slot == src[si].slot
                    && tgt[sj + k].slot == tgt[sj].slot
                {
                    k += 1;
                }
                if k >= 2 {
                    candidates.push((si, sj, k));
                }
            }
        }
        candidates.sort_by(|a, b| {
            b.2.cmp(&a.2).then_with(|| {
                let la: usize = src[a.0..a.0 + a.2].iter().map(|t| t.text.len()).sum();
                let lb: usize = src[b.0..b.0 + b.2].iter().map(|t| t.text.len()).sum();
                lb.cmp(&la)
            })
        });
        candidates.dedup();
        // Accept the longest admissible block, then restart with fresh pools
        // so crossed orders (opener early on one side, terminator early on
        // the other) settle one block per round. A rejected candidate is
        // skipped rather than fatal: a non-unique long block must not
        // suppress a shorter unique frame such as `Some(`.
        let mut applied = false;
        for (bi, bj, blen) in candidates {
            let block_src = &src[bi..bi + blen];
            let block_tgt = &tgt[bj..bj + blen];
            let src_slot = block_src[0].slot;
            let tgt_slot = block_tgt[0].slot;
            let joined: String = block_src.iter().map(|t| t.text.as_str()).collect();
            // Frame blocks carry words: bare delimiter soup (`,`, `;`, `)`),
            // pure numbers, and punctuation-only scraps identify nothing.
            if !joined.chars().any(char::is_alphabetic) {
                continue;
            }
            // Generic vocabulary only: a block carrying a domain identifier
            // (`AddLine::with_spans(`, `spans.clone(`) belongs to affinity
            // and the coverage floor, not to frame completion.
            if !is_generic_subtoken(&joined, profile) {
                continue;
            }
            // Jurisdiction: inside relocation's window a bracketed block was
            // relocation's call — verbatim whole-run matching proposed it and
            // declining it was deliberate (affinity, coverage, single-donor).
            // Only what verbatim matching could not see is completed here:
            // glue whose counterpart ends in a different terminator
            // (` as usize,` vs ` as usize;`), or a frame beyond the window.
            let dist = src_slot.abs_diff(tgt_slot);
            if dist <= DEFAULT_MAX_SLOT_DISTANCE && joined.chars().any(is_bracket) {
                continue;
            }
            // Beyond the window, only a bracket-unbalanced block may travel:
            // an opener whose closer must live in another slot (`Some(`). A
            // balanced far fragment is coincidence, and blindness of
            // relocation is not evidence.
            let externals = |toks: &[LeftoverTok]| {
                let mut depth = 0i32;
                let mut closed_early = false;
                for t in toks {
                    for c in slot_runs[t.slot][t.run].1[t.start..t.end].chars() {
                        match dyck_classifier.classify(&c) {
                            Some(DelimiterCategory::Open(_)) => depth += 1,
                            Some(DelimiterCategory::Close(_)) => {
                                depth -= 1;
                                closed_early |= depth < 0;
                            }
                            _ => {}
                        }
                    }
                }
                depth != 0 || closed_early
            };
            if dist > DEFAULT_MAX_SLOT_DISTANCE && !(externals(block_src) || externals(block_tgt)) {
                continue;
            }
            // Structural frame gate: on each side the block holds a bracket
            // itself, or its nearest non-whitespace neighbor in the slot
            // projection does (` as usize` follows `)`). Whitespace is
            // skipped so indentation never counts as structure; separators
            // never count either, which keeps `,`/`;` glued to their sides.
            let structural = |toks: &[LeftoverTok], old_side: bool| {
                let slot = toks[0].slot;
                if toks.iter().any(|t| {
                    slot_runs[t.slot][t.run].1[t.start..t.end]
                        .chars()
                        .any(is_bracket)
                }) {
                    return true;
                }
                let (proj, run_off) = if old_side {
                    (&old_proj[slot], &old_run_off[slot])
                } else {
                    (&new_proj[slot], &new_run_off[slot])
                };
                let first = &toks[0];
                let last = &toks[toks.len() - 1];
                let (Some(first_off), Some(last_off)) = (run_off[first.run], run_off[last.run])
                else {
                    return false;
                };
                let start = first_off + first.start;
                let end = last_off + last.end;
                let prev = proj[..start].chars().rev().find(|c| !c.is_whitespace());
                let next = proj[end..].chars().find(|c| !c.is_whitespace());
                prev.is_some_and(is_bracket) || next.is_some_and(is_bracket)
            };
            if !(structural(block_src, true) && structural(block_tgt, false)) {
                continue;
            }
            // Donor exhaustion: the stranded `Del` fragment carries the frame
            // alone, with no words left beside it (orphan `Some(`, suffix
            // ` as usize,` before `,`). A donor keeping real content
            // (`.clone()` out of `let mut line = del_line.clone();`) is a
            // scrap of a contentful line, not a stranded frame part. The
            // recipient needs no such test: whatever remains there is
            // genuine addition content (`Some(` out of `Some(prop_thumb`).
            let donor_exhausted = |toks: &[LeftoverTok]| {
                let first = &toks[0];
                let last = &toks[toks.len() - 1];
                let head = &slot_runs[first.slot][first.run].1[..first.start];
                let tail = &slot_runs[last.slot][last.run].1[last.end..];
                !head.chars().chain(tail.chars()).any(char::is_alphanumeric)
            };
            if !donor_exhausted(block_src) {
                continue;
            }
            // Uniqueness: the block as a unit occurs exactly once among the
            // stranded `Del` text and once among the stranded `Add` text, and
            // exactly one slot per side holds it. `)` and `.` fail here;
            // `Some(` and ` as usize` pass 1-to-1.
            if src_all.matches(joined.as_str()).count() != 1
                || tgt_all.matches(joined.as_str()).count() != 1
            {
                continue;
            }
            let mut src_holders = 0usize;
            let mut tgt_holders = 0usize;
            for (idx, runs) in slot_runs.iter().enumerate() {
                if is_moved_slot.get(idx).copied().unwrap_or(false) {
                    continue;
                }
                let side_text = |kind: RunKind| -> String {
                    runs.iter()
                        .filter(|(k, _)| *k == kind)
                        .map(|(_, t)| t.as_str())
                        .collect()
                };
                if side_text(RunKind::Del).contains(joined.as_str()) {
                    src_holders += 1;
                }
                if is_pure_add.get(idx).copied().unwrap_or(false) {
                    continue;
                }
                if side_text(RunKind::Add).contains(joined.as_str()) {
                    tgt_holders += 1;
                }
            }
            if src_holders != 1 || tgt_holders != 1 {
                continue;
            }
            // Mod-anchored recipient: the target slot carries context or an
            // exchange, so it is a paired row rather than an orphan. Pure
            // `Add` rows never reach here (excluded from the pool above), so
            // an addition row can never be scavenged into a faux modification.
            let tgt_anchored = slot_runs[tgt_slot]
                .iter()
                .any(|(k, _)| matches!(k, RunKind::Ctx | RunKind::MovedSrc | RunKind::MovedTgt));
            if !tgt_anchored {
                continue;
            }
            // Donor stranded: a donor whose old side already donated was
            // adjudicated by relocation, and its remaining fragments are the
            // residue of that decision (`.any(|r| ` beside an already-moved
            // `matches!(`). Receiving does not disqualify a donor: group 6's
            // slot 5 hosts the moved `)` and still owns ` as usize`.
            if slot_runs[src_slot]
                .iter()
                .any(|(k, _)| *k == RunKind::MovedSrc)
            {
                continue;
            }
            // Retype only after proving exact byte-for-byte identity of the
            // two spans, so conservation holds pairwise and `,`/`;` never
            // ride along.
            let src_spans = run_spans_of(block_src, &slot_runs[src_slot], RunKind::Del);
            let tgt_spans = run_spans_of(block_tgt, &slot_runs[tgt_slot], RunKind::Add);
            let (Some(src_spans), Some(tgt_spans)) = (src_spans, tgt_spans) else {
                continue;
            };
            let src_text: String = src_spans
                .iter()
                .map(|&(r, s, e)| &slot_runs[src_slot][r].1[s..e])
                .collect();
            let tgt_text: String = tgt_spans
                .iter()
                .map(|&(r, s, e)| &slot_runs[tgt_slot][r].1[s..e])
                .collect();
            if src_text != tgt_text {
                continue;
            }
            applied = true;
            // Bottom-up so earlier byte offsets stay valid.
            for &(r, s, e) in tgt_spans.iter().rev() {
                let piece = slot_runs[tgt_slot][r].1[s..e].to_string();
                replace_span_in_run(&mut slot_runs[tgt_slot], r, RunKind::MovedTgt, s, e, piece);
            }
            for &(r, s, e) in src_spans.iter().rev() {
                let piece = slot_runs[src_slot][r].1[s..e].to_string();
                replace_span_in_run(&mut slot_runs[src_slot], r, RunKind::MovedSrc, s, e, piece);
            }
            break;
        }
        if !applied {
            return;
        }
    }
}

/// One leftover token with provenance into its run string.
#[derive(Clone, Debug)]
struct LeftoverTok {
    slot: usize,
    run: usize,
    start: usize,
    end: usize,
    text: String,
}

/// Tokenize a run string with the shared line subtokenizer, preserving
/// byte offsets for later span retyping.
fn tokenize_run_spans(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    for tok in tokenize_line(text) {
        // `LineTokenizer` yields borrowed slices of `text`: recover byte
        // offsets via pointer arithmetic (same string, same lifetime).
        let start = (tok.as_ptr() as usize).saturating_sub(text.as_ptr() as usize);
        out.push((start, start + tok.len(), tok.to_string()));
    }
    out
}

/// Groups a contiguous token block into per-run byte spans, verifying each
/// covered run carries the expected kind. Returns `None` when the block
/// reaches a run of another kind (the tokens are then not a clean frame
/// slice of the projection).
fn run_spans_of(
    toks: &[LeftoverTok],
    runs: &[(RunKind, String)],
    kind: RunKind,
) -> Option<Vec<(usize, usize, usize)>> {
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    for t in toks {
        if runs.get(t.run).map(|(k, _)| *k) != Some(kind) {
            return None;
        }
        match spans.last_mut() {
            Some(last) if last.0 == t.run => last.2 = t.end,
            Some(last) if t.run < last.0 => return None,
            _ => spans.push((t.run, t.start, t.end)),
        }
    }
    Some(spans)
}

/// Collects the leftover tokens of one kind, in slot order, with provenance
/// into their run strings.
///
/// Moved slots were adjudicated elsewhere, and pure `Add` rows are barred
/// from both pools: as recipients that keeps an addition row from being
/// scavenged into a faux modification, and as donors they hold no `Del` text
/// anyway.
fn leftover_pool(
    slot_runs: &[Runs],
    is_moved_slot: &[bool],
    is_pure_add: &[bool],
    want: RunKind,
) -> Vec<LeftoverTok> {
    let mut toks = Vec::new();
    for (slot, runs) in slot_runs.iter().enumerate() {
        if is_moved_slot.get(slot).copied().unwrap_or(false)
            || is_pure_add.get(slot).copied().unwrap_or(false)
        {
            continue;
        }
        for (run, (kind, text)) in runs.iter().enumerate() {
            if *kind != want || text.trim().is_empty() {
                continue;
            }
            for (start, end, text) in tokenize_run_spans(text) {
                toks.push(LeftoverTok {
                    slot,
                    run,
                    start,
                    end,
                    text,
                });
            }
        }
    }
    toks
}

/// Extracts text belonging to the pre-image (old side): Del, Ctx, and MovedSrc
/// runs.
fn slot_old_text(runs: &[(RunKind, String)]) -> String {
    runs.iter()
        .filter(|(k, _)| *k == RunKind::Del || *k == RunKind::Ctx || *k == RunKind::MovedSrc)
        .map(|(_, t)| t.as_str())
        .collect()
}

/// Extracts text belonging to the post-image (new side): Add, Ctx, and MovedTgt
/// runs.
fn slot_new_text(runs: &[(RunKind, String)]) -> String {
    runs.iter()
        .filter(|(k, _)| *k == RunKind::Add || *k == RunKind::Ctx || *k == RunKind::MovedTgt)
        .map(|(_, t)| t.as_str())
        .collect()
}

/// Whether a pre-image line terminates a statement or block (`);`, `{`, `}`).
fn is_closed_statement_line(text: &str) -> bool {
    let t = text.trim();
    t.ends_with(';') || t.ends_with('{') || t.ends_with('}')
}

/// Extracts words of length >= 3 for contextual affinity scoring.
fn contextual_words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(str::trim)
        .filter(|w| w.len() >= 3 && w.chars().any(char::is_alphabetic))
        .collect()
}

/// Computes distance-decayed contextual affinity between source slot `src` and
/// candidate target slot `tgt` for relocating `token`.
fn compute_contextual_affinity(
    src: usize,
    tgt: usize,
    token: &str,
    slot_runs: &[Runs],
    _profile: Option<&LangProfile>,
) -> f64 {
    let src_text = slot_old_text(&slot_runs[src]);
    let tgt_text = slot_new_text(&slot_runs[tgt]);

    let src_words = contextual_words(&src_text);
    let tgt_words = contextual_words(&tgt_text);
    let token_words = contextual_words(token);

    // Direct shared salient characters outside of the relocated token
    let direct_shared_chars: usize = src_words
        .iter()
        .filter(|sw| !token_words.iter().any(|tw| is_salient_word_match(sw, tw)))
        .filter(|sw| tgt_words.iter().any(|tw| is_salient_word_match(sw, tw)))
        .map(|w| w.len())
        .sum();

    // Adjacent operand bonus: if a neighboring pre-image line in the reflow
    // block shares operands with the post-image target (e.g. multi-line
    // expression whose operands precede or follow the line carrying the method
    // call/token). The neighbor must be an unclosed continuation line (not
    // terminated by `;`, `{`, or `}`).
    let mut neighbor_shared_chars: usize = 0;
    let num_slots = slot_runs.len();
    let neighbors = [
        src.checked_sub(1),
        if src + 1 < num_slots {
            Some(src + 1)
        } else {
            None
        },
    ];
    for adj in neighbors.into_iter().flatten() {
        let adj_text = slot_old_text(&slot_runs[adj]);
        let is_continuation = if adj < src {
            !is_closed_statement_line(&adj_text)
        } else {
            !is_closed_statement_line(&src_text)
        };
        if is_continuation {
            let adj_words = contextual_words(&adj_text);
            let shared: usize = adj_words
                .iter()
                .filter(|aw| !token_words.iter().any(|tw| is_salient_word_match(aw, tw)))
                .filter(|aw| tgt_words.iter().any(|tw| is_salient_word_match(aw, tw)))
                .map(|w| w.len())
                .sum();
            if shared > neighbor_shared_chars {
                neighbor_shared_chars = shared;
            }
        }
    }

    let distance = (src as isize - tgt as isize).unsigned_abs() as f64;
    let score =
        token.len() as f64 + 2.0 * direct_shared_chars as f64 + 1.5 * neighbor_shared_chars as f64;
    let distance_decay = 1.0 / (1.0 + 0.5 * distance);
    score * distance_decay
}

/// Whether a slot side carries nothing but punctuation/whitespace: every
/// run of the given kind is free of alphanumeric characters and at least
/// one run has non-whitespace content (a bare `);` or `{` line).
fn slot_side_is_bare(runs: &[(RunKind, String)], kind: RunKind) -> bool {
    let mut has_text = false;
    for (k, t) in runs {
        if *k != kind {
            continue;
        }
        if t.chars().any(char::is_alphanumeric) {
            return false;
        }
        if !t.trim().is_empty() {
            has_text = true;
        }
    }
    has_text
}

/// Whether a candidate token is a generic sub-token (primitive type, common
/// short method call <= 8 chars, common literal, or short keyword construct).
/// Generic sub-tokens require explicit contextual evidence (a mutual anchor or
/// paired Mod alignment) to travel beyond immediate neighbor slots.
fn is_generic_subtoken(token: &str, profile: Option<&LangProfile>) -> bool {
    let t = token.trim();
    if t.is_empty() {
        return true;
    }
    // Method call <= 8 chars without underscore: e.g. .round(), .len(),
    // .clone()
    if t.starts_with('.') && t.len() <= 8 && !t.contains('_') {
        return true;
    }
    const PRIMITIVES_AND_LITERALS: &[&str] = &[
        "u8",
        "u16",
        "u32",
        "u64",
        "u128",
        "usize",
        "i8",
        "i16",
        "i32",
        "i64",
        "i128",
        "isize",
        "f32",
        "f64",
        "bool",
        "char",
        "str",
        "String",
        "int",
        "float",
        "None",
        "Some",
        "Ok",
        "Err",
        "true",
        "false",
        "null",
        "nil",
        "undefined",
    ];
    let words = contextual_words(t);
    if words.is_empty() {
        return true;
    }
    words.iter().all(|&w| {
        COMMON_KEYWORDS.contains(&w)
            || PRIMITIVES_AND_LITERALS.contains(&w)
            || profile.is_some_and(|p| p.is_non_salient(w))
            || (w.len() <= 4 && !w.contains('_'))
    })
}

fn replace_run_kind(runs: &mut [(RunKind, String)], from: RunKind, to: RunKind, text: &str) {
    for (k, t) in runs.iter_mut() {
        if *k == from && t == text {
            *k = to;
            break;
        }
    }
}

fn coalesce_runs(runs: &mut Runs) {
    if runs.len() <= 1 {
        return;
    }
    let mut coalesced: Runs = Vec::with_capacity(runs.len());
    for (k, t) in runs.drain(..) {
        if let Some((last_k, last_t)) = coalesced.last_mut()
            && *last_k == k
        {
            last_t.push_str(&t);
        } else {
            coalesced.push((k, t));
        }
    }
    *runs = coalesced;
}

/// Relocates one candidate token from Del runs to Add runs across slot
/// pairs within [`DEFAULT_MAX_SLOT_DISTANCE`]. When `gate` is `Some`, only
/// pairs in it may exchange the token — unless `allow_bare_lines` and one
/// side's whole line is bare punctuation (see above). Pairs that actually
/// exchange the token are appended to `linked`.
#[allow(clippy::too_many_arguments)]
fn apply_relocation_candidate(
    slot_runs: &mut [Runs],
    is_moved_slot: &[bool],
    is_pure_add: &[bool],
    donors_for_tgt: &mut [Vec<usize>],
    exchanges: &mut Vec<(usize, usize, String)>,
    token: &str,
    gate: Option<&[(usize, usize)]>,
    allow_bare_lines: bool,
    linked: &mut Vec<(usize, usize)>,
    profile: Option<&LangProfile>,
) {
    let num_slots = slot_runs.len();
    let is_weak = is_generic_subtoken(token, profile);

    for src in 0..num_slots {
        if is_moved_slot.get(src).copied().unwrap_or(false) {
            continue;
        }
        let mut candidate_targets = Vec::new();
        for tgt in 0..num_slots {
            if src == tgt || is_moved_slot.get(tgt).copied().unwrap_or(false) {
                continue;
            }
            // Bare candidate gating:
            if let Some(g) = gate
                && !g.contains(&(src, tgt))
                && !(allow_bare_lines
                    && (slot_side_is_bare(&slot_runs[src], RunKind::Del)
                        || slot_side_is_bare(&slot_runs[tgt], RunKind::Add)))
            {
                continue;
            }

            // Evidence and travel radius check for salient candidates:
            if gate.is_none() {
                let has_linked_anchor = linked.iter().any(|&(s, t)| {
                    t == tgt && (s == src || (s as isize - src as isize).abs() <= 1)
                });
                let is_paired_mod = !is_pure_add.get(tgt).copied().unwrap_or(false);
                let has_mutual_salient_anchor = {
                    let src_text = slot_old_text(&slot_runs[src]);
                    let tgt_text = slot_new_text(&slot_runs[tgt]);
                    let src_words = contextual_words(&src_text);
                    let tgt_words = contextual_words(&tgt_text);
                    let token_words = contextual_words(token);
                    src_words.iter().any(|sw| {
                        token_is_salient(sw, profile)
                            && !token_words.iter().any(|tw| is_salient_word_match(sw, tw))
                            && tgt_words.iter().any(|tw| is_salient_word_match(sw, tw))
                    })
                };
                let has_evidence = has_linked_anchor || is_paired_mod || has_mutual_salient_anchor;

                let max_distance = if !is_weak || has_evidence {
                    DEFAULT_MAX_SLOT_DISTANCE
                } else {
                    1
                };
                if src.abs_diff(tgt) > max_distance {
                    continue;
                }
            } else if src.abs_diff(tgt) > DEFAULT_MAX_SLOT_DISTANCE {
                continue;
            }

            // Single-donor rule on pure Add rows:
            if is_pure_add.get(tgt).copied().unwrap_or(false) {
                let existing_donors = &donors_for_tgt[tgt];
                if !existing_donors.is_empty()
                    && !existing_donors
                        .iter()
                        .any(|&d| (src as isize - d as isize).abs() <= 1)
                {
                    continue;
                }
            }

            if find_matching_pair(&slot_runs[src], &slot_runs[tgt], token).is_some() {
                let affinity = compute_contextual_affinity(src, tgt, token, slot_runs, profile);
                candidate_targets.push((tgt, affinity));
            }
        }

        // Sort candidate targets by descending contextual affinity.
        // In case of ties, prefer closer distance, then lower tgt index.
        candidate_targets.sort_by(|&(t1, a1), &(t2, a2)| {
            a2.partial_cmp(&a1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| {
                    let d1 = (src as isize - t1 as isize).unsigned_abs();
                    let d2 = (src as isize - t2 as isize).unsigned_abs();
                    d1.cmp(&d2)
                })
                .then_with(|| t1.cmp(&t2))
        });

        for (tgt, _) in candidate_targets {
            let mut moved = false;
            while let Some((d_run_idx, d_pos, a_run_idx, a_pos)) =
                find_matching_pair(&slot_runs[src], &slot_runs[tgt], token)
            {
                let (start_d, end_d, start_a, end_a) = expand_match(
                    &slot_runs[src][d_run_idx].1,
                    d_pos,
                    &slot_runs[tgt][a_run_idx].1,
                    a_pos,
                    token.len(),
                );

                let matched_text = slot_runs[src][d_run_idx].1[start_d..end_d].to_string();

                replace_span_in_run(
                    &mut slot_runs[src],
                    d_run_idx,
                    RunKind::MovedSrc,
                    start_d,
                    end_d,
                    matched_text.clone(),
                );
                replace_span_in_run(
                    &mut slot_runs[tgt],
                    a_run_idx,
                    RunKind::MovedTgt,
                    start_a,
                    end_a,
                    matched_text.clone(),
                );
                exchanges.push((src, tgt, matched_text));
                if is_pure_add.get(tgt).copied().unwrap_or(false)
                    && !donors_for_tgt[tgt].contains(&src)
                {
                    donors_for_tgt[tgt].push(src);
                }
                moved = true;
            }
            if moved && !linked.contains(&(src, tgt)) {
                linked.push((src, tgt));
            }
        }
    }
}

fn find_matching_pair(
    src_runs: &[(RunKind, String)],
    tgt_runs: &[(RunKind, String)],
    token: &str,
) -> Option<(usize, usize, usize, usize)> {
    for (d_i, (d_k, d_s)) in src_runs.iter().enumerate() {
        if *d_k == RunKind::Del
            && let Some(d_pos) = d_s.find(token)
        {
            for (a_i, (a_k, a_s)) in tgt_runs.iter().enumerate() {
                if *a_k == RunKind::Add
                    && let Some(a_pos) = a_s.find(token)
                {
                    return Some((d_i, d_pos, a_i, a_pos));
                }
            }
        }
    }
    None
}

/// Expands a candidate token match naturally in both directions (including
/// whitespace and delimiters).
///
/// Guard: does not expand backward into pure leading indentation of a line.
fn expand_match(
    del_str: &str,
    d_pos: usize,
    add_str: &str,
    a_pos: usize,
    token_len: usize,
) -> (usize, usize, usize, usize) {
    let before_d = &del_str[..d_pos];
    let before_a = &add_str[..a_pos];
    let mut expand_left = 0;
    if !before_d.trim().is_empty() && !before_a.trim().is_empty() {
        for (cd, ca) in before_d.chars().rev().zip(before_a.chars().rev()) {
            if cd == ca {
                expand_left += cd.len_utf8();
            } else {
                break;
            }
        }
    }

    let after_d = &del_str[d_pos + token_len..];
    let after_a = &add_str[a_pos + token_len..];
    let mut expand_right = 0;
    for (cd, ca) in after_d.chars().zip(after_a.chars()) {
        if cd == ca {
            expand_right += cd.len_utf8();
        } else {
            break;
        }
    }

    let mut start_d = d_pos - expand_left;
    let mut end_d = d_pos + token_len + expand_right;
    let mut start_a = a_pos - expand_left;
    let mut end_a = a_pos + token_len + expand_right;

    // Do not slice across alphanumeric word boundaries: if expanding left or
    // right halted inside an alphanumeric token, roll back to the token
    // boundary.
    while start_d < d_pos
        && start_d > 0
        && del_str[..start_d]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        && del_str[start_d..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        let ch = del_str[start_d..].chars().next().unwrap();
        start_d += ch.len_utf8();
        start_a += ch.len_utf8();
    }

    while end_d > d_pos + token_len
        && end_d < del_str.len()
        && del_str[..end_d]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        && del_str[end_d..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        let ch = del_str[..end_d].chars().next_back().unwrap();
        end_d -= ch.len_utf8();
        end_a -= ch.len_utf8();
    }

    (start_d, end_d, start_a, end_a)
}

fn replace_span_in_run(
    runs: &mut Runs,
    run_idx: usize,
    replacement_kind: RunKind,
    start: usize,
    end: usize,
    matched_text: String,
) {
    let (target_kind, full) = &runs[run_idx];
    let target_kind = *target_kind;
    let mut new_pieces = Vec::with_capacity(3);
    if start > 0 {
        new_pieces.push((target_kind, full[..start].to_string()));
    }
    new_pieces.push((replacement_kind, matched_text));
    if end < full.len() {
        new_pieces.push((target_kind, full[end..].to_string()));
    }

    runs.splice(run_idx..=run_idx, new_pieces);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_slot_relocation_respects_distance_window() {
        // The `alpha` candidate is legitimately shared by the adjacent
        // slots 0/1, but slots 2 and 7 sit 5 apart: typing their token
        // as a move steals it from far away. The apply loop must honor
        // the same ±4 window the candidate gather uses.
        let mut slot_runs: Vec<Runs> = vec![
            vec![
                (RunKind::Del, "aaa alpha".to_string()),
                (RunKind::Add, "bbb".to_string()),
            ],
            vec![
                (RunKind::Del, "www".to_string()),
                (RunKind::Add, "eee alpha".to_string()),
            ],
            vec![(RunKind::Del, "xxx alpha yyy".to_string())],
            vec![(RunKind::Add, "kilo".to_string())],
            vec![(RunKind::Add, "lima".to_string())],
            vec![(RunKind::Del, "mike".to_string())],
            vec![(RunKind::Del, "november".to_string())],
            vec![(RunKind::Add, "zzz alpha www".to_string())],
        ];
        let is_moved_slot = vec![false; 8];
        let is_pure_add = vec![false; 8];
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, None);
        // The nearby pair still relocates.
        assert!(
            slot_runs[0]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t.contains("alpha")),
            "nearby Del token must relocate, got: {:?}",
            slot_runs[0]
        );
        assert!(
            slot_runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t.contains("alpha")),
            "nearby Add token must relocate, got: {:?}",
            slot_runs[1]
        );
        // The far pair must not: distance 5 exceeds the window.
        assert!(
            slot_runs[2].iter().all(|(k, _)| *k != RunKind::MovedSrc),
            "far Del token must stay Del, got: {:?}",
            slot_runs[2]
        );
        assert!(
            slot_runs[7].iter().all(|(k, _)| *k != RunKind::MovedTgt),
            "far Add token must stay Add, got: {:?}",
            slot_runs[7]
        );
    }

    #[test]
    fn test_bare_delimiter_piggybacks_on_salient_exchange() {
        // A bare `(` relocates between slots 0/1 because that pair also
        // exchanges the salient `alpha` — but the same bracket shared by
        // the unrelated slots 4/5 (no salient link) must stay Del/Add.
        // This is the `self.on_scrollbar_click(row, col);` case: parens
        // from a far-away, unrelated line must not render as moved.
        let mut slot_runs: Vec<Runs> = vec![
            vec![
                (RunKind::Del, "call alpha (x".to_string()),
                (RunKind::Add, "nothing here".to_string()),
            ],
            vec![
                (RunKind::Del, "other".to_string()),
                (RunKind::Add, "use alpha (y".to_string()),
            ],
            vec![(RunKind::Del, "zzz ( qqq".to_string())],
            vec![(RunKind::Add, "www ( vvv".to_string())],
        ];
        let is_moved_slot = vec![false; 4];
        let is_pure_add = vec![false; 4];
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, None);
        // Salient exchange links slots 0/1, so the bracket travels with
        // it (inside the shared `alpha (` span).
        assert!(
            slot_runs[0]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t.contains('(')),
            "linked Del bracket must relocate, got: {:?}",
            slot_runs[0]
        );
        assert!(
            slot_runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t.contains('(')),
            "linked Add bracket must relocate, got: {:?}",
            slot_runs[1]
        );
        // No salient link between slots 2/3, and both lines carry real
        // content: brackets stay put.
        assert!(
            slot_runs[2].iter().all(|(k, _)| *k != RunKind::MovedSrc),
            "unrelated Del bracket must stay Del, got: {:?}",
            slot_runs[2]
        );
        assert!(
            slot_runs[3].iter().all(|(k, _)| *k != RunKind::MovedTgt),
            "unrelated Add bracket must stay Add, got: {:?}",
            slot_runs[3]
        );
    }

    #[test]
    fn test_bare_delimiter_may_land_on_bare_line() {
        // A bare `[` may still travel without any salient link when the
        // recipient line is itself bare punctuation (a reflowed bracket
        // landing on its own line) — the `);` / `{`-line cases.
        let mut slot_runs: Vec<Runs> = vec![
            vec![(RunKind::Del, "qux = wod [ gamma".to_string())],
            vec![(RunKind::Add, "[]".to_string())],
        ];
        let is_moved_slot = vec![false; 2];
        let is_pure_add = vec![false, true];
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, None);
        assert!(
            slot_runs[0]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t.contains('[')),
            "donor bracket must relocate to bare line, got: {:?}",
            slot_runs[0]
        );
        assert!(
            slot_runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t.contains('[')),
            "bare recipient line must receive bracket, got: {:?}",
            slot_runs[1]
        );
    }

    #[test]
    fn test_competing_destination_affinity_relocates_to_highest_context_recipient() {
        let profile = diffuse_syntax::registry::lang_profile("rs");
        let mut slot_runs: Vec<Runs> = vec![
            vec![(
                RunKind::Del,
                "((vh as f64 / total_vis_lines as f64) * track_h as f64)".to_string(),
            )],
            vec![(RunKind::Del, "    .round()".to_string())],
            vec![(
                RunKind::Add,
                "let min_thumb = ((track_h as f64 * Self::SCROLLBAR_MIN_PERCENT).round() as usize)"
                    .to_string(),
            )],
            vec![(
                RunKind::Add,
                "let prop_thumb = ((vh as f64 / total_vis_lines as f64) * track_h as f64).round() as usize;"
                    .to_string(),
            )],
        ];
        let is_moved_slot = vec![false; 4];
        let is_pure_add = vec![false; 4];
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);

        assert!(
            !slot_runs[2]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t.contains("round")),
            "min_thumb at slot 2 must NOT steal .round() due to lower affinity, got: {:?}",
            slot_runs[2]
        );

        assert!(
            slot_runs[3]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t.contains("round")),
            "prop_thumb at slot 3 must receive .round() due to higher contextual affinity, got: {:?}",
            slot_runs[3]
        );
    }

    #[test]
    fn test_weak_salient_token_cannot_jump_across_unrelated_distant_slots() {
        let profile = diffuse_syntax::registry::lang_profile("rs");
        // Slot 0 (Del): old_var = None;
        // Slot 1 (Del): middle_line();
        // Slot 2 (Del): other_stuff();
        // Slot 3 (Add): new_line = None;
        let mut slot_runs: Vec<Runs> = vec![
            vec![(RunKind::Del, "let old_var = None;".to_string())],
            vec![(RunKind::Del, "middle_line();".to_string())],
            vec![(RunKind::Del, "other_stuff();".to_string())],
            vec![(RunKind::Add, "let new_feature = None;".to_string())],
        ];
        let is_moved_slot = vec![false; 4];
        let is_pure_add = vec![false, false, false, true];
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);

        assert!(
            !slot_runs[3]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t.contains("None")),
            "distant weak token 'None' must NOT relocate across 3 unrelated slots, got: {:?}",
            slot_runs[3]
        );
    }

    #[test]
    fn test_pure_add_row_anti_scavenging_floor_rejects_minor_scraps() {
        let profile = diffuse_syntax::registry::lang_profile("rs");
        // Slot 0 (Del): let x = calculate();
        // Slot 1 (Add): let
        // very_long_brand_new_statement_with_many_tokens_and_args =
        // calculate();
        let mut slot_runs: Vec<Runs> = vec![
            vec![(RunKind::Del, "let x = calculate();".to_string())],
            vec![(
                RunKind::Add,
                "let very_long_brand_new_statement_with_many_tokens_and_args = calculate();"
                    .to_string(),
            )],
        ];
        let is_moved_slot = vec![false; 2];
        let is_pure_add = vec![false, true];
        // calculate() is ~11 chars out of 76 chars (14% < 35% threshold)
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);

        assert!(
            !slot_runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t.contains("calculate")),
            "pure Add row with only 14% coverage must reject minor scrap, got: {:?}",
            slot_runs[1]
        );
    }

    #[test]
    fn test_pure_add_row_single_donor_rule_rejects_disjoint_donors() {
        let profile = diffuse_syntax::registry::lang_profile("rs");
        // Slot 0 (Del): let alpha = compute_alpha_channel_value();
        // Slot 1 (Del): unrelated_middle_line();
        // Slot 2 (Del): let beta = compute_beta_channel_value();
        // Slot 3 (Add): let combined = compute_alpha_channel_value() +
        // compute_beta_channel_value();
        let mut slot_runs: Vec<Runs> = vec![
            vec![(
                RunKind::Del,
                "let alpha = compute_alpha_channel_value();".to_string(),
            )],
            vec![(RunKind::Del, "unrelated_middle_line();".to_string())],
            vec![(
                RunKind::Del,
                "let beta = compute_beta_channel_value();".to_string(),
            )],
            vec![(
                RunKind::Add,
                "let combined = compute_alpha_channel_value() + compute_beta_channel_value();"
                    .to_string(),
            )],
        ];
        let is_moved_slot = vec![false; 4];
        let is_pure_add = vec![false, false, false, true];
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);

        // Under single-donor rule: pure Add slot 3 cannot accept tokens from
        // both slot 0 and slot 2 (which are disjoint). It may accept from slot
        // 0 OR slot 2, but NOT both!
        let has_alpha = slot_runs[3]
            .iter()
            .any(|(k, t)| *k == RunKind::MovedTgt && t.contains("compute_alpha"));
        let has_beta = slot_runs[3]
            .iter()
            .any(|(k, t)| *k == RunKind::MovedTgt && t.contains("compute_beta"));
        assert!(
            !(has_alpha && has_beta),
            "pure Add slot 3 must not scavenge from disjoint donors (slot 0 and slot 2), got: {:?}",
            slot_runs[3]
        );
    }

    #[test]
    fn test_leftover_frame_two_candidate_weak_token_abstains() {
        // Phase-3 preservation: a weak token with two eligible Mod
        // recipients is ambiguous — affinity owns it, the leftover pass
        // must not grab it for the first slot in group order.
        let mut slot_runs: Vec<Runs> = vec![
            vec![(RunKind::Del, "alpha = 1;".to_string())],
            vec![
                (RunKind::Ctx, "keep".to_string()),
                (RunKind::Add, " alpha;".to_string()),
            ],
            vec![
                (RunKind::Ctx, "keep".to_string()),
                (RunKind::Add, " alpha;".to_string()),
            ],
        ];
        let is_moved_slot = vec![false; 3];
        let is_pure_add = vec![false; 3];
        relocate_leftover_frames(&mut slot_runs, &is_moved_slot, &is_pure_add, None);
        assert!(
            slot_runs[0].iter().all(|(k, _)| *k != RunKind::MovedSrc),
            "two-candidate donor must abstain, got: {:?}",
            slot_runs[0]
        );
        assert!(
            slot_runs[1].iter().all(|(k, _)| *k != RunKind::MovedTgt),
            "first of two recipients must not steal, got: {:?}",
            slot_runs[1]
        );
        assert!(
            slot_runs[2].iter().all(|(k, _)| *k != RunKind::MovedTgt),
            "second of two recipients must not steal, got: {:?}",
            slot_runs[2]
        );
    }

    /// Concatenated text of all runs of one kind: retyping splits a block
    /// across the word diff's color runs, so a moved frame is one logical
    /// span spread over several runs.
    fn kind_text(runs: &[(RunKind, String)], kind: RunKind) -> String {
        runs.iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, t)| t.as_str())
            .collect()
    }

    /// Runs of `scrollbar-grab-drag` group 6 transcribed from the golden wire
    /// output (file `app/geometry.rs` hunk 0, rows 26-32), after earlier
    /// relocation has already placed `.round()` and the stray `)`.
    fn scrollbar_group_6_runs() -> Vec<Runs> {
        let r = |k: RunKind, t: &str| (k, t.to_string());
        vec![
            // slot 0: old231 orphan `Some(`
            vec![r(RunKind::Del, "        Some(")],
            // slot 1: new241 pure addition
            vec![
                r(RunKind::Add, "        "),
                r(RunKind::Add, "let"),
                r(RunKind::Add, " min_thumb "),
                r(RunKind::Add, "= (("),
                r(RunKind::Add, "track_h "),
                r(RunKind::Add, "as "),
                r(RunKind::Add, "f64 "),
                r(RunKind::Add, "* "),
                r(RunKind::Add, "Self"),
                r(RunKind::Add, "::"),
                r(RunKind::Add, "SCROLLBAR_MIN_PERCENT"),
                r(RunKind::Add, ")."),
                r(RunKind::Add, "round"),
                r(RunKind::Add, "() "),
                r(RunKind::Add, "as "),
                r(RunKind::Add, "usize"),
                r(RunKind::Add, ")"),
            ],
            // slot 2: new242 pure addition
            vec![
                r(RunKind::Add, "            "),
                r(RunKind::Add, "."),
                r(RunKind::Add, "clamp"),
                r(RunKind::Add, "("),
                r(RunKind::Add, "Self"),
                r(RunKind::Add, "::"),
                r(RunKind::Add, "SCROLLBAR_MIN_THUMB_LINES"),
                r(RunKind::Add, ","),
                r(RunKind::Add, " max_thumb"),
                r(RunKind::Add, ");"),
            ],
            // slot 3: old232 -> new243
            vec![
                r(RunKind::Del, "    "),
                r(RunKind::Ctx, "        "),
                r(RunKind::Add, "let"),
                r(RunKind::Ctx, " prop_thumb "),
                r(RunKind::Ctx, "= "),
                r(RunKind::Ctx, "(("),
                r(RunKind::Ctx, "vh "),
                r(RunKind::Ctx, "as "),
                r(RunKind::Ctx, "f64 "),
                r(RunKind::Ctx, "/"),
                r(RunKind::Ctx, " total_vis_lines "),
                r(RunKind::Ctx, "as "),
                r(RunKind::Ctx, "f64"),
                r(RunKind::Ctx, ") *"),
                r(RunKind::Ctx, " track_h "),
                r(RunKind::Ctx, "as "),
                r(RunKind::Ctx, "f64"),
                r(RunKind::Ctx, ")"),
                r(RunKind::MovedTgt, "."),
                r(RunKind::MovedTgt, "round"),
                r(RunKind::MovedTgt, "()"),
                r(RunKind::Add, " "),
                r(RunKind::Add, "as "),
                r(RunKind::Add, "usize"),
                r(RunKind::Add, ";"),
            ],
            // slot 4: old233 `.round()` already relocated
            vec![
                r(RunKind::Del, "                "),
                r(RunKind::MovedSrc, ".round()"),
            ],
            // slot 5: old234 -> new244
            vec![
                r(RunKind::Del, "        "),
                r(RunKind::Ctx, "        "),
                r(RunKind::Add, "Some"),
                r(RunKind::Add, "("),
                r(RunKind::Add, "prop_thumb"),
                r(RunKind::Ctx, " "),
                r(RunKind::Ctx, "."),
                r(RunKind::Ctx, "clamp"),
                r(RunKind::Ctx, "("),
                r(RunKind::Del, "2.0"),
                r(RunKind::Add, "min_thumb"),
                r(RunKind::Ctx, " "),
                r(RunKind::Ctx, ","),
                r(RunKind::Ctx, " max_thumb"),
                r(RunKind::Del, " as f64"),
                r(RunKind::MovedTgt, ")"),
                r(RunKind::Ctx, ")"),
                r(RunKind::Del, " as usize,"),
            ],
            // slot 6: old235 stray `)` already relocated
            vec![r(RunKind::Del, "        "), r(RunKind::MovedSrc, ")")],
        ]
    }

    #[test]
    fn test_leftover_frame_completes_scrollbar_group_6() {
        let mut slot_runs = scrollbar_group_6_runs();
        let is_moved_slot = vec![false; 7];
        // new241 / new242 are pure additions.
        let is_pure_add = vec![false, true, true, false, false, false, false];
        relocate_leftover_frames(&mut slot_runs, &is_moved_slot, &is_pure_add, None);

        // Opener: slot 0 `Some(` -> slot 5 `Some(` despite distance 5.
        assert_eq!(
            kind_text(&slot_runs[0], RunKind::MovedSrc),
            "Some(",
            "orphan `Some(` must become MovedSrc, got: {:?}",
            slot_runs[0]
        );
        assert!(
            kind_text(&slot_runs[5], RunKind::MovedTgt).starts_with("Some("),
            "slot 5 must receive `Some(` as MovedTgt, got: {:?}",
            slot_runs[5]
        );
        // ` as usize` block moves; `,`/`;` stay unmatched.
        assert!(
            kind_text(&slot_runs[5], RunKind::MovedSrc).ends_with("as usize"),
            "` as usize` must become MovedSrc in slot 5, got: {:?}",
            slot_runs[5]
        );
        assert!(
            slot_runs[5]
                .iter()
                .any(|(k, t)| *k == RunKind::Del && t == ","),
            "slot 5 `,` must stay Del, got: {:?}",
            slot_runs[5]
        );
        assert!(
            kind_text(&slot_runs[3], RunKind::MovedTgt).ends_with("as usize"),
            "` as usize` must become MovedTgt in slot 3, got: {:?}",
            slot_runs[3]
        );
        assert!(
            slot_runs[3]
                .iter()
                .any(|(k, t)| *k == RunKind::Add && t == ";"),
            "slot 3 `;` must stay Add, got: {:?}",
            slot_runs[3]
        );
        // Phase-3 protection: pure additions untouched, `.round()`/`)`
        // already placed by relocation stay put.
        for idx in [1usize, 2] {
            assert!(
                slot_runs[idx].iter().all(|(k, _)| *k == RunKind::Add),
                "pure addition slot {idx} must stay pure, got: {:?}",
                slot_runs[idx]
            );
        }
        assert_eq!(
            slot_runs[4].last().map(|(k, _)| *k),
            Some(RunKind::MovedSrc)
        );
        assert_eq!(
            slot_runs[6].last().map(|(k, _)| *k),
            Some(RunKind::MovedSrc)
        );
    }

    #[test]
    fn test_leftover_frame_bracketed_within_window_abstains() {
        // Jurisdiction: a bracketed expression within relocation's window
        // (`.clone()` at distance 1) was relocation's call — declining it
        // was deliberate. The leftover pass must not second-guess it,
        // even though the fragment is unique and Mod-anchored.
        let mut slot_runs: Vec<Runs> = vec![
            vec![(RunKind::Del, "let mut line = del_line.clone();".to_string())],
            vec![
                (RunKind::Ctx, "Row::Del(del_line".to_string()),
                (RunKind::Add, ".clone()".to_string()),
                (RunKind::Ctx, ")".to_string()),
            ],
        ];
        let is_moved_slot = vec![false; 2];
        let is_pure_add = vec![false; 2];
        relocate_leftover_frames(&mut slot_runs, &is_moved_slot, &is_pure_add, None);
        assert!(
            slot_runs[0].iter().all(|(k, _)| *k != RunKind::MovedSrc),
            "within-window `.clone()` donor must abstain, got: {:?}",
            slot_runs[0]
        );
        assert!(
            slot_runs[1].iter().all(|(k, _)| *k != RunKind::MovedTgt),
            "within-window `.clone()` recipient must abstain, got: {:?}",
            slot_runs[1]
        );
    }
}
