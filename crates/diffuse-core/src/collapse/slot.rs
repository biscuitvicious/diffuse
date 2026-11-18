//! Micro slot-alignment DP and closed-economy token typing for peer reflow
//! groups.

use diffuse_algo::hirschberg::HirschbergLcs;
use diffuse_syntax::profile::LangProfile;

use crate::{
    cluster::factor::FactoredGroup,
    lcs::spans::word_spans_and_runs_with_profile,
    merge::RunKind,
    parse::{GroupId, Row, RowGroup, RowKind, Spans},
};

/// An aligned slot within a peer reflow group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignedSlotRow {
    pub slot_index: usize,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub kind: RowKind,
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
    /// The number of `MovedSrc` characters emitted on the left pane must
    /// equal the number of `MovedTgt` characters emitted on the right pane.
    pub fn assert_character_conservation(&self) {
        let mut moved_src_chars: Vec<char> = Vec::new();
        let mut moved_tgt_chars: Vec<char> = Vec::new();

        for slot in &self.slots {
            for (kind, text) in &slot.runs {
                match kind {
                    RunKind::MovedSrc => moved_src_chars.extend(text.chars()),
                    RunKind::MovedTgt => moved_tgt_chars.extend(text.chars()),
                    _ => {}
                }
            }
        }

        moved_src_chars.sort_unstable();
        moved_tgt_chars.sort_unstable();

        assert_eq!(
            moved_src_chars, moved_tgt_chars,
            "Peer group {} character conservation mismatch: MovedSrc={:?} vs MovedTgt={:?}",
            self.group_id, moved_src_chars, moved_tgt_chars
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
    } else if n == 1 && m == 1 {
        vec![(Some(&group_old[0]), Some(&group_new[0]))]
    } else {
        align_slots_micro_dp(group_old, group_new)
    };

    let total_slots = aligned_pairs.len();

    // 2. Intra-slot word diff and initial runs
    let mut slot_runs: Vec<Vec<(RunKind, String)>> = Vec::with_capacity(total_slots);

    for (old_opt, new_opt) in &aligned_pairs {
        match (old_opt, new_opt) {
            (Some(u), Some(v)) => {
                let (_, runs) = word_spans_and_runs_with_profile(u.text(), v.text(), profile);
                slot_runs.push(runs);
            }
            (Some(u), None) => {
                slot_runs.push(vec![(RunKind::Del, u.text().to_string())]);
            }
            (None, Some(v)) => {
                slot_runs.push(vec![(RunKind::Add, v.text().to_string())]);
            }
            (None, None) => unreachable!(),
        }
    }

    // 3. Cross-slot token relocation (when total_slots > 1)
    if total_slots > 1 {
        relocate_cross_slot_tokens(&mut slot_runs);
    }

    // 4. Construct final AlignedSlotRow list
    let mut slots = Vec::with_capacity(total_slots);
    for (idx, ((old_opt, new_opt), runs)) in aligned_pairs.into_iter().zip(slot_runs).enumerate() {
        let spans = compute_slot_spans(&runs);
        let old_line = old_opt.and_then(Row::old_line);
        let new_line = new_opt.and_then(Row::new_line);
        let kind = match (old_line, new_line) {
            (Some(_), Some(_)) => RowKind::Mod,
            (Some(_), None) => RowKind::Del,
            (None, Some(_)) => RowKind::Add,
            (None, None) => RowKind::Ctx,
        };

        slots.push(AlignedSlotRow {
            slot_index: idx,
            old_line,
            new_line,
            kind,
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

fn align_slots_micro_dp<'a>(
    old_rows: &'a [Row],
    new_rows: &'a [Row],
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
        let u_chars: Vec<char> = u_trim.chars().collect();
        let u_indent = u_text.len() - u_text.trim_start().len();

        for j in 1..=m {
            let v_text = new_rows[j - 1].text();
            let v_trim = v_text.trim();
            let v_chars: Vec<char> = v_trim.chars().collect();
            let v_indent = v_text.len() - v_text.trim_start().len();

            let lcs_len = HirschbergLcs::lcs(&u_chars, &v_chars).len() as i32;
            let indent_diff = (u_indent as i32 - v_indent as i32).abs();
            let mut match_score = (lcs_len * 10) - (indent_diff * 4);

            if !u_trim.is_empty() && u_trim == v_trim {
                match_score += 100;
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

fn relocate_cross_slot_tokens(slot_runs: &mut [Vec<(RunKind, String)>]) {
    // Collect potential token candidates from Del runs
    let mut candidates = Vec::new();
    for runs in slot_runs.iter() {
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
            for tgt in 0..num_slots {
                if src == tgt {
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
        result.assert_character_conservation();
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
        eprintln!("SLOTS: {:#?}", result.slots);
        assert_eq!(result.slots.len(), 3);
        assert_eq!(result.slots[0].kind, RowKind::Mod);
        assert_eq!(result.slots[1].kind, RowKind::Add);
        assert_eq!(result.slots[2].kind, RowKind::Add);
        result.assert_character_conservation();
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
        eprintln!("SLOTS: {:#?}", result.slots);
        assert_eq!(result.slots.len(), 3);
        assert_eq!(result.slots[0].kind, RowKind::Mod);
        assert_eq!(result.slots[1].kind, RowKind::Mod);
        assert_eq!(result.slots[2].kind, RowKind::Add);
        result.assert_character_conservation();
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
        add_res.assert_character_conservation();

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
        del_res.assert_character_conservation();
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
        result.assert_character_conservation();

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
