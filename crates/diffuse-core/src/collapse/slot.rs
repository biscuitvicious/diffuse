//! Micro slot-alignment DP and closed-economy token typing for peer reflow
//! groups.

use diffuse_syntax::profile::LangProfile;

use crate::{
    cluster::factor::{FactoredGroup, is_salient_word_match},
    lcs::{spans::word_spans_and_runs_with_profile, tokenize_line},
    merge::RunKind,
    parse::{GroupId, PeerGroup, Row, RowKind, Spans},
};

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

/// Aligns rows in a factored group into monotonic visual slots and performs
/// closed-economy token classification.
pub fn align_and_type_peer_group(
    group: &FactoredGroup,
    old_rows: &[Row],
    new_rows: &[Row],
    profile: Option<&LangProfile>,
) -> PeerGroupRows {
    align_and_type_peer_group_with_content(group, old_rows, new_rows, None, None, profile)
}

/// Aligns rows in a factored group into monotonic visual slots and performs
/// closed-economy token classification, aware of surrounding snapshot content.
pub fn align_and_type_peer_group_with_content(
    group: &FactoredGroup,
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

    // 1. Monotonic micro-DP slot alignment
    let aligned_pairs: Vec<(Option<&Row>, Option<&Row>)> = if n == 0 {
        group_new.iter().map(|r| (None, Some(r))).collect()
    } else if m == 0 {
        group_old.iter().map(|r| (Some(r), None)).collect()
    } else if n == 1 && m == 1 {
        if group_old[0].is_moved() || group_new[0].is_moved() {
            vec![(Some(&group_old[0]), None), (None, Some(&group_new[0]))]
        } else if group_old[0].text() == group_new[0].text() {
            let u = &group_old[0];
            let v = &group_new[0];
            let slot = AlignedSlotRow {
                slot_index: 0,
                old_line: u.old_line(),
                new_line: v.new_line(),
                old_text: Some(u.text().to_string()),
                new_text: Some(v.text().to_string()),
                kind: RowKind::Mod,
                is_moved_src: false,
                is_moved_tgt: false,
                runs: vec![(RunKind::Ctx, u.text().to_string())],
                spans: Spans::default(),
                group: PeerGroup {
                    id: group.id,
                    slot: 0,
                    n: 1,
                    m: 1,
                },
            };
            return PeerGroupRows {
                group_id: group.id,
                slots: vec![slot],
            };
        } else {
            let u_text = group_old[0].text();
            let v_text = group_new[0].text();
            let (spans, runs) = word_spans_and_runs_with_profile(u_text, v_text, profile);

            if !is_admissible_slot_pair(
                u_text,
                v_text,
                &spans,
                old_content,
                new_content,
                profile,
                true,
            ) {
                vec![(Some(&group_old[0]), None), (None, Some(&group_new[0]))]
            } else {
                let u = &group_old[0];
                let v = &group_new[0];
                let slot = AlignedSlotRow {
                    slot_index: 0,
                    old_line: u.old_line(),
                    new_line: v.new_line(),
                    old_text: Some(u_text.to_string()),
                    new_text: Some(v_text.to_string()),
                    kind: RowKind::Mod,
                    is_moved_src: false,
                    is_moved_tgt: false,
                    runs,
                    spans,
                    group: PeerGroup {
                        id: group.id,
                        slot: 0,
                        n: 1,
                        m: 1,
                    },
                };
                return PeerGroupRows {
                    group_id: group.id,
                    slots: vec![slot],
                };
            }
        }
    } else {
        let raw_pairs =
            align_slots_micro_dp(group_old, group_new, old_content, new_content, profile);
        canonicalize_slot_order(raw_pairs)
    };

    if n == 1
        && m > 1
        && aligned_pairs.len() == m
        && aligned_pairs[0].0.is_some()
        && aligned_pairs[0].1.is_some()
        && !group_old.iter().any(Row::is_moved)
        && !group_new.iter().any(Row::is_moved)
    {
        return align_expansion_1_to_m(group, &group_old[0], group_new, profile);
    }

    if n > 1
        && m == 1
        && aligned_pairs.len() == n
        && aligned_pairs[0].0.is_some()
        && aligned_pairs[0].1.is_some()
        && !group_old.iter().any(Row::is_moved)
        && !group_new.iter().any(Row::is_moved)
    {
        return align_collapse_n_to_1(group, group_old, &group_new[0], profile);
    }

    let total_slots = aligned_pairs.len();

    // 2. Intra-slot word diff and initial runs
    let mut slot_runs: Vec<Vec<(RunKind, String)>> = Vec::with_capacity(total_slots);
    let is_moved_slot: Vec<bool> = aligned_pairs
        .iter()
        .map(|(old_opt, new_opt)| {
            old_opt.is_some_and(|r| matches!(r, Row::MovSrc { .. }))
                || new_opt.is_some_and(|r| matches!(r, Row::MovTgt { .. }))
        })
        .collect();

    for (old_opt, new_opt) in &aligned_pairs {
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
            .map(|(old_opt, new_opt)| old_opt.is_none() && new_opt.is_some())
            .collect();
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);
    }

    // 4. Construct final AlignedSlotRow list
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

fn align_expansion_1_to_m(
    group: &FactoredGroup,
    old_row: &Row,
    new_rows: &[Row],
    profile: Option<&LangProfile>,
) -> PeerGroupRows {
    let m = new_rows.len();
    let u = old_row.text();
    let v_texts: Vec<&str> = new_rows.iter().map(Row::text).collect();
    let joined = v_texts.join("\n");
    let (_, full_runs) = word_spans_and_runs_with_profile(u, &joined, profile);

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
                push(&mut u_runs, RunKind::Del, &text);
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
                        u_runs.push((RunKind::Ctx, text.to_string()));
                        line_0_had_add = false;
                    } else {
                        push(&mut u_runs, RunKind::Ctx, &text);
                    }
                    push(&mut v_runs[0], RunKind::Ctx, &text);
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

    let slot_0_runs = combine_mod_runs_expansion(&u_runs, &v_runs[0]);
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

    PeerGroupRows {
        group_id: group.id,
        slots,
    }
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

fn align_collapse_n_to_1(
    group: &FactoredGroup,
    old_rows: &[Row],
    new_row: &Row,
    profile: Option<&LangProfile>,
) -> PeerGroupRows {
    let n = old_rows.len();
    let v = new_row.text();
    let u_texts: Vec<&str> = old_rows.iter().map(Row::text).collect();
    let joined = u_texts.join("\n");
    let (_, full_runs) = word_spans_and_runs_with_profile(&joined, v, profile);

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
                push(&mut v_runs, RunKind::Add, &text);
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
                    push(&mut u_runs[0], RunKind::Ctx, &text);
                    if line_0_had_del {
                        v_runs.push((RunKind::Ctx, text.to_string()));
                        line_0_had_del = false;
                    } else {
                        push(&mut v_runs, RunKind::Ctx, &text);
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

    let slot_0_runs = combine_mod_runs_collapse(&u_runs[0], &v_runs);
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

    PeerGroupRows {
        group_id: group.id,
        slots,
    }
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

fn canonicalize_slot_order<'a>(
    pairs: Vec<(Option<&'a Row>, Option<&'a Row>)>,
) -> Vec<(Option<&'a Row>, Option<&'a Row>)> {
    let mut out = Vec::with_capacity(pairs.len());
    let mut pending_dels = Vec::new();
    let mut pending_adds = Vec::new();

    for pair in pairs {
        if pair.0.is_some() && pair.1.is_some() {
            out.append(&mut pending_dels);
            out.append(&mut pending_adds);
            out.push(pair);
        } else if pair.0.is_some() {
            pending_dels.push(pair);
        } else if pair.1.is_some() {
            pending_adds.push(pair);
        }
    }

    out.append(&mut pending_dels);
    out.append(&mut pending_adds);
    out
}

/// Determines if a pair of deletion and addition lines is admissible for
/// slot-level Mod pairing, checking wholesale bounds, declaration
/// compatibility, and verbatim rivals.
/// Statement heads weak enough to need a shared salient token when they
/// are the only link between two lines (mirrors the micro-DP gate).
const STATEMENT_HEADS: &[&str] = &[
    "if", "fn", "let", "match", "for", "while", "return", "pub", "struct", "enum", "type", "use",
    "impl",
];

/// Whether two trimmed lines share a statement head (`fn`/`let`/`pub`…)
/// or an assignment key (`key = …`). Such a head alone must not anchor a
/// pairing.
fn shares_statement_head(u_trim: &str, v_trim: &str) -> bool {
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
        (Some(u_h), Some(v_h)) => u_h == v_h && STATEMENT_HEADS.contains(&u_h),
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
    if !(crate::cluster::dp::solver::is_admissible_mod_pair(u_text, v_text, profile)
        || is_decl_compat)
    {
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
        || !shares_statement_head(u_trim, v_trim)
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

fn align_slots_micro_dp<'a>(
    old_rows: &'a [Row],
    new_rows: &'a [Row],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> Vec<(Option<&'a Row>, Option<&'a Row>)> {
    let n = old_rows.len();
    let m = new_rows.len();

    let mut dp = vec![vec![(0i32, 0u8); m + 1]; n + 1];
    const GAP_PENALTY: i32 = 2;

    for (i, row) in dp.iter_mut().enumerate().take(n + 1).skip(1) {
        row[0] = (-(i as i32) * GAP_PENALTY, 2);
    }
    for (j, cell) in dp[0].iter_mut().enumerate().take(m + 1).skip(1) {
        *cell = (-(j as i32) * GAP_PENALTY, 3);
    }

    struct PrecomputedRow<'a> {
        text: &'a str,
        trim: &'a str,
        indent: usize,
        delim: i32,
        tokens: Vec<&'a str>,
        has_salient: bool,
        salient_chars: usize,
    }

    let precompute = |r: &'a Row| {
        let text = r.text();
        let trim = text.trim();
        let indent = text.len() - text.trim_start().len();
        let delim = crate::cluster::factor::robust_delim_delta(trim);
        let tokens: Vec<&'a str> = tokenize_line(text).collect();
        let mut has_salient = false;
        let mut salient_chars = 0;
        for i in 0..tokens.len() {
            if crate::lcs::is_token_salient_in_context(&tokens, i, profile) {
                has_salient = true;
                salient_chars += tokens[i].chars().count();
            }
        }
        PrecomputedRow {
            text,
            trim,
            indent,
            delim,
            tokens,
            has_salient,
            salient_chars,
        }
    };

    let old_info: Vec<PrecomputedRow> = old_rows.iter().map(&precompute).collect();
    let new_info: Vec<PrecomputedRow> = new_rows.iter().map(&precompute).collect();

    for i in 1..=n {
        let u = &old_info[i - 1];
        let u_text = u.text;
        let u_trim = u.trim;
        let u_indent = u.indent;

        for j in 1..=m {
            let v = &new_info[j - 1];
            let v_text = v.text;
            let v_trim = v.trim;
            let v_indent = v.indent;

            let indent_diff = (u_indent as i32 - v_indent as i32).abs();
            let diag_dist = ((i as i32) - (j as i32)).abs();
            let mut cached_runs: Option<Vec<crate::merge::Run<'a>>> = None;
            let mut match_score = if old_rows[i - 1].is_moved() || new_rows[j - 1].is_moved() {
                -1000
            } else if u_trim == v_trim {
                (u_trim.chars().count() as i32 * 10) + 1000 - (indent_diff * 2) - diag_dist
            } else if diag_dist > 8 {
                -1000
            } else if crate::lcs::is_affix_pair(u_trim, v_trim) {
                let common = u_trim.len().min(v_trim.len());
                (common as i32 * 10) + 900 - (indent_diff * 2) - diag_dist
            } else {
                let shared_bound: usize = u
                    .tokens
                    .iter()
                    .filter(|ut| !ut.trim().is_empty() && v.tokens.contains(ut))
                    .map(|ut| ut.chars().count())
                    .sum();
                if shared_bound < 3 {
                    -1000
                } else {
                    let spans = crate::lcs::word_spans(u_text, v_text);

                    if !is_admissible_slot_pair(
                        u_text,
                        v_text,
                        &spans,
                        old_content,
                        new_content,
                        profile,
                        false,
                    ) {
                        -1000
                    } else {
                        let runs = crate::merge::merged_runs_from_texts(u_text, v_text, &spans);
                        let matched_chars: usize = runs
                            .iter()
                            .filter(|r| {
                                r.kind == crate::merge::RunKind::Ctx && !r.text.trim().is_empty()
                            })
                            .map(|r| r.text.chars().count())
                            .sum();
                        let u_chars = u_trim.chars().count();
                        let v_chars = v_trim.chars().count();
                        let unmatched = if n == 1 && m > 1 {
                            v_chars.saturating_sub(matched_chars)
                        } else if n > 1 && m == 1 {
                            u_chars.saturating_sub(matched_chars)
                        } else {
                            u_chars.max(v_chars).saturating_sub(matched_chars)
                        };

                        let min_tokens = if n == 1 && m == 1 { 0 } else { 3 };
                        if matched_chars < min_tokens {
                            -1000
                        } else {
                            let mut score = (matched_chars as i32 * 10)
                                - (unmatched as i32 * 3)
                                - (indent_diff * 2)
                                - diag_dist;
                            let u_head = diffuse_syntax::prelude::first_word(u_trim);
                            let v_head = diffuse_syntax::prelude::first_word(v_trim);
                            if let (Some(u_h), Some(v_h)) = (u_head, v_head)
                                && u_h == v_h
                                && STATEMENT_HEADS.contains(&u_h)
                            {
                                score += 80;
                                if (u_trim.ends_with('{') && v_trim.ends_with('{'))
                                    || (u_trim.ends_with(';') && v_trim.ends_with(';'))
                                {
                                    score += 40;
                                }
                            }
                            let shares_key = if let (Some(u_eq), Some(v_eq)) =
                                (u_trim.find('='), v_trim.find('='))
                            {
                                let u_key = u_trim[..u_eq].trim();
                                let v_key = v_trim[..v_eq].trim();
                                !u_key.is_empty() && u_key == v_key
                            } else {
                                false
                            };
                            if shares_key {
                                score += 200;
                            }
                            if indent_diff == 0
                                && (u_trim.starts_with(v_trim) || v_trim.starts_with(u_trim))
                            {
                                let opens_container = v.delim > 0 || u.delim > 0;
                                if opens_container {
                                    score += 200;
                                } else {
                                    score += 40;
                                }
                            }
                            cached_runs = Some(runs);
                            if n == 1 && m == 1 {
                                score.max(10)
                            } else if score <= 0 {
                                -1000
                            } else {
                                score
                            }
                        }
                    }
                }
            };

            if match_score > 0
                && u_trim != v_trim
                && (n > 1 || m > 1)
                && u.has_salient
                && v.has_salient
            {
                let has_salient_ctx = cached_runs.as_ref().is_some_and(|runs| {
                    runs.iter().any(|run| {
                        run.kind == crate::merge::RunKind::Ctx
                            && text_has_salient_token(run.text, profile)
                    })
                });
                let shares_key =
                    if let (Some(u_eq), Some(v_eq)) = (u_trim.find('='), v_trim.find('=')) {
                        let u_key = u_trim[..u_eq].trim();
                        let v_key = v_trim[..v_eq].trim();
                        !u_key.is_empty() && u_key == v_key
                    } else {
                        false
                    };
                let shares_statement_head = match (
                    diffuse_syntax::prelude::first_word(u_trim),
                    diffuse_syntax::prelude::first_word(v_trim),
                ) {
                    (Some(u_h), Some(v_h)) => u_h == v_h && STATEMENT_HEADS.contains(&u_h),
                    _ => false,
                } || shares_key;
                // A shared keyword head (e.g. `if`, `let`) is too weak
                // to justify pairing across a diagonal offset; require at
                // least one shared salient identifier context token.
                if !has_salient_ctx && (!shares_statement_head || diag_dist > 1) {
                    match_score = -1000;
                } else if !shares_statement_head {
                    let del_salient_chars = u.salient_chars;
                    let add_salient_chars = v.salient_chars;
                    if del_salient_chars > 0 && add_salient_chars > 0 {
                        let matched_salient_chars: usize = u
                            .tokens
                            .iter()
                            .enumerate()
                            .filter(|&(idx, _)| {
                                crate::lcs::is_token_salient_in_context(&u.tokens, idx, profile)
                            })
                            .filter(|&(_, t)| v.tokens.contains(t))
                            .map(|(_, t)| t.chars().count())
                            .sum();
                        let min_salient = del_salient_chars.min(add_salient_chars);
                        // HEAD parity: at least 4 matched salient chars — a
                        // lone 3-char identifier (e.g. `row`) must not anchor
                        // a compound Mod pairing on its own.
                        if matched_salient_chars < 4
                            || (matched_salient_chars as f64 / min_salient as f64) < 0.30
                        {
                            match_score = -1000;
                        }
                    }
                }
            }

            let from_diag = dp[i - 1][j - 1].0 + match_score;
            let from_up = dp[i - 1][j].0 - GAP_PENALTY;
            let from_left = dp[i][j - 1].0 - GAP_PENALTY;

            let mut best_score = from_diag;
            let mut best_dir = 1;

            if from_up > best_score {
                best_score = from_up;
                best_dir = 2;
            }
            if from_left > best_score {
                best_score = from_left;
                best_dir = 3;
            }

            dp[i][j] = (best_score, best_dir);
        }
    }

    let mut i = n;
    let mut j = m;
    let mut path = Vec::new();

    while i > 0 || j > 0 {
        let dir = dp[i][j].1;
        match dir {
            1 => {
                path.push((Some(&old_rows[i - 1]), Some(&new_rows[j - 1])));
                i -= 1;
                j -= 1;
            }
            2 => {
                path.push((Some(&old_rows[i - 1]), None));
                i -= 1;
            }
            3 => {
                path.push((None, Some(&new_rows[j - 1])));
                j -= 1;
            }
            _ => {
                if i > 0 && j > 0 {
                    path.push((Some(&old_rows[i - 1]), Some(&new_rows[j - 1])));
                    i -= 1;
                    j -= 1;
                } else if i > 0 {
                    path.push((Some(&old_rows[i - 1]), None));
                    i -= 1;
                } else {
                    path.push((None, Some(&new_rows[j - 1])));
                    j -= 1;
                }
            }
        }
    }

    path.reverse();
    path
}

/// Maximum slot distance a relocated token may travel. The candidate
/// gather and the apply loop must honor the same window: otherwise a
/// token gathered from a nearby pair (e.g. a lone `{`) is stamped as
/// moved between arbitrarily distant slots.
const CROSS_SLOT_WINDOW: i32 = 4;

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

    // Adjacent operand bonus: if a neighboring pre-image line in the reflow block
    // shares operands with the post-image target (e.g. multi-line expression
    // whose operands precede or follow the line carrying the method
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
    // Method call <= 8 chars without underscore: e.g. .round(), .len(), .clone()
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
    // right halted inside an alphanumeric token, roll back to the token boundary.
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

fn compute_slot_spans(runs: &[(RunKind, String)]) -> Spans {
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
        let group = FactoredGroup {
            id: GroupId(1),
            old_range: 0..1,
            new_range: 0..1,
        };
        let dels = vec![make_del(10, "let alpha = 100;")];
        let adds = vec![make_add(10, "let alpha = 101;")];

        let result = align_and_type_peer_group(&group, &dels, &adds, None);
        assert_eq!(result.slots.len(), 1);
        let s0 = &result.slots[0];
        assert_eq!(s0.kind, RowKind::Mod);
        assert_eq!(s0.old_line, Some(10));
        assert_eq!(s0.new_line, Some(10));
        result.assert_token_conservation();
    }

    #[test]
    fn test_align_expansion_block() {
        let group = FactoredGroup {
            id: GroupId(2),
            old_range: 0..1,
            new_range: 0..3,
        };
        let dels = vec![make_del(20, "fn run() { active(); }")];
        let adds = vec![
            make_add(20, "fn run() {"),
            make_add(21, "    active();"),
            make_add(22, "}"),
        ];

        let result = align_and_type_peer_group(&group, &dels, &adds, None);
        assert_eq!(result.slots.len(), 3);
        assert_eq!(result.slots[0].kind, RowKind::Mod);
        assert_eq!(result.slots[1].kind, RowKind::Add);
        assert_eq!(result.slots[2].kind, RowKind::Add);
        result.assert_token_conservation();
    }

    #[test]
    fn test_align_compound_scrollbar_reflow() {
        let group = FactoredGroup {
            id: GroupId(3),
            old_range: 0..2,
            new_range: 0..3,
        };
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

        let result = align_and_type_peer_group(&group, &dels, &adds, None);
        assert_eq!(result.slots.len(), 3);
        assert_eq!(result.slots[0].kind, RowKind::Mod);
        assert_eq!(result.slots[1].kind, RowKind::Mod);
        assert_eq!(result.slots[2].kind, RowKind::Add);
        result.assert_token_conservation();
    }

    #[test]
    fn test_align_pure_add_and_del_groups() {
        let add_group = FactoredGroup {
            id: GroupId(4),
            old_range: 0..0,
            new_range: 0..2,
        };
        let adds = vec![make_add(10, "line 1"), make_add(11, "line 2")];
        let add_res = align_and_type_peer_group(&add_group, &[], &adds, None);
        assert_eq!(add_res.slots.len(), 2);
        assert_eq!(add_res.slots[0].kind, RowKind::Add);
        assert_eq!(add_res.slots[1].kind, RowKind::Add);
        add_res.assert_token_conservation();

        let del_group = FactoredGroup {
            id: GroupId(5),
            old_range: 0..2,
            new_range: 0..0,
        };
        let dels = vec![make_del(10, "del 1"), make_del(11, "del 2")];
        let del_res = align_and_type_peer_group(&del_group, &dels, &[], None);
        assert_eq!(del_res.slots.len(), 2);
        assert_eq!(del_res.slots[0].kind, RowKind::Del);
        assert_eq!(del_res.slots[1].kind, RowKind::Del);
        del_res.assert_token_conservation();
    }

    #[test]
    fn test_align_token_relocation() {
        let group = FactoredGroup {
            id: GroupId(6),
            old_range: 0..2,
            new_range: 0..2,
        };
        let dels = vec![
            make_del(10, "let target_identifier = compute();"),
            make_del(11, "finalize();"),
        ];
        let adds = vec![
            make_add(10, "prepare();"),
            make_add(11, "finalize(target_identifier);"),
        ];

        let result = align_and_type_peer_group(&group, &dels, &adds, None);
        assert_eq!(result.slots.len(), 3);
        result.assert_token_conservation();

        // Check that "finalize" was relocated from slot 2 (Del) to slot 1 (Mod Add)
        let has_moved_src = result.slots[2]
            .runs
            .iter()
            .any(|(k, s)| *k == RunKind::MovedSrc && s.contains("finalize"));
        let has_moved_tgt = result.slots[1]
            .runs
            .iter()
            .any(|(k, s)| *k == RunKind::MovedTgt && s.contains("finalize"));
        assert!(has_moved_src, "Expected MovedSrc in slot 2");
        assert!(has_moved_tgt, "Expected MovedTgt in slot 1");
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

    #[test]
    fn test_align_single_head_only_pair_dissolves() {
        // The 1×1 fast path must apply the same head gate as the micro-DP:
        // `fn row() {` shares only the `fn` head with `fn total() {` and no
        // salient token, so it must dissolve to Del + Add, not pair as Mod.
        let group = FactoredGroup {
            id: GroupId(7),
            old_range: 0..1,
            new_range: 0..1,
        };
        let dels = vec![make_del(10, "fn row() {")];
        let adds = vec![make_add(10, "fn total() {")];

        let result = align_and_type_peer_group(&group, &dels, &adds, None);
        assert_eq!(result.slots.len(), 2);
        assert_eq!(result.slots[0].kind, RowKind::Del);
        assert_eq!(result.slots[1].kind, RowKind::Add);
        result.assert_token_conservation();
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
        // Slot 1 (Add): let very_long_brand_new_statement_with_many_tokens_and_args =
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

        // Under single-donor rule: pure Add slot 3 cannot accept tokens from both slot
        // 0 and slot 2 (which are disjoint). It may accept from slot 0 OR slot
        // 2, but NOT both!
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
}
