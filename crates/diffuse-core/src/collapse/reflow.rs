//! Path segmentation: turns a cluster path into formations and plain rows.
//!
//! A **plateau** is a run of `Diag` steps with no skip between them; each step
//! is one plain modification. A **window** is a path interval in which at least
//! one skip participates in a formation, and its members are read off the path
//! in order. Membership is decided by information, never by position:
//!
//! - **Construct continuity.** Each line reduces to its unmatched leading
//!   closers and trailing openers via `UniversalDelimiterClassifier`. A skip
//!   joins a correspondence while the two sides' Dyck stacks diverge, or when a
//!   balanced correspondence sits inside an enclosing skip frame; speculative
//!   extensions commit only when the two sides converge.
//! - **Unmatched salient evidence.** When a balanced correspondence does not
//!   extend by delimiter balance, an immediately adjacent skip sharing
//!   unmatched salient content (tracked by token multiplicity) joins the
//!   window, guarded against verbatim pairs and symmetric block boundaries.
//!
//! Everything else stays a plain deletion or addition. This is what prevents a
//! hull: rows that took part in no correspondence never become members.

use std::ops::Range;

use diffuse_algo::{DelimiterCategory, UniversalDelimiterClassifier};
use diffuse_syntax::profile::LangProfile;
use rustc_hash::FxHashMap;

use crate::{
    collapse::tokens::is_salient_word_match,
    lcs::{is_token_salient_in_context, tokenize_line},
    parse::Row,
};

use super::lattice::{ClusterPath, PathStep};

/// One position of a segmented cluster, in emission order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// A correspondence that stands alone: a plain modification.
    Pair { old: usize, new: usize },
    /// A formation span with at least one participating skip.
    Window {
        old: Range<usize>,
        new: Range<usize>,
    },
    /// A deletion that belongs to no formation.
    Del { old: usize },
    /// An addition that belongs to no formation.
    Add { new: usize },
}

/// Segments one solved cluster path into formations and plain rows.
pub fn segment_path(
    path: &ClusterPath,
    old_rows: &[Row],
    new_rows: &[Row],
    profile: Option<&LangProfile>,
) -> Vec<Segment> {
    PathSegmenter::new(path, old_rows, new_rows, profile).segment()
}

/// One step of a path with its resolved cluster row indices.
#[derive(Clone, Copy, Debug)]
struct ResolvedStep {
    old: Option<usize>,
    new: Option<usize>,
}

impl ResolvedStep {
    fn is_diag(&self) -> bool {
        self.old.is_some() && self.new.is_some()
    }
}

/// Unconverged per-side Dyck opener stacks handed from `pair_span` to the
/// multi-pair tentative fold when a forward scan reaches the next `Diag` step.
struct OpenResidual {
    next_scan: usize,
    old_stack: Vec<u8>,
    new_stack: Vec<u8>,
}

/// Reduced unmatched delimiters of a single line, ignoring string literals,
/// character literals, and ANSI escape sequences.
///
/// After canceling intra-line balanced pairs, a line's delimiter effect is its
/// unmatched leading closers (`closes`, left-to-right) followed by its
/// unmatched trailing openers (`opens`, left-to-right).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LineDelims {
    closes: Vec<u8>,
    opens: Vec<u8>,
}

impl LineDelims {
    fn of(row: &Row) -> Self {
        let text = row.text().trim();
        let bytes = text.as_bytes();
        let len = bytes.len();
        let mut closes = Vec::new();
        let mut opens = Vec::new();
        let mut in_str = false;
        let mut str_char = b'"';
        let mut escaped = false;
        let mut i = 0;

        while i < len {
            let b = bytes[i];
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if in_str {
                if b == b'\\' {
                    escaped = true;
                } else if b == str_char {
                    in_str = false;
                }
                i += 1;
                continue;
            }
            if b == 0x1b && i + 1 < len && bytes[i + 1] == b'[' {
                i += 2;
                while i < len && !bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                continue;
            }
            if b == b'"' || b == b'`' {
                in_str = true;
                str_char = b;
                i += 1;
                continue;
            }
            if b == b'\'' {
                let is_char_lit = (i + 2 < len && bytes[i + 2] == b'\'')
                    || (i + 3 < len && bytes[i + 1] == b'\\' && bytes[i + 3] == b'\'');
                if is_char_lit {
                    in_str = true;
                    str_char = b'\'';
                }
                i += 1;
                continue;
            }
            let ch = text[i..].chars().next().expect("valid UTF-8 boundary");
            match UniversalDelimiterClassifier.classify_char(ch) {
                Some(DelimiterCategory::Open(id)) => opens.push(id),
                Some(DelimiterCategory::Close(id)) => {
                    if opens.last() == Some(&id) {
                        opens.pop();
                    } else {
                        closes.push(id);
                    }
                }
                _ => {}
            }
            i += ch.len_utf8();
        }

        Self { closes, opens }
    }
}

/// Precomputed per-row delimiter and salient-token tables for one path.
struct PathSegmenter<'a> {
    steps: Vec<ResolvedStep>,
    old_rows: &'a [Row],
    new_rows: &'a [Row],
    old_tokens: Vec<FxHashMap<&'a str, usize>>,
    new_tokens: Vec<FxHashMap<&'a str, usize>>,
    old_delims: Vec<LineDelims>,
    new_delims: Vec<LineDelims>,
}

impl<'a> PathSegmenter<'a> {
    fn new(
        path: &ClusterPath,
        old_rows: &'a [Row],
        new_rows: &'a [Row],
        profile: Option<&LangProfile>,
    ) -> Self {
        Self {
            steps: resolve_steps(path),
            old_rows,
            new_rows,
            old_tokens: salient_tokens(old_rows, profile),
            new_tokens: salient_tokens(new_rows, profile),
            old_delims: old_rows.iter().map(LineDelims::of).collect(),
            new_delims: new_rows.iter().map(LineDelims::of).collect(),
        }
    }

    fn segment(&self) -> Vec<Segment> {
        let plateaus = self.plateaus();
        let pairs: Vec<usize> = plateaus
            .iter()
            .flat_map(|plateau| plateau.clone())
            .collect();
        let mut windows: Vec<Range<usize>> = Vec::new();
        let mut a = 0;

        while a < pairs.len() {
            let seed = pairs[a];
            let floor = windows.last().map_or(0, |prev| prev.end);
            let (mut span, residual) = if self.blank_at(&plateaus, seed) {
                (seed..seed + 1, None)
            } else {
                self.pair_span(seed, floor)
            };

            let mut b = a + 1;
            if let Some(open) = residual
                && let Some((end, next_b)) = self.fold_multi_pair(&plateaus, &pairs, a, open)
            {
                span.end = end;
                b = next_b;
            }

            windows.push(span);
            a = b;
        }

        emit_segments(&self.steps, &windows)
    }

    /// Maximal runs of consecutive `Diag` steps: the plateaus of the path.
    fn plateaus(&self) -> Vec<Range<usize>> {
        let mut plateaus = Vec::new();
        let mut index = 0;
        while index < self.steps.len() {
            if self.steps[index].is_diag() {
                let start = index;
                while index < self.steps.len() && self.steps[index].is_diag() {
                    index += 1;
                }
                plateaus.push(start..index);
            } else {
                index += 1;
            }
        }
        plateaus
    }

    fn apply_step_fwd(
        &self,
        step: &ResolvedStep,
        old_stack: &mut Vec<u8>,
        new_stack: &mut Vec<u8>,
    ) -> bool {
        let old_ok = step
            .old
            .is_none_or(|o| apply_delims_forward(old_stack, &self.old_delims[o]));
        let new_ok = step
            .new
            .is_none_or(|n| apply_delims_forward(new_stack, &self.new_delims[n]));
        old_ok && new_ok
    }

    fn apply_step_bwd(
        &self,
        step: &ResolvedStep,
        old_closes: &mut Vec<u8>,
        new_closes: &mut Vec<u8>,
    ) -> bool {
        let old_ok = step
            .old
            .is_none_or(|o| apply_delims_backward(old_closes, &self.old_delims[o]));
        let new_ok = step
            .new
            .is_none_or(|n| apply_delims_backward(new_closes, &self.new_delims[n]));
        old_ok && new_ok
    }

    fn step_blank(&self, step: &ResolvedStep) -> bool {
        match (step.old, step.new) {
            (Some(o), _) => is_blank(&self.old_rows[o]),
            (None, Some(n)) => is_blank(&self.new_rows[n]),
            (None, None) => true,
        }
    }

    fn is_blank_pair(&self, index: usize) -> bool {
        match (self.steps[index].old, self.steps[index].new) {
            (Some(o), Some(n)) => is_blank(&self.old_rows[o]) && is_blank(&self.new_rows[n]),
            _ => false,
        }
    }

    fn blank_at(&self, plateaus: &[Range<usize>], i: usize) -> bool {
        plateaus
            .iter()
            .any(|plateau| plateau.len() == 1 && plateau.start == i && self.is_blank_pair(i))
    }

    fn shares_unmatched_salient(&self, step: &ResolvedStep, anchor: &ResolvedStep) -> bool {
        let (Some(ao), Some(an)) = (anchor.old, anchor.new) else {
            return false;
        };
        if self.old_rows[ao].text().trim() == self.new_rows[an].text().trim() {
            return false;
        }
        let matches_tokens = |skip_tokens: &FxHashMap<&str, usize>,
                              donor: &FxHashMap<&str, usize>,
                              already_kept: &FxHashMap<&str, usize>| {
            skip_tokens.keys().any(|t| {
                t.len() >= 4
                    && donor.iter().any(|(a, &donor_count)| {
                        let kept_count = already_kept.get(a).copied().unwrap_or(0);
                        a.len() >= 4 && donor_count > kept_count && is_salient_word_match(t, a)
                    })
            })
        };
        match (step.old, step.new) {
            (None, Some(sn)) => matches_tokens(
                &self.new_tokens[sn],
                &self.old_tokens[ao],
                &self.new_tokens[an],
            ),
            (Some(so), None) => matches_tokens(
                &self.old_tokens[so],
                &self.new_tokens[an],
                &self.old_tokens[ao],
            ),
            _ => false,
        }
    }

    /// Computes `(span, residual)` for correspondence `i`, bounded on the left
    /// by `floor`.
    fn pair_span(&self, i: usize, floor: usize) -> (Range<usize>, Option<OpenResidual>) {
        let mut start = i;
        let mut end = i;
        let (Some(ao), Some(an)) = (self.steps[i].old, self.steps[i].new) else {
            return (i..i + 1, None);
        };

        // Backwards: close any leading unmatched closers that differ between
        // the two sides of row `i`, bounded by `floor`.
        let mut bwd_old = self.old_delims[ao].closes.clone();
        let mut bwd_new = self.new_delims[an].closes.clone();
        if bwd_old != bwd_new {
            let mut j = i;
            let mut ok = true;
            while j > floor
                && bwd_old != bwd_new
                && !self.steps[j - 1].is_diag()
                && !self.step_blank(&self.steps[j - 1])
            {
                if !self.apply_step_bwd(&self.steps[j - 1], &mut bwd_old, &mut bwd_new) {
                    ok = false;
                    break;
                }
                j -= 1;
            }
            if ok && bwd_old == bwd_new {
                start = j;
            }
        }

        // Forwards: extend while the two sides' trailing opener stacks differ,
        // committing only when they converge.
        let mut fwd_old = self.old_delims[ao].opens.clone();
        let mut fwd_new = self.new_delims[an].opens.clone();
        let mut residual: Option<OpenResidual> = None;
        if fwd_old != fwd_new {
            let mut j = i + 1;
            let mut ok = true;
            while j < self.steps.len()
                && fwd_old != fwd_new
                && !self.steps[j].is_diag()
                && !self.step_blank(&self.steps[j])
            {
                if !self.apply_step_fwd(&self.steps[j], &mut fwd_old, &mut fwd_new) {
                    ok = false;
                    break;
                }
                j += 1;
            }
            if ok && fwd_old == fwd_new {
                end = j - 1;
            } else if ok && j < self.steps.len() && self.steps[j].is_diag() {
                residual = Some(OpenResidual {
                    next_scan: j,
                    old_stack: fwd_old,
                    new_stack: fwd_new,
                });
            }
        }

        // Enclosing frame: claim `[op_start..=closer]` when a balanced row sits
        // inside an open skip construct (e.g. `check!(` ... `Mod(arg)` ...
        // `);`).
        if start == i
            && end == i
            && residual.is_none()
            && self.old_delims[ao].closes == self.new_delims[an].closes
            && self.old_delims[ao].opens == self.new_delims[an].opens
            && let Some((frame_start, frame_end)) = self.try_enclosing_frame(i, floor)
        {
            start = frame_start;
            end = frame_end;
        }

        let is_symmetric_opener = !self.old_delims[ao].opens.is_empty()
            && self.old_delims[ao].opens == self.new_delims[an].opens;
        let is_symmetric_closer = !self.old_delims[ao].closes.is_empty()
            && self.old_delims[ao].closes == self.new_delims[an].closes;

        let delim_extended = start < i || end > i || residual.is_some();
        if !delim_extended
            && !is_symmetric_closer
            && i > floor
            && !self.steps[i - 1].is_diag()
            && !self.step_blank(&self.steps[i - 1])
            && self.shares_unmatched_salient(&self.steps[i - 1], &self.steps[i])
        {
            start = i - 1;
        }
        if !delim_extended
            && !is_symmetric_opener
            && i + 1 < self.steps.len()
            && !self.steps[i + 1].is_diag()
            && !self.step_blank(&self.steps[i + 1])
            && self.shares_unmatched_salient(&self.steps[i + 1], &self.steps[i])
        {
            end = i + 1;
        }

        (start..end + 1, residual)
    }

    fn try_enclosing_frame(&self, i: usize, floor: usize) -> Option<(usize, usize)> {
        let mut opener_idx = None;
        let mut j = i;
        while j > floor && !self.steps[j - 1].is_diag() && !self.step_blank(&self.steps[j - 1]) {
            let delims = match (self.steps[j - 1].old, self.steps[j - 1].new) {
                (Some(o), None) => &self.old_delims[o],
                (None, Some(n)) => &self.new_delims[n],
                _ => break,
            };
            if !delims.opens.is_empty() {
                opener_idx = Some(j - 1);
            }
            j -= 1;
        }
        let op_start = opener_idx?;

        let mut enc_old = Vec::new();
        let mut enc_new = Vec::new();
        for step in &self.steps[op_start..i] {
            if !self.apply_step_fwd(step, &mut enc_old, &mut enc_new) {
                return None;
            }
        }
        if enc_old == enc_new {
            return None;
        }
        let mut k = i + 1;
        while k < self.steps.len()
            && enc_old != enc_new
            && !self.steps[k].is_diag()
            && !self.step_blank(&self.steps[k])
        {
            if !self.apply_step_fwd(&self.steps[k], &mut enc_old, &mut enc_new) {
                return None;
            }
            k += 1;
        }
        (enc_old == enc_new && k > i + 1).then_some((op_start, k - 1))
    }

    /// Tentatively absorbs subsequent correspondences while the two sides'
    /// opener stacks remain divergent, returning `(span_end, next_pair_index)`
    /// only if they converge cleanly.
    fn fold_multi_pair(
        &self,
        plateaus: &[Range<usize>],
        pairs: &[usize],
        seed_pair_idx: usize,
        open: OpenResidual,
    ) -> Option<(usize, usize)> {
        let OpenResidual {
            next_scan: mut tent_end,
            old_stack: mut tent_old,
            new_stack: mut tent_new,
        } = open;
        let mut tent_b = seed_pair_idx + 1;

        while tent_b < pairs.len() && tent_old != tent_new {
            let next = pairs[tent_b];
            if self.blank_at(plateaus, next) {
                return None;
            }
            let mut j = tent_end;
            while j < next && tent_old != tent_new && !self.step_blank(&self.steps[j]) {
                if !self.apply_step_fwd(&self.steps[j], &mut tent_old, &mut tent_new) {
                    return None;
                }
                j += 1;
            }
            if j < next {
                return (tent_old == tent_new).then_some((j, tent_b));
            }
            if !self.apply_step_fwd(&self.steps[next], &mut tent_old, &mut tent_new) {
                return None;
            }
            let mut k = next + 1;
            while k < self.steps.len()
                && tent_old != tent_new
                && !self.steps[k].is_diag()
                && !self.step_blank(&self.steps[k])
            {
                if !self.apply_step_fwd(&self.steps[k], &mut tent_old, &mut tent_new) {
                    return None;
                }
                k += 1;
            }
            tent_end = k;
            tent_b += 1;
        }

        (tent_old == tent_new).then_some((tent_end, tent_b))
    }
}

fn resolve_steps(path: &ClusterPath) -> Vec<ResolvedStep> {
    let mut steps = Vec::with_capacity(path.steps.len());
    let (mut old, mut new) = (0usize, 0usize);
    for step in &path.steps {
        match step {
            PathStep::Diag => {
                steps.push(ResolvedStep {
                    old: Some(old),
                    new: Some(new),
                });
                old += 1;
                new += 1;
            }
            PathStep::DropOld => {
                steps.push(ResolvedStep {
                    old: Some(old),
                    new: None,
                });
                old += 1;
            }
            PathStep::DropNew => {
                steps.push(ResolvedStep {
                    old: None,
                    new: Some(new),
                });
                new += 1;
            }
        }
    }
    steps
}

/// Applies `delims` forward onto an active opener stack. Returns `false` if a
/// leading closer does not match the top of `stack` (exiting an outer scope or
/// mismatched delimiter).
fn apply_delims_forward(stack: &mut Vec<u8>, delims: &LineDelims) -> bool {
    for &c in &delims.closes {
        if stack.last() == Some(&c) {
            stack.pop();
        } else {
            return false;
        }
    }
    stack.extend_from_slice(&delims.opens);
    true
}

/// Applies `delims` backward onto pending unmatched closers (left-to-right).
/// Returns `false` if a trailing opener does not match the leftmost pending
/// closer.
fn apply_delims_backward(pending_closes: &mut Vec<u8>, delims: &LineDelims) -> bool {
    for &o in delims.opens.iter().rev() {
        if pending_closes.first() == Some(&o) {
            pending_closes.remove(0);
        } else {
            return false;
        }
    }
    if !delims.closes.is_empty() {
        pending_closes.splice(0..0, delims.closes.iter().copied());
    }
    true
}

fn is_blank(row: &Row) -> bool {
    row.text().trim().is_empty()
}

/// Identifier-grade tokens of each row with multiplicities, for terminal
/// participation tests.
///
/// Multiplicities matter when a collapsed or expanded line repeats a subword
/// (e.g. `step1(); step2();` carries `"step"` twice while `step1();` carries it
/// once): set subtraction would discard the unmatched extra occurrence.
fn salient_tokens<'a>(
    rows: &'a [Row],
    profile: Option<&LangProfile>,
) -> Vec<FxHashMap<&'a str, usize>> {
    rows.iter()
        .map(|row| {
            let tokens: Vec<&str> = tokenize_line(row.text()).collect();
            let mut salient: FxHashMap<&'a str, usize> = FxHashMap::default();
            for (i, token) in tokens.iter().enumerate() {
                if is_token_salient_in_context(&tokens, i, profile) {
                    *salient.entry(*token).or_insert(0) += 1;
                }
            }
            salient
        })
        .collect()
}

/// Projects step windows onto cluster row spans, in path order, emitting the
/// rows no window claims as plain deletions and additions.
fn emit_segments(steps: &[ResolvedStep], window_steps: &[Range<usize>]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut index = 0usize;
    for window in window_steps {
        while index < window.start {
            segments.push(segment_for_step(&steps[index]));
            index += 1;
        }
        let mut old_range: Option<Range<usize>> = None;
        let mut new_range: Option<Range<usize>> = None;
        let mut skips = 0usize;
        for step in &steps[window.clone()] {
            if let Some(o) = step.old {
                old_range = Some(extend(old_range, o));
            }
            if let Some(n) = step.new {
                new_range = Some(extend(new_range, n));
            }
            if !step.is_diag() {
                skips += 1;
            }
        }
        index = window.end;
        match (old_range, new_range) {
            // No skip is a member, so every correspondence is a plain
            // modification and no formation exists.
            (Some(_), Some(_)) if skips == 0 => {
                for step in &steps[window.clone()] {
                    if let (Some(o), Some(n)) = (step.old, step.new) {
                        segments.push(Segment::Pair { old: o, new: n });
                    }
                }
            }
            (Some(old), Some(new)) => segments.push(Segment::Window { old, new }),
            (Some(old), None) => segments.extend(old.map(|o| Segment::Del { old: o })),
            (None, Some(new)) => segments.extend(new.map(|n| Segment::Add { new: n })),
            (None, None) => {}
        }
    }
    while index < steps.len() {
        segments.push(segment_for_step(&steps[index]));
        index += 1;
    }
    segments
}

fn extend(range: Option<Range<usize>>, value: usize) -> Range<usize> {
    match range {
        Some(range) => range.start.min(value)..range.end.max(value + 1),
        None => value..value + 1,
    }
}

fn segment_for_step(step: &ResolvedStep) -> Segment {
    match (step.old, step.new) {
        (Some(o), Some(n)) => Segment::Pair { old: o, new: n },
        (Some(o), None) => Segment::Del { old: o },
        (None, Some(n)) => Segment::Add { new: n },
        (None, None) => unreachable!("a path step consumes at least one row"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{AddLine, DelLine};

    fn del_row(num: u32, text: &str) -> Row {
        Row::del(DelLine::new(num, text))
    }

    fn add_row(num: u32, text: &str) -> Row {
        Row::add(AddLine::new(num, text))
    }

    fn segment(old: &[&str], new: &[&str], steps: &[PathStep]) -> Vec<Segment> {
        let old_rows: Vec<Row> = old
            .iter()
            .enumerate()
            .map(|(i, t)| del_row(i as u32 + 1, t))
            .collect();
        let new_rows: Vec<Row> = new
            .iter()
            .enumerate()
            .map(|(i, t)| add_row(i as u32 + 1, t))
            .collect();
        let path = ClusterPath {
            steps: steps.to_vec(),
            score: 0,
        };
        segment_path(&path, &old_rows, &new_rows, None)
    }

    #[test]
    fn plateau_of_one_is_a_plain_pair() {
        let segments = segment(&["let alpha = 1;"], &["let alpha = 1;"], &[PathStep::Diag]);
        assert_eq!(segments, vec![Segment::Pair { old: 0, new: 0 }]);
    }

    #[test]
    fn plateau_of_k_is_k_plain_pairs() {
        let segments = segment(
            &["let alpha = 1;", "let beta = 2;"],
            &["let alpha = 1;", "let beta = 2;"],
            &[PathStep::Diag, PathStep::Diag],
        );
        assert_eq!(
            segments,
            vec![
                Segment::Pair { old: 0, new: 0 },
                Segment::Pair { old: 1, new: 1 },
            ]
        );
    }

    #[test]
    fn leading_skip_without_evidence_stays_plain() {
        let segments = segment(
            &["totally unrelated prose here", "let alpha = 1;"],
            &["let alpha = 1;"],
            &[PathStep::DropOld, PathStep::Diag],
        );
        assert_eq!(
            segments,
            vec![Segment::Del { old: 0 }, Segment::Pair { old: 1, new: 0 }]
        );
    }

    #[test]
    fn leading_skip_that_opens_a_construct_joins_the_window() {
        let segments = segment(
            &["        Some(", "        value);"],
            &["        value;"],
            &[PathStep::DropOld, PathStep::Diag],
        );
        assert_eq!(
            segments,
            vec![Segment::Window {
                old: 0..2,
                new: 0..1
            }]
        );
    }

    #[test]
    fn trailing_skip_without_evidence_stays_plain() {
        let segments = segment(
            &["let alpha = 1;"],
            &["let alpha = 1;", "totally unrelated prose here"],
            &[PathStep::Diag, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![Segment::Pair { old: 0, new: 0 }, Segment::Add { new: 1 },]
        );
    }

    #[test]
    fn trailing_skip_carrying_relocated_text_extends_the_span() {
        let segments = segment(
            &["    self.vis = (target_slot - offset).min(limit);"],
            &[
                "    self.vis =",
                "            (target_slot - offset).min(limit);",
            ],
            &[PathStep::Diag, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![Segment::Window {
                old: 0..1,
                new: 0..2
            }]
        );
    }

    #[test]
    fn trailing_skip_closing_a_construct_extends_the_span() {
        let segments = segment(
            &["    let hit =", "        |pat: &str| call(pat);"],
            &[
                "    let hit = |pat: &str| {",
                "        call(pat);",
                "    };",
            ],
            &[PathStep::Diag, PathStep::Diag, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![Segment::Window {
                old: 0..2,
                new: 0..3
            }]
        );
    }

    #[test]
    fn interior_skip_is_retained_inside_the_window() {
        let segments = segment(
            &["        let x = value", "        value"],
            &[
                "        let x = Some(",
                "        value",
                "        ).clamp(x);",
            ],
            &[PathStep::Diag, PathStep::Diag, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![Segment::Window {
                old: 0..2,
                new: 0..3
            }]
        );
    }

    #[test]
    fn span_whose_construct_never_closes_is_not_a_formation() {
        let segments = segment(
            &["    let x = foo("],
            &["    let x = bar(", "        first,", "        second,"],
            &[PathStep::Diag, PathStep::DropNew, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![
                Segment::Pair { old: 0, new: 0 },
                Segment::Add { new: 1 },
                Segment::Add { new: 2 },
            ]
        );
    }

    #[test]
    fn prose_with_an_unbalanced_bracket_stays_plain() {
        let segments = segment(
            &["(see the note"],
            &["(see the note, revised", "and continued"],
            &[PathStep::Diag, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![Segment::Pair { old: 0, new: 0 }, Segment::Add { new: 1 }]
        );
    }

    #[test]
    fn consecutive_windows_stay_separate() {
        let segments = segment(
            &["    let alpha = 1,", "    let beta = 2,"],
            &[
                "    let alpha = {",
                "        first: 1,",
                "    },",
                "    let beta = {",
                "        second: 2,",
                "    },",
            ],
            &[
                PathStep::Diag,
                PathStep::DropNew,
                PathStep::DropNew,
                PathStep::Diag,
                PathStep::DropNew,
                PathStep::DropNew,
            ],
        );
        assert_eq!(
            segments,
            vec![
                Segment::Window {
                    old: 0..1,
                    new: 0..3
                },
                Segment::Window {
                    old: 1..2,
                    new: 3..6
                },
            ]
        );
    }

    #[test]
    fn collapsed_construct_joins_from_the_leading_side() {
        let segments = segment(
            &["    let alpha = Some(", "        value", "    );"],
            &["    let alpha = Some(value);"],
            &[PathStep::Diag, PathStep::DropOld, PathStep::DropOld],
        );
        assert_eq!(
            segments,
            vec![Segment::Window {
                old: 0..3,
                new: 0..1
            }]
        );
    }

    /// Specification test on the motivating hunk (`theme-scrollbar-palette`,
    /// `AGENTS.md` 18–25): the paragraph rewrite the factorizer read as one
    /// 4×4 hull (three additions, one modification, three deletions).
    ///
    /// The path keeps the single admissible correspondence (`old 0 ↔ new 3`)
    /// and segments only the immediately adjacent continuation (`old 1`,
    /// carrying the relocated `"the pi review extension"` phrase) into a tight
    /// `2×1` formation; every surrounding addition and deletion stays plain,
    /// eliminating the 4×4 hull. See §2.4 of the plan for why `pair_evidence`
    /// admits only `old 0 ↔ new 3` (old 1 ↔ new 4 sits at 29% salient overlap,
    /// below the 30% floor, and old 2 ↔ new 5 shares no salient token).
    #[test]
    fn paragraph_reflow_pairs_continuations_instead_of_hull_group() {
        let old = [
            "The workspace also holds the engine's first consumer: **`extension/pi/`** —",
            "the pi review extension. The extension is a **thin client**: `/review`",
            "resolves the git repository, stops the pi TUI, and spawns the bundled",
            "`diffuse` binary (`bin/diffuse`) in interactive TUI mode with `stdio: \"inherit\"`.",
            "All diff parsing, rendering, TUI interaction, note persistence, and git actions",
            "are owned entirely by the native Rust implementation in `crates/`.",
        ];
        let new = [
            "The primary review flow is the `diffuse` CLI binary itself (interactive TUI",
            "reviewer, `--print` pager, `--json` token-stream output).",
            "",
            "The workspace also holds **`extension/pi/`** — the pi review extension",
            "(a thin client over the bundled binary). It remains a supported customer",
            "of the engine and a project deliverable; its dev install is handled",
            "automatically by `./scripts/check.sh`, so install it explicitly only if",
            "requested.",
        ];
        let old_rows: Vec<Row> = old
            .iter()
            .enumerate()
            .map(|(i, t)| del_row(18 + i as u32, t))
            .collect();
        let new_rows: Vec<Row> = new
            .iter()
            .enumerate()
            .map(|(i, t)| add_row(18 + i as u32, t))
            .collect();
        let path =
            super::super::lattice::solve_cluster_path(&old_rows, &new_rows, None, None, None);
        let segments = segment_path(&path, &old_rows, &new_rows, None);

        // Only the relocated continuation (`old 1` carrying "the pi review
        // extension") joins `old 0 -> new 3` in a tight 2×1 formation; the
        // surrounding unrelated additions and deletions stay plain (no hull).
        let windows: Vec<(Range<usize>, Range<usize>)> = segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Window { old, new } => Some((old.clone(), new.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            windows,
            vec![(0..2, 3..4)],
            "tight 2×1 continuation formation without hull; segments {segments:?}"
        );
        assert_eq!(
            segments
                .iter()
                .filter(|s| matches!(s, Segment::Del { .. }))
                .count(),
            4
        );
        assert_eq!(
            segments
                .iter()
                .filter(|s| matches!(s, Segment::Add { .. }))
                .count(),
            7
        );
    }

    #[test]
    fn synchronized_blank_pair_separates_windows() {
        let segments = segment(
            &["let alpha = 1;", "", "let beta = 2;"],
            &["let alpha = 1;", "", "let beta = 2;"],
            &[PathStep::Diag, PathStep::Diag, PathStep::Diag],
        );
        assert_eq!(
            segments,
            vec![
                Segment::Pair { old: 0, new: 0 },
                Segment::Pair { old: 1, new: 1 },
                Segment::Pair { old: 2, new: 2 },
            ]
        );
    }

    #[test]
    fn adjacent_expanded_enum_variants_both_form_windows() {
        let segments = segment(
            &["    MouseScrollUp(usize),", "    MouseScrollDown(usize),"],
            &[
                "    MouseScrollUp {",
                "        n: usize,",
                "        row: usize,",
                "        col: usize,",
                "    },",
                "    MouseScrollDown {",
                "        n: usize,",
                "        row: usize,",
                "        col: usize,",
                "    },",
            ],
            &[
                PathStep::Diag,
                PathStep::DropNew,
                PathStep::DropNew,
                PathStep::DropNew,
                PathStep::DropNew,
                PathStep::Diag,
                PathStep::DropNew,
                PathStep::DropNew,
                PathStep::DropNew,
                PathStep::DropNew,
            ],
        );
        assert_eq!(
            segments,
            vec![
                Segment::Window {
                    old: 0..1,
                    new: 0..5,
                },
                Segment::Window {
                    old: 1..2,
                    new: 5..10,
                },
            ]
        );
    }

    #[test]
    fn unclosed_expansion_dissolves_to_plain_rows() {
        let segments = segment(
            &["    foo(a, b);"],
            &["    foo(", "        a,", "        b,"],
            &[PathStep::Diag, PathStep::DropNew, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![
                Segment::Pair { old: 0, new: 0 },
                Segment::Add { new: 1 },
                Segment::Add { new: 2 },
            ]
        );
    }

    #[test]
    fn verbatim_closed_pair_does_not_absorb_unrelated_adjacent_add() {
        let segments = segment(
            &["                self.is_dragging_scrollbar = false;"],
            &[
                "        self.is_dragging_scrollbar = false;",
                "        self.scrollbar_drag_grab = None;",
            ],
            &[PathStep::Diag, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![Segment::Pair { old: 0, new: 0 }, Segment::Add { new: 1 }]
        );
    }

    #[test]
    fn symmetric_opener_does_not_absorb_body_statement() {
        let segments = segment(
            &["    if !run.is_empty() && !moved_dels.contains(&run[0]) {"],
            &[
                "    if !run.is_empty() {",
                "        let moved_in_run: Vec<usize> = run.clone();",
            ],
            &[PathStep::Diag, PathStep::DropNew],
        );
        assert_eq!(
            segments,
            vec![Segment::Pair { old: 0, new: 0 }, Segment::Add { new: 1 }]
        );
    }

    #[test]
    fn multiline_signature_collapse_converges_on_paren_to_brace() {
        let segments = segment(
            &[
                "pub(crate) fn is_statement_start(",
                "    text: &str,",
                "    profile: Option<&LangProfile>,",
                ") -> bool {",
            ],
            &[
                "pub(crate) fn is_statement_start(text: &str, profile: Option<&LangProfile>) -> bool {",
            ],
            &[
                PathStep::Diag,
                PathStep::DropOld,
                PathStep::DropOld,
                PathStep::DropOld,
            ],
        );
        assert_eq!(
            segments,
            vec![Segment::Window {
                old: 0..4,
                new: 0..1,
            }]
        );
    }
}
