//! Closed-economy token typing and window slot layout for peer reflow
//! formations.
//!
//! Orchestrates two complementary typing stages over a segmented window:
//! 1. **Stage 1** ([`crate::collapse::project`]): monotone 2D coordinate-aware
//!    multi-line LCS and lockstep slot projection.
//! 2. **Stage 2** ([`crate::collapse::relocate`]): non-monotone residual token
//!    and stranded-frame relocation over leftover `Del` / `Add` runs.

use diffuse_syntax::profile::LangProfile;
use rustc_hash::FxHashMap;

use crate::{
    collapse::{
        project::align_window_2d_tape,
        relocate::{relocate_cross_slot_tokens, relocate_leftover_frames},
    },
    lcs::{line_has_salient_token, owned_runs_for_texts_with_profile},
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
    let mut window_slots: Vec<WindowSlot<'_>> = Vec::with_capacity(old_rows.len() + new_rows.len());
    let (mut o, mut n) = (0usize, 0usize);
    let mut pi = 0usize;
    while o < old_rows.len() || n < new_rows.len() {
        if let Some(&(po, pn)) = pairs.get(pi) {
            if (o, n) == (po, pn) {
                window_slots.push(WindowSlot::Mod(&old_rows[o], &new_rows[n]));
                o += 1;
                n += 1;
                pi += 1;
                continue;
            }
            if o < po {
                window_slots.push(WindowSlot::from_old(&old_rows[o]));
                o += 1;
                continue;
            }
            if n < pn {
                window_slots.push(WindowSlot::from_new(&new_rows[n]));
                n += 1;
                continue;
            }
        }
        if o < old_rows.len() {
            window_slots.push(WindowSlot::from_old(&old_rows[o]));
            o += 1;
        } else {
            window_slots.push(WindowSlot::from_new(&new_rows[n]));
            n += 1;
        }
    }
    type_window_slots(
        window_slots,
        group_id,
        old_rows.len(),
        new_rows.len(),
        profile,
    )
}

/// One styled run sequence for a slot: each entry pairs a kind with its exact
/// source text.
pub(crate) type Runs = Vec<(RunKind, String)>;

/// One aligned slot in a segmented peer-reflow window.
#[derive(Clone, Copy, Debug)]
pub(crate) enum WindowSlot<'a> {
    Mod(&'a Row, &'a Row),
    Del(&'a Row),
    Add(&'a Row),
    MovSrc(&'a Row),
    MovTgt(&'a Row),
}

impl<'a> WindowSlot<'a> {
    #[inline]
    fn from_old(row: &'a Row) -> Self {
        if matches!(row, Row::MovSrc { .. }) {
            Self::MovSrc(row)
        } else {
            Self::Del(row)
        }
    }

    #[inline]
    fn from_new(row: &'a Row) -> Self {
        if matches!(row, Row::MovTgt { .. }) {
            Self::MovTgt(row)
        } else {
            Self::Add(row)
        }
    }

    #[inline]
    pub(crate) fn is_moved(self) -> bool {
        matches!(self, Self::MovSrc(_) | Self::MovTgt(_))
    }

    #[inline]
    pub(crate) fn is_pure_add(self) -> bool {
        matches!(self, Self::Add(_))
    }

    #[inline]
    pub(crate) fn old_row(self) -> Option<&'a Row> {
        match self {
            Self::Mod(u, _) | Self::Del(u) | Self::MovSrc(u) => Some(u),
            Self::Add(_) | Self::MovTgt(_) => None,
        }
    }

    #[inline]
    pub(crate) fn new_row(self) -> Option<&'a Row> {
        match self {
            Self::Mod(_, v) | Self::Add(v) | Self::MovTgt(v) => Some(v),
            Self::Del(_) | Self::MovSrc(_) => None,
        }
    }

    #[inline]
    pub(crate) fn cached_moved_runs(self) -> Option<&'a Runs> {
        match self {
            Self::MovSrc(Row::MovSrc { cached_runs, .. })
            | Self::MovTgt(Row::MovTgt { cached_runs, .. }) => Some(cached_runs),
            _ => None,
        }
    }
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
        matches!(k, RunKind::MovedSrc | RunKind::MovedTgt) && line_has_salient_token(t, profile)
    })
}

/// Computes normalized character highlight spans from a slot's typed runs.
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

/// Types an ordered slot list into classified rows.
///
/// Slot alignment, not membership, is this function's subject: the caller has
/// already decided which rows belong together, which is what lets a segmented
/// window be typed without a second alignment pass over the cluster.
fn type_window_slots(
    window_slots: Vec<WindowSlot<'_>>,
    group_id: GroupId,
    n: usize,
    m: usize,
    profile: Option<&LangProfile>,
) -> PeerGroupRows {
    let total_slots = window_slots.len();
    let is_moved_slot: Vec<bool> = window_slots.iter().map(|s| s.is_moved()).collect();

    // 1. Stage 1: unified 2D coordinate-aware multi-line tape for multi-slot
    //    windows (`1 × m`, `n × 1`, and general `n × m`), falling back cleanly
    //    to per-slot alignment on cap overflow or wholesale `Mod` slots.
    let mut slot_runs: Vec<Runs> = if total_slots > 1
        && let Some(runs_2d) = align_window_2d_tape(&window_slots, profile)
    {
        runs_2d
    } else {
        let mut fallback = Vec::with_capacity(total_slots);
        for &slot in &window_slots {
            match slot {
                WindowSlot::Mod(u, v) => {
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
                WindowSlot::Del(u) => fallback.push(vec![(RunKind::Del, u.text().to_string())]),
                WindowSlot::Add(v) => fallback.push(vec![(RunKind::Add, v.text().to_string())]),
                WindowSlot::MovSrc(_) | WindowSlot::MovTgt(_) => {
                    fallback.push(
                        slot.cached_moved_runs()
                            .expect("moved slot holds MovSrc/MovTgt row")
                            .clone(),
                    );
                }
            }
        }
        fallback
    };

    // 2. Stage 2: residual cross-slot relocation for non-monotone / transposed
    //    moves over leftover `Del` and `Add` runs (when `total_slots > 1`).
    if total_slots > 1 {
        let is_pure_add: Vec<bool> = window_slots.iter().map(|s| s.is_pure_add()).collect();
        relocate_cross_slot_tokens(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);
        relocate_leftover_frames(&mut slot_runs, &is_moved_slot, &is_pure_add, profile);
    }

    // 3. Construct final AlignedSlotRow list
    let mut slots = Vec::with_capacity(total_slots);
    for (idx, (slot, runs)) in window_slots.into_iter().zip(slot_runs).enumerate() {
        let spans = match slot {
            WindowSlot::MovSrc(Row::MovSrc { spans, .. })
            | WindowSlot::MovTgt(Row::MovTgt { spans, .. }) => spans.clone(),
            _ => compute_slot_spans(&runs),
        };
        let old_opt = slot.old_row();
        let new_opt = slot.new_row();
        let old_line = old_opt.and_then(Row::old_line);
        let new_line = new_opt.and_then(Row::new_line);
        let old_text = old_opt.map(|r| r.text().to_string());
        let new_text = new_opt.map(|r| r.text().to_string());
        let is_moved_src = matches!(slot, WindowSlot::MovSrc(_));
        let is_moved_tgt = matches!(slot, WindowSlot::MovTgt(_));
        let kind = match slot {
            WindowSlot::Mod(_, _) => RowKind::Mod,
            WindowSlot::Del(_) => RowKind::Del,
            WindowSlot::Add(_) => RowKind::Add,
            WindowSlot::MovSrc(_) | WindowSlot::MovTgt(_) => RowKind::Mov,
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
    use crate::{
        collapse::project::slot_runs_are_wholesale,
        parse::{AddLine, DelLine},
    };

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
