//! Single monotone cluster path.
//!
//! The whole change cluster is solved once: `V(i, j)` is the best evidence of
//! aligning the first `i` old rows with the first `j` new rows, and the path is
//! a shortest path in the `(n + 1) × (m + 1)` grid with three transitions:
//!
//! ```text
//! V(i, j) = max(
//!     V(i-1, j-1) + pair_evidence(o[i-1], n[j-1]),   // Diag
//!     V(i-1, j)   - PATH_GAP_COST,                   // DropOld
//!     V(i,   j-1) - PATH_GAP_COST,                   // DropNew
//! )
//! ```
//!
//! Every correspondence in the cluster is decided here. Formations are read
//! off the resulting path (see `reflow`); no rectangle, hull, or interior
//! boundary exists, so a row taking part in a distant correspondence is never
//! invisible to the alignment.

use std::mem;

use diffuse_algo::landmarks::find_unique_anchors;
use diffuse_syntax::{prelude::first_word, profile::LangProfile};
use rustc_hash::FxHashSet;

use crate::{
    cluster::dp::{
        config::{PATH_GAP_COST, cluster_budget},
        with_tls_arena,
    },
    lcs::{is_token_salient_in_context, tokenize_line},
    parse::Row,
};

use super::evidence::{assignment_target, field_label, is_statement_head_keyword, pair_evidence};

/// One transition of a cluster path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStep {
    /// Consume one old and one new row as a correspondence.
    Diag,
    /// Consume one old row with no counterpart.
    DropOld,
    /// Consume one new row with no counterpart.
    DropNew,
}

/// A monotone alignment of a whole cluster, in emission order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClusterPath {
    pub steps: Vec<PathStep>,
    pub score: i64,
}

impl ClusterPath {
    /// Number of correspondences on the path.
    pub fn pairs(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| **step == PathStep::Diag)
            .count()
    }

    /// Whether the path consumes nothing (an empty cluster or a budget
    /// fallback over empty input).
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Monotone sequence of `(old_idx, new_idx)` correspondences on the path.
    pub fn diag_pairs(&self) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        let (mut o, mut n) = (0usize, 0usize);
        for step in &self.steps {
            match step {
                PathStep::Diag => {
                    pairs.push((o, n));
                    o += 1;
                    n += 1;
                }
                PathStep::DropOld => o += 1,
                PathStep::DropNew => n += 1,
            }
        }
        pairs
    }
}

/// Solve one cluster as a single monotone path.
///
/// Small clusters are evaluated on the whole grid. Large ones are evaluated
/// inside a corridor interpolated through their exact-match anchors, which
/// bounds the cost of a single large hunk while following the drift of the
/// blocks around it. Above the cluster cell budget the path degenerates to
/// every deletion followed by every addition: a resource ceiling, not a
/// compatibility path.
pub fn solve_cluster_path(
    old_rows: &[Row],
    new_rows: &[Row],
    old_content: Option<&[&str]>,
    new_content: Option<&[&str]>,
    profile: Option<&LangProfile>,
) -> ClusterPath {
    let n = old_rows.len();
    let m = new_rows.len();

    if n == 0 && m == 0 {
        return ClusterPath {
            steps: Vec::new(),
            score: 0,
        };
    }
    if n == 0 {
        return ClusterPath {
            steps: vec![PathStep::DropNew; m],
            score: -(m as i64) * PATH_GAP_COST,
        };
    }
    if m == 0 {
        return ClusterPath {
            steps: vec![PathStep::DropOld; n],
            score: -(n as i64) * PATH_GAP_COST,
        };
    }
    if n.saturating_mul(m) > cluster_budget() {
        return degenerate_path(n, m);
    }

    let prefilter = n.saturating_mul(m) > FULL_GRID_CELLS;
    let bounds = if prefilter {
        let anchors = exact_anchors(old_rows, new_rows);
        corridor_bounds(n, m, &anchors, CORRIDOR_PAD)
    } else {
        vec![(0, m); n + 1]
    };
    let old_hints: Vec<RowHints<'_>> = if prefilter {
        old_rows
            .iter()
            .map(|row| RowHints::of(row.text(), profile))
            .collect()
    } else {
        Vec::new()
    };
    let new_hints: Vec<RowHints<'_>> = if prefilter {
        new_rows
            .iter()
            .map(|row| RowHints::of(row.text(), profile))
            .collect()
    } else {
        Vec::new()
    };

    let ctx = EvidenceCtx {
        old_content,
        new_content,
        profile,
    };
    solve_in_corridor(
        old_rows, new_rows, &old_hints, &new_hints, &ctx, &bounds, prefilter,
    )
    .unwrap_or_else(|| degenerate_path(n, m))
}

/// Clusters at or below this cell count are solved on the whole grid. Larger
/// ones are solved inside a corridor interpolated through exact-match anchors;
/// the full grid keeps every corpus-sized cluster exact, while the corridor
/// bounds the cost of a single large hunk.
const FULL_GRID_CELLS: usize = 20_000;

/// Rows of slack the corridor allows around the anchor chain.
const CORRIDOR_PAD: usize = 2;

/// Maximum number of cells one anchor-to-anchor rectangle may contribute before
/// the segment is interpolated instead.
const RECTANGLE_CAP: usize = 4096;

/// Cheap per-row hints used only to skip corridor candidates that cannot share
/// any evidence.
struct RowHints<'a> {
    trimmed: &'a str,
    salient: FxHashSet<&'a str>,
    head: Option<&'a str>,
    assignment: Option<&'a str>,
    field: Option<&'a str>,
}

impl<'a> RowHints<'a> {
    fn of(text: &'a str, profile: Option<&LangProfile>) -> Self {
        let trimmed = text.trim();
        let tokens: Vec<&'a str> = tokenize_line(text).collect();
        let salient = tokens
            .iter()
            .enumerate()
            .filter(|(i, _)| is_token_salient_in_context(&tokens, *i, profile))
            .map(|(_, token)| *token)
            .collect();
        let head = first_word(trimmed).filter(|word| is_statement_head_keyword(word, profile));
        let assignment = assignment_target(trimmed);
        let field = field_label(trimmed);
        Self {
            trimmed,
            salient,
            head,
            assignment,
            field,
        }
    }
}

/// Snapshot text and language profile used to score a candidate pair.
struct EvidenceCtx<'a> {
    old_content: Option<&'a [&'a str]>,
    new_content: Option<&'a [&'a str]>,
    profile: Option<&'a LangProfile>,
}

fn degenerate_path(n: usize, m: usize) -> ClusterPath {
    let mut steps = vec![PathStep::DropOld; n];
    steps.extend(vec![PathStep::DropNew; m]);
    ClusterPath {
        steps,
        score: -((n + m) as i64) * PATH_GAP_COST,
    }
}

/// Exact-match anchors: non-empty trimmed lines that occur once on each side,
/// chained by Longest Increasing Subsequence so a single moved unique line
/// cannot discard the surrounding monotone anchors.
fn exact_anchors(old_rows: &[Row], new_rows: &[Row]) -> Vec<(usize, usize)> {
    let old_trimmed: Vec<&str> = old_rows.iter().map(|row| row.text().trim()).collect();
    let new_trimmed: Vec<&str> = new_rows.iter().map(|row| row.text().trim()).collect();
    find_unique_anchors(&old_trimmed, &new_trimmed, |s| !s.is_empty())
}

/// Allowed `j` range for every `i` in `0..=n`.
///
/// Each anchor-to-anchor segment is a monotone rectangle: any cell between two
/// anchors can lie on a monotone path connecting them, so the corridor uses the
/// whole rectangle while it is small (or degenerate along one row/column), and
/// interpolates a connected `pad`-wide diagonal band through it once it is not.
/// The rectangles keep an inserted or deleted block inside the corridor without
/// paying for the block's area.
fn corridor_bounds(
    n: usize,
    m: usize,
    anchors: &[(usize, usize)],
    pad: usize,
) -> Vec<(usize, usize)> {
    let mut chain = Vec::with_capacity(anchors.len() + 2);
    chain.push((0usize, 0usize));
    chain.extend_from_slice(anchors);
    chain.push((n, m));

    let mut bounds = vec![(usize::MAX, 0usize); n + 1];
    for window in chain.windows(2) {
        let (i0, j0) = window[0];
        let (i1, j1) = window[1];
        if i1 < i0 || j1 < j0 {
            continue;
        }
        let span = i1 - i0;
        let col_span = j1 - j0;
        let area = (span + 1).saturating_mul(col_span + 1);
        for (offset, entry) in bounds[i0..=i1].iter_mut().enumerate() {
            let (lo, hi) = if span == 0 || col_span == 0 || area <= RECTANGLE_CAP {
                (j0, j1)
            } else {
                // Span the full column interval crossed between row `offset`
                // and `offset + 1` so steep slopes never disconnect consecutive
                // rows.
                let j_start = j0 + col_span.saturating_mul(offset) / span;
                let j_end = if offset < span {
                    j0 + col_span.saturating_mul(offset + 1) / span
                } else {
                    j1
                };
                (j_start.saturating_sub(pad), (j_end + pad).min(m))
            };
            *entry = (entry.0.min(lo), entry.1.max(hi));
        }
    }
    bounds[0].0 = 0;
    bounds[n].1 = m;
    bounds
}

/// Corridor admission test: a pair is scored only when it shares something the
/// evidence can use — a salient token, a statement head, an assignment target,
/// or a field key. Short, affix, identical, and blank pairs are always scored
/// because their evidence is cheap or rests on shape alone. This is a cost
/// policy for large hunks, not a semantic rule: the whole-grid solver is used
/// for every cluster where the alignment can afford it.
fn plausible_pair(u: &RowHints<'_>, v: &RowHints<'_>) -> bool {
    if u.trimmed.is_empty() || v.trimmed.is_empty() || u.trimmed == v.trimmed {
        return true;
    }
    if u.trimmed.len().max(v.trimmed.len()) <= 32 {
        return true;
    }
    if u.trimmed.starts_with(v.trimmed)
        || v.trimmed.starts_with(u.trimmed)
        || u.trimmed.ends_with(v.trimmed)
        || v.trimmed.ends_with(u.trimmed)
    {
        return true;
    }
    if u.head.is_some() && u.head == v.head {
        return true;
    }
    if (u.assignment.is_some() && u.assignment == v.assignment)
        || (u.field.is_some() && u.field == v.field)
    {
        return true;
    }
    let (smaller, larger) = if u.salient.len() <= v.salient.len() {
        (&u.salient, &v.salient)
    } else {
        (&v.salient, &u.salient)
    };
    smaller.iter().any(|token| larger.contains(token))
}

/// Solve the cluster inside the corridor given by `bounds`.
fn solve_in_corridor(
    old_rows: &[Row],
    new_rows: &[Row],
    old_hints: &[RowHints<'_>],
    new_hints: &[RowHints<'_>],
    ctx: &EvidenceCtx<'_>,
    bounds: &[(usize, usize)],
    prefilter: bool,
) -> Option<ClusterPath> {
    let n = old_rows.len();
    let m = new_rows.len();
    const NEG: i64 = i64::MIN / 4;
    with_tls_arena(|arena| {
        arena.prepare_stripes(m + 1, NEG);
        let prev = &mut arena.prev;
        let cur = &mut arena.cur;
        let mut dirs: Vec<(usize, Vec<u8>)> = Vec::with_capacity(n + 1);

        let (row0_lo, row0_hi) = bounds[0];
        for (j, cell) in prev.iter_mut().enumerate().take(row0_hi + 1).skip(row0_lo) {
            *cell = -(j as i64) * PATH_GAP_COST;
        }
        dirs.push((row0_lo, vec![3u8; row0_hi - row0_lo + 1]));

        for i in 1..=n {
            let (lo, hi) = bounds[i];
            let prev_lo = dirs[i - 1].0;
            let mut row_dirs = vec![0u8; hi + 1 - lo];
            for j in lo..=hi {
                let slot = j - lo;
                if j == 0 {
                    cur[0] = -(i as i64) * PATH_GAP_COST;
                    row_dirs[slot] = 2;
                    continue;
                }
                let old_row = &old_rows[i - 1];
                let new_row = &new_rows[j - 1];
                let skipped = old_row.is_moved()
                    || new_row.is_moved()
                    || (prefilter && !plausible_pair(&old_hints[i - 1], &new_hints[j - 1]));
                let pair = if skipped {
                    None
                } else {
                    pair_evidence(
                        old_row.text(),
                        new_row.text(),
                        ctx.old_content,
                        ctx.new_content,
                        ctx.profile,
                    )
                };

                let diag = match pair {
                    Some(evidence) if j > prev_lo && prev[j - 1] > NEG => prev[j - 1] + evidence,
                    _ => NEG,
                };
                let up = if j >= prev_lo && prev[j] > NEG {
                    prev[j] - PATH_GAP_COST
                } else {
                    NEG
                };
                let left = if cur[j - 1] > NEG {
                    cur[j - 1] - PATH_GAP_COST
                } else {
                    NEG
                };

                // Tie-break during backtracking: `Diag` > `DropNew` (`left`) >
                // `DropOld` (`up`), so after `steps.reverse()` forward path
                // order emits unpaired deletions (`DropOld`) before unpaired
                // additions (`DropNew`).
                let mut best = diag;
                let mut dir = 1u8;
                if left > best {
                    best = left;
                    dir = 3;
                }
                if up > best {
                    best = up;
                    dir = 2;
                }
                cur[j] = best;
                row_dirs[slot] = dir;
            }
            dirs.push((lo, row_dirs));
            mem::swap(prev, cur);
            cur.fill(NEG);
        }

        let score = prev[m];
        if score <= NEG {
            return None;
        }
        let mut steps = Vec::with_capacity(n + m);
        let (mut i, mut j) = (n, m);
        while i > 0 || j > 0 {
            let (lo, ref dirs_row) = dirs[i];
            let dir = dirs_row[j - lo];
            match dir {
                1 => {
                    steps.push(PathStep::Diag);
                    i -= 1;
                    j -= 1;
                }
                2 => {
                    steps.push(PathStep::DropOld);
                    i -= 1;
                }
                3 => {
                    steps.push(PathStep::DropNew);
                    j -= 1;
                }
                _ => unreachable!("every interior cell receives a direction"),
            }
        }
        steps.reverse();
        Some(ClusterPath { steps, score })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cluster::dp::config::{DEFAULT_CLUSTER_BUDGET, set_cluster_budget},
        parse::{AddLine, DelLine},
    };

    fn del(num: u32, text: &str) -> Row {
        Row::del(DelLine::new(num, text))
    }

    fn add(num: u32, text: &str) -> Row {
        Row::add(AddLine::new(num, text))
    }

    fn solve(old: &[&str], new: &[&str]) -> ClusterPath {
        let old_rows: Vec<Row> = old
            .iter()
            .enumerate()
            .map(|(i, t)| del(i as u32 + 1, t))
            .collect();
        let new_rows: Vec<Row> = new
            .iter()
            .enumerate()
            .map(|(i, t)| add(i as u32 + 1, t))
            .collect();
        solve_cluster_path(&old_rows, &new_rows, None, None, None)
    }

    #[test]
    fn empty_cluster_has_empty_path() {
        let path = solve(&[], &[]);
        assert!(path.is_empty());
        assert_eq!(path.score, 0);
    }

    #[test]
    fn all_additions_drop_every_new_row() {
        let path = solve(&[], &["let alpha = 1;", "let beta = 2;"]);
        assert_eq!(path.steps, vec![PathStep::DropNew; 2]);
    }

    #[test]
    fn all_deletions_drop_every_old_row() {
        let path = solve(&["let alpha = 1;", "let beta = 2;"], &[]);
        assert_eq!(path.steps, vec![PathStep::DropOld; 2]);
    }

    #[test]
    fn identical_single_row_pairs() {
        let path = solve(&["let alpha = 1;"], &["let alpha = 1;"]);
        assert_eq!(path.steps, vec![PathStep::Diag]);
    }

    #[test]
    fn uniform_shift_pairs_every_row() {
        let path = solve(
            &["let alpha = 1;", "let beta = 2;", "let gamma = 3;"],
            &["let alpha = 1;", "let beta = 2;", "let gamma = 3;"],
        );
        assert_eq!(path.steps, vec![PathStep::Diag; 3]);
    }

    #[test]
    fn offset_jump_pairs_across_a_gap() {
        let path = solve(
            &["let alpha = 1;", "let beta = 2;"],
            &["let inserted = 0;", "let alpha = 1;", "let beta = 2;"],
        );
        assert_eq!(
            path.steps,
            vec![PathStep::DropNew, PathStep::Diag, PathStep::Diag]
        );
    }

    #[test]
    fn unrelated_lines_do_not_pair() {
        // The two drop orders score identically; emission order is normalized
        // downstream, so only the absence of a correspondence is asserted.
        let path = solve(
            &["let alpha = 1;"],
            &["totally different prose about nothing at all"],
        );
        assert_eq!(path.pairs(), 0);
        assert_eq!(path.steps.len(), 2);
    }

    #[test]
    fn moved_rows_never_pair() {
        let old_rows = vec![Row::MovSrc {
            line: DelLine::new(1, "let alpha = 1;"),
            spans: Default::default(),
            cached_runs: vec![(
                crate::merge::RunKind::MovedSrc,
                "let alpha = 1;".to_string(),
            )],
            group: None,
        }];
        let new_rows = vec![Row::MovTgt {
            line: AddLine::new(1, "let alpha = 1;"),
            spans: Default::default(),
            cached_runs: vec![(
                crate::merge::RunKind::MovedTgt,
                "let alpha = 1;".to_string(),
            )],
            group: None,
        }];
        let path = solve_cluster_path(&old_rows, &new_rows, None, None, None);
        assert_eq!(path.pairs(), 0);
    }

    #[test]
    fn repeated_solves_agree() {
        let old = ["let alpha = 1;", "let beta = 2;", "fn run() {"];
        let new = ["fn run() {", "let alpha = 1;", "let beta = 2;"];
        assert_eq!(solve(&old, &new), solve(&old, &new));
    }

    #[test]
    fn budget_fallback_emits_all_deletions_then_additions() {
        set_cluster_budget(2);
        let path = solve(
            &["let alpha = 1;", "let beta = 2;"],
            &["let alpha = 1;", "let beta = 2;"],
        );
        set_cluster_budget(DEFAULT_CLUSTER_BUDGET);
        assert_eq!(
            path.steps,
            vec![
                PathStep::DropOld,
                PathStep::DropOld,
                PathStep::DropNew,
                PathStep::DropNew
            ]
        );
    }

    /// Every path consumes each row exactly once: the number of `Diag` steps
    /// plus the number of drops on a side equals that side's length, so no row
    /// is skipped twice or emitted out of order.
    #[test]
    fn every_path_is_monotone_and_consumes_each_row_once() {
        let texts = ["let alpha = 1;", "let beta = 2;", "fn run() {"];
        let mut clusters: Vec<Vec<&str>> = Vec::new();
        for len in 1..=3usize {
            let total = texts.len().pow(len as u32);
            for code in 0..total {
                let mut cluster = Vec::with_capacity(len);
                let mut rest = code;
                for _ in 0..len {
                    cluster.push(texts[rest % texts.len()]);
                    rest /= texts.len();
                }
                clusters.push(cluster);
            }
        }
        for old in &clusters {
            for new in &clusters {
                let path = solve(old, new);
                let diags = path.pairs();
                let drops_old = path
                    .steps
                    .iter()
                    .filter(|step| **step == PathStep::DropOld)
                    .count();
                let drops_new = path
                    .steps
                    .iter()
                    .filter(|step| **step == PathStep::DropNew)
                    .count();
                assert_eq!(diags + drops_old, old.len());
                assert_eq!(diags + drops_new, new.len());
                assert_eq!(path.steps.len(), diags + drops_old + drops_new);
            }
        }
    }

    /// A large cluster is solved inside a corridor interpolated through its
    /// exact-match anchors, so an inserted block shifts the rest of the
    /// alignment instead of stranding it.
    #[test]
    fn corridor_follows_an_inserted_block() {
        let old: Vec<String> = (0..150).map(|i| format!("let value_{i} = {i};")).collect();
        let mut new = old.clone();
        for k in 0..10 {
            new.insert(40, format!("let inserted_{k} = 0;"));
        }
        let old_rows: Vec<Row> = old
            .iter()
            .enumerate()
            .map(|(i, text)| del(i as u32 + 1, text))
            .collect();
        let new_rows: Vec<Row> = new
            .iter()
            .enumerate()
            .map(|(i, text)| add(i as u32 + 1, text))
            .collect();
        let path = solve_cluster_path(&old_rows, &new_rows, None, None, None);
        assert!(
            path.pairs() >= 140,
            "the corridor must follow the shift, paired {} of 150",
            path.pairs()
        );
    }

    #[test]
    fn corridor_follows_leading_and_trailing_insertions() {
        let old: Vec<String> = (0..150).map(|i| format!("let value_{i} = {i};")).collect();
        let mut new = Vec::with_capacity(170);
        for k in 0..10 {
            new.push(format!("let leading_insert_{k} = 0;"));
        }
        new.extend(old.iter().cloned());
        for k in 0..10 {
            new.push(format!("let trailing_insert_{k} = 0;"));
        }
        let old_rows: Vec<Row> = old
            .iter()
            .enumerate()
            .map(|(i, text)| del(i as u32 + 1, text))
            .collect();
        let new_rows: Vec<Row> = new
            .iter()
            .enumerate()
            .map(|(i, text)| add(i as u32 + 1, text))
            .collect();
        let path = solve_cluster_path(&old_rows, &new_rows, None, None, None);
        assert!(
            path.pairs() >= 145,
            "leading/trailing insertions must not disconnect corridor, paired {} of 150",
            path.pairs()
        );
    }

    #[test]
    fn corridor_ignores_single_moved_unique_line_via_lis() {
        let old: Vec<String> = (0..150).map(|i| format!("let value_{i} = {i};")).collect();
        let mut new = Vec::with_capacity(150);
        // Move a single unique line from near the end to index 0; greedy
        // anchor selection jumps its cursor to 145 and discards all 145 real
        // monotone anchors, whereas LIS retains them.
        new.push(old[145].clone());
        for (i, line) in old.iter().enumerate() {
            if i != 145 {
                new.push(line.clone());
            }
        }
        let old_rows: Vec<Row> = old
            .iter()
            .enumerate()
            .map(|(i, text)| del(i as u32 + 1, text))
            .collect();
        let new_rows: Vec<Row> = new
            .iter()
            .enumerate()
            .map(|(i, text)| add(i as u32 + 1, text))
            .collect();
        let path = solve_cluster_path(&old_rows, &new_rows, None, None, None);
        assert!(
            path.pairs() >= 145,
            "a single moved unique line must not discard LIS anchors, paired {} of 149",
            path.pairs()
        );
    }

    #[test]
    fn corridor_stays_connected_on_steep_slope_gap() {
        // An anchor gap with 20 old rows vs 250 new rows has area >
        // RECTANGLE_CAP and slope > 2 * CORRIDOR_PAD + 1; interpolation must
        // stay connected.
        let mut old = vec!["let anchor_start = 1;".to_string()];
        for i in 0..20 {
            old.push(format!("let old_body_{i} = {i};"));
        }
        old.push("let anchor_end = 2;".to_string());
        for i in 0..100 {
            old.push(format!("let tail_{i} = {i};"));
        }

        let mut new = vec!["let anchor_start = 1;".to_string()];
        for i in 0..250 {
            new.push(format!("let new_body_{i} = {i};"));
        }
        new.push("let anchor_end = 2;".to_string());
        for i in 0..100 {
            new.push(format!("let tail_{i} = {i};"));
        }

        let old_rows: Vec<Row> = old
            .iter()
            .enumerate()
            .map(|(i, text)| del(i as u32 + 1, text))
            .collect();
        let new_rows: Vec<Row> = new
            .iter()
            .enumerate()
            .map(|(i, text)| add(i as u32 + 1, text))
            .collect();
        let path = solve_cluster_path(&old_rows, &new_rows, None, None, None);
        assert!(
            path.pairs() >= 100,
            "steep anchor gap must stay connected, paired {} of 102",
            path.pairs()
        );
    }

    #[test]
    fn equal_cost_skips_emit_drop_old_before_drop_new() {
        let path = solve(
            &[
                "The workspace also holds the engine's first consumer: extension/pi —",
                "the pi review extension. The extension is a thin client.",
            ],
            &[
                "The workspace also holds extension/pi — the pi review extension",
                "It remains a supported customer of the project deliverable.",
            ],
        );
        assert_eq!(
            path.steps,
            vec![PathStep::Diag, PathStep::DropOld, PathStep::DropNew],
            "unpaired deletions must precede unpaired additions in forward path order"
        );
    }
}
