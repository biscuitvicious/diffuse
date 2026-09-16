# Banded Block-DP — Adaptive Wavefront Skyline Matrix for Large-Scale Diff Alignment

- [x] Phase 1 — Generic Skyline Matrix & Adaptive Wavefront Envelope Builder
- [x] Phase 2 — Transparent Block-DP Integration with Skyline Grid
- [x] Phase 3 — Adaptive Parameter Formulation & Configuration Options
- [x] Phase 4 — Test-First Validation & Large-Scale Alignment Test Suite
- [x] Phase 5 — Verification Gate, Performance Budgets & Golden Invariants

---

## Executive Summary

When diffing files with extensive modifications (such as full-file replacements, mass reformats, or large hunks where every line is touched), diffuse's Block-DP currently encounters a hard cliff: if the quadratic product of deleted lines ($M$) and added lines ($N$) exceeds `cluster_product_cap` (100,000), it aborts DP exploration and falls back to wholesale deletion followed by wholesale addition (`Row::Del...`, `Row::Add...`). This results in zero modification rows (`~0`) and leaves users with no line correspondence or subline token highlighting on large changesets.

This plan resolves the limitation by combining **Wavefront Alignment (WFA)** and **Adaptive Banded DP (Chao et al. 1992)** over a dedicated, independent **Skyline Matrix (Jennings Profile Storage)** data structure. Instead of allocating an $O(M \times N)$ rectangular table, the engine allocates an adaptive variable-band skyline grid where each row stores only its active corridor $[j_{\min}(i) \dots j_{\max}(i)]$ contiguously. The corridor dynamically stretches where code diverges and tightens along clean diagonal matches. This slashes space and time complexity to $O(K \cdot \min(M, N))$, preserving 100% of the Block-DP's semantic intelligence (syntax head agreement, delimiter alignment, token LCS, and 1:N expand / N:1 collapse) while allowing massive hunks (such as 7,000+ line whole-file touch-ups) to align in milliseconds with full subline highlighting.

### Core Mathematical & Architectural Invariants
1. **Separation of Concerns**: The matrix topology (`SkylineMatrix<T>`) and corridor calculation (`AdaptiveEnvelopeBuilder`) reside in `crates/diffuse-core/src/algo/skyline.rs` as independent, fully generic data structures with zero diff-specific coupling.
2. **Trajectory Invariant**: The corridor is centered on the expected line trajectory $j_{\text{expected}}(i) = \lfloor i \cdot N / M \rfloor$, augmented by wavefront diagonal guidance. This guarantees that cell $(0, 0)$ is always the corridor start, and cell $(M, N)$ is always the corridor end, regardless of sequence asymmetry ($M \ne N$).
3. **Semantics & Feature Parity**: When $K \ge \max(M, N)$, the skyline grid covers the entire matrix, yielding identical transitions and scores to unbanded Block-DP.
4. **Budget Safety Ceiling**: A configurable safety budget (default: $10,000,000$ cells) limits maximum memory usage. If a cluster's cells exceed the budget even under minimal bandwidth ($K < 6$), the engine safely degrades to wholesale Del/Add.
5. **Performance Budgets**: A $7,000 \times 7,000$ hunk must execute in $< 100\,\text{ms}$, and warm verification (`scripts/check.sh`) must remain $< 15\,\text{s}$.

---

## Table of Contents

1. [Phase 1 — Generic Skyline Matrix & Adaptive Wavefront Envelope Builder](#1-phase-1--generic-skyline-matrix--adaptive-wavefront-envelope-builder)
2. [Phase 2 — Transparent Block-DP Integration with Skyline Grid](#2-phase-2--transparent-block-dp-integration-with-skyline-grid)
3. [Phase 3 — Adaptive Parameter Formulation & Configuration Options](#3-phase-3--adaptive-parameter-formulation--configuration-options)
4. [Phase 4 — Test-First Validation & Large-Scale Alignment Test Suite](#4-phase-4--test-first-validation--large-scale-alignment-test-suite)
5. [Phase 5 — Verification Gate, Performance Budgets & Golden Invariants](#5-phase-5--verification-gate-performance-budgets--golden-invariants)

---

## 1. Phase 1 — Generic Skyline Matrix & Adaptive Wavefront Envelope Builder

**Goal:** Author an independent, generic `SkylineMatrix<T>` and an `AdaptiveEnvelopeBuilder` in `crates/diffuse-core/src/algo/skyline.rs`, backed by comprehensive unit tests.

### Key Deliverables & Affected Files
- `crates/diffuse-core/src/algo/skyline.rs`:
  - `SkylineMatrix<T>`:
    - Stores contiguous cells in a single `Vec<T>`.
    - Per-row bounds `SkylineRow { start: usize, end: usize, offset: usize }`.
    - `get(i, j) -> Option<&T>` and `get_mut(i, j) -> Option<&mut T>` with $O(1)$ arithmetic translation.
    - `row_range(i) -> RangeInclusive<usize>` for active column iterations.
    - Reusable allocation: `init_envelope(&mut self, m, n, bounds, default_val)` to avoid reallocating `Vec<T>` across diff clusters.
  - `AdaptiveEnvelope`:
    - `bounds: Vec<(usize, usize)>` representing $(j_{\min}, j_{\max})$ for each row $i \in 0..=M$.
    - `total_cells: usize`.
  - `AdaptiveEnvelopeBuilder`:
    - Computes corridor bounds using $K = \max(K_{\min}, |M - N| + K_{\text{slack}})$.
    - Adapts trajectory based on slope $j = i \cdot N / M$.
    - Clamps corridor width when `total_cells > budget`.
- `crates/diffuse-core/src/algo/skyline/tests.rs`:
  - Unit tests verifying:
    - Rectangular full-matrix equivalence.
    - Diagonal band coordinate mapping and out-of-bounds `None` returns.
    - Dynamic stretching and asymmetric row spans.
    - Zero-allocation reuse across different envelope shapes.
- `crates/diffuse-core/src/algo.rs`:
  - Register `pub mod skyline;`.

### Verification Gate
- `cargo test -p diffuse-core --lib algo::skyline` achieves 100% branch coverage with sub-millisecond execution.

---

## 2. Phase 2 — Transparent Block-DP Integration with Skyline Grid

**Goal:** Upgrade `solve_cluster_with_arena` in `crates/diffuse-core/src/cluster/dp.rs` to use `SkylineMatrix<Cell>` instead of the flat `Vec<Cell>`.

### Key Deliverables & Affected Files
- `crates/diffuse-core/src/cluster/dp.rs`:
  - Embed `SkylineMatrix<Cell>` inside `DpScratchArena` so its underlying storage is reused across clusters with zero new heap allocations.
  - Replace raw index calculations with `grid.row_range(i)` and `grid.get_mut(dest_i, dest_j)`:
    - Forward step:
      ```rust
      for i in 0..=m {
          for j in grid.row_range(i) {
              // Only evaluates cells within the active corridor
          }
      }
      ```
    - Transition writes (Del, Add, Mod, Mov, Expand, Collapse) write via `grid.get_mut(dest_i, dest_j)`. Destinations landing outside the skyline corridor return `None` and are naturally pruned.
    - Traceback starts at cell $(M, N)$ (guaranteed to be within the corridor) and follows `prev_i, prev_j` via `grid.get(i, j)` back to $(0, 0)$.

### Verification Gate
- Path continuity invariant: traceback strictly reconstructs continuous transitions from $(M, N)$ to $(0, 0)$ without index out-of-bounds or broken links.
- Existing cluster DP tests pass.

---

## 3. Phase 3 — Adaptive Parameter Formulation & Configuration Options

**Goal:** Expose tunable parameters for `cluster_budget`, `K_min`, and `K_slack` across options, thread-locals, and CLI flags.

### Key Deliverables & Affected Files
- `crates/diffuse-core/src/cluster/dp.rs`:
  - `pub const DEFAULT_CLUSTER_BUDGET: usize = 10_000_000;`
  - `pub const DEFAULT_CLUSTER_BANDWIDTH_MIN: usize = 64;`
  - `pub const DEFAULT_CLUSTER_BANDWIDTH_SLACK: usize = 32;`
  - Thread-local accessors and setters:
    - `set_cluster_budget(budget: usize)` / `cluster_budget() -> usize`
    - `set_cluster_bandwidth_min(min: usize)` / `cluster_bandwidth_min() -> usize`
    - `set_cluster_bandwidth_slack(slack: usize)` / `cluster_bandwidth_slack() -> usize`
- `crates/diffuse-core/src/pipeline.rs`:
  - Expose `cluster_budget`, `cluster_bandwidth_min`, `cluster_bandwidth_slack` in `PipelineOptions`.
  - Propagate options to worker threads.
- `crates/diffuse/src/main.rs`:
  - Add CLI arguments `--cluster-budget`, `--cluster-bandwidth-min`, `--cluster-bandwidth-slack`.

### Verification Gate
- Unit tests verifying parameter propagation and dynamic band adaptation under varying budgets.

---

## 4. Phase 4 — Test-First Validation & Large-Scale Alignment Test Suite

**Goal:** Author and pass targeted tests replicating massive whole-file replacements, asymmetric bursts, and safety degradation.

### Key Deliverables & Affected Files
- `crates/diffuse-core/src/cluster/tests.rs`:
  - `test_skyline_dp_7000_line_replacement_pairs_into_mods`:
    Replicates the user's $7,183$-line test case. Verifies that all 7,183 lines pair into `Row::Mod` rows in $< 100\,\text{ms}$ with positive score.
  - `test_skyline_dp_asymmetric_burst_preserves_alignment`:
    Tests a 50-line insertion burst within a 200-line cluster, asserting that the adaptive corridor tracks the shift and realigns downstream code.
  - `test_skyline_dp_budget_exceeded_falls_back_safely`:
    Sets `cluster_budget` to a tiny value and verifies safe, graceful fallback to wholesale Del/Add without panic or buffer overrun.
- `crates/diffuse-core/src/pipeline/tests.rs`:
  - End-to-end integration test verifying that `run_pipeline_full` on a large replacement produces populated `word_spans` on all modified rows.

### Verification Gate
- All tests pass in $< 0.5\,\text{s}$ per unit test.

---

## 5. Phase 5 — Verification Gate, Performance Budgets & Golden Invariants

**Goal:** Execute full verification via `scripts/check.sh`, assert 100% byte-identity across all existing golden corpora, and verify benchmarks.

### Key Deliverables & Affected Files
- Verify `scripts/check.sh` passes cleanly (fmt, clippy, unit tests, integration tests, doctests).
- Verify all 8 golden corpora in `tests/golden/` match byte-for-byte.
- Measure execution time on the $7,183$-line scenario to confirm it runs well under the 500ms budget.
- Document the Skyline Matrix and Adaptive Wavefront Corridor in `docs/Algorithms.md`.

### Verification Gate
- `scripts/check.sh` passes with 0 warnings.
- Human review and signoff of the complete implementation.
