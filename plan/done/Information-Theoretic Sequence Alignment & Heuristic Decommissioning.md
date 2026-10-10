# Information-Theoretic Sequence Alignment & Heuristic Decommissioning

- [x] **Phase 1: Dyck Stratum Depth Tracking & Anchor Disambiguation**
- [x] **Phase 2: Information-Theoretic Gotoh Dynamic Programming (Entropy-Weighted Scoring)**
- [x] **Phase 3: Decommissioning of Path-Matching Heuristics & Ad-Hoc Rules**
- [x] **Phase 4: Golden Corpus Verification & Full Pipeline Sign-Off**

---

## Executive Summary

*diffuse* reconstructs granular line-level and sub-line modifications across unified diffs. Historically, the engine relied on ad-hoc heuristics—specifically `path_root_and_stem` and special-cased boundary delimiters—to prevent "chimeric fragmentation" (e.g. aligning isolated slashes across unrelated directory structures). While effective as an initial defense, these domain-specific heuristics are inherently brittle: they fail on Unicode alphanumerics, roman numerals with punctuation, compact mathematical division ($1/2$), prose slashes (`and/or`), regex patterns, and URLs.

This engineering plan replaces all ad-hoc path-matching and separator heuristics with a principled foundation grounded in **Formal Language Theory (Dyck Languages)** and **Algorithmic Information Theory (Shannon Self-Information and Minimum Description Length)**:

1. **Dyck Stratum Invariance (Phase 1)**: Computes nesting depths via a pushdown automaton to preserve sequence geometry, preventing primary root-level entities from erroneously aligning into subordinate parenthetical or bracketed strata.
2. **Entropy-Weighted Gotoh DP (Phase 2)**: Replaces uniform token match rewards with self-information scoring ($I(t) = -\log_2 P(t)$). High-entropy tokens drive alignment, while low-entropy punctuation cannot overcome affine gap costs unless supported by high-entropy shared context.
3. **Decommissioning Heuristics (Phase 3)**: Permanently removes `path_root_and_stem`, `is_path_boundary_delim`, and delimiter-specific path inspection without backwards compatibility.
4. **Verification & Golden Sign-Off (Phase 4)**: Verifies that all 23 golden corpora and unit test suites match or improve baselines without regression.

---

## Table of Contents

1. [Phase 1 — Dyck Stratum Depth Tracking & Anchor Disambiguation](#1-phase-1--dyck-stratum-depth-tracking--anchor-disambiguation)
2. [Phase 2 — Information-Theoretic Gotoh Dynamic Programming (Entropy-Weighted Scoring)](#2-phase-2--information-theoretic-gotoh-dynamic-programming-entropy-weighted-scoring)
3. [Phase 3 — Decommissioning of Path-Matching Heuristics & Ad-Hoc Rules](#3-phase-3--decommissioning-of-path-matching-heuristics--ad-hoc-rules)
4. [Phase 4 — Golden Corpus Verification & Full Pipeline Sign-Off](#4-phase-4--golden-corpus-verification--full-pipeline-sign-off)

---

## 1. Phase 1 — Dyck Stratum Depth Tracking & Anchor Disambiguation

### Objective
Track hierarchical nesting depth over syntactic containers and use stratum depth matching to resolve landmark anchor ambiguities on the data plane without domain-specific string inspection.

### Architecture & Mechanics
- **Pushdown Automaton**: Implement `compute_parenthesis_depth` in [`crates/diffuse-core/src/token.rs`](../crates/diffuse-core/src/token.rs), computing the container-aware nesting depth for every token in a line.
- **Stratum Landmark Condition**: In `multi_token_phrase_anchors`, require that a landmark phrase be globally unique in source line $A$ (`total_a == 1`), and either globally unique in line $B$ (`total_b == 1`) or unique at matching nesting depth (`depth_b == 1 && b_depth == a_depth`).
- **Isolation of Subordinate Clauses**: When a line adds a parenthetical alternative (e.g. `(or ~/.local/state/diffuse/notes.json)`), the landmark anchor `notes.json` at depth 0 aligns strictly to the primary occurrence, keeping the replacement local.

### Verification
- Targeted unit tests in `crates/diffuse-core/src/token/tests.rs` verifying depth calculation across containers.
- End-to-end integration test `test_path_change_local_to_parenthetical_addition` in `crates/diffuse-core/src/lcs/tests.rs` (marked with `#[ignore]` until Phase 2 boundary trimming lands).

---

## 2. Phase 2 — Information-Theoretic Gotoh Dynamic Programming (Entropy-Weighted Scoring)

### Objective
Scale Gotoh dynamic programming match rewards by token self-information $I(t)$, eliminating the "low-entropy separator gravity trap" where isolated punctuation pulls alignments across disjoint terms.

### Architecture & Mechanics
- **Information-Weighted Reward**: In `raw_lcs_pairs_with_arena` ([`crates/diffuse-core/src/lcs.rs`](../crates/diffuse-core/src/lcs.rs)), replace static `CONTIG_BONUS` with an entropy-proportional reward function:
  $$\text{MatchReward}(t) = \text{base\_bonus} + \alpha \cdot \min(I(t), I_{\max})$$
- **Separator Gap Invariant**: Ensure that for any isolated delimiter ($I(t) < 2\text{ bits}$), $\text{MatchReward}(t) < \text{GAP\_OPEN} + \text{GAP\_EXTEND}$, mathematically preventing isolated punctuation matches across disjoint semantic units.
- **Boundary Trimming in Phrase Anchors**: With entropy-scaled DP in place, trim leading/trailing dangling operators and separators in `multi_token_phrase_anchors` without causing chimeras.

### Verification
- Unit tests verifying that disjoint compound expressions (e.g. `alpha/beta/gamma` vs `one/two/three`) never align internal slashes.
- Activation of `test_path_change_local_to_parenthetical_addition`.

---

## 3. Phase 3 — Decommissioning of Path-Matching Heuristics & Ad-Hoc Rules

### Objective
Completely remove all path-specific functions and heuristics from the diff engine without backwards compatibility.

### Architecture & Mechanics
- Remove `path_root_and_stem` and `is_path_boundary_delim` from [`crates/diffuse-core/src/lcs.rs`](../crates/diffuse-core/src/lcs.rs).
- Remove path-specific gating branches in `tokens_can_match`.
- Remove any heuristic slash checks in `token.rs` and `tape.rs`.
- Replace `test_paths_with_different_roots_and_basenames_not_fragmented` with an information-theoretic compound preservation test.

### Verification
- Compile-time verification that zero path-inspection symbols remain in `diffuse-core`.
- 100% branch coverage across the streamlined `tokens_can_match` predicate.

---

## 4. Phase 4 — Golden Corpus Verification & Full Pipeline Sign-Off

### Objective
Ensure that the entire 23-corpus golden test suite either matches byte-for-byte or produces verified improvements under human review.

### Verification
- Execute `cargo test -p diffuse-core`.
- Execute `cargo test --test golden`.
- Execute `./scripts/check.sh` (nightly fmt, clippy with zero warnings, test suite < 3s warm).
- Visual inspection of any golden diffs per `tests/AGENTS.md`.
