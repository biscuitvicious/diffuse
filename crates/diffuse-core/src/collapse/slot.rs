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
}

/// Aligns rows in a factored group into monotonic visual slots and performs
/// closed-economy token classification.
pub fn align_and_type_peer_group(
    group: &FactoredGroup,
    old_rows: &[Row],
    new_rows: &[Row],
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
    } else {
        let raw_pairs = align_slots_micro_dp(group_old, group_new, profile);
        canonicalize_slot_order(raw_pairs)
    };

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
                let (_, runs) = word_spans_and_runs_with_profile(u.text(), v.text(), profile);
                slot_runs.push(runs);
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

fn align_slots_micro_dp<'a>(
    old_rows: &'a [Row],
    new_rows: &'a [Row],
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

    for i in 1..=n {
        let u_text = old_rows[i - 1].text();
        let u_trim = u_text.trim();
        let ot: Vec<&str> = tokenize_line(u_text).collect();
        let u_indent = u_text.len() - u_text.trim_start().len();

        for j in 1..=m {
            let v_text = new_rows[j - 1].text();
            let v_trim = v_text.trim();
            let nt: Vec<&str> = tokenize_line(v_text).collect();
            let v_indent = v_text.len() - v_text.trim_start().len();

            let common_non_ws_tokens: usize = ot
                .iter()
                .filter(|t| !t.trim().is_empty())
                .filter(|t| nt.iter().any(|n| n == *t))
                .map(|t| t.chars().count())
                .sum();
            let indent_diff = (u_indent as i32 - v_indent as i32).abs();

            let min_tokens = if n == 1 && m == 1 { 0 } else { 3 };
            let mut match_score = if old_rows[i - 1].is_moved() || new_rows[j - 1].is_moved() {
                -1000
            } else if u_trim == v_trim {
                (common_non_ws_tokens as i32 * 10) + 100 - (indent_diff * 4)
            } else if common_non_ws_tokens < min_tokens {
                -1000
            } else {
                let score = (common_non_ws_tokens as i32 * 10) - (indent_diff * 4);
                if n == 1 && m == 1 {
                    score.max(10)
                } else if score <= 0 {
                    -1000
                } else {
                    score
                }
            };

            if match_score > 0 && u_trim != v_trim {
                let spans = crate::lcs::word_spans(u_text, v_text);
                if crate::merge::is_wholesale_replacement_spans(u_text, v_text, &spans) {
                    match_score = -1000;
                } else if n > 1 || m > 1 {
                    let del_has_salient = text_has_salient_token(u_text, profile);
                    let add_has_salient = text_has_salient_token(v_text, profile);
                    if del_has_salient && add_has_salient {
                        let runs = crate::merge::merged_runs_from_texts(u_text, v_text, &spans);
                        let has_salient_ctx = runs.iter().any(|run| {
                            run.kind == crate::merge::RunKind::Ctx
                                && text_has_salient_token(run.text, profile)
                        });
                        if !has_salient_ctx {
                            match_score = -1000;
                        } else {
                            let del_salient_chars: usize = ot
                                .iter()
                                .filter(|t| is_salient_token(t, profile))
                                .map(|t| t.chars().count())
                                .sum();
                            let add_salient_chars: usize = nt
                                .iter()
                                .filter(|t| is_salient_token(t, profile))
                                .map(|t| t.chars().count())
                                .sum();
                            if del_salient_chars > 0 && add_salient_chars > 0 {
                                let matched_salient_chars: usize = ot
                                    .iter()
                                    .filter(|t| is_salient_token(t, profile))
                                    .filter(|t| nt.iter().any(|n| n == *t))
                                    .map(|t| t.chars().count())
                                    .sum();
                                let min_salient = del_salient_chars.min(add_salient_chars);
                                if matched_salient_chars < 4
                                    || (matched_salient_chars as f64 / min_salient as f64) < 0.30
                                {
                                    match_score = -1000;
                                }
                            }
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
    // Collect potential token candidates from Del runs
    let mut candidates = Vec::new();
    for (slot_idx, runs) in slot_runs.iter().enumerate() {
        if is_moved_slot.get(slot_idx).copied().unwrap_or(false) {
            continue;
        }
        for (kind, text) in runs {
            if *kind == RunKind::Del {
                let trimmed = text.trim();
                if trimmed.len() >= 3 {
                    if text.ends_with(' ') {
                        candidates.push(format!("{trimmed} "));
                    }
                    candidates.push(trimmed.to_string());
                }
                for part in trimmed.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    let p_trim = part.trim();
                    if p_trim.len() >= 4 {
                        candidates.push(p_trim.to_string());
                    }
                }
                if trimmed.ends_with(';') {
                    candidates.push(";".to_string());
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
        assert_eq!(result.slots[0].kind, RowKind::Add);
        assert_eq!(result.slots[1].kind, RowKind::Mod);
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
}
