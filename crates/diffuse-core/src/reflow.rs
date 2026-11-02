//! Involution-symmetric block reflow and reversible multi-scale alignment
//! algebra.

use std::ops::Range;

use diffuse_syntax::profile::LangProfile;

use crate::{
    cluster::dp::Weights,
    lcs::char_len,
    require,
    tape::{AlignmentTape, TapeChunk},
};

/// An involution-symmetric reflow transition between a block of old lines and a
/// block of new lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockReflow<'a> {
    /// Zero-based index of the anchor line within the old range.
    pub anchor_old: usize,
    /// Zero-based index of the anchor line within the new range.
    pub anchor_new: usize,
    /// Line index range on the old side.
    pub old_range: Range<usize>,
    /// Line index range on the new side.
    pub new_range: Range<usize>,
    /// Unified subword alignment tape across the reflowed block.
    pub tape: AlignmentTape<'a>,
}

impl<'a> BlockReflow<'a> {
    /// Construct a 1-to-K line expansion reflow.
    pub fn new_expansion(
        old_idx: usize,
        new_start: usize,
        k: usize,
        anchor_new_offset: usize,
        tape: AlignmentTape<'a>,
    ) -> Self {
        Self {
            anchor_old: 0,
            anchor_new: anchor_new_offset,
            old_range: old_idx..old_idx + 1,
            new_range: new_start..new_start + k,
            tape,
        }
    }

    /// Construct a K-to-1 line collapse reflow.
    pub fn new_collapse(
        old_start: usize,
        k: usize,
        new_idx: usize,
        anchor_old_offset: usize,
        tape: AlignmentTape<'a>,
    ) -> Self {
        Self {
            anchor_old: anchor_old_offset,
            anchor_new: 0,
            old_range: old_start..old_start + k,
            new_range: new_idx..new_idx + 1,
            tape,
        }
    }

    /// The algebraic reverse reflow: swaps old and new ranges, swaps anchors,
    /// and reverses the alignment tape.
    ///
    /// Satisfies the involution property:
    /// `reflow.reverse().reverse() == reflow`
    pub fn reverse(mut self) -> BlockReflow<'a> {
        std::mem::swap(&mut self.anchor_old, &mut self.anchor_new);
        std::mem::swap(&mut self.old_range, &mut self.new_range);
        self.tape = self.tape.reverse();
        self
    }

    /// Is this reflow a 1-to-K expansion?
    #[inline]
    pub fn is_expansion(&self) -> bool {
        self.old_range.len() == 1 && self.new_range.len() > 1
    }

    /// Is this reflow a K-to-1 collapse?
    #[inline]
    pub fn is_collapse(&self) -> bool {
        self.old_range.len() > 1 && self.new_range.len() == 1
    }

    /// The expansion/collapse degree `K`.
    #[inline]
    pub fn k(&self) -> usize {
        self.old_range.len().max(self.new_range.len())
    }

    /// Compute the symmetric reflow score with linear indentation penalty.
    #[allow(clippy::too_many_arguments)]
    pub fn score(
        indent_old: usize,
        indent_new: usize,
        side_chars: usize,
        block_chars: usize,
        matched: usize,
        k: usize,
        moved_lines: usize,
        w: &Weights,
    ) -> f64 {
        let total = side_chars.max(block_chars);
        let unmatched = total.saturating_sub(matched);
        let indent_diff = (indent_old as isize - indent_new as isize).unsigned_abs() as f64;
        let indent_penalty = w.sprawl_penalty * indent_diff * w.indent_penalty_factor;

        w.matched_char_reward * matched as f64
            - w.unmatched_char_penalty * unmatched as f64
            - w.row_cost * k as f64
            - w.sprawl_penalty * (k - 1) as f64
            - w.sprawl_penalty * moved_lines as f64 * 0.5
            - indent_penalty
    }
}

/// Construct a block reflow expansion model from a single old line and K new
/// continuation lines.
pub fn build_expansion_reflow<'a>(
    old_line: &'a str,
    new_lines: &[&'a str],
    anchor_new: usize,
    profile: Option<&LangProfile>,
) -> BlockReflow<'a> {
    require!(!new_lines.is_empty(), "new_lines must not be empty");
    require!(anchor_new < new_lines.len(), "anchor_new out of bounds");

    let anchor_text = new_lines[anchor_new];
    let mut tape = AlignmentTape::from_texts(old_line, anchor_text, profile);

    // Track subline relocation: if continuation lines contain text from the old
    // line, attach them as TapeChunk::Relocate rather than ad-hoc pure
    // additions.
    for (idx, &line) in new_lines.iter().enumerate() {
        if idx == anchor_new {
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.is_empty()
            && let Some(byte_idx) = old_line.find(trimmed)
        {
            let from_start = char_len(&old_line[..byte_idx]);
            let from_end = from_start + char_len(trimmed);
            tape.push(TapeChunk::Relocate {
                from_span: (from_start, from_end),
                to_span: (0, char_len(trimmed)),
                text: trimmed,
            });
        }
    }

    BlockReflow::new_expansion(0, 0, new_lines.len(), anchor_new, tape)
}

/// Construct a block reflow collapse model from K old continuation lines and a
/// single new line.
pub fn build_collapse_reflow<'a>(
    old_lines: &[&'a str],
    new_line: &'a str,
    anchor_old: usize,
    profile: Option<&LangProfile>,
) -> BlockReflow<'a> {
    require!(!old_lines.is_empty(), "old_lines must not be empty");
    require!(anchor_old < old_lines.len(), "anchor_old out of bounds");

    let anchor_text = old_lines[anchor_old];
    let mut tape = AlignmentTape::from_texts(anchor_text, new_line, profile);

    for (idx, &line) in old_lines.iter().enumerate() {
        if idx == anchor_old {
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.is_empty()
            && let Some(byte_idx) = new_line.find(trimmed)
        {
            let to_start = char_len(&new_line[..byte_idx]);
            let to_end = to_start + char_len(trimmed);
            tape.push(TapeChunk::Relocate {
                from_span: (0, char_len(trimmed)),
                to_span: (to_start, to_end),
                text: trimmed,
            });
        }
    }

    BlockReflow::new_collapse(0, old_lines.len(), 0, anchor_old, tape)
}

#[cfg(test)]
mod tests;
