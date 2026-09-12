# Algebraic Diff Engine & Invertible Alignment Tape

- [x] 1. [Phase 1 — Invertible Alignment Tape & Reversible Run Algebra (`crates/diffuse-core/src/tape/`)](#1-phase-1--invertible-alignment-tape--reversible-run-algebra-cratesdiffuse-coresrctape)
- [x] 2. [Phase 2 — Information Entropy Anchoring & Multi-Token Phrase Landmarks (`crates/diffuse-core/src/token/`)](#2-phase-2--information-entropy-anchoring--multi-token-phrase-landmarks-cratesdiffuse-coresrctoken)
- [x] 3. [Phase 3 — Additive Cost Semiring & Upstream Wholesale Elimination (`crates/diffuse-core/src/cluster/`)](#3-phase-3--additive-cost-semiring--upstream-wholesale-elimination-cratesdiffuse-coresrccluster)
- [x] 4. [Phase 4 — Symmetric Multi-Scale Reflow & Reversible Block Algebra (`crates/diffuse-core/src/reflow/`)](#4-phase-4--symmetric-multi-scale-reflow--reversible-block-algebra-cratesdiffuse-coresrcreflow)
- [·] 5. [Phase 5 — Golden Acceptance, Performance Benchmarking & Legacy Retirement](#5-phase-5--golden-acceptance-performance-benchmarking--legacy-retirement)

---

## Executive Summary

Diffuse currently suffers from an architectural impedance mismatch inherited from its original prototype (`review.ts`):
1. **Lossy Intermediate Representation**: Subword LCS computes the exact token-to-token correspondences between modified lines, but flattens the result into disconnected index tuples: `Spans { del: Vec<(usize, usize)>, add: Vec<(usize, usize)> }`.
2. **Heuristic Reverse Reconstruction**: Downstream modules (`merge.rs`) receive those raw tuples and must execute an elaborate reconstruction pass (`del_display_positions`, `del_pairing`, `op_anchor`) to guess where deletions belong relative to additions. When this heuristic guessing falters, `project_reverse` fails Design-by-Contract assertions (`rev == del_text`).
3. **Reactive Circuit Breakers ("Wholesale")**: Line-level dynamic programming pairs lines optimistically using coarse similarity heuristics. When subword LCS subsequently discovers that paired lines share zero common tokens (or produce mid-word character splices like `m[-them-]ust`), downstream stages (`collapse.rs`, `pipeline.rs`) must destructively mutate row vectors, tearing `Row::Mod` back into `Row::Del` and `Row::Add`.
4. **Trivial Anchor Hijacking**: Single-token patience anchors (`unique_token_anchors`) blindly anchor any token occurring once on each side (`count == 1`). Grammatical stop words (`"the"`, `"a"`) hijack the alignment and forcibly break long, high-information common phrases (e.g. `**`extension/pi/`** —`).

This plan redesigns Diffuse around **Transparent Additivity** and **Reversible Combinatorics**:
- **Invertible Alignment Tape**: Alignments are represented as a typed, monoidal tape of atomic operations (`Keep`, `Strike`, `Fill`, `Relocate`). Projections $\pi_{\text{old}}(\text{Tape}) \equiv \text{old}$ and $\pi_{\text{new}}(\text{Tape}) \equiv \text{new}$ are mathematical identities **by construction**, eliminating heuristic span re-pairing and DbC projection failures permanently.
- **Information Entropy Anchors**: Anchor selection is governed by Shannon self-information $I(t) = -\log_2 P(t)$ and multi-token phrase hashing, guaranteeing that high-entropy phrases anchor first and trivial grammatical glue can never fracture structural landmarks.
- **Additive Cost Semiring**: Cluster DP evaluates substitutions using the exact additive metric of the optimal subword alignment tape. If two lines share zero common entropy, $\text{Cost}(\text{Substitution}) \ge \text{Cost}(\text{Del}) + \text{Cost}(\text{Add})$. The DP *never* chooses a disjoint modification, making the entire downstream "wholesale replacement" deconstruction pipeline obsolete.

---

## Table of Contents

1. [Phase 1 — Invertible Alignment Tape & Reversible Run Algebra (`crates/diffuse-core/src/tape/`)](#1-phase-1--invertible-alignment-tape--reversible-run-algebra-cratesdiffuse-coresrctape)
2. [Phase 2 — Information Entropy Anchoring & Multi-Token Phrase Landmarks (`crates/diffuse-core/src/token/`)](#2-phase-2--information-entropy-anchoring--multi-token-phrase-landmarks-cratesdiffuse-coresrctoken)
3. [Phase 3 — Additive Cost Semiring & Upstream Wholesale Elimination (`crates/diffuse-core/src/cluster/`)](#3-phase-3--additive-cost-semiring--upstream-wholesale-elimination-cratesdiffuse-coresrccluster)
4. [Phase 4 — Symmetric Multi-Scale Reflow & Reversible Block Algebra (`crates/diffuse-core/src/reflow/`)](#4-phase-4--symmetric-multi-scale-reflow--reversible-block-algebra-cratesdiffuse-coresrcreflow)
5. [Phase 5 — Golden Acceptance, Performance Benchmarking & Legacy Retirement](#5-phase-5--golden-acceptance-performance-benchmarking--legacy-retirement)

---

## 1. Phase 1 — Invertible Alignment Tape & Reversible Run Algebra (`crates/diffuse-core/src/tape/`)

### Objective
Replace lossy character spans and heuristic span re-pairing with a strongly typed, bidirectional `AlignmentTape` that preserves token correspondences directly from the combinatorial alignment pass.

### Concrete Implementation Steps
1. **Define the Tape Type System** in `crates/diffuse-core/src/tape.rs`:
   ```rust
   #[derive(Clone, Debug, PartialEq, Eq)]
   pub enum TapeChunk<'a> {
       /// Identical token run surviving unmodified across both sides.
       Keep(&'a str),
       /// Struck deletion from old text, anchored directly at its common context.
       Strike(&'a str),
       /// Filled addition to new text, anchored directly at its common context.
       Fill(&'a str),
       /// Relocated / moved segment (subline reflow or cross-line transfer).
       Relocate {
           from_span: (usize, usize),
           to_span: (usize, usize),
           text: &'a str,
       },
   }

   #[derive(Clone, Debug, Default, PartialEq, Eq)]
   pub struct AlignmentTape<'a> {
       chunks: Vec<TapeChunk<'a>>,
   }
   ```
2. **Implement Provable Algebraic Invariants**:
   ```rust
   impl<'a> AlignmentTape<'a> {
       /// Projects the old text: concatenates Keep + Strike + Relocate(from).
       pub fn project_old(&self) -> String;

       /// Projects the new text: concatenates Keep + Fill + Relocate(to).
       pub fn project_new(&self) -> String;

       /// Monoidal composition: concatenate two contiguous alignment tapes.
       pub fn compose(&mut self, other: AlignmentTape<'a>);

       /// Direct, zero-guess conversion to renderer OutputRuns.
       pub fn to_merged_runs(&self) -> Vec<Run<'a>>;
   }
   ```
3. **Emit Tape Directly from LCS Backtrace**:
   - In `crates/diffuse-core/src/lcs.rs`, modify the DP traceback to construct `AlignmentTape` directly while walking matched pairs.
   - Whitespace minimum optimization (e.g. $8 \to 12$ spaces = 8 keep + 4 fill) is performed locally during chunk construction rather than via retroactive string slicing.
4. **Verification & Testing**:
   - Write property tests (`proptest`) proving $\forall (s_1, s_2): \pi_{\text{old}}(\text{tape}(s_1, s_2)) \equiv s_1 \land \pi_{\text{new}}(\text{tape}(s_1, s_2)) \equiv s_2$.
   - Confirm that DbC assertion `rev == del_text` in `crates/diffuse-core/src/merge.rs` is satisfied tautologically without any heuristic coordinate shifts.

---

## 2. Phase 2 — Information Entropy Anchoring & Multi-Token Phrase Landmarks (`crates/diffuse-core/src/token/`)

### Objective
Eliminate trivial single-token anchor misalignments by ranking anchor candidates via information entropy and multi-token phrase hashing.

### Concrete Implementation Steps
1. **Semantic Token Model with Structural Roles** (`crates/diffuse-core/src/token.rs`):
   ```rust
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   pub enum TokenClass {
       Whitespace,
       Delimiter,
       Operator,
       Keyword,
       Identifier,
       Literal,
       ProseWord,
   }

   #[derive(Clone, Copy, Debug)]
   pub struct AnalyzedToken<'a> {
       pub text: &'a str,
       pub class: TokenClass,
       pub byte_offset: usize,
       pub char_offset: usize,
   }
   ```
2. **Multi-Token Phrase Landmark Detection**:
   - Before single-token patience indexing, run a rolling Karp-Rabin or prefix-tree scan to identify maximal shared token sequences ($k \ge 2$) that appear uniquely in the cluster (such as `**`extension/pi/`** —`).
   - Lock multi-token phrases as rigid anchor blocks.
3. **Information-Theoretic Anchor Filtering**:
   - Compute self-information score for single-token candidates:
     $$I(t) = \text{class\_weight}(t.\text{class}) \times \ln(1 + \text{char\_len}(t))$$
   - Enforce an entropy threshold $I(t) \ge I_{\text{min}}$ before any token can enter the Longest Increasing Subsequence (LIS) anchor candidate set.
   - Grammatical glue (`"the"`, `"a"`, `"and"`) and single-character identifiers naturally score below $I_{\text{min}}$ and cannot fracture structural phrases.
4. **Verification**:
   - Targeted unit tests in `crates/diffuse-core/src/lcs/tests.rs` verifying that phrases with repeating internal tokens (like punctuation wraps `**` ... `**`) anchor as unified blocks without requiring hardcoded stop-word exclusions.

---

## 3. Phase 3 — Additive Cost Semiring & Upstream Wholesale Elimination (`crates/diffuse-core/src/cluster/`)

### Objective
Harmonize the cluster dynamic programming score with the subword alignment tape cost so that disjoint line modifications are mathematically disqualified upstream, eliminating the downstream wholesale deconstruction hack.

### Concrete Implementation Steps
1. **The Additive Cost Lattice**:
   - In `crates/diffuse-core/src/cluster/dp.rs`, define substitution cost as the additive monoidal cost of the subword alignment tape:
     $$\text{Cost}(\text{Mod}(u, v)) = \text{TapeCost}(\text{align}(u, v))$$
   - An affine gap penalty ensures that an in-place substitution is chosen if and only if:
     $$\text{Cost}(\text{Mod}(u, v)) < \text{Cost}(\text{Del}(u)) + \text{Cost}(\text{Add}(v)) - \Delta_{\text{mod\_incentive}}$$
2. **Fast Subword Upper-Bounding Filter**:
   - To maintain sub-millisecond execution times without running full subword LCS on all $M \times N$ line pairs, compute a fast token-bag $q$-gram intersection upper bound.
   - If the upper bound indicates common characters $< \theta_{\text{min\_common}}$, bypass LCS and assign $\infty$ substitution cost.
3. **Retire Wholesale Row Deconstruction**:
   - With the additive cost semiring in place, remove the retroactive un-pairing loop in `crates/diffuse-core/src/collapse.rs:66` and the hunk decomposition in `crates/diffuse-core/src/pipeline.rs:351`.
   - Promote `!is_wholesale_replacement` from a recovered condition to an invariant established directly by cluster DP.
4. **Verification**:
   - Run the full golden suite with assertions confirming that zero `Row::Mod` rows ever evaluate to wholesale replacement at the exit of DP pairing.

---

## 4. Phase 4 — Symmetric Multi-Scale Reflow & Reversible Block Algebra (`crates/diffuse-core/src/reflow/`)

### Objective
Unify 1-to-K line expansions and K-to-1 line collapses into an involution-symmetric, reversible block reflow model.

### Concrete Implementation Steps
1. **Algebraic Reflow Transitions**:
   - Define reflow as an affine transformation on an alignment tape spanning multiple rows:
     ```rust
     pub struct BlockReflow<'a> {
         pub anchor_old: usize,
         pub anchor_new: usize,
         pub old_range: Range<usize>,
         pub new_range: Range<usize>,
         pub tape: AlignmentTape<'a>,
     }
     ```
2. **Symmetric Indentation Penalty**:
   - Calculate reflow cost using absolute indentation deviation $| \text{indent}_{\text{old}} - \text{indent}_{\text{new}} |$ penalized linearly by `w.gamma * w.indent_penalty_factor`.
   - Guarantees that running diff on $(A, B)$ produces the exact algebraic inverse of diff on $(B, A)$ under hunk inversion.
3. **Subline Relocation Tracking**:
   - When wrapped function calls or chained method calls reflow across line breaks, record character-level `TapeChunk::Relocate` entries.
   - Feed these directly to Phase 4 `RunKind::Mov` syntax rendering without requiring ad-hoc line-matching heuristics.
4. **Verification**:
   - Prove inverse involution: `apply(diff(B, A), B) == A` across multi-line reflow test cases in `crates/diffuse-core/src/shift/tests.rs`.

---

## 5. Phase 5 — Golden Acceptance, Performance Benchmarking & Legacy Retirement

### Objective
Verify the new engine against the 8 golden corpora, enforce strict latency budgets ($< 0.5\text{s}$ per test, $< 3\text{s}$ full `./scripts/check.sh`), and retire obsolete legacy heuristics.

### Concrete Implementation Steps
1. **Acceptance Baseline Verification**:
   - Execute `tests/golden.rs` across all corpora:
     - `diffuse-2026-08`
     - `theme-scrollbar-palette`
     - `opt-1-cache-regex-window-slice`
     - `opt-2-cache-markdown-regex`
     - `opt-3-cache-lang-profile`
     - `opt-4-manual-scanner-prefilter`
     - `diffuse-history-mid`
     - `untracked-files`
   - Walk golden diffs per `tests/AGENTS.md` and confirm strictly better or equivalent visual quality.
2. **Retire Deprecated Modules**:
   - Remove `del_display_positions`, `del_pairing`, and `wholesale_pair` in `crates/diffuse-core/src/merge.rs`.
   - Remove post-hoc wholesale deconstruction in `crates/diffuse-core/src/pipeline.rs`.
3. **Performance Gate & Benchmarks**:
   - Confirm warm `./scripts/check.sh` completes in $< 3\text{s}$.
   - Verify that thread-local scratch arena re-use maintains zero-allocation throughput during multi-threaded Rayon execution.
4. **Human Review & Signoff**:
   - Present final wire/render diffs to the human reviewer for visual inspection and architectural signoff.
