//! Micro slot-alignment DP and closed-economy token typing for peer reflow
//! groups.

use diffuse_syntax::profile::LangProfile;

use crate::{
    cluster::factor::FactoredGroup,
    lcs::{spans::word_spans_and_runs_with_profile, tokenize_line},
    merge::RunKind,
    parse::{GroupId, Row, RowGroup, RowKind, Spans},
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
    pub group: RowGroup,
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
                group: RowGroup::Peer {
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

            if !is_admissible_slot_pair(u_text, v_text, &spans, old_content, new_content, profile) {
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
                    group: RowGroup::Peer {
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
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot);
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
            group: RowGroup::Peer {
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
        group: RowGroup::Peer {
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
            group: RowGroup::Peer {
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
        group: RowGroup::Peer {
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
            group: RowGroup::Peer {
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

const COMMON_KEYWORDS: &[&str] = &[
    "let", "mut", "const", "static", "pub", "fn", "for", "in", "if", "else", "match", "return",
    "true", "false", "impl", "trait", "struct", "enum", "type", "where", "use", "mod", "crate",
    "self", "super", "async", "await", "while", "loop",
];

fn is_salient_token(t: &str, profile: Option<&LangProfile>) -> bool {
    crate::lcs::token_is_alnum(t)
        && !COMMON_KEYWORDS.contains(&t)
        && !profile.is_some_and(|p| p.is_keyword(t) || p.is_literal(t))
}

fn text_has_salient_token(text: &str, profile: Option<&LangProfile>) -> bool {
    let mut has = false;
    crate::lcs::for_each_token(text, |t| {
        if has {
            return;
        }
        if is_salient_token(t, profile) {
            has = true;
        }
    });
    has
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
fn is_admissible_slot_pair(
    u_text: &str,
    v_text: &str,
    spans: &crate::lcs::WordSpans,
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
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

    !decl_has_rival
        && !crate::merge::is_wholesale_replacement_spans(u_text, v_text, spans)
        && (crate::cluster::dp::solver::is_admissible_mod_pair(u_text, v_text, profile)
            || is_decl_compat)
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
    const STATEMENT_HEADS: &[&str] = &[
        "if", "fn", "let", "match", "for", "while", "return", "pub", "struct", "enum", "type",
        "use", "impl",
    ];

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
        for &t in &tokens {
            if is_salient_token(t, profile) {
                has_salient = true;
                salient_chars += t.chars().count();
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
                            .filter(|t| is_salient_token(t, profile))
                            .filter(|t| v.tokens.contains(t))
                            .map(|t| t.chars().count())
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

fn relocate_cross_slot_tokens(slot_runs: &mut [Vec<(RunKind, String)>], is_moved_slot: &[bool]) {
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
            if src == tgt || (*src as i32 - *tgt as i32).abs() > 4 {
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

    for token in candidates {
        // Find a source slot with Del containing token and a target slot with Add
        // containing token
        let num_slots = slot_runs.len();
        for src in 0..num_slots {
            if is_moved_slot.get(src).copied().unwrap_or(false) {
                continue;
            }
            for tgt in 0..num_slots {
                if src == tgt || is_moved_slot.get(tgt).copied().unwrap_or(false) {
                    continue;
                }
                while contains_token(&slot_runs[src], RunKind::Del, &token)
                    && contains_token(&slot_runs[tgt], RunKind::Add, &token)
                {
                    replace_first_occurrence(
                        &mut slot_runs[src],
                        RunKind::Del,
                        RunKind::MovedSrc,
                        &token,
                    );
                    replace_first_occurrence(
                        &mut slot_runs[tgt],
                        RunKind::Add,
                        RunKind::MovedTgt,
                        &token,
                    );
                }
            }
        }
    }
}

fn contains_token(runs: &[(RunKind, String)], target_kind: RunKind, token: &str) -> bool {
    runs.iter()
        .any(|(k, s)| *k == target_kind && s.contains(token))
}

fn replace_first_occurrence(
    runs: &mut Vec<(RunKind, String)>,
    target_kind: RunKind,
    replacement_kind: RunKind,
    token: &str,
) -> bool {
    for i in 0..runs.len() {
        if runs[i].0 == target_kind && runs[i].1.contains(token) {
            let full = runs[i].1.clone();
            if let Some(idx) = full.find(token) {
                let before = full[..idx].to_string();
                let matched = full[idx..idx + token.len()].to_string();
                let after = full[idx + token.len()..].to_string();

                let mut new_pieces = Vec::new();
                if !before.is_empty() {
                    new_pieces.push((target_kind, before));
                }
                new_pieces.push((replacement_kind, matched));
                if !after.is_empty() {
                    new_pieces.push((target_kind, after));
                }

                runs.splice(i..=i, new_pieces);
                return true;
            }
        }
    }
    false
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
        Row::Del(DelLine::new(num, text))
    }

    fn make_add(num: u32, text: &str) -> Row {
        Row::Add(AddLine::new(num, text))
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
            group: RowGroup::Peer {
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
}
