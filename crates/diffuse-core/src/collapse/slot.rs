//! Closed-economy token typing and window slot layout for peer reflow
//! formations.

use diffuse_syntax::profile::LangProfile;
use rustc_hash::FxHashSet;

use crate::{
    collapse::tokens::is_salient_word_match,
    lcs::{
        DEFAULT_MAX_SLOT_DISTANCE, WindowSlotMap, align_lines_2d_with_cap,
        merged_runs_for_texts_with_profile, owned_runs_for_texts_with_profile, token_is_ws,
        token_product_cap, tokenize_line,
    },
    merge::RunKind,
    parse::{GroupId, PeerGroup, Row, RowKind, Spans},
    tape::spans::{LeadingIndentSplit, split_leading_indent},
};

/// One styled run of a slot's text: a kind plus its exact source text.
type Runs = Vec<(RunKind, String)>;

/// One aligned slot in a segmented window: `(old_row, new_row)`.
type SlotPair<'a> = (Option<&'a Row>, Option<&'a Row>);

/// An aligned slot within a peer reflow group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignedSlotRow {
    pub slot_index: usize,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    pub kind: RowKind,
    pub is_moved_src: bool,
    pub is_moved_tgt: bool,
    pub runs: Vec<(RunKind, String)>,
    pub spans: Spans,
    pub group: PeerGroup,
}

/// Result of aligning and typing tokens for a factored peer reflow group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerGroupRows {
    pub group_id: GroupId,
    pub slots: Vec<AlignedSlotRow>,
}

impl PeerGroupRows {
    /// Validates the closed-economy thermodynamic conservation invariant:
    /// All token runs marked `MovedSrc` on the left pane must exactly equal
    /// the token runs marked `MovedTgt` on the right pane in sequence.
    pub fn assert_token_conservation(&self) {
        let mut moved_src_tokens: Vec<&str> = Vec::new();
        let mut moved_tgt_tokens: Vec<&str> = Vec::new();

        for slot in &self.slots {
            for (kind, text) in &slot.runs {
                match kind {
                    RunKind::MovedSrc => moved_src_tokens.push(text.as_str()),
                    RunKind::MovedTgt => moved_tgt_tokens.push(text.as_str()),
                    _ => {}
                }
            }
        }

        moved_src_tokens.sort_unstable();
        moved_tgt_tokens.sort_unstable();

        assert_eq!(
            moved_src_tokens, moved_tgt_tokens,
            "Peer group {} token conservation mismatch: MovedSrc={:?} vs MovedTgt={:?}",
            self.group_id, moved_src_tokens, moved_tgt_tokens
        );
    }

    /// Enforces closed economy by construction: demote unmatched moved
    /// runs to plain struck/fill. Move-typing is heuristic, and real-world
    /// diffs (quoting, punctuation) routinely leave a moved mark without a
    /// partner; a lone `MovedSrc`/`MovedTgt` would break downstream
    /// conservation checks and projection, so it renders as Del/Add.
    /// Call before [`Self::assert_token_conservation`].
    pub fn repair_unmatched_moves(&mut self) {
        use rustc_hash::FxHashMap;
        // Owned keys: the counts must not borrow the runs they retype.
        let mut src_counts: FxHashMap<String, usize> = FxHashMap::default();
        let mut tgt_counts: FxHashMap<String, usize> = FxHashMap::default();
        for slot in &self.slots {
            for (kind, text) in &slot.runs {
                match kind {
                    RunKind::MovedSrc => *src_counts.entry(text.clone()).or_default() += 1,
                    RunKind::MovedTgt => *tgt_counts.entry(text.clone()).or_default() += 1,
                    _ => {}
                }
            }
        }
        let mut src_surplus: FxHashMap<String, usize> = FxHashMap::default();
        let mut tgt_surplus: FxHashMap<String, usize> = FxHashMap::default();
        for (text, &n) in &src_counts {
            let m = tgt_counts.get(text).copied().unwrap_or(0);
            if n > m {
                src_surplus.insert(text.clone(), n - m);
            }
        }
        for (text, &m) in &tgt_counts {
            let n = src_counts.get(text).copied().unwrap_or(0);
            if m > n {
                tgt_surplus.insert(text.clone(), m - n);
            }
        }
        if src_surplus.is_empty() && tgt_surplus.is_empty() {
            return;
        }
        for slot in &mut self.slots {
            for (kind, text) in &mut slot.runs {
                if *kind == RunKind::MovedSrc && src_surplus.get(text).copied().unwrap_or(0) > 0 {
                    *kind = RunKind::Del;
                    *src_surplus.get_mut(text).expect("present") -= 1;
                } else if *kind == RunKind::MovedTgt
                    && tgt_surplus.get(text).copied().unwrap_or(0) > 0
                {
                    *kind = RunKind::Add;
                    *tgt_surplus.get_mut(text).expect("present") -= 1;
                }
            }
        }
    }
}

/// Types one segmented window, whose membership is already decided.
///
/// `pairs` lists the window's correspondences as `(old, new)` offsets into
/// `old_rows` and `new_rows`, in path order. Rows the path left unpaired become
/// deletions and additions of the window, so every row of both slices is
/// accounted for exactly once.
pub fn type_window(
    group_id: GroupId,
    old_rows: &[Row],
    new_rows: &[Row],
    pairs: &[(usize, usize)],
    profile: Option<&LangProfile>,
) -> PeerGroupRows {
    let mut aligned_pairs: Vec<SlotPair<'_>> = Vec::with_capacity(old_rows.len() + new_rows.len());
    let (mut o, mut n) = (0usize, 0usize);
    let mut pi = 0usize;
    while o < old_rows.len() || n < new_rows.len() {
        if let Some(&(po, pn)) = pairs.get(pi) {
            if (o, n) == (po, pn) {
                aligned_pairs.push((Some(&old_rows[o]), Some(&new_rows[n])));
                o += 1;
                n += 1;
                pi += 1;
                continue;
            }
            if o < po {
                aligned_pairs.push((Some(&old_rows[o]), None));
                o += 1;
                continue;
            }
            if n < pn {
                aligned_pairs.push((None, Some(&new_rows[n])));
                n += 1;
                continue;
            }
        }
        if o < old_rows.len() {
            aligned_pairs.push((Some(&old_rows[o]), None));
            o += 1;
        } else {
            aligned_pairs.push((None, Some(&new_rows[n])));
            n += 1;
        }
    }
    type_slot_pairs(
        aligned_pairs,
        group_id,
        old_rows.len(),
        new_rows.len(),
        profile,
    )
}

/// Types an ordered slot list into classified rows and, when required, the
/// formation that holds those rows.
///
/// Slot alignment, not membership, is this function's subject: the caller has
/// already decided which rows belong together, which is what lets a segmented
/// window be typed without a second alignment pass over the cluster.
fn type_slot_pairs(
    aligned_pairs: Vec<SlotPair<'_>>,
    group_id: GroupId,
    n: usize,
    m: usize,
    profile: Option<&LangProfile>,
) -> PeerGroupRows {
    let total_slots = aligned_pairs.len();

    let is_moved_slot: Vec<bool> = aligned_pairs
        .iter()
        .map(|(old_opt, new_opt)| {
            old_opt.is_some_and(|r| matches!(r, Row::MovSrc { .. }))
                || new_opt.is_some_and(|r| matches!(r, Row::MovTgt { .. }))
        })
        .collect();

    // 1. Stage 1: unified 2D coordinate-aware multi-line tape for multi-slot
    //    windows (`1 × m`, `n × 1`, and general `n × m`), falling back cleanly
    //    to per-slot alignment on cap overflow or wholesale `Mod` slots.
    let mut slot_runs: Vec<Vec<(RunKind, String)>> = if total_slots > 1
        && let Some(runs_2d) = align_window_2d_tape(&aligned_pairs, profile)
    {
        runs_2d
    } else {
        let mut fallback = Vec::with_capacity(total_slots);
        for (old_opt, new_opt) in &aligned_pairs {
            match (old_opt, new_opt) {
                (Some(u), Some(v)) => {
                    if u.text() == v.text() {
                        fallback.push(vec![(RunKind::Ctx, u.text().to_string())]);
                    } else {
                        fallback.push(owned_runs_for_texts_with_profile(
                            u.text(),
                            v.text(),
                            profile,
                        ));
                    }
                }
                (Some(u), None) => {
                    if let Row::MovSrc { cached_runs, .. } = u {
                        fallback.push(cached_runs.clone());
                    } else {
                        fallback.push(vec![(RunKind::Del, u.text().to_string())]);
                    }
                }
                (None, Some(v)) => {
                    if let Row::MovTgt { cached_runs, .. } = v {
                        fallback.push(cached_runs.clone());
                    } else {
                        fallback.push(vec![(RunKind::Add, v.text().to_string())]);
                    }
                }
                (None, None) => unreachable!(),
            }
        }
        fallback
    };

    // 2. Stage 2: residual cross-slot relocation for non-monotone / transposed
    //    moves over leftover `Del` and `Add` runs (when `total_slots > 1`).
    if total_slots > 1 {
        let is_pure_add: Vec<bool> = aligned_pairs
            .iter()
            .map(|(old_opt, new_opt)| old_opt.is_none() && new_opt.is_some())
            .collect();
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);
        relocate_leftover_frames(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);
    }

    // 3. Construct final AlignedSlotRow list
    let mut slots = Vec::with_capacity(total_slots);
    for (idx, ((old_opt, new_opt), runs)) in aligned_pairs.into_iter().zip(slot_runs).enumerate() {
        let spans = if let Some(Row::MovSrc { spans, .. }) = old_opt {
            spans.clone()
        } else if let Some(Row::MovTgt { spans, .. }) = new_opt {
            spans.clone()
        } else {
            compute_slot_spans(&runs)
        };
        let old_line = old_opt.and_then(Row::old_line);
        let new_line = new_opt.and_then(Row::new_line);
        let old_text = old_opt.map(|r| r.text().to_string());
        let new_text = new_opt.map(|r| r.text().to_string());
        let is_moved_src = old_opt.is_some_and(|r| matches!(r, Row::MovSrc { .. }));
        let is_moved_tgt = new_opt.is_some_and(|r| matches!(r, Row::MovTgt { .. }));
        let kind = match (old_line, new_line) {
            (Some(_), Some(_)) => RowKind::Mod,
            (Some(_), None) => {
                if is_moved_src {
                    RowKind::Mov
                } else {
                    RowKind::Del
                }
            }
            (None, Some(_)) => {
                if is_moved_tgt {
                    RowKind::Mov
                } else {
                    RowKind::Add
                }
            }
            (None, None) => RowKind::Ctx,
        };

        slots.push(AlignedSlotRow {
            slot_index: idx,
            old_line,
            new_line,
            old_text,
            new_text,
            kind,
            is_moved_src,
            is_moved_tgt,
            runs,
            spans,
            group: PeerGroup {
                id: group_id,
                slot: idx,
                n,
                m,
            },
        });
    }

    PeerGroupRows { group_id, slots }
}

/// One classified token on an old or new line during 2D slot projection.
#[derive(Clone, Copy, Debug)]
enum ProjectedPiece<'a> {
    Ctx(&'a str),
    Edge {
        kind: RunKind,
        text: &'a str,
        phrase_id: Option<usize>,
        is_leading_ws: bool,
    },
}

/// Projects a 2D coordinate-aware multi-line alignment directly onto a window's
/// ordered slot list, emitting `Ctx` for same-slot matches and balanced
/// `MovedSrc`/`MovedTgt` phrases for cross-slot matches.
///
/// Guarantees by construction:
/// - Exact old/new text projection on every slot (`project_old == old_text`,
///   `project_new == new_text`).
/// - Exact closed-economy conservation (`Σ MovedSrc ≡ Σ MovedTgt`, identical
///   phrase multisets on both sides).
///
/// Returns `None` when the window has no non-moved lines on both sides, exceeds
/// `token_product_cap`, or when any `Mod` slot would degrade into a wholesale
/// replacement (allowing the caller to fall back to per-slot alignment).
pub(crate) fn align_window_2d_tape(
    aligned_pairs: &[SlotPair<'_>],
    profile: Option<&LangProfile>,
) -> Option<Vec<Runs>> {
    let total_slots = aligned_pairs.len();
    let mut old_lines: Vec<&str> = Vec::new();
    let mut new_lines: Vec<&str> = Vec::new();
    let mut slot_of_old: Vec<usize> = Vec::new();
    let mut slot_of_new: Vec<usize> = Vec::new();
    let mut old_line_of_slot: Vec<Option<usize>> = vec![None; total_slots];
    let mut new_line_of_slot: Vec<Option<usize>> = vec![None; total_slots];

    for (slot_idx, (old_opt, new_opt)) in aligned_pairs.iter().enumerate() {
        if let Some(u) = old_opt
            && !matches!(u, Row::MovSrc { .. })
        {
            old_line_of_slot[slot_idx] = Some(old_lines.len());
            old_lines.push(u.text());
            slot_of_old.push(slot_idx);
        }
        if let Some(v) = new_opt
            && !matches!(v, Row::MovTgt { .. })
        {
            new_line_of_slot[slot_idx] = Some(new_lines.len());
            new_lines.push(v.text());
            slot_of_new.push(slot_idx);
        }
    }

    if old_lines.is_empty() || new_lines.is_empty() {
        return None;
    }

    // In `1 × m` expansions and `n × 1` collapses, every row belongs to the
    // single opposite-side construct (bounded by `reflow.rs`), so cross-slot
    // matches must reach the terminal closing delimiter regardless of window
    // height (`m` or `n`). Multi-row `n × m` windows keep
    // `DEFAULT_MAX_SLOT_DISTANCE`.
    let is_single_construct = old_lines.len() == 1 || new_lines.len() == 1;
    let max_slot_distance = if is_single_construct {
        usize::MAX
    } else {
        DEFAULT_MAX_SLOT_DISTANCE
    };

    let aln = align_lines_2d_with_cap(
        &old_lines,
        &new_lines,
        WindowSlotMap {
            slot_of_old: &slot_of_old,
            slot_of_new: &slot_of_new,
        },
        profile,
        max_slot_distance,
        token_product_cap(),
    )?;

    // Determine which cross-slot pairs carry salient content or touch a
    // bare-punctuation line, demoting coincidental punctuation matches between
    // unrelated full lines back to Del/Add.
    let old_line_has_salient: Vec<bool> = old_lines
        .iter()
        .map(|line| text_has_salient_token(line, profile))
        .collect();
    let new_line_has_salient: Vec<bool> = new_lines
        .iter()
        .map(|line| text_has_salient_token(line, profile))
        .collect();
    let mut salient_linked: FxHashSet<(usize, usize)> = FxHashSet::default();
    for &(oi, ni) in &aln.pairs {
        let ol = aln.old.tokens[oi].line;
        let nl = aln.new.tokens[ni].line;
        let so = slot_of_old[ol];
        let sn = slot_of_new[nl];
        if so != sn && text_has_salient_token(aln.old.tokens[oi].text, profile) {
            salient_linked.insert((so, sn));
        }
    }

    let kept_pairs: Vec<(usize, usize)> = aln
        .pairs
        .iter()
        .copied()
        .filter(|&(oi, ni)| {
            let ol = aln.old.tokens[oi].line;
            let nl = aln.new.tokens[ni].line;
            let so = slot_of_old[ol];
            let sn = slot_of_new[nl];
            so == sn
                || is_single_construct
                || salient_linked.contains(&(so, sn))
                || !old_line_has_salient[ol]
                || !new_line_has_salient[nl]
        })
        .collect();

    // Extend cross-slot moved phrases across adjacent unmatched identical
    // non-whitespace boundary tokens (optionally separated by one identical
    // non-leading interior space) on the same `(ol, nl)` line pair — recovering
    // isolated low-weight operators/punctuation such as `self.vis_line =` or
    // trailing `,`, plus a trailing interior separator space (such as
    // `|pat: &str| `) when both lines carry the exact same separator before a
    // following token (preventing stray red/green space flashes).
    let mut extended_pairs: Vec<(usize, usize)> = Vec::with_capacity(kept_pairs.len());
    for (idx, &(oi, ni)) in kept_pairs.iter().enumerate() {
        let ol = aln.old.tokens[oi].line;
        let nl = aln.new.tokens[ni].line;
        let is_cross = slot_of_old[ol] != slot_of_new[nl];

        // Backward boundary extension strictly after the previous matched pair.
        if is_cross {
            let min_oi = extended_pairs.last().map_or(0, |&(po, _)| po + 1);
            let min_ni = extended_pairs.last().map_or(0, |&(_, pn)| pn + 1);
            let mut prefix = Vec::new();
            let (mut co, mut cn) = (oi, ni);
            loop {
                if co > min_oi && cn > min_ni {
                    let po = aln.old.tokens[co - 1];
                    let pn = aln.new.tokens[cn - 1];
                    if po.line == ol && pn.line == nl && !token_is_ws(po.text) && po.text == pn.text
                    {
                        prefix.push((co - 1, cn - 1));
                        co -= 1;
                        cn -= 1;
                        continue;
                    }
                    if co >= min_oi + 2 && cn >= min_ni + 2 {
                        let wo = aln.old.tokens[co - 1];
                        let wn = aln.new.tokens[cn - 1];
                        let to = aln.old.tokens[co - 2];
                        let tn = aln.new.tokens[cn - 2];
                        if wo.line == ol
                            && wn.line == nl
                            && to.line == ol
                            && tn.line == nl
                            && !wo.is_leading_ws
                            && !wn.is_leading_ws
                            && token_is_ws(wo.text)
                            && wo.text == wn.text
                            && !token_is_ws(to.text)
                            && to.text == tn.text
                        {
                            prefix.push((co - 1, cn - 1));
                            prefix.push((co - 2, cn - 2));
                            co -= 2;
                            cn -= 2;
                            continue;
                        }
                    }
                }
                break;
            }
            prefix.reverse();
            extended_pairs.extend(prefix);
        }

        extended_pairs.push((oi, ni));

        // Forward boundary extension strictly before the next matched pair.
        if is_cross {
            let max_oi = kept_pairs
                .get(idx + 1)
                .map_or(aln.old.tokens.len(), |&(no, _)| no);
            let max_ni = kept_pairs
                .get(idx + 1)
                .map_or(aln.new.tokens.len(), |&(_, nn)| nn);
            let (mut co, mut cn) = (oi, ni);
            loop {
                if co + 1 < max_oi && cn + 1 < max_ni {
                    let no = aln.old.tokens[co + 1];
                    let nn = aln.new.tokens[cn + 1];
                    if no.line == ol && nn.line == nl && !token_is_ws(no.text) && no.text == nn.text
                    {
                        extended_pairs.push((co + 1, cn + 1));
                        co += 1;
                        cn += 1;
                        continue;
                    }
                    if co + 2 < max_oi && cn + 2 < max_ni {
                        let wo = aln.old.tokens[co + 1];
                        let wn = aln.new.tokens[cn + 1];
                        let to = aln.old.tokens[co + 2];
                        let tn = aln.new.tokens[cn + 2];
                        if wo.line == ol
                            && wn.line == nl
                            && to.line == ol
                            && tn.line == nl
                            && !wo.is_leading_ws
                            && !wn.is_leading_ws
                            && token_is_ws(wo.text)
                            && wo.text == wn.text
                            && !token_is_ws(to.text)
                            && to.text == tn.text
                        {
                            extended_pairs.push((co + 1, cn + 1));
                            extended_pairs.push((co + 2, cn + 2));
                            co += 2;
                            cn += 2;
                            continue;
                        }
                    }
                }
                break;
            }

            // Absorb a single identical interior separator space immediately
            // following a moved phrase when both lines have a subsequent token
            // on the same line (e.g. `|pat: &str| `), avoiding stray red/green
            // separator flashes.
            if co + 1 < max_oi && cn + 1 < max_ni {
                let wo = aln.old.tokens[co + 1];
                let wn = aln.new.tokens[cn + 1];
                let old_has_after = co + 2 < aln.old.line_ranges[ol].end;
                let new_has_after = cn + 2 < aln.new.line_ranges[nl].end;
                if wo.line == ol
                    && wn.line == nl
                    && !wo.is_leading_ws
                    && !wn.is_leading_ws
                    && token_is_ws(wo.text)
                    && wo.text == wn.text
                    && old_has_after
                    && new_has_after
                {
                    extended_pairs.push((co + 1, cn + 1));
                }
            }
        }
    }

    // Bridge identical non-leading intervening token spans strictly enclosed
    // between two cross-slot moved tokens on the same `(ol, nl)` line pair so
    // relocated phrases like `tag = "kind",` or `the pi review extension` stay
    // one clean moved run (runs AFTER boundary extension so newly extended
    // tokens bridge cleanly with adjacent matches).
    let mut bridged_pairs: Vec<(usize, usize)> = Vec::with_capacity(extended_pairs.len());
    for pair in extended_pairs.drain(..) {
        if let Some(&(prev_oi, prev_ni)) = bridged_pairs.last() {
            let (oi, ni) = pair;
            let gap = oi.saturating_sub(prev_oi);
            if gap > 1 && ni.saturating_sub(prev_ni) == gap {
                let prev_ol = aln.old.tokens[prev_oi].line;
                let prev_nl = aln.new.tokens[prev_ni].line;
                let cur_ol = aln.old.tokens[oi].line;
                let cur_nl = aln.new.tokens[ni].line;
                if prev_ol == cur_ol
                    && prev_nl == cur_nl
                    && slot_of_old[cur_ol] != slot_of_new[cur_nl]
                    && (1..gap).all(|d| {
                        let mo = aln.old.tokens[prev_oi + d];
                        let mn = aln.new.tokens[prev_ni + d];
                        mo.line == cur_ol
                            && mn.line == cur_nl
                            && !mo.is_leading_ws
                            && !mn.is_leading_ws
                            && mo.text == mn.text
                    })
                {
                    for d in 1..gap {
                        bridged_pairs.push((prev_oi + d, prev_ni + d));
                    }
                }
            }
        }
        bridged_pairs.push(pair);
    }

    // Assign identical `phrase_id` to contiguous cross-slot runs adjacent on
    // BOTH old and new lines; same-slot matches map to `Ctx`.
    #[derive(Clone, Copy)]
    enum TokenMatch {
        Ctx,
        Moved(usize),
    }
    let mut old_matches: Vec<Option<TokenMatch>> = vec![None; aln.old.tokens.len()];
    let mut new_matches: Vec<Option<TokenMatch>> = vec![None; aln.new.tokens.len()];
    let mut next_phrase_id = 0usize;
    let mut prev_moved: Option<(usize, usize, usize, usize, usize)> = None;

    for &(oi, ni) in &bridged_pairs {
        let ol = aln.old.tokens[oi].line;
        let nl = aln.new.tokens[ni].line;
        if slot_of_old[ol] == slot_of_new[nl] {
            old_matches[oi] = Some(TokenMatch::Ctx);
            new_matches[ni] = Some(TokenMatch::Ctx);
            prev_moved = None;
        } else {
            let pid = match prev_moved {
                Some((p_oi, p_ni, p_ol, p_nl, p_id))
                    if oi == p_oi + 1 && ni == p_ni + 1 && ol == p_ol && nl == p_nl =>
                {
                    p_id
                }
                _ => {
                    let id = next_phrase_id;
                    next_phrase_id += 1;
                    id
                }
            };
            old_matches[oi] = Some(TokenMatch::Moved(pid));
            new_matches[ni] = Some(TokenMatch::Moved(pid));
            prev_moved = Some((oi, ni, ol, nl, pid));
        }
    }

    let build_old_pieces = |ol: usize| -> Vec<ProjectedPiece<'_>> {
        aln.old.line_ranges[ol]
            .clone()
            .map(|oi| {
                let tok = aln.old.tokens[oi];
                match old_matches[oi] {
                    Some(TokenMatch::Ctx) => ProjectedPiece::Ctx(tok.text),
                    Some(TokenMatch::Moved(pid)) => ProjectedPiece::Edge {
                        kind: RunKind::MovedSrc,
                        text: tok.text,
                        phrase_id: Some(pid),
                        is_leading_ws: tok.is_leading_ws,
                    },
                    None => ProjectedPiece::Edge {
                        kind: RunKind::Del,
                        text: tok.text,
                        phrase_id: None,
                        is_leading_ws: tok.is_leading_ws,
                    },
                }
            })
            .collect()
    };

    let build_new_pieces = |nl: usize| -> Vec<ProjectedPiece<'_>> {
        aln.new.line_ranges[nl]
            .clone()
            .map(|ni| {
                let tok = aln.new.tokens[ni];
                match new_matches[ni] {
                    Some(TokenMatch::Ctx) => ProjectedPiece::Ctx(tok.text),
                    Some(TokenMatch::Moved(pid)) => ProjectedPiece::Edge {
                        kind: RunKind::MovedTgt,
                        text: tok.text,
                        phrase_id: Some(pid),
                        is_leading_ws: tok.is_leading_ws,
                    },
                    None => ProjectedPiece::Edge {
                        kind: RunKind::Add,
                        text: tok.text,
                        phrase_id: None,
                        is_leading_ws: tok.is_leading_ws,
                    },
                }
            })
            .collect()
    };

    let mut slot_runs: Vec<Runs> = Vec::with_capacity(total_slots);
    for (slot_idx, (old_opt, new_opt)) in aligned_pairs.iter().enumerate() {
        match (old_line_of_slot[slot_idx], new_line_of_slot[slot_idx]) {
            (Some(ol), Some(nl)) => {
                let old_pieces = build_old_pieces(ol);
                let new_pieces = build_new_pieces(nl);
                let runs = merge_2d_mod_slot(&old_pieces, &new_pieces);
                if slot_runs_are_wholesale(&runs, old_lines[ol], new_lines[nl]) {
                    return None;
                }
                slot_runs.push(runs);
            }
            (Some(ol), None) => {
                let pieces = build_old_pieces(ol);
                slot_runs.push(emit_2d_single_side(&pieces));
            }
            (None, Some(nl)) => {
                let pieces = build_new_pieces(nl);
                slot_runs.push(emit_2d_single_side(&pieces));
            }
            (None, None) => {
                // Pre-moved `Row::MovSrc` / `Row::MovTgt` slot retains its
                // cached runs untouched.
                if let Some(Row::MovSrc { cached_runs, .. }) = old_opt {
                    slot_runs.push(cached_runs.clone());
                } else if let Some(Row::MovTgt { cached_runs, .. }) = new_opt {
                    slot_runs.push(cached_runs.clone());
                } else {
                    unreachable!("every slot owns at least one old or new row");
                }
            }
        }
    }

    Some(slot_runs)
}

fn push_projected_run(
    out: &mut Runs,
    last_phrase: &mut Option<usize>,
    kind: RunKind,
    text: &str,
    phrase_id: Option<usize>,
) {
    if text.is_empty() {
        return;
    }
    let can_merge = match (out.last(), phrase_id, *last_phrase) {
        (Some((last_k, _)), None, None) => *last_k == kind,
        (Some((last_k, _)), Some(cur_pid), Some(prev_pid)) => {
            *last_k == kind && cur_pid == prev_pid
        }
        _ => false,
    };
    if can_merge {
        if let Some((_, last_t)) = out.last_mut() {
            last_t.push_str(text);
        }
    } else {
        out.push((kind, text.to_string()));
        *last_phrase = phrase_id;
    }
}

fn emit_2d_single_side(pieces: &[ProjectedPiece<'_>]) -> Runs {
    let mut out = Vec::new();
    let mut last_phrase = None;
    for &piece in pieces {
        match piece {
            ProjectedPiece::Ctx(text) => {
                push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, text, None);
            }
            ProjectedPiece::Edge {
                kind,
                text,
                phrase_id,
                ..
            } => {
                push_projected_run(&mut out, &mut last_phrase, kind, text, phrase_id);
            }
        }
    }
    out
}

/// Lockstep `Ctx`-anchored merger for a paired `Mod(ol, nl)` slot, applying
/// per-line leading-indentation splitting on the initial leading whitespace
/// pair.
fn merge_2d_mod_slot(old_pieces: &[ProjectedPiece<'_>], new_pieces: &[ProjectedPiece<'_>]) -> Runs {
    let mut out = Vec::new();
    let mut last_phrase = None;
    let (mut oi, mut ni) = (0usize, 0usize);
    let mut is_leading = true;

    loop {
        let old_gap_start = oi;
        while oi < old_pieces.len() && !matches!(old_pieces[oi], ProjectedPiece::Ctx(_)) {
            oi += 1;
        }
        let new_gap_start = ni;
        while ni < new_pieces.len() && !matches!(new_pieces[ni], ProjectedPiece::Ctx(_)) {
            ni += 1;
        }

        let mut old_gap = &old_pieces[old_gap_start..oi];
        let mut new_gap = &new_pieces[new_gap_start..ni];

        if is_leading
            && let Some(ProjectedPiece::Edge {
                kind: RunKind::Del,
                text: d_ws,
                is_leading_ws: true,
                ..
            }) = old_gap.first().copied()
            && let Some(ProjectedPiece::Edge {
                kind: RunKind::Add,
                text: a_ws,
                is_leading_ws: true,
                ..
            }) = new_gap.first().copied()
            && let Some(split) = split_leading_indent(d_ws, a_ws)
        {
            match split {
                LeadingIndentSplit::Same(shared) => {
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, shared, None);
                }
                LeadingIndentSplit::Indented { added, shared } => {
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Add, added, None);
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, shared, None);
                }
                LeadingIndentSplit::Dedented { removed, shared } => {
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Del, removed, None);
                    push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, shared, None);
                }
            }
            old_gap = &old_gap[1..];
            new_gap = &new_gap[1..];
        }

        for &piece in old_gap {
            if let ProjectedPiece::Edge {
                kind,
                text,
                phrase_id,
                ..
            } = piece
            {
                if !text.trim().is_empty() {
                    is_leading = false;
                }
                push_projected_run(&mut out, &mut last_phrase, kind, text, phrase_id);
            }
        }
        for &piece in new_gap {
            if let ProjectedPiece::Edge {
                kind,
                text,
                phrase_id,
                ..
            } = piece
            {
                if !text.trim().is_empty() {
                    is_leading = false;
                }
                push_projected_run(&mut out, &mut last_phrase, kind, text, phrase_id);
            }
        }

        if oi < old_pieces.len() && ni < new_pieces.len() {
            if let ProjectedPiece::Ctx(text) = old_pieces[oi] {
                if !text.trim().is_empty() {
                    is_leading = false;
                }
                push_projected_run(&mut out, &mut last_phrase, RunKind::Ctx, text, None);
            }
            oi += 1;
            ni += 1;
        } else {
            break;
        }
    }

    out
}

/// Returns `true` when `runs` would turn a `Mod` slot into a wholesale
/// replacement (zero non-whitespace `Ctx` runs despite non-empty text).
#[inline]
fn slot_runs_are_wholesale(runs: &[(RunKind, String)], old_text: &str, new_text: &str) -> bool {
    let has_non_ws_ctx = runs
        .iter()
        .any(|(kind, text)| *kind == RunKind::Ctx && !text.trim().is_empty());
    !has_non_ws_ctx && (!old_text.trim().is_empty() || !new_text.trim().is_empty())
}

fn text_has_salient_token(text: &str, profile: Option<&LangProfile>) -> bool {
    crate::lcs::line_has_salient_token(text, profile)
}

/// Whether a peer group without any paired `Mod` slot still establishes a
/// strong cross-slot relationship: at least one balanced
/// `MovedSrc`/`MovedTgt` run carries a salient identifier token. Call after
/// [`PeerGroupRows::repair_unmatched_moves`], so any surviving moved run is
/// balanced by construction; the salient filter rejects delimiter-only
/// relocations (`};`, `(`, …) that share no identifier content.
pub(crate) fn has_salient_moved_link(
    group_rows: &PeerGroupRows,
    profile: Option<&LangProfile>,
) -> bool {
    group_rows.slots.iter().flat_map(|s| &s.runs).any(|(k, t)| {
        matches!(k, RunKind::MovedSrc | RunKind::MovedTgt) && text_has_salient_token(t, profile)
    })
}

/// Maximum slot distance a relocated token may travel. The candidate
/// gather and the apply loop must honor the same window: otherwise a
/// token gathered from a nearby pair (e.g. a lone `{`) is stamped as
/// moved between arbitrarily distant slots.
const CROSS_SLOT_WINDOW: i32 = 4;

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
    slot_runs: &[Vec<(RunKind, String)>],
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

/// Completes stranded expression frames from the leftovers of the earlier
/// passes (Phase 6).
///
/// `relocate_cross_slot_tokens` only proposes verbatim whole-run matches
/// within [`CROSS_SLOT_WINDOW`], so two kinds of frame part survive it:
/// openers or closers whose partner lives beyond the window (`Some(` at
/// distance 5), and glue whose counterpart run ends in a different
/// terminator (` as usize,` vs ` as usize;`). Both are recovered here by
/// matching leftover tokens by exact identity: `,` and `;` simply never
/// enter a common block, so they stay `Del`/`Add` with no terminator
/// normalization and no language-specific rule.
///
/// Runs strictly last, over the runs the earlier passes stranded, and is
/// gated so Phase-3 adjudications are never second-guessed:
/// - *generic frame vocabulary only*: blocks that carry a domain identifier
///   (`AddLine::with_spans(`, `spans.clone`) stay with affinity/coverage;
/// - *bracket structure on both sides*: the block holds a bracket, or its
///   nearest non-whitespace neighbour in the slot projection does (` as usize`
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
fn relocate_leftover_frames(
    slot_runs: &mut [Vec<(RunKind, String)>],
    is_moved_slot: &[bool],
    is_pure_add: &[bool],
    profile: Option<&LangProfile>,
) {
    // Per-slot projections of each side, with each run's byte offset into
    // its slot projection. Retyping never alters slot texts, so these are
    // computed once; they locate a frame block's neighbours exactly.
    use diffuse_algo::dyck::{
        DelimiterCategory, DelimiterClassifier, UniversalDelimiterClassifier,
    };
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
            let dist = (src_slot as i32 - tgt_slot as i32).abs();
            if dist <= CROSS_SLOT_WINDOW && joined.chars().any(is_bracket) {
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
            if dist > CROSS_SLOT_WINDOW && !(externals(block_src) || externals(block_tgt)) {
                continue;
            }
            // Structural frame gate: on each side the block holds a bracket
            // itself, or its nearest non-whitespace neighbour in the slot
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

fn relocate_cross_slot_tokens(
    slot_runs: &mut [Vec<(RunKind, String)>],
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
            if src == tgt || (*src as i32 - *tgt as i32).abs() > CROSS_SLOT_WINDOW {
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

    candidates.sort_by_key(|a| std::cmp::Reverse(a.len()));
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
        .partition(|t| text_has_salient_token(t, profile));
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
    slot_runs: &[Vec<(RunKind, String)>],
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
        crate::lcs::tokenizer::COMMON_KEYWORDS.contains(&w)
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

fn coalesce_runs(runs: &mut Vec<(RunKind, String)>) {
    if runs.len() <= 1 {
        return;
    }
    let mut coalesced: Vec<(RunKind, String)> = Vec::with_capacity(runs.len());
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
/// pairs within [`CROSS_SLOT_WINDOW`]. When `gate` is `Some`, only pairs
/// in it may exchange the token — unless `allow_bare_lines` and one
/// side's whole line is bare punctuation (see above). Pairs that actually
/// exchange the token are appended to `linked`.
#[allow(clippy::too_many_arguments)]
fn apply_relocation_candidate(
    slot_runs: &mut [Vec<(RunKind, String)>],
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
                        crate::lcs::token_is_salient(sw, profile)
                            && !token_words.iter().any(|tw| is_salient_word_match(sw, tw))
                            && tgt_words.iter().any(|tw| is_salient_word_match(sw, tw))
                    })
                };
                let has_evidence = has_linked_anchor || is_paired_mod || has_mutual_salient_anchor;

                let max_distance = if !is_weak || has_evidence {
                    CROSS_SLOT_WINDOW
                } else {
                    1
                };
                if (src as i32 - tgt as i32).abs() > max_distance {
                    continue;
                }
            } else if (src as i32 - tgt as i32).abs() > CROSS_SLOT_WINDOW {
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
                .unwrap_or(std::cmp::Ordering::Equal)
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
    runs: &mut Vec<(RunKind, String)>,
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

pub(crate) fn compute_slot_spans(runs: &[(RunKind, String)]) -> Spans {
    let mut del_spans = Vec::new();
    let mut add_spans = Vec::new();
    let mut old_off = 0usize;
    let mut new_off = 0usize;

    for (kind, text) in runs {
        let len = text.chars().count();
        match kind {
            RunKind::Ctx => {
                old_off += len;
                new_off += len;
            }
            RunKind::Add => {
                if len > 0 {
                    add_spans.push((new_off, new_off + len));
                }
                new_off += len;
            }
            RunKind::Del | RunKind::MovedSrc => {
                if len > 0 {
                    del_spans.push((old_off, old_off + len));
                }
                old_off += len;
            }
            RunKind::MovedTgt => {
                new_off += len;
            }
        }
    }

    normalize_spans(&mut del_spans);
    normalize_spans(&mut add_spans);

    Spans {
        del: del_spans,
        add: add_spans,
    }
}

fn normalize_spans(spans: &mut Vec<(usize, usize)>) {
    if spans.is_empty() {
        return;
    }
    spans.sort_unstable_by_key(|&(s, _)| s);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for &(s, e) in spans.iter() {
        if let Some(last) = merged.last_mut()
            && s <= last.1
        {
            last.1 = last.1.max(e);
            continue;
        }
        merged.push((s, e));
    }
    *spans = merged;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{AddLine, DelLine};

    fn make_del(num: u32, text: &str) -> Row {
        Row::del(DelLine::new(num, text))
    }

    fn make_add(num: u32, text: &str) -> Row {
        Row::add(AddLine::new(num, text))
    }

    #[test]
    fn test_align_single_mod() {
        let dels = vec![make_del(10, "let alpha = 100;")];
        let adds = vec![make_add(10, "let alpha = 101;")];

        let result = type_window(GroupId(1), &dels, &adds, &[(0, 0)], None);
        assert_eq!(result.slots.len(), 1);
        let s0 = &result.slots[0];
        assert_eq!(s0.kind, RowKind::Mod);
        assert_eq!(s0.old_line, Some(10));
        assert_eq!(s0.new_line, Some(10));
        result.assert_token_conservation();
    }

    #[test]
    fn test_align_expansion_block() {
        let dels = vec![make_del(20, "fn run() { active(); }")];
        let adds = vec![
            make_add(20, "fn run() {"),
            make_add(21, "    active();"),
            make_add(22, "}"),
        ];

        let result = type_window(GroupId(2), &dels, &adds, &[(0, 0)], None);
        assert_eq!(result.slots.len(), 3);
        assert_eq!(result.slots[0].kind, RowKind::Mod);
        assert_eq!(result.slots[1].kind, RowKind::Add);
        assert_eq!(result.slots[2].kind, RowKind::Add);
        result.assert_token_conservation();
    }

    #[test]
    fn test_align_compound_scrollbar_reflow() {
        let dels = vec![
            make_del(293, "find_entry(pat: &str) -> Option<usize>"),
            make_del(
                294,
                "|pat: &str| self.entries.iter().position(|e| e.name == pat);",
            ),
        ];
        let adds = vec![
            make_add(
                288,
                "find_entry<'a>(&'a self, pat: &str) -> Option<usize> {",
            ),
            make_add(289, "    self.entries.iter().position(|e| e.name == pat)"),
            make_add(290, "};"),
        ];

        let result = type_window(GroupId(3), &dels, &adds, &[(0, 0), (1, 1)], None);
        assert_eq!(result.slots.len(), 3);
        assert_eq!(result.slots[0].kind, RowKind::Mod);
        assert_eq!(result.slots[1].kind, RowKind::Mod);
        assert_eq!(result.slots[2].kind, RowKind::Add);
        result.assert_token_conservation();
    }

    #[test]
    fn test_align_pure_add_and_del_groups() {
        let adds = vec![make_add(10, "line 1"), make_add(11, "line 2")];
        let add_res = type_window(GroupId(4), &[], &adds, &[], None);
        assert_eq!(add_res.slots.len(), 2);
        assert_eq!(add_res.slots[0].kind, RowKind::Add);
        assert_eq!(add_res.slots[1].kind, RowKind::Add);
        add_res.assert_token_conservation();

        let dels = vec![make_del(10, "del 1"), make_del(11, "del 2")];
        let del_res = type_window(GroupId(5), &dels, &[], &[], None);
        assert_eq!(del_res.slots.len(), 2);
        assert_eq!(del_res.slots[0].kind, RowKind::Del);
        assert_eq!(del_res.slots[1].kind, RowKind::Del);
        del_res.assert_token_conservation();
    }

    #[test]
    fn test_cross_slot_relocation_respects_distance_window() {
        // The `alpha` candidate is legitimately shared by the adjacent
        // slots 0/1, but slots 2 and 7 sit 5 apart: typing their token
        // as a move steals it from far away. The apply loop must honor
        // the same ±4 window the candidate gather uses.
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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

    fn link_slot(runs: Vec<(RunKind, String)>) -> AlignedSlotRow {
        AlignedSlotRow {
            slot_index: 0,
            old_line: Some(1),
            new_line: None,
            old_text: Some("x".to_string()),
            new_text: None,
            kind: RowKind::Del,
            is_moved_src: false,
            is_moved_tgt: false,
            runs,
            spans: Spans::default(),
            group: PeerGroup {
                id: GroupId(1),
                slot: 0,
                n: 1,
                m: 1,
            },
        }
    }

    fn link_rows(runs_list: Vec<Vec<(RunKind, String)>>) -> PeerGroupRows {
        PeerGroupRows {
            group_id: GroupId(1),
            slots: runs_list.into_iter().map(link_slot).collect(),
        }
    }

    #[test]
    fn test_has_salient_moved_link_no_moves() {
        let rows = link_rows(vec![vec![(RunKind::Del, "aaa".to_string())]]);
        assert!(!has_salient_moved_link(&rows, None));
    }

    #[test]
    fn test_has_salient_moved_link_delimiter_only() {
        let rows = link_rows(vec![vec![(RunKind::MovedSrc, ";".to_string())]]);
        assert!(!has_salient_moved_link(&rows, None));
    }

    #[test]
    fn test_has_salient_moved_link_salient() {
        let rows = link_rows(vec![vec![(RunKind::MovedSrc, "row".to_string())]]);
        assert!(has_salient_moved_link(&rows, None));
    }

    #[test]
    fn test_has_salient_moved_link_keyword_excluded() {
        let rows = link_rows(vec![vec![(RunKind::MovedSrc, "let".to_string())]]);
        assert!(!has_salient_moved_link(&rows, None));
    }

    #[test]
    fn test_competing_destination_affinity_relocates_to_highest_context_recipient() {
        let profile = diffuse_syntax::registry::lang_profile("rs");
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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
    /// output (file `app/geometry.rs` hunk 0, rows 26-32), after Phase-3
    /// relocation has already placed `.round()` and the stray `)`.
    fn scrollbar_group_6_runs() -> Vec<Vec<(RunKind, String)>> {
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
        let mut slot_runs: Vec<Vec<(RunKind, String)>> = vec![
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

    #[test]
    fn test_type_window_emits_del_before_add_for_unpaired_skips() {
        let dels = vec![
            make_del(10, "let shared = alpha();"),
            make_del(11, "let removed_tail = beta();"),
        ];
        let adds = vec![
            make_add(10, "let shared = alpha_v2();"),
            make_add(11, "let added_tail = gamma();"),
        ];
        let res = type_window(GroupId(9), &dels, &adds, &[(0, 0)], None);
        assert_eq!(res.slots.len(), 3);
        assert_eq!(res.slots[0].kind, RowKind::Mod);
        assert_eq!(
            res.slots[1].kind,
            RowKind::Del,
            "unpaired Del must precede unpaired Add, got: {:?}",
            res.slots
        );
        assert_eq!(res.slots[2].kind, RowKind::Add);
    }

    #[test]
    fn test_expansion_1_to_m_falls_back_when_slot_zero_would_be_wholesale() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let dels = vec![make_del(
            380,
            "        frame.contains(\"\\x1b[38;5;203m-\\x1b[39m\\x1b[23m\\x1b[38;5;203m\"),",
        )];
        let adds = vec![
            make_add(
                382,
                "    for (marker, color) in [(\"-\", \"\\x1b[38;5;203m\"), (\"+\", \"\\x1b[38;5;120m\")] {",
            ),
            make_add(383, "        frame.contains(color),"),
        ];
        let res = type_window(GroupId(1), &dels, &adds, &[(0, 0)], rs);
        assert!(
            !slot_runs_are_wholesale(&res.slots[0].runs, dels[0].text(), adds[0].text()),
            "slot 0 must never degrade into a wholesale Mod row: {:?}",
            res.slots[0].runs
        );
    }

    fn runs_project_to_text(
        runs: &[(RunKind, String)],
        old_text: Option<&str>,
        new_text: Option<&str>,
    ) -> bool {
        let old_proj: String = runs
            .iter()
            .filter(|(k, _)| !k.is_add_like())
            .map(|(_, t)| t.as_str())
            .collect();
        let new_proj: String = runs
            .iter()
            .filter(|(k, _)| !k.is_del_like())
            .map(|(_, t)| t.as_str())
            .collect();
        match (old_text, new_text) {
            (Some(o), Some(n)) => old_proj == o && new_proj == n,
            (Some(o), None) => old_proj == o,
            (None, Some(n)) => new_proj == n,
            (None, None) => true,
        }
    }

    fn assert_2d_conservation_and_projection(pairs: &[SlotPair<'_>], slot_runs: &[Runs]) {
        assert_eq!(pairs.len(), slot_runs.len());
        let mut src: Vec<&str> = Vec::new();
        let mut tgt: Vec<&str> = Vec::new();
        for ((old_opt, new_opt), runs) in pairs.iter().zip(slot_runs) {
            assert!(
                runs_project_to_text(runs, old_opt.map(Row::text), new_opt.map(Row::text)),
                "slot projection mismatch: old={:?}, new={:?}, runs={:?}",
                old_opt.map(Row::text),
                new_opt.map(Row::text),
                runs
            );
            for (k, t) in runs {
                match k {
                    RunKind::MovedSrc => src.push(t.as_str()),
                    RunKind::MovedTgt => tgt.push(t.as_str()),
                    _ => {}
                }
            }
        }
        src.sort_unstable();
        tgt.sort_unstable();
        assert_eq!(src, tgt, "closed-economy conservation violated");
    }

    #[test]
    fn test_align_window_2d_tape_1x4_mod_at_slot_1_assert_eq_frame() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let old = make_del(
            134,
            "    assert_eq!(super::resolve_theme(Some(\"dark\")), DiffuseTheme::dark());",
        );
        let new0 = make_add(134, "    assert_eq!(");
        let new1 = make_add(135, "        super::resolve_theme(Some(\"dark\"), false),");
        let new2 = make_add(136, "        DiffuseTheme::dark()");
        let new3 = make_add(137, "    );");
        let pairs: Vec<SlotPair<'_>> = vec![
            (None, Some(&new0)),
            (Some(&old), Some(&new1)),
            (None, Some(&new2)),
            (None, Some(&new3)),
        ];

        let runs = align_window_2d_tape(&pairs, rs).expect("2D tape succeeds");
        assert_2d_conservation_and_projection(&pairs, &runs);

        // Slot 0 receives `assert_eq!(` as MovedTgt, Slot 2 receives
        // `DiffuseTheme::dark()`, Slot 3 receives `);`.
        assert!(
            runs[0]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t == "assert_eq!(")
        );
        assert!(
            runs[2]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t == "DiffuseTheme::dark()")
        );
        assert!(
            runs[3]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t == ");")
        );
        // Slot 1 Mod keeps `super::resolve_theme(Some("dark")` as Ctx and
        // `, false` as Add with leading-indent split (`Add("    ")`, `Ctx("
        // ")`).
        assert!(
            runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::Ctx && t.contains("resolve_theme"))
        );
        assert!(
            runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::Add && t.contains("false"))
        );
        assert_eq!(runs[1][0], (RunKind::Add, "    ".to_string()));
        assert_eq!(runs[1][1], (RunKind::Ctx, "    ".to_string()));
    }

    #[test]
    fn test_align_window_2d_tape_5x1_collapse_mod_at_slot_2_with_interior_ws_bridge() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        let o0 = make_del(42, "#[serde(");
        let o1 = make_del(43, "    tag = \"kind\",");
        let o2 = make_del(44, "    rename_all = \"camelCase\",");
        let o3 = make_del(45, ")]");
        let n0 = make_add(97, "#[serde(tag = \"kind\", rename_all = \"camelCase\")]");
        let pairs: Vec<SlotPair<'_>> = vec![
            (Some(&o0), None),
            (Some(&o1), None),
            (Some(&o2), Some(&n0)),
            (Some(&o3), None),
        ];

        let runs = align_window_2d_tape(&pairs, rs).expect("2D collapse succeeds");
        assert_2d_conservation_and_projection(&pairs, &runs);

        // Interior spaces inside `tag = "kind",` bridge into one clean moved
        // phrase rather than splitting into red/green space flashes.
        assert!(
            runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t == "tag = \"kind\","),
            "expected bridged phrase on slot 1, got: {:?}",
            runs[1]
        );
        assert!(
            runs[0]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t == "#[serde("),
            "expected moved opener on slot 0, got: {:?}",
            runs[0]
        );
        assert!(
            runs[3]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedSrc && t == ")]"),
            "expected moved closer on slot 3, got: {:?}",
            runs[3]
        );
    }

    #[test]
    fn test_align_window_2d_tape_general_nxm_and_pre_moved_and_fallbacks() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // General 2x3 monotone reflow.
        let o0 = make_del(10, "        let alpha = compute(first, second);");
        let o1 = make_del(11, "        let beta = finish(alpha);");
        let n0 = make_add(10, "    let alpha = compute(");
        let n1 = make_add(11, "        first, second");
        let n2 = make_add(12, "    );");
        let n3 = make_add(13, "    let beta = finish(alpha);");
        let pairs: Vec<SlotPair<'_>> = vec![
            (Some(&o0), Some(&n0)),
            (None, Some(&n1)),
            (None, Some(&n2)),
            (Some(&o1), Some(&n3)),
        ];
        let runs = align_window_2d_tape(&pairs, rs).expect("2x4 2D alignment succeeds");
        assert_2d_conservation_and_projection(&pairs, &runs);
        // Slot 3 has dedented leading spaces (`Del("    ")` followed by
        // coalesced `Ctx("    let beta = finish(alpha);")`).
        assert_eq!(runs[3][0], (RunKind::Del, "    ".to_string()));
        assert_eq!(
            runs[3][1],
            (RunKind::Ctx, "    let beta = finish(alpha);".to_string())
        );
        // Slot 1 receives `first, second` as MovedTgt from slot 0.
        assert!(
            runs[1]
                .iter()
                .any(|(k, t)| *k == RunKind::MovedTgt && t == "first, second"),
            "expected bridged moved args on slot 1, got: {:?}",
            runs[1]
        );

        // Single-sided window returns None.
        let del_only: Vec<SlotPair<'_>> = vec![(Some(&o0), None)];
        assert!(align_window_2d_tape(&del_only, rs).is_none());

        // Wholesale Mod slot triggers clean None fallback.
        let w_old = make_del(1, "    alpha_one");
        let w_new = make_add(1, "    beta_two");
        let wholesale_pairs: Vec<SlotPair<'_>> = vec![(Some(&w_old), Some(&w_new))];
        assert!(align_window_2d_tape(&wholesale_pairs, rs).is_none());

        // Pre-moved slot retains cached_runs untouched alongside normal slots.
        let mov_src = Row::MovSrc {
            line: crate::parse::DelLine::new(20, "    moved_line();"),
            spans: Spans::default(),
            cached_runs: vec![(RunKind::Ctx, "    moved_line();".to_string())],
            group: None,
        };
        let with_moved: Vec<SlotPair<'_>> = vec![(Some(&o0), Some(&n0)), (Some(&mov_src), None)];
        let moved_runs = align_window_2d_tape(&with_moved, rs).expect("preserves MovSrc slot");
        assert_eq!(
            moved_runs[1],
            vec![(RunKind::Ctx, "    moved_line();".to_string())]
        );
    }

    #[test]
    fn test_type_window_hybrid_2d_monotone_plus_non_monotone_transposition() {
        let rs = diffuse_syntax::registry::lang_profile("rs");
        // Reproduces `scrollbar-grab-drag` `geometry.rs` Hunk 1 (5x2):
        // Stage 1 (2D monotone tape) aligns `.round()` and both Mod slots;
        // Stage 2 (residual cross-slot relocation) catches the transposed
        // `Some(` crossing `((vh ...)` and closing `)`.
        let dels = vec![
            make_del(231, "        Some("),
            make_del(
                232,
                "            ((vh as f64 / total_vis_lines as f64) * track_h as f64)",
            ),
            make_del(233, "                .round()"),
            make_del(
                234,
                "                .clamp(2.0, max_thumb as f64) as usize,",
            ),
            make_del(235, "        )"),
        ];
        let adds = vec![
            make_add(
                243,
                "        let prop_thumb = ((vh as f64 / total_vis_lines as f64) * track_h as f64).round() as usize;",
            ),
            make_add(244, "        Some(prop_thumb.clamp(min_thumb, max_thumb))"),
        ];

        let mut res = type_window(GroupId(1), &dels, &adds, &[(1, 0), (3, 1)], rs);
        res.repair_unmatched_moves();
        res.assert_token_conservation();

        let moved_src: String = res
            .slots
            .iter()
            .flat_map(|s| s.runs.iter())
            .filter(|(k, _)| *k == RunKind::MovedSrc)
            .map(|(_, t)| t.as_str())
            .collect();
        assert!(
            moved_src.contains("Some(")
                && moved_src.contains(".round()")
                && moved_src.contains("as usize")
                && moved_src.contains(')'),
            "hybrid 2D + residual relocation must capture both monotone (.round()) and transposed (Some(, as usize, )) moves, got: {moved_src:?}"
        );
    }
}
