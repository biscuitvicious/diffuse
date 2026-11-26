//! Path segmentation: turns a cluster path into formations and plain rows.
//!
//! A solved cluster path is decomposed into its monotone sequence of `Diag`
//! correspondences and the `K + 1` two-sided gaps `(gap.old, gap.new)` between
//! them, making segmentation invariant to arbitrary 1D `DropOld`/`DropNew`
//! tie-break ordering in mixed gaps. Membership in a formation window is
//! decided by information, never by position:
//!
//! - **Construct continuity.** Each line reduces to its unmatched leading
//!   closers and trailing openers via `UniversalDelimiterClassifier`. Skips on
//!   each side join a correspondence independently while the two sides' Dyck
//!   stacks diverge, or when one or more interior correspondences sit inside a
//!   balanced single-side enclosing frame (with multi-`Diag` frames restricted
//!   to non-brace expression wrappers); speculative extensions commit only when
//!   the two sides converge.
//! - **Unmatched salient evidence.** When a balanced correspondence does not
//!   extend by delimiter balance, the immediately adjacent skip on each side
//!   sharing unmatched salient content (tracked by token multiplicity) joins
//!   the window independently, guarded against verbatim pairs and symmetric
//!   block boundaries.
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

use super::lattice::ClusterPath;

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

/// Two independent ordered skip queues between consecutive `Diag` anchors.
#[derive(Clone, Debug)]
struct Gap {
    old: Range<usize>,
    new: Range<usize>,
}

/// Which side of a cluster owns an enclosing delimiter frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Old,
    New,
}

/// A segmented window over `diags[first_diag..=last_diag]` claiming `old` and
/// `new` cluster row ranges.
#[derive(Clone, Debug)]
struct WindowSpan {
    first_diag: usize,
    last_diag: usize,
    old: Range<usize>,
    new: Range<usize>,
}

impl WindowSpan {
    fn single_pair(diag_idx: usize, old: usize, new: usize) -> Self {
        Self {
            first_diag: diag_idx,
            last_diag: diag_idx,
            old: old..old + 1,
            new: new..new + 1,
        }
    }

    fn has_skips(&self) -> bool {
        let diag_count = self.last_diag - self.first_diag + 1;
        self.old.len() > diag_count || self.new.len() > diag_count
    }
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

/// Precomputed per-row delimiter and salient-token tables for one path,
/// decomposed into monotone `Diag` anchors and two-sided inter-`Diag` gaps.
struct PathSegmenter<'a> {
    diags: Vec<(usize, usize)>,
    gaps: Vec<Gap>,
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
        let (diags, gaps) = decompose_path(path, old_rows.len(), new_rows.len());
        Self {
            diags,
            gaps,
            old_rows,
            new_rows,
            old_tokens: salient_tokens(old_rows, profile),
            new_tokens: salient_tokens(new_rows, profile),
            old_delims: old_rows.iter().map(LineDelims::of).collect(),
            new_delims: new_rows.iter().map(LineDelims::of).collect(),
        }
    }

    fn segment(&self) -> Vec<Segment> {
        let mut windows: Vec<WindowSpan> = Vec::new();
        let mut floor_old = 0usize;
        let mut floor_new = 0usize;
        let mut p = 0usize;

        while p < self.diags.len() {
            let gap_in = &self.gaps[p];
            let f_old = floor_old.max(gap_in.old.start);
            let f_new = floor_new.max(gap_in.new.start);

            let win = if self.is_blank_pair(p) {
                let (o, n) = self.diags[p];
                WindowSpan::single_pair(p, o, n)
            } else {
                self.build_window(p, f_old, f_new)
            };

            floor_old = win.old.end;
            floor_new = win.new.end;
            p = win.last_diag + 1;
            windows.push(win);
        }

        self.emit_segments(&windows)
    }

    fn is_blank_pair(&self, diag_idx: usize) -> bool {
        let (o, n) = self.diags[diag_idx];
        is_blank(&self.old_rows[o]) && is_blank(&self.new_rows[n])
    }

    fn side_rows(&self, side: Side) -> &'a [Row] {
        match side {
            Side::Old => self.old_rows,
            Side::New => self.new_rows,
        }
    }

    fn side_delims(&self, side: Side) -> &[LineDelims] {
        match side {
            Side::Old => &self.old_delims,
            Side::New => &self.new_delims,
        }
    }

    fn gap_side(&self, gap_idx: usize, side: Side) -> Range<usize> {
        match side {
            Side::Old => self.gaps[gap_idx].old.clone(),
            Side::New => self.gaps[gap_idx].new.clone(),
        }
    }

    fn shares_unmatched_salient(&self, side: Side, skip: usize, ao: usize, an: usize) -> bool {
        if self.old_rows[ao].text().trim() == self.new_rows[an].text().trim() {
            return false;
        }
        match side {
            Side::Old => matches_unmatched_tokens(
                &self.old_tokens[skip],
                &self.new_tokens[an],
                &self.old_tokens[ao],
            ),
            Side::New => matches_unmatched_tokens(
                &self.new_tokens[skip],
                &self.old_tokens[ao],
                &self.new_tokens[an],
            ),
        }
    }

    fn salient_preceding_skip(
        &self,
        side: Side,
        p: usize,
        floor: usize,
        ao: usize,
        an: usize,
    ) -> Option<usize> {
        let gap_end = self.gap_side(p, side).end;
        if gap_end <= floor {
            return None;
        }
        let skip = gap_end - 1;
        (!is_blank(&self.side_rows(side)[skip])
            && self.shares_unmatched_salient(side, skip, ao, an))
        .then_some(skip)
    }

    fn salient_following_skip(&self, side: Side, p: usize, ao: usize, an: usize) -> Option<usize> {
        let gap = self.gap_side(p + 1, side);
        if gap.is_empty() {
            return None;
        }
        let skip = gap.start;
        (!is_blank(&self.side_rows(side)[skip])
            && self.shares_unmatched_salient(side, skip, ao, an))
        .then_some(skip)
    }

    /// Computes the maximal window starting at correspondence `p`, bounded on
    /// the left by `(floor_old, floor_new)` within `gaps[p]`.
    fn build_window(&self, p: usize, floor_old: usize, floor_new: usize) -> WindowSpan {
        let (ao, an) = self.diags[p];
        let mut old_start = ao;
        let mut new_start = an;
        let mut old_end = ao + 1;
        let mut new_end = an + 1;
        let mut last_diag = p;

        // 1. Backwards: close leading unmatched closers that differ between the
        //    two sides of row `p`, scanning `gaps[p].old` and `gaps[p].new`
        //    independently so unrelated opposite-side skips are never absorbed.
        let bwd_old = &self.old_delims[ao].closes;
        let bwd_new = &self.new_delims[an].closes;
        if bwd_old != bwd_new
            && let Some((os, ns)) =
                self.converge_backward(p, floor_old, floor_new, bwd_old, bwd_new)
        {
            old_start = os;
            new_start = ns;
        }

        // 2. Forwards: extend while trailing opener stacks differ, first within
        //    `gaps[p + 1]` independently per side, then tentatively folding
        //    subsequent correspondences if still open.
        let fwd_old = &self.old_delims[ao].opens;
        let fwd_new = &self.new_delims[an].opens;
        if fwd_old != fwd_new {
            if let Some((oe, ne)) = self.converge_forward(p + 1, fwd_old, fwd_new) {
                old_end = oe;
                new_end = ne;
            } else if p + 1 < self.diags.len()
                && let Some((end_diag, oe, ne)) = self.fold_multi_pair(p, fwd_old, fwd_new)
            {
                last_diag = end_diag;
                old_end = oe;
                new_end = ne;
            }
        }

        let delim_extended = old_start < ao
            || new_start < an
            || old_end > ao + 1
            || new_end > an + 1
            || last_diag > p;

        // 3. Enclosing frame: claim an `old`-only or `new`-only balanced outer
        //    delimiter frame spanning one or more interior `Diag` steps while
        //    leaving unrelated opposite-side skips outside the window.
        if !delim_extended
            && bwd_old == bwd_new
            && fwd_old == fwd_new
            && let Some(frame) = self.try_enclosing_frame(p, floor_old, floor_new)
        {
            return frame;
        }

        // 4. Unmatched salient evidence: when delimiter balance did not extend
        //    the correspondence, check the immediately adjacent skip on each
        //    side independently (invariant to 1D `DropOld`/`DropNew` order).
        let is_symmetric_opener = !fwd_old.is_empty() && fwd_old == fwd_new;
        let is_symmetric_closer = !bwd_old.is_empty() && bwd_old == bwd_new;

        if !delim_extended && !is_symmetric_closer {
            if let Some(so) = self.salient_preceding_skip(Side::Old, p, floor_old, ao, an) {
                old_start = so;
            }
            if let Some(sn) = self.salient_preceding_skip(Side::New, p, floor_new, ao, an) {
                new_start = sn;
            }
        }
        if !delim_extended && !is_symmetric_opener {
            if let Some(so) = self.salient_following_skip(Side::Old, p, ao, an) {
                old_end = so + 1;
            }
            if let Some(sn) = self.salient_following_skip(Side::New, p, ao, an) {
                new_end = sn + 1;
            }
        }

        WindowSpan {
            first_diag: p,
            last_diag,
            old: old_start..old_end,
            new: new_start..new_end,
        }
    }

    /// Scans backward within `gaps[gap_idx]` (bounded by
    /// `floor_old`/`floor_new`) to make `bwd_old` and `bwd_new` converge
    /// without pulling unrelated opposite-side skips into a single-side
    /// delimiter closure.
    fn converge_backward(
        &self,
        gap_idx: usize,
        floor_old: usize,
        floor_new: usize,
        bwd_old: &[u8],
        bwd_new: &[u8],
    ) -> Option<(usize, usize)> {
        let gap = &self.gaps[gap_idx];
        let common = common_suffix_len(bwd_old, bwd_new);
        let common_slice = &bwd_old[bwd_old.len() - common..];
        converge_gap_sides(
            bwd_old,
            bwd_new,
            common_slice,
            (gap.old.end, gap.new.end),
            |target| self.scan_side_backward(Side::Old, floor_old..gap.old.end, bwd_old, target),
            |target| self.scan_side_backward(Side::New, floor_new..gap.new.end, bwd_new, target),
        )
    }

    fn scan_side_backward(
        &self,
        side: Side,
        range: Range<usize>,
        start_closes: &[u8],
        target_closes: &[u8],
    ) -> Option<usize> {
        let rows = self.side_rows(side);
        let delims = self.side_delims(side);
        let mut stack = start_closes.to_vec();
        let mut idx = range.end;
        while idx > range.start && stack != target_closes {
            let prev = idx - 1;
            if is_blank(&rows[prev]) || !apply_delims_backward(&mut stack, &delims[prev]) {
                return None;
            }
            idx = prev;
        }
        (stack == target_closes).then_some(idx)
    }

    /// Scans forward within `gaps[gap_idx]` to make `fwd_old` and `fwd_new`
    /// converge without pulling unrelated opposite-side skips into a
    /// single-side delimiter closure.
    fn converge_forward(
        &self,
        gap_idx: usize,
        fwd_old: &[u8],
        fwd_new: &[u8],
    ) -> Option<(usize, usize)> {
        let gap = &self.gaps[gap_idx];
        let common = common_prefix_len(fwd_old, fwd_new);
        let common_slice = &fwd_old[..common];
        converge_gap_sides(
            fwd_old,
            fwd_new,
            common_slice,
            (gap.old.start, gap.new.start),
            |target| self.scan_side_forward(Side::Old, gap.old.clone(), fwd_old, target),
            |target| self.scan_side_forward(Side::New, gap.new.clone(), fwd_new, target),
        )
    }

    fn scan_side_forward(
        &self,
        side: Side,
        range: Range<usize>,
        start_opens: &[u8],
        target_opens: &[u8],
    ) -> Option<usize> {
        let rows = self.side_rows(side);
        let delims = self.side_delims(side);
        let mut stack = start_opens.to_vec();
        let mut idx = range.start;
        while idx < range.end && stack != target_opens {
            if is_blank(&rows[idx]) || !apply_delims_forward(&mut stack, &delims[idx]) {
                return None;
            }
            idx += 1;
        }
        (stack == target_opens).then_some(idx)
    }

    /// Detects an `old`-only or `new`-only balanced outer delimiter frame
    /// starting in `gaps[p]` and closing in `gaps[q + 1]` across one or more
    /// interior `Diag` steps (`p..=q`), while leaving unrelated skips on the
    /// opposite side outside the window.
    fn try_enclosing_frame(
        &self,
        p: usize,
        floor_old: usize,
        floor_new: usize,
    ) -> Option<WindowSpan> {
        self.try_enclosing_frame_side(Side::Old, p, floor_old)
            .or_else(|| self.try_enclosing_frame_side(Side::New, p, floor_new))
    }

    fn try_enclosing_frame_side(&self, side: Side, p: usize, floor: usize) -> Option<WindowSpan> {
        let gap_in = self.gap_side(p, side);
        let rows = self.side_rows(side);
        let delims = self.side_delims(side);

        // Collect candidate opener rows in `floor..gap_in.end` before any blank
        // line, ordered outermost first.
        let mut candidates = Vec::new();
        let mut j = gap_in.end;
        while j > floor {
            let prev = j - 1;
            if is_blank(&rows[prev]) {
                break;
            }
            if !delims[prev].opens.is_empty() {
                candidates.push(prev);
            }
            j = prev;
        }
        candidates.reverse();

        let brace_id = match UniversalDelimiterClassifier.classify_char('{') {
            Some(DelimiterCategory::Open(id)) => Some(id),
            _ => None,
        };

        for op_start in candidates {
            let Some(enc_stack) = open_frame_stack(&delims[op_start..gap_in.end]) else {
                continue;
            };

            // Multi-`Diag` enclosing frames are restricted to expression
            // wrappers (`(`, `[`), never `{` statement blocks (`for`/`while`/
            // `if`/`fn` bodies), so unwrapping a multi-statement control-flow
            // block never drags its entire body into one giant formation.
            let max_q = if brace_id.is_some_and(|id| enc_stack.contains(&id)) {
                p + 1
            } else {
                self.diags.len()
            };

            let (mut old_stack, mut new_stack) = match side {
                Side::Old => (enc_stack, Vec::new()),
                Side::New => (Vec::new(), enc_stack),
            };

            for q in p..max_q {
                if q > p && !self.apply_full_gap_fwd(q, &mut old_stack, &mut new_stack) {
                    break;
                }
                if self.is_blank_pair(q) {
                    break;
                }
                let (qo, qn) = self.diags[q];
                if !apply_delims_forward(&mut old_stack, &self.old_delims[qo])
                    || !apply_delims_forward(&mut new_stack, &self.new_delims[qn])
                {
                    break;
                }

                // Check whether the frame side closes cleanly in `gaps[q + 1]`.
                let gap_out = self.gap_side(q + 1, side);
                let (frame_stack, other_stack) = match side {
                    Side::Old => (&old_stack, &new_stack),
                    Side::New => (&new_stack, &old_stack),
                };
                if let Some(close_end) =
                    self.scan_side_forward(side, gap_out.clone(), frame_stack, other_stack)
                    && (close_end > gap_out.start || q > p)
                {
                    let (p_old, p_new) = self.diags[p];
                    let (old, new) = match side {
                        Side::Old => (op_start..close_end, p_new..qn + 1),
                        Side::New => (p_old..qo + 1, op_start..close_end),
                    };
                    return Some(WindowSpan {
                        first_diag: p,
                        last_diag: q,
                        old,
                        new,
                    });
                }
            }
        }
        None
    }

    fn apply_full_gap_fwd(
        &self,
        gap_idx: usize,
        old_stack: &mut Vec<u8>,
        new_stack: &mut Vec<u8>,
    ) -> bool {
        let gap = &self.gaps[gap_idx];
        for o in gap.old.clone() {
            if is_blank(&self.old_rows[o]) || !apply_delims_forward(old_stack, &self.old_delims[o])
            {
                return false;
            }
        }
        for n in gap.new.clone() {
            if is_blank(&self.new_rows[n]) || !apply_delims_forward(new_stack, &self.new_delims[n])
            {
                return false;
            }
        }
        true
    }

    /// Tentatively absorbs subsequent correspondences while the two sides'
    /// opener stacks remain divergent, returning `(last_diag, old_end,
    /// new_end)` only if they converge cleanly.
    fn fold_multi_pair(
        &self,
        seed_diag: usize,
        start_old: &[u8],
        start_new: &[u8],
    ) -> Option<(usize, usize, usize)> {
        let mut tent_old = start_old.to_vec();
        let mut tent_new = start_new.to_vec();

        for q in (seed_diag + 1)..self.diags.len() {
            if !self.apply_full_gap_fwd(q, &mut tent_old, &mut tent_new) {
                return None;
            }
            if tent_old == tent_new {
                let prev_gap = &self.gaps[q];
                return Some((q - 1, prev_gap.old.end, prev_gap.new.end));
            }
            if self.is_blank_pair(q) {
                return None;
            }
            let (qo, qn) = self.diags[q];
            if !apply_delims_forward(&mut tent_old, &self.old_delims[qo])
                || !apply_delims_forward(&mut tent_new, &self.new_delims[qn])
            {
                return None;
            }
            if tent_old == tent_new {
                return Some((q, qo + 1, qn + 1));
            }
            if let Some((oe, ne)) = self.converge_forward(q + 1, &tent_old, &tent_new) {
                return Some((q, oe, ne));
            }
        }
        None
    }

    /// Emits segments in canonical order: unclaimed deletions before unclaimed
    /// additions in every gap between windows, plain `Pair`s for skip-free
    /// windows, and `Window`s for formations carrying at least one skip.
    fn emit_segments(&self, windows: &[WindowSpan]) -> Vec<Segment> {
        let mut segments = Vec::new();
        let (mut oi, mut ni) = (0usize, 0usize);

        for win in windows {
            while oi < win.old.start {
                segments.push(Segment::Del { old: oi });
                oi += 1;
            }
            while ni < win.new.start {
                segments.push(Segment::Add { new: ni });
                ni += 1;
            }

            if win.has_skips() {
                segments.push(Segment::Window {
                    old: win.old.clone(),
                    new: win.new.clone(),
                });
            } else {
                for &(old, new) in &self.diags[win.first_diag..=win.last_diag] {
                    segments.push(Segment::Pair { old, new });
                }
            }
            oi = win.old.end;
            ni = win.new.end;
        }

        while oi < self.old_rows.len() {
            segments.push(Segment::Del { old: oi });
            oi += 1;
        }
        while ni < self.new_rows.len() {
            segments.push(Segment::Add { new: ni });
            ni += 1;
        }

        segments
    }
}

fn decompose_path(
    path: &ClusterPath,
    old_len: usize,
    new_len: usize,
) -> (Vec<(usize, usize)>, Vec<Gap>) {
    let diags = path.diag_pairs();
    let mut gaps = Vec::with_capacity(diags.len() + 1);
    let (mut prev_o, mut prev_n) = (0usize, 0usize);
    for &(do_idx, dn_idx) in &diags {
        gaps.push(Gap {
            old: prev_o..do_idx,
            new: prev_n..dn_idx,
        });
        prev_o = do_idx + 1;
        prev_n = dn_idx + 1;
    }
    gaps.push(Gap {
        old: prev_o..old_len,
        new: prev_n..new_len,
    });
    (diags, gaps)
}

/// Resolves a stack divergence `(old_stack, new_stack)` whose shared base is
/// `common_slice` (a common suffix backward or common prefix forward) by
/// trying: (1) closing the excess side(s), (2) letting the shorter side catch
/// up, and (3) closing both sides independently down to `common_slice`.
fn converge_gap_sides(
    old_stack: &[u8],
    new_stack: &[u8],
    common_slice: &[u8],
    unchanged: (usize, usize),
    mut scan_old: impl FnMut(&[u8]) -> Option<usize>,
    mut scan_new: impl FnMut(&[u8]) -> Option<usize>,
) -> Option<(usize, usize)> {
    let common = common_slice.len();
    // 1. Prefer closing the side(s) with excess unmatched delimiters.
    if old_stack.len() > common
        && let Some(o) = scan_old(new_stack)
    {
        return Some((o, unchanged.1));
    }
    if new_stack.len() > common
        && let Some(n) = scan_new(old_stack)
    {
        return Some((unchanged.0, n));
    }
    // 2. Otherwise allow the shorter side to catch up (e.g. a reflowed `{` or
    //    `=> {` moved onto an adjacent line).
    if old_stack.len() == common
        && let Some(o) = scan_old(new_stack)
    {
        return Some((o, unchanged.1));
    }
    if new_stack.len() == common
        && let Some(n) = scan_new(old_stack)
    {
        return Some((unchanged.0, n));
    }
    // 3. Finally, close both sides independently down to their shared base.
    if old_stack.len() > common
        && new_stack.len() > common
        && let Some(o) = scan_old(common_slice)
        && let Some(n) = scan_new(common_slice)
    {
        return Some((o, n));
    }
    None
}

/// Accumulates the strictly-open delimiter stack across a candidate frame
/// prefix, returning `None` if any row closes outside the frame or empties it.
fn open_frame_stack(delims: &[LineDelims]) -> Option<Vec<u8>> {
    let mut stack = Vec::new();
    for delim in delims {
        if !apply_delims_forward(&mut stack, delim) || stack.is_empty() {
            return None;
        }
    }
    (!stack.is_empty()).then_some(stack)
}

fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn common_suffix_len(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

fn matches_unmatched_tokens(
    skip_tokens: &FxHashMap<&str, usize>,
    donor: &FxHashMap<&str, usize>,
    already_kept: &FxHashMap<&str, usize>,
) -> bool {
    skip_tokens.keys().any(|t| {
        t.len() >= 4
            && donor.iter().any(|(a, &donor_count)| {
                let kept_count = already_kept.get(a).copied().unwrap_or(0);
                a.len() >= 4 && donor_count > kept_count && is_salient_word_match(t, a)
            })
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        collapse::lattice::PathStep,
        parse::{AddLine, DelLine},
    };

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

    /// Reproduces `scrollbar-grab-drag` `geometry.rs` (`old 230..=235` ->
    /// `new 237..=244`): an `old`-only `Some(` ... `)` wrapper spans two
    /// `Diag` correspondences while 5 unrelated pure additions sit in the
    /// preceding gap. Both 1D `DropOld`/`DropNew` tie-break orders must yield
    /// the same hull-free `5x2` window with the 5 pure additions emitted ahead
    /// as plain `Add` rows.
    #[test]
    fn two_sided_gap_enclosing_frame_wraps_multiple_diags_without_hull() {
        let old = [
            "        let max_thumb = track_h.saturating_sub(1).max(2);",
            "        Some(",
            "            ((vh as f64 / total_vis_lines as f64) * track_h as f64)",
            "                .round()",
            "                .clamp(2.0, max_thumb as f64) as usize,",
            "        )",
        ];
        let new = [
            "        let max_thumb = track_h.saturating_sub(1);",
            "        if max_thumb < Self::SCROLLBAR_MIN_THUMB_LINES {",
            "            return None;",
            "        }",
            "        let min_thumb = ((track_h as f64 * Self::SCROLLBAR_MIN_PERCENT).round() as usize)",
            "            .clamp(Self::SCROLLBAR_MIN_THUMB_LINES, max_thumb);",
            "        let prop_thumb = ((vh as f64 / total_vis_lines as f64) * track_h as f64).round() as usize;",
            "        Some(prop_thumb.clamp(min_thumb, max_thumb))",
        ];
        let expected = vec![
            Segment::Pair { old: 0, new: 0 },
            Segment::Add { new: 1 },
            Segment::Add { new: 2 },
            Segment::Add { new: 3 },
            Segment::Add { new: 4 },
            Segment::Add { new: 5 },
            Segment::Window {
                old: 1..6,
                new: 6..8,
            },
        ];

        // Order 1: DropOld before DropNew in the mixed gap.
        let steps_old_first = [
            PathStep::Diag,
            PathStep::DropOld,
            PathStep::DropNew,
            PathStep::DropNew,
            PathStep::DropNew,
            PathStep::DropNew,
            PathStep::DropNew,
            PathStep::Diag,
            PathStep::DropOld,
            PathStep::Diag,
            PathStep::DropOld,
        ];
        assert_eq!(
            segment(&old, &new, &steps_old_first),
            expected,
            "DropOld-first mixed gap must form hull-free 5x2 window"
        );

        // Order 2: DropNew before DropOld in the mixed gap.
        let steps_new_first = [
            PathStep::Diag,
            PathStep::DropNew,
            PathStep::DropNew,
            PathStep::DropNew,
            PathStep::DropNew,
            PathStep::DropNew,
            PathStep::DropOld,
            PathStep::Diag,
            PathStep::DropOld,
            PathStep::Diag,
            PathStep::DropOld,
        ];
        assert_eq!(
            segment(&old, &new, &steps_new_first),
            expected,
            "DropNew-first mixed gap must produce identical hull-free 5x2 window"
        );
    }

    /// A mixed inter-`Diag` gap must attach a salient skip on one side
    /// regardless of whether the 1D path emitted `DropOld` or `DropNew` first,
    /// without dragging the opposite side's unrelated skip into the window.
    #[test]
    fn two_sided_gap_salient_attachment_ignores_tie_break_order() {
        let old = ["    let value = compute_alpha();", "    input_token_extra"];
        let new = [
            "    let value = compute_alpha(input_token);",
            "    unrelated_added_line();",
        ];
        // Even if DropNew precedes DropOld in the 1D path, `old[1]` shares
        // unmatched salient `input_token` with `new[0]` (relocated into the
        // Mod) and joins while `new[1]` stays a plain Add.
        let steps = [PathStep::Diag, PathStep::DropNew, PathStep::DropOld];
        assert_eq!(
            segment(&old, &new, &steps),
            vec![
                Segment::Window {
                    old: 0..2,
                    new: 0..1,
                },
                Segment::Add { new: 1 },
            ]
        );
    }

    /// A `{` statement block (`for`/`if`/`while`) wrapping multiple `Diag`
    /// correspondences must not be absorbed into one multi-`Diag` enclosing
    /// frame.
    #[test]
    fn brace_block_wrapping_multiple_diags_stays_plain() {
        let old = [
            "    for item in items {",
            "        let a = compute_old(item);",
            "        let b = finish_old(a);",
            "    }",
        ];
        let new = [
            "    let a = compute_new(item);",
            "    let b = finish_new(a);",
        ];
        let steps = [
            PathStep::DropOld,
            PathStep::Diag,
            PathStep::Diag,
            PathStep::DropOld,
        ];
        assert_eq!(
            segment(&old, &new, &steps),
            vec![
                Segment::Del { old: 0 },
                Segment::Pair { old: 1, new: 0 },
                Segment::Pair { old: 2, new: 1 },
                Segment::Del { old: 3 },
            ]
        );
    }

    /// Symmetric `new`-side multi-`Diag` enclosing frame (`Some(` ... `)`)
    /// wraps multiple interior correspondences while leaving unrelated `old`
    /// deletions outside the window.
    #[test]
    fn new_side_enclosing_frame_wraps_multiple_diags_without_hull() {
        let old = [
            "    let deleted_before = 1;",
            "    let x = alpha(1);",
            "    let y = beta(2);",
        ];
        let new = [
            "    Some(",
            "        let x = alpha(10);",
            "        let y = beta(20);",
            "    )",
        ];
        let steps = [
            PathStep::DropOld,
            PathStep::DropNew,
            PathStep::Diag,
            PathStep::Diag,
            PathStep::DropNew,
        ];
        assert_eq!(
            segment(&old, &new, &steps),
            vec![
                Segment::Del { old: 0 },
                Segment::Window {
                    old: 1..3,
                    new: 0..4,
                },
            ]
        );
    }
}
