//! Closed-economy token typing and window slot layout for peer reflow
//! formations.

use diffuse_syntax::profile::LangProfile;

use crate::{
    collapse::{
        evidence::{is_admissible_mod_pair, is_statement_head_keyword},
        formation::Formation,
        frame::{is_open_continuation_line, robust_delim_delta},
        tokens::is_salient_word_match,
    },
    lcs::{spans::word_spans_and_runs_with_profile, tokenize_line},
    merge::RunKind,
    parse::{GroupId, PeerGroup, Row, RowKind, Spans},
};

/// One styled run of a slot's text: a kind plus its exact source text.
type Runs = Vec<(RunKind, String)>;

/// A slot's runs split across several lines, one entry per line.
type LineRuns = Vec<Runs>;

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
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> PeerGroupRows {
    let formation = Formation {
        id: group_id,
        old_range: 0..old_rows.len(),
        new_range: 0..new_rows.len(),
    };
    let mut aligned_pairs: Vec<SlotPair<'_>> = Vec::with_capacity(old_rows.len() + new_rows.len());
    let (mut o, mut n) = (0usize, 0usize);
    let mut pi = 0usize;
    while o < old_rows.len() || n < new_rows.len() {
        if let Some(&(po, pn)) = pairs.get(pi) {
            if (o, n) == (po, pn) {
                aligned_pairs.push((Some(&old_rows[o]), Some(&new_rows[n]), None));
                o += 1;
                n += 1;
                pi += 1;
                continue;
            }
            if o < po {
                aligned_pairs.push((Some(&old_rows[o]), None, None));
                o += 1;
                continue;
            }
            if n < pn {
                aligned_pairs.push((None, Some(&new_rows[n]), None));
                n += 1;
                continue;
            }
        }
        if o < old_rows.len() {
            aligned_pairs.push((Some(&old_rows[o]), None, None));
            o += 1;
        } else {
            aligned_pairs.push((None, Some(&new_rows[n]), None));
            n += 1;
        }
    }
    type_slot_pairs(
        aligned_pairs,
        &formation,
        old_rows,
        new_rows,
        old_content,
        new_content,
        profile,
    )
}

/// Types an ordered slot list into classified rows and, when required, the
/// formation that holds those rows.
///
/// Slot alignment, not membership, is this function's subject: the caller has
/// already decided which rows belong together, which is what lets a segmented
/// window be typed without a second alignment pass over the cluster.
#[allow(clippy::too_many_arguments)]
fn type_slot_pairs(
    mut aligned_pairs: Vec<SlotPair<'_>>,
    group: &Formation,
    old_rows: &[Row],
    new_rows: &[Row],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> PeerGroupRows {
    let group_old = &old_rows[group.old_range.clone()];
    let group_new = &new_rows[group.new_range.clone()];
    let n = group_old.len();
    let m = group_new.len();

    if n == 1
        && m > 1
        && aligned_pairs.len() == m
        && aligned_pairs[0].0.is_some()
        && aligned_pairs[0].1.is_some()
        && group_old[0].text().trim() != group_new[0].text().trim()
        && !group_old.iter().any(Row::is_moved)
        && !group_new.iter().any(Row::is_moved)
        && let Some(rows) = align_expansion_1_to_m(group, &group_old[0], group_new, profile)
    {
        return rows;
    }

    if n > 1
        && m == 1
        && aligned_pairs.len() == n
        && aligned_pairs[0].0.is_some()
        && aligned_pairs[0].1.is_some()
        && group_old[0].text().trim() != group_new[0].text().trim()
        && !group_old.iter().any(Row::is_moved)
        && !group_new.iter().any(Row::is_moved)
        && let Some(rows) = align_collapse_n_to_1(group, group_old, &group_new[0], profile)
    {
        return rows;
    }

    // Phase-4 post-DP absorption: fold stranded continuation orphans into
    // adjacent Mods without re-routing the DP alignment.
    fold_tail_orphans(&mut aligned_pairs, old_content, new_content, profile);

    let total_slots = aligned_pairs.len();

    // 2. Intra-slot word diff and initial runs
    let mut slot_runs: Vec<Vec<(RunKind, String)>> = Vec::with_capacity(total_slots);
    let is_moved_slot: Vec<bool> = aligned_pairs
        .iter()
        .map(|(old_opt, new_opt, _)| {
            old_opt.is_some_and(|r| matches!(r, Row::MovSrc { .. }))
                || new_opt.is_some_and(|r| matches!(r, Row::MovTgt { .. }))
        })
        .collect();

    for (old_opt, new_opt, pre_runs) in &aligned_pairs {
        // Phase-4 multi-line slots carry their `\n`-split runs directly:
        // shared text is already `MovedSrc`/`MovedTgt`, never bare bait.
        if let Some(runs) = pre_runs {
            slot_runs.push(runs.clone());
            continue;
        }
        match (old_opt, new_opt) {
            (Some(u), Some(v)) => {
                if u.text() == v.text() {
                    slot_runs.push(vec![(RunKind::Ctx, u.text().to_string())]);
                } else {
                    let (_, runs) = word_spans_and_runs_with_profile(u.text(), v.text(), profile);
                    slot_runs.push(runs);
                }
            }
            (Some(u), None) => {
                if let Row::MovSrc { cached_runs, .. } = u {
                    slot_runs.push(cached_runs.clone());
                } else {
                    slot_runs.push(vec![(RunKind::Del, u.text().to_string())]);
                }
            }
            (None, Some(v)) => {
                if let Row::MovTgt { cached_runs, .. } = v {
                    slot_runs.push(cached_runs.clone());
                } else {
                    slot_runs.push(vec![(RunKind::Add, v.text().to_string())]);
                }
            }
            (None, None) => unreachable!(),
        }
    }

    // 3. Cross-slot token relocation (when total_slots > 1)
    if total_slots > 1 {
        let is_pure_add: Vec<bool> = aligned_pairs
            .iter()
            .map(|(old_opt, new_opt, _)| old_opt.is_none() && new_opt.is_some())
            .collect();
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);
        // Phase 6: leftover frame completion runs strictly last, over the
        // stranded runs the earlier passes left behind. Pure-Add coverage
        // repair below still applies to `relocate_cross_slot_tokens`
        // exchanges only; leftover retypes are pairwise by construction.
        relocate_leftover_frames(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);
    }

    // 4. Construct final AlignedSlotRow list
    let mut slots = Vec::with_capacity(total_slots);
    for (idx, ((old_opt, new_opt, _), runs)) in aligned_pairs.into_iter().zip(slot_runs).enumerate()
    {
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
                id: group.id,
                slot: idx,
                n,
                m,
            },
        });
    }

    PeerGroupRows {
        group_id: group.id,
        slots,
    }
}

fn combine_mod_runs_expansion(
    u_runs: &[(RunKind, String)],
    v0_runs: &[(RunKind, String)],
) -> Vec<(RunKind, String)> {
    let mut out = Vec::new();
    let mut v_idx = 0;

    let push = |out: &mut Vec<(RunKind, String)>, kind: RunKind, text: &str| {
        if text.is_empty() {
            return;
        }
        if let Some((last_k, last_t)) = out.last_mut()
            && *last_k == kind
        {
            last_t.push_str(text);
            return;
        }
        out.push((kind, text.to_string()));
    };

    for (u_kind, u_text) in u_runs {
        match u_kind {
            RunKind::Ctx => {
                while v_idx < v0_runs.len() && v0_runs[v_idx].0 == RunKind::Add {
                    push(&mut out, RunKind::Add, &v0_runs[v_idx].1);
                    v_idx += 1;
                }
                if v_idx < v0_runs.len() && v0_runs[v_idx].0 == RunKind::Ctx {
                    v_idx += 1;
                }
                push(&mut out, RunKind::Ctx, u_text);
            }
            RunKind::Del => {
                push(&mut out, RunKind::Del, u_text);
            }
            RunKind::MovedSrc => {
                out.push((RunKind::MovedSrc, u_text.clone()));
            }
            _ => {}
        }
    }
    while v_idx < v0_runs.len() {
        if v0_runs[v_idx].0 == RunKind::Add {
            push(&mut out, RunKind::Add, &v0_runs[v_idx].1);
        }
        v_idx += 1;
    }
    out
}

/// Splits `\n`-joined expansion runs back across the `m` constituent new
/// lines. Line-0 overlap stays `Ctx` on both sides; later-line overlap
/// becomes `MovedSrc` (Mod) / `MovedTgt` (continuation Add) pairs, which
/// keeps the closed economy balanced by construction and hides the shared
/// runs from cross-slot relocation gathering (it only collects `Del`/`Add`).
fn distribute_expansion_runs(full_runs: &[(RunKind, String)], m: usize) -> (Runs, LineRuns) {
    let mut u_runs: Vec<(RunKind, String)> = Vec::new();
    let mut v_runs: Vec<Vec<(RunKind, String)>> = vec![Vec::new(); m];
    let mut current_line = 0usize;
    let mut line_0_had_add = false;

    let push = |runs: &mut Vec<(RunKind, String)>, kind: RunKind, text: &str| {
        if text.is_empty() {
            return;
        }
        if let Some((last_k, last_t)) = runs.last_mut()
            && *last_k == kind
        {
            last_t.push_str(text);
            return;
        }
        runs.push((kind, text.to_string()));
    };

    for (kind, text) in full_runs {
        match kind {
            RunKind::Del => {
                push(&mut u_runs, RunKind::Del, text.as_str());
            }
            RunKind::Add => {
                if current_line == 0 {
                    line_0_had_add = true;
                }
                let mut lines = text.split('\n');
                if let Some(first) = lines.next()
                    && current_line < m
                {
                    push(&mut v_runs[current_line], RunKind::Add, first);
                }
                for next_part in lines {
                    current_line += 1;
                    if current_line < m {
                        push(&mut v_runs[current_line], RunKind::Add, next_part);
                    }
                }
            }
            RunKind::Ctx => {
                if current_line == 0 {
                    if line_0_had_add {
                        u_runs.push((RunKind::Ctx, text.clone()));
                        line_0_had_add = false;
                    } else {
                        push(&mut u_runs, RunKind::Ctx, text.as_str());
                    }
                    push(&mut v_runs[0], RunKind::Ctx, text.as_str());
                } else {
                    u_runs.push((RunKind::MovedSrc, text.clone()));
                    if current_line < m {
                        v_runs[current_line].push((RunKind::MovedTgt, text.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    (u_runs, v_runs)
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

fn align_expansion_1_to_m(
    group: &Formation,
    old_row: &Row,
    new_rows: &[Row],
    profile: Option<&LangProfile>,
) -> Option<PeerGroupRows> {
    let m = new_rows.len();
    let u = old_row.text();
    let v_texts: Vec<&str> = new_rows.iter().map(Row::text).collect();
    let joined = v_texts.join("\n");
    let (_, full_runs) = word_spans_and_runs_with_profile(u, &joined, profile);

    let (u_runs, mut v_runs) = distribute_expansion_runs(&full_runs, m);

    let slot_0_runs = combine_mod_runs_expansion(&u_runs, &v_runs[0]);
    if slot_runs_are_wholesale(&slot_0_runs, old_row.text(), new_rows[0].text()) {
        return None;
    }
    let mut slots = Vec::with_capacity(m);

    slots.push(AlignedSlotRow {
        slot_index: 0,
        old_line: old_row.old_line(),
        new_line: new_rows[0].new_line(),
        old_text: Some(old_row.text().to_string()),
        new_text: Some(new_rows[0].text().to_string()),
        kind: RowKind::Mod,
        is_moved_src: false,
        is_moved_tgt: false,
        spans: compute_slot_spans(&slot_0_runs),
        runs: slot_0_runs,
        group: PeerGroup {
            id: group.id,
            slot: 0,
            n: 1,
            m,
        },
    });

    for k in 1..m {
        let runs = std::mem::take(&mut v_runs[k]);
        let spans = compute_slot_spans(&runs);
        slots.push(AlignedSlotRow {
            slot_index: k,
            old_line: None,
            new_line: new_rows[k].new_line(),
            old_text: None,
            new_text: Some(new_rows[k].text().to_string()),
            kind: RowKind::Add,
            is_moved_src: false,
            is_moved_tgt: false,
            spans,
            runs,
            group: PeerGroup {
                id: group.id,
                slot: k,
                n: 1,
                m,
            },
        });
    }

    Some(PeerGroupRows {
        group_id: group.id,
        slots,
    })
}

fn combine_mod_runs_collapse(
    u0_runs: &[(RunKind, String)],
    v_runs: &[(RunKind, String)],
) -> Vec<(RunKind, String)> {
    let mut out = Vec::new();
    let mut u_idx = 0;

    let push = |out: &mut Vec<(RunKind, String)>, kind: RunKind, text: &str| {
        if text.is_empty() {
            return;
        }
        if let Some((last_k, last_t)) = out.last_mut()
            && *last_k == kind
        {
            last_t.push_str(text);
            return;
        }
        out.push((kind, text.to_string()));
    };

    for (v_kind, v_text) in v_runs {
        match v_kind {
            RunKind::Ctx => {
                while u_idx < u0_runs.len() && u0_runs[u_idx].0 == RunKind::Del {
                    push(&mut out, RunKind::Del, &u0_runs[u_idx].1);
                    u_idx += 1;
                }
                if u_idx < u0_runs.len() && u0_runs[u_idx].0 == RunKind::Ctx {
                    u_idx += 1;
                }
                push(&mut out, RunKind::Ctx, v_text);
            }
            RunKind::Add => {
                push(&mut out, RunKind::Add, v_text);
            }
            RunKind::MovedTgt => {
                out.push((RunKind::MovedTgt, v_text.clone()));
            }
            _ => {}
        }
    }
    while u_idx < u0_runs.len() {
        if u0_runs[u_idx].0 == RunKind::Del {
            push(&mut out, RunKind::Del, &u0_runs[u_idx].1);
        }
        u_idx += 1;
    }
    out
}

/// Splits `\n`-joined collapse runs back across the `n` constituent old
/// lines. Line-0 overlap stays `Ctx` on both sides; later-line overlap
/// becomes `MovedSrc` (continuation Del) / `MovedTgt` (Mod) pairs, which
/// keeps the closed economy balanced by construction and hides the shared
/// runs from cross-slot relocation gathering (it only collects `Del`/`Add`).
fn distribute_collapse_runs(full_runs: &[(RunKind, String)], n: usize) -> (LineRuns, Runs) {
    let mut u_runs: Vec<Vec<(RunKind, String)>> = vec![Vec::new(); n];
    let mut v_runs: Vec<(RunKind, String)> = Vec::new();
    let mut current_line = 0usize;
    let mut line_0_had_del = false;

    let push = |runs: &mut Vec<(RunKind, String)>, kind: RunKind, text: &str| {
        if text.is_empty() {
            return;
        }
        if let Some((last_k, last_t)) = runs.last_mut()
            && *last_k == kind
        {
            last_t.push_str(text);
            return;
        }
        runs.push((kind, text.to_string()));
    };

    for (kind, text) in full_runs {
        match kind {
            RunKind::Add => {
                push(&mut v_runs, RunKind::Add, text.as_str());
            }
            RunKind::Del => {
                if current_line == 0 {
                    line_0_had_del = true;
                }
                let mut lines = text.split('\n');
                if let Some(first) = lines.next()
                    && current_line < n
                {
                    push(&mut u_runs[current_line], RunKind::Del, first);
                }
                for next_part in lines {
                    current_line += 1;
                    if current_line < n {
                        push(&mut u_runs[current_line], RunKind::Del, next_part);
                    }
                }
            }
            RunKind::Ctx => {
                if current_line == 0 {
                    push(&mut u_runs[0], RunKind::Ctx, text.as_str());
                    if line_0_had_del {
                        v_runs.push((RunKind::Ctx, text.clone()));
                        line_0_had_del = false;
                    } else {
                        push(&mut v_runs, RunKind::Ctx, text.as_str());
                    }
                } else {
                    if current_line < n {
                        u_runs[current_line].push((RunKind::MovedSrc, text.clone()));
                    }
                    v_runs.push((RunKind::MovedTgt, text.clone()));
                }
            }
            _ => {}
        }
    }

    (u_runs, v_runs)
}

fn align_collapse_n_to_1(
    group: &Formation,
    old_rows: &[Row],
    new_row: &Row,
    profile: Option<&LangProfile>,
) -> Option<PeerGroupRows> {
    let n = old_rows.len();
    let v = new_row.text();
    let u_texts: Vec<&str> = old_rows.iter().map(Row::text).collect();
    let joined = u_texts.join("\n");
    let (_, full_runs) = word_spans_and_runs_with_profile(&joined, v, profile);

    let (mut u_runs, v_runs) = distribute_collapse_runs(&full_runs, n);

    let slot_0_runs = combine_mod_runs_collapse(&u_runs[0], &v_runs);
    if slot_runs_are_wholesale(&slot_0_runs, old_rows[0].text(), new_row.text()) {
        return None;
    }
    let mut slots = Vec::with_capacity(n);

    slots.push(AlignedSlotRow {
        slot_index: 0,
        old_line: old_rows[0].old_line(),
        new_line: new_row.new_line(),
        old_text: Some(old_rows[0].text().to_string()),
        new_text: Some(new_row.text().to_string()),
        kind: RowKind::Mod,
        is_moved_src: false,
        is_moved_tgt: false,
        spans: compute_slot_spans(&slot_0_runs),
        runs: slot_0_runs,
        group: PeerGroup {
            id: group.id,
            slot: 0,
            n,
            m: 1,
        },
    });

    for k in 1..n {
        let runs = std::mem::take(&mut u_runs[k]);
        let spans = compute_slot_spans(&runs);
        slots.push(AlignedSlotRow {
            slot_index: k,
            old_line: old_rows[k].old_line(),
            new_line: None,
            old_text: Some(old_rows[k].text().to_string()),
            new_text: None,
            kind: RowKind::Del,
            is_moved_src: false,
            is_moved_tgt: false,
            spans,
            runs,
            group: PeerGroup {
                id: group.id,
                slot: k,
                n,
                m: 1,
            },
        });
    }

    Some(PeerGroupRows {
        group_id: group.id,
        slots,
    })
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

/// Determines if a pair of deletion and addition lines is admissible for
/// slot-level Mod pairing, checking wholesale bounds, declaration
/// compatibility, and verbatim rivals.
/// Whether two trimmed lines share a statement head (`fn`/`let`/`pub`…)
/// or an assignment key (`key = …`). Such a head alone must not anchor a
/// pairing.
fn shares_statement_head(u_trim: &str, v_trim: &str, profile: Option<&LangProfile>) -> bool {
    if let (Some(u_eq), Some(v_eq)) = (u_trim.find('='), v_trim.find('=')) {
        let u_key = u_trim[..u_eq].trim();
        let v_key = v_trim[..v_eq].trim();
        if !u_key.is_empty() && u_key == v_key {
            return true;
        }
    }
    match (
        diffuse_syntax::prelude::first_word(u_trim),
        diffuse_syntax::prelude::first_word(v_trim),
    ) {
        (Some(u_h), Some(v_h)) => u_h == v_h && is_statement_head_keyword(u_h, profile),
        _ => false,
    }
}

/// `enforce_head_gate` is true only for singleton (1×1) groups. In a
/// compound group the DP argmax weighs shape and position evidence across
/// competing candidates — a zero-overlap winner there is usually a rename
/// (e.g. `let mut s = String::new();` → `let lines = …`), and vetoing it
/// merely promotes a worse, coincidentally-overlapping rival (which can
/// even collapse into a wholesale Mod). In a singleton group there is no
/// competition, so a shared head with zero salient support dissolves.
fn is_admissible_slot_pair(
    u_text: &str,
    v_text: &str,
    spans: &crate::lcs::WordSpans,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
    enforce_head_gate: bool,
) -> bool {
    let u_trim = u_text.trim();
    let v_trim = v_text.trim();

    let u_decl_role = crate::collapse::residual::declaration_role(u_trim);
    let v_decl_role = crate::collapse::residual::declaration_role(v_trim);

    // For declaration-type lines, a verbatim twin elsewhere in the
    // file content signals relocation; block pairing independently of
    // admissibility so that e.g. `pub enum Foo {` is not paired with
    // `pub struct Bar {` when `pub enum Foo {` still exists in the
    // new snapshot.
    let decl_has_rival = u_decl_role.is_some()
        && v_decl_role.is_some()
        && (crate::collapse::residual::has_verbatim_rival(u_trim, new_content)
            || crate::collapse::residual::has_verbatim_rival(v_trim, old_content));

    let is_decl_compat = !decl_has_rival
        && if let (Some(d_role), Some(a_role)) = (u_decl_role, v_decl_role) {
            crate::collapse::residual::are_roles_compatible(d_role, a_role)
                && u_trim.ends_with('{') == v_trim.ends_with('{')
        } else {
            false
        };

    if decl_has_rival || crate::merge::is_wholesale_replacement_spans(u_text, v_text, spans) {
        return false;
    }
    if !(is_admissible_mod_pair(u_text, v_text, profile) || is_decl_compat) {
        return false;
    }
    // A shared statement head alone must not anchor a singleton pairing:
    // the 1×1 fast path bypasses the micro-DP argmax, so without at least
    // one shared salient token, `fn row() {` vs `fn total() {` dissolves
    // to Del + Add. Pairs with aligned salient context keep the
    // historical behavior; headless pairs are untouched here. A verbatim
    // reindent (`return None` at a new indent) always pairs: there is no
    // content difference for a salient token to witness.
    let is_cross_type_decl = is_decl_compat && u_decl_role != v_decl_role;
    if !enforce_head_gate
        || is_cross_type_decl
        || u_trim == v_trim
        || !shares_statement_head(u_trim, v_trim, profile)
    {
        return true;
    }
    let (_, runs) = crate::lcs::word_spans_and_runs_with_profile(u_text, v_text, profile);
    let has_salient_ctx = runs.iter().any(|(kind, text)| {
        *kind == crate::merge::RunKind::Ctx && text_has_salient_token(text, profile)
    });
    if has_salient_ctx {
        return true;
    }
    let v_tokens: Vec<&str> = crate::lcs::tokenize_line(v_text).collect();
    let u_tokens: Vec<&str> = crate::lcs::tokenize_line(u_text).collect();
    u_tokens
        .iter()
        .enumerate()
        .filter(|&(i, _)| crate::lcs::is_token_salient_in_context(&u_tokens, i, profile))
        .any(|(_, t)| v_tokens.contains(t))
}

/// Evidence that `head` opens a multi-line construct continued on a later
/// line: unclosed delimiters or an open operator ending (mirrors the
/// cluster-factoring continuation guard). Block openers (`{`) count too:
/// a flattened `fn f() { body(); }` is a genuine 2-to-1 collapse, and the
/// coverage gate — not this pre-filter — rejects merged independents.
fn is_expression_head_open(trim: &str, delim: i32) -> bool {
    delim > 0
        || is_open_continuation_line(trim)
        || trim.ends_with(',')
        || trim.ends_with("&&")
        || trim.ends_with("||")
}

/// Evidence that a line is the tail of a multi-line expression, using the
/// Invariant-2 continuation vocabulary: `&&`, `||`, or a chaining `.`
/// (which can never start a standalone statement).
fn is_expression_tail(trim: &str) -> bool {
    trim.starts_with("&&")
        || trim.starts_with("||")
        || (trim.starts_with('.') && !trim.starts_with(".."))
}

/// Strong expression-head evidence: an unclosed `(`/`[` (block `{`
/// openers start a new unit — see below) or an open operator ending.
/// A structurally open head proves continuation on its own; weaker
/// evidence (trailing `{`/`,`/`&&`/`||` on balanced lines) needs the
/// bilateral crutch: the other side must be substantially explained too,
/// otherwise a sibling line (next struct field, next arg) gets absorbed.
fn is_strong_expression_head(trim: &str, delim: i32) -> bool {
    (delim > 0 && !trim.ends_with('{')) || is_open_continuation_line(trim)
}

/// Gates a `\n`-joined multi-line side against a single-line side for
/// post-DP absorption. Mirrors the diag scorer's gates (token-overlap
/// floor, admissibility, salient minimum) plus the Phase-4 coverage gate:
/// the joined side must be substantially explained, otherwise two
/// independent statements would merge into a Frankenstein Mod. Returns
/// the matched and other-side character counts for the bilateral check.
fn joined_alignment(
    joined: &str,
    other: &str,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Option<(usize, usize)> {
    let joined_trim = joined.trim();
    let other_trim = other.trim();
    if joined_trim.is_empty() || other_trim.is_empty() {
        return None;
    }
    let joined_tokens: Vec<&str> = tokenize_line(joined).collect();
    let other_tokens: Vec<&str> = tokenize_line(other).collect();
    let shared_bound: usize = joined_tokens
        .iter()
        .filter(|t| !t.trim().is_empty() && other_tokens.contains(t))
        .map(|t| t.chars().count())
        .sum();
    if shared_bound < 3 {
        return None;
    }
    let spans = crate::lcs::word_spans(joined, other);
    if !is_admissible_slot_pair(
        joined,
        other,
        &spans,
        old_content,
        new_content,
        profile,
        false,
    ) {
        return None;
    }
    let runs = crate::merge::merged_runs_from_texts(joined, other, &spans);
    let matched_chars: usize = runs
        .iter()
        .filter(|r| r.kind == crate::merge::RunKind::Ctx && !r.text.trim().is_empty())
        .map(|r| r.text.chars().count())
        .sum();
    if matched_chars < 3 {
        return None;
    }
    let joined_chars = joined_trim.chars().count();
    let other_chars = other_trim.chars().count();
    let coverage = matched_chars as f64 / joined_chars.max(1) as f64;
    // Strict bar: a wrongly-folded orphan is worse than the honest orphan
    // it replaced (its text would be double-attributed and then scavenged).
    // Only near-complete reflows may fold; partial overlaps stay 1-to-1
    // for the evidence-gated relocation stages to handle. The bar sits
    // below 1.0 to tolerate single-char tokenizer noise on delimiters.
    if coverage < 0.85 {
        return None;
    }
    Some((matched_chars, other_chars))
}

/// A micro-DP aligned pair plus optional precomputed runs for Phase-4
/// post-DP folds: absorbed 2-to-1 collapse and 1-to-2 expansion slots carry
/// their `\n`-split runs (shared text as `MovedSrc`/`MovedTgt`, never bare
/// `Del`/`Add` bait), while 1-to-1 slots carry `None` and compute runs from
/// their paired texts in the slot-typing pass.
///
/// Fold runs must never contain the `\n` join artifact: per-line runs feed
/// `slots_project_to_text`, and a stray newline breaks the projection and
/// dissolves the whole group into raw rows. The join side's unmatched text
/// can straddle the boundary inside an `Add` (expansion) or `Del`
/// (collapse) run, so the folds strip it after distributing.
type SlotPair<'a> = (
    Option<&'a Row>,
    Option<&'a Row>,
    Option<Vec<(RunKind, String)>>,
);

/// Removes the `\n` join artifact from distributed fold runs (see
/// `SlotPair`): unmatched join-side text can straddle the line boundary
/// inside one run, and the newline is not part of any real line. Runs left
/// empty are dropped and same-kind neighbors re-coalesced.
fn strip_join_newlines(runs: &mut [Runs]) {
    for line_runs in runs.iter_mut() {
        let mut clean = Vec::with_capacity(line_runs.len());
        for (kind, text) in line_runs.drain(..) {
            let stripped: String = text.chars().filter(|c| *c != '\n').collect();
            if !stripped.is_empty() {
                clean.push((kind, stripped));
            }
        }
        coalesce_runs(&mut clean);
        *line_runs = clean;
    }
}

/// Verifies folded runs project back to their slot texts, mirroring the
/// group DBC `slots_project_to_text`: the distribute+combine machinery
/// assumes a clean split at the join boundary, and cross-boundary shared
/// text can scramble the Mod's run order (see the `len,` expansion case).
/// A garbled fold abstains instead of tripping the group check, which
/// would dissolve the whole group into raw rows.
fn folded_runs_project(
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

/// Phase-4 post-DP absorption: folds a stranded continuation orphan into
/// an adjacent Mod slot without re-routing the DP alignment.
///
/// In-DP multi-line transitions were tried and abandoned: letting a joined
/// pair compete in the argmax perturbs the global path, stranding lines
/// whose rightful partners were absorbed elsewhere (and tripping DBC
/// invariants downstream). Repairing the already-chosen alignment instead
/// — only ever converting (Mod + honest orphan) into (Mod + grouped
/// continuation) — cannot steal partners, and keeps the closed economy
/// balanced by construction (shared text becomes `MovedSrc`/`MovedTgt`).
fn fold_tail_orphans(
    pairs: &mut [SlotPair<'_>],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) {
    let mut idx = 0;
    while idx + 1 < pairs.len() {
        // Each fold consumes its two slots; advance past them either way so
        // a continuation line folds into exactly one Mod.
        idx += if fold_window(&mut pairs[idx..idx + 2], old_content, new_content, profile) {
            2
        } else {
            1
        };
    }
}

/// Attempts the collapse and expansion folds on one adjacent slot pair.
fn fold_window(
    window: &mut [SlotPair<'_>],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> bool {
    fold_collapse_window(window, old_content, new_content, profile)
        || fold_expansion_window(window, old_content, new_content, profile)
}

/// Folds `(Mod(u, v), Del(u'))` into a joined Mod when `u'` directly
/// continues `u`'s expression and `v` covers the joined text.
fn fold_collapse_window(
    window: &mut [SlotPair<'_>],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> bool {
    if window.len() != 2 || window[0].2.is_some() || window[1].2.is_some() {
        return false;
    }
    let (Some(u), Some(v)) = (window[0].0, window[0].1) else {
        return false;
    };
    let (Some(up), None) = (window[1].0, window[1].1) else {
        return false;
    };
    if u.is_moved() || up.is_moved() || v.is_moved() {
        return false;
    }
    // The orphan must continue the Mod's old line directly.
    if up.old_line() != u.old_line().map(|n| n + 1) {
        return false;
    }
    let u_text = u.text();
    let up_text = up.text();
    let v_text = v.text();
    let u_trim = u_text.trim();
    let up_trim = up_text.trim();
    // Delimiter-soup orphans (`,`, `;`, `}],`) always "cover" trivially
    // and their shared glyphs double-attribute (struck in the Mod and
    // moved into it). Only lexical continuations may fold.
    if up_trim.chars().count() < 3 || !up_trim.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let u_delim = robust_delim_delta(u_trim);
    if !(is_expression_head_open(u_trim, u_delim) || is_expression_tail(up_trim)) {
        return false;
    }
    let joined = format!("{u_text}\n{up_text}");
    let Some((matched, other_chars)) =
        joined_alignment(&joined, v_text, old_content, new_content, profile)
    else {
        return false;
    };
    // Weak head evidence (a balanced line ending in `{`/`,`/`&&`/`||`)
    // may merely separate siblings: demand the other side be
    // substantially explained too.
    if !is_strong_expression_head(u_trim, u_delim)
        && (matched as f64 / other_chars.max(1) as f64) < 0.60
    {
        return false;
    }
    let (_, full_runs) = word_spans_and_runs_with_profile(&joined, v_text, profile);
    let (mut u_runs, v_runs) = distribute_collapse_runs(&full_runs, 2);
    strip_join_newlines(&mut u_runs);
    let mod_runs = combine_mod_runs_collapse(&u_runs[0], &v_runs);
    if slot_runs_are_wholesale(&mod_runs, u_text, v_text)
        || !folded_runs_project(&mod_runs, Some(u_text), Some(v_text))
        || !folded_runs_project(&u_runs[1], Some(up_text), None)
    {
        return false;
    }
    let cont_runs = u_runs[1].clone();
    window[0].2 = Some(mod_runs);
    window[1].2 = Some(cont_runs);
    true
}

/// Folds `(Mod(u, v), Add(v'))` into a joined Mod when `v'` directly
/// continues `v`'s expression and the joined text covers `u`.
fn fold_expansion_window(
    window: &mut [SlotPair<'_>],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> bool {
    if window.len() != 2 || window[0].2.is_some() || window[1].2.is_some() {
        return false;
    }
    let (Some(u), Some(v)) = (window[0].0, window[0].1) else {
        return false;
    };
    let (None, Some(vp)) = (window[1].0, window[1].1) else {
        return false;
    };
    if u.is_moved() || v.is_moved() || vp.is_moved() {
        return false;
    }
    // The orphan must continue the Mod's new line directly.
    if vp.new_line() != v.new_line().map(|n| n + 1) {
        return false;
    }
    let u_text = u.text();
    let v_text = v.text();
    let vp_text = vp.text();
    let v_trim = v_text.trim();
    let vp_trim = vp_text.trim();
    // Delimiter-soup orphans (`,`, `;`, `}],`) always "cover" trivially
    // and their shared glyphs double-attribute (struck in the Mod and
    // moved into it). Only lexical continuations may fold.
    if vp_trim.chars().count() < 3 || !vp_trim.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let v_delim = robust_delim_delta(v_trim);
    if !(is_expression_head_open(v_trim, v_delim) || is_expression_tail(vp_trim)) {
        return false;
    }
    let joined_new = format!("{v_text}\n{vp_text}");
    let Some((matched, other_chars)) =
        joined_alignment(u_text, &joined_new, old_content, new_content, profile)
    else {
        return false;
    };
    // Weak head evidence (a balanced line ending in `{`/`,`/`&&`/`||`)
    // may merely separate siblings: demand the other side be
    // substantially explained too.
    if !is_strong_expression_head(v_trim, v_delim)
        && (matched as f64 / other_chars.max(1) as f64) < 0.60
    {
        return false;
    }
    let (_, full_runs) = word_spans_and_runs_with_profile(u_text, &joined_new, profile);
    let (u_runs, mut v_runs) = distribute_expansion_runs(&full_runs, 2);
    strip_join_newlines(&mut v_runs);
    let mod_runs = combine_mod_runs_expansion(&u_runs, &v_runs[0]);
    if slot_runs_are_wholesale(&mod_runs, u_text, v_text)
        || !folded_runs_project(&mod_runs, Some(u_text), Some(v_text))
        || !folded_runs_project(&v_runs[1], None, Some(vp_text))
    {
        return false;
    }
    let cont_runs = v_runs[1].clone();
    window[0].2 = Some(mod_runs);
    window[1].2 = Some(cont_runs);
    true
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
                let spans = crate::lcs::word_spans(del_text, add_text);
                if !crate::merge::is_wholesale_replacement_spans(del_text, add_text, &spans) {
                    let runs = crate::merge::merged_runs_from_texts(del_text, add_text, &spans);
                    for r in runs {
                        if r.kind == crate::merge::RunKind::Ctx {
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

        let result = type_window(GroupId(1), &dels, &adds, &[(0, 0)], None, None, None);
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

        let result = type_window(GroupId(2), &dels, &adds, &[(0, 0)], None, None, None);
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

        let result = type_window(
            GroupId(3),
            &dels,
            &adds,
            &[(0, 0), (1, 1)],
            None,
            None,
            None,
        );
        assert_eq!(result.slots.len(), 3);
        assert_eq!(result.slots[0].kind, RowKind::Mod);
        assert_eq!(result.slots[1].kind, RowKind::Mod);
        assert_eq!(result.slots[2].kind, RowKind::Add);
        result.assert_token_conservation();
    }

    #[test]
    fn test_align_pure_add_and_del_groups() {
        let adds = vec![make_add(10, "line 1"), make_add(11, "line 2")];
        let add_res = type_window(GroupId(4), &[], &adds, &[], None, None, None);
        assert_eq!(add_res.slots.len(), 2);
        assert_eq!(add_res.slots[0].kind, RowKind::Add);
        assert_eq!(add_res.slots[1].kind, RowKind::Add);
        add_res.assert_token_conservation();

        let dels = vec![make_del(10, "del 1"), make_del(11, "del 2")];
        let del_res = type_window(GroupId(5), &dels, &[], &[], None, None, None);
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
        let res = type_window(GroupId(9), &dels, &adds, &[(0, 0)], None, None, None);
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
        let res = type_window(GroupId(1), &dels, &adds, &[(0, 0)], None, None, rs);
        assert!(
            !slot_runs_are_wholesale(&res.slots[0].runs, dels[0].text(), adds[0].text()),
            "slot 0 must never degrade into a wholesale Mod row: {:?}",
            res.slots[0].runs
        );
    }
}
