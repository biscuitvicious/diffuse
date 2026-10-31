//! Configuration parameters and penalty/bonus weights for the DP cluster
//! solver.

/// Maximum block size K for 1-to-K expansions and K-to-1 collapses.
pub const MAX_BLOCK_K: usize = 10;

/// Admission floors on the *salient* similarity ratio (matched salient chars
/// over the larger side's salient chars). Salient content is
/// `alphanumeric && !keyword`: whitespace, punctuation, and keywords never
/// count, so ratios read higher than the legacy all-char scale and the code
/// floor is correspondingly steeper. A code pair must clear a third of its
/// salient content (`MIN_RATIO_CODE`); a comment pair still carries prose
/// interjections on top of its anchor words, so its same-slot floor is the
/// legacy 0.35 while a reordered (offset-drift) one must clear 0.65.
pub const MIN_RATIO_CODE: f64 = 0.35;
pub const MIN_RATIO_COMMENT: f64 = 0.35;
pub const MIN_RATIO_COMMENT_OFFSET: f64 = 0.65;

/// Floor for the char scale — the exact inverse of the salient regime: a
/// pair where either line carries no salient tokens has no salient ratio,
/// and is scored on non-whitespace chars instead. Calibrated at 0.20 by a
/// full-corpus sweep of the frozen corpora:
/// - 0.30 is inert — no accepted salient-free pair sits in (0.25, 0.30].
/// - 0.20 admits exactly one reviewed improvement (`diffuse-history-mid`:
///   `return &s[start_byte..start_byte];` ↔ `return "";` at 7/33) with zero
///   degenerations.
/// - 0.15 admits five further row-groups, all bare-brace/paren-shape absorption
///   with at best neutral quality; 0.10 goes on to absorb structural tails into
///   struck fill (`.round() as usize;` paired with a call terminator) and
///   incomplete `import {` heads with full statements — degenerate. 0.20 is the
///   floor: guard test pins 1/6 as rejection.
pub const MIN_RATIO_CHAR_SCALE: f64 = 0.20;

pub const DEFAULT_MIN_CONTAINMENT_LEN: usize = 8;

/// Default maximum cluster cell budget allowed for DP pairing before falling
/// back to wholesale deletion/addition.
pub const DEFAULT_CLUSTER_BUDGET: usize = 10_000_000;
pub const DEFAULT_CLUSTER_PRODUCT_CAP: usize = DEFAULT_CLUSTER_BUDGET;

/// Default minimum corridor half-bandwidth (K_min) for Block-DP exploration.
pub const DEFAULT_CLUSTER_BANDWIDTH_MIN: usize = 64;

/// Default bandwidth slack added to sequence length difference |M - N|.
pub const DEFAULT_CLUSTER_BANDWIDTH_SLACK: usize = 32;

thread_local! {
    static CLUSTER_BUDGET: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_CLUSTER_BUDGET) };
    static CLUSTER_BANDWIDTH_MIN: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_CLUSTER_BANDWIDTH_MIN) };
    static CLUSTER_BANDWIDTH_SLACK: std::cell::Cell<usize> = const { std::cell::Cell::new(DEFAULT_CLUSTER_BANDWIDTH_SLACK) };
}

/// Set the maximum cluster cell budget allowed for DP pairing before falling
/// back.
pub fn set_cluster_budget(budget: usize) {
    CLUSTER_BUDGET.with(|c| c.set(budget));
}

/// Get the current cluster cell budget.
pub fn cluster_budget() -> usize {
    CLUSTER_BUDGET.with(|c| c.get())
}

/// Backwards-compatible alias for set_cluster_budget.
pub fn set_cluster_product_cap(cap: usize) {
    set_cluster_budget(cap);
}

/// Backwards-compatible alias for cluster_budget.
pub fn cluster_product_cap() -> usize {
    cluster_budget()
}

/// Set the minimum corridor half-bandwidth (K_min).
pub fn set_cluster_bandwidth_min(min: usize) {
    CLUSTER_BANDWIDTH_MIN.with(|c| c.set(min));
}

/// Get the minimum corridor half-bandwidth (K_min).
pub fn cluster_bandwidth_min() -> usize {
    CLUSTER_BANDWIDTH_MIN.with(|c| c.get())
}

/// Set the bandwidth slack added to |M - N|.
pub fn set_cluster_bandwidth_slack(slack: usize) {
    CLUSTER_BANDWIDTH_SLACK.with(|c| c.set(slack));
}

/// Get the bandwidth slack added to |M - N|.
pub fn cluster_bandwidth_slack() -> usize {
    CLUSTER_BANDWIDTH_SLACK.with(|c| c.get())
}

/// The multi-objective weights (plan §3.3): the engine's model of what
/// reads well. The DP solver is optimal *relative to these weights*, so
/// every pairing judgment below is a statement about them, not about the
/// search. Read all sizes in matched-char units (÷ `matched_char_reward`):
/// a bonus worth N chars outvotes any content margin smaller than N.
/// Golden review gates any change.
#[derive(Clone, Debug)]
pub struct Weights {
    /// Reward per token-matched content character: the evidence currency
    /// everything else is priced in (15.0). A 20-char shared run is worth
    /// ~300 points. Historical initial tuning, unchanged since.
    pub matched_char_reward: f64,
    /// Penalty per unmatched content character (8.0 ≈ 0.53 matched chars):
    /// what a pairing fails to account for. This is what makes a garbage
    /// `Mod` lose to a standalone `Del` + `Add` — a 100-char mismatch costs
    /// ~800, i.e. exactly one old-δ. Historical initial tuning.
    pub unmatched_char_penalty: f64,
    /// Visual cost of one emitted row (5.0 ≈ ⅓ char): rows are nearly free,
    /// so the engine is recall-heavy — it pairs whenever anything supports
    /// the pairing, and almost never prefers `Del` + `Add` over a `Mod` on
    /// row economy alone. Raise only if the output gets over-paired.
    pub row_cost: f64,
    /// Fragmentation penalty per extra sprawl line of a block transition
    /// (2.0), also scaling the indent-shift penalty. Small by design: block
    /// shape is cheap, content carries the decision.
    pub sprawl_penalty: f64,
    /// Slot-continuity bonus (250.0 ≈ 17 matched chars): pairing two lines
    /// that occupy the same structural slot (same tails/indent next to a
    /// block boundary) even when their bodies differ.
    pub slot_continuity_bonus: f64,
    /// Boundary-proximity bonus (500.0 ≈ 33 matched chars per side): lines
    /// adjacent to a block open get `open` credit, lines adjacent to a
    /// block close/dedent get `close` credit, each up to 500, stacking with
    /// each other and with the slot bonus.
    pub boundary_proximity_bonus: f64,
    /// Penalty for pairing a del whose text survives elsewhere with
    /// multiplicity intact (a twin copy remains, so the del instance is
    /// redundant rather than transformed).
    pub survives_elsewhere_penalty: f64,
    /// Bonus for statement head keyword agreement when similarity is
    /// sufficient.
    pub head_agree_bonus: f64,
    /// Bonus for statement head agreement at equal indent (160.0 = 2x the
    /// plain head bonus): a reflowed statement head belongs with its head
    /// line — the block's opener at the same level — rather than with a
    /// higher-overlap continuation line deeper in the block.
    pub same_indent_head_bonus: f64,
    /// Bonus for matching compound block closing delimiters (e.g. `};` or `}`).
    pub close_agree_bonus: f64,
    /// Multiplier on line indentation differences.
    pub indent_penalty_factor: f64,
    /// Minimum trimmed character length for prefix-containment claims
    /// (8 chars): one line containing the other at a token boundary.
    pub min_containment_len: usize,
    /// Maximum indentation shift permitted for prefix containment and block
    /// reflow (16 columns ≈ four indent levels).
    pub max_indent_shift: usize,
    /// Minimum content-match ratio across a 1-to-K expansion or K-to-1
    /// collapse (0.50): at least half the block's lines must share content
    /// tokens.
    pub min_block_line_coverage: f64,
    /// Credit given to a matched salient token whose every occurrence is
    /// buried inside a longer identifier on BOTH lines (0.6).
    pub embedded_salient_factor: f64,
    /// Multiplier on matched salient tokens occurring in a contiguous (trimmed)
    /// prefix or suffix of the token sequence.
    pub boundary_salient_multiplier: f64,
    /// Score bonus for pairs sharing a contiguous (trimmed) prefix or suffix
    /// containing at least `min_containment_len` salient characters.
    pub contiguous_boundary_bonus: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            matched_char_reward: 15.0,
            unmatched_char_penalty: 8.0,
            row_cost: 5.0,
            sprawl_penalty: 2.0,
            slot_continuity_bonus: 250.0,
            boundary_proximity_bonus: 500.0,
            survives_elsewhere_penalty: 150.0,
            head_agree_bonus: 80.0,
            same_indent_head_bonus: 160.0,
            close_agree_bonus: 250.0,
            indent_penalty_factor: 5.0,
            min_containment_len: DEFAULT_MIN_CONTAINMENT_LEN,
            max_indent_shift: 16,
            min_block_line_coverage: 0.50,
            embedded_salient_factor: 0.6,
            boundary_salient_multiplier: 1.5,
            contiguous_boundary_bonus: 0.0,
        }
    }
}
