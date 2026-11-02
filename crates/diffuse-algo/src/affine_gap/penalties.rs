/// Scoring parameters for Gotoh's affine gap penalty model.
///
/// Under affine gap costs, opening a gap of length $k$ incurs:
/// $$\text{Cost}(k) = \text{gap\_open} + k \cdot \text{gap\_extend}$$
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AffineGapPenalties {
    /// Reward for matching identical elements ($s > 0$).
    pub match_score: i32,
    /// Optional penalty subtracted for mismatched aligned elements ($p_{mis}
    /// \ge 0$), or `None` to disallow substitutions (standard diff engine
    /// behavior where only identical items are Kept).
    pub mismatch_penalty: Option<i32>,
    /// High initial cost to open a gap ($g_{open} > 0$).
    pub gap_open_penalty: i32,
    /// Incremental cost to extend an existing gap ($g_{extend} > 0$).
    pub gap_extend_penalty: i32,
}

impl Default for AffineGapPenalties {
    /// Default penalties tuned to suppress micro-token fragmentation:
    /// High gap opening penalty (5) prevents isolated coincidental punctuation
    /// matches from splitting cohesive replacement spans, and substitutions
    /// are disabled (`None`) for diff LCS correctness.
    fn default() -> Self {
        Self {
            match_score: 2,
            mismatch_penalty: None,
            gap_open_penalty: 5,
            gap_extend_penalty: 1,
        }
    }
}

impl AffineGapPenalties {
    pub const fn new(
        match_score: i32,
        mismatch_penalty: Option<i32>,
        gap_open_penalty: i32,
        gap_extend_penalty: i32,
    ) -> Self {
        Self {
            match_score,
            mismatch_penalty,
            gap_open_penalty,
            gap_extend_penalty,
        }
    }
}
