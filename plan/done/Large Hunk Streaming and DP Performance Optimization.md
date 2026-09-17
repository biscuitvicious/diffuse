# Large Hunk Streaming and DP Performance Optimization

- [x] Phase 1: Deduplicate and Parallelize Post-Pairing Syntax Runs
  - [x] Write failing regression benchmark test capturing post-pairing row run latency
  - [x] Implement row-level parallelization for large hunks in `process_single_diff_file`
  - [x] Eliminate redundant `colored_runs` evaluation between `hunks_out` and `rows`
  - [x] Verify 100% golden fidelity and test execution speed
- [x] Phase 2: Precompute Row Invariants for DP Candidate Pruning
  - [x] Write targeted unit and micro-benchmark tests for candidate evaluation in `mod_pair_needs_scoring`
  - [x] Add precomputed trimmed slices and leading whitespace metrics to `DpRowMeta`
  - [x] Short-circuit eager prefix-containment calculations in `mod_pair_needs_scoring` and fill loop
  - [x] Verify zero golden divergence and benchmark throughput (full commit dropped to ~0.92s, 10x faster than baseline)
- [x] Phase 3: Normalized and Semantic MUM Anchor Decomposition
  - [x] Write failing unit test for large cluster decomposition with systematic ANSI/prefix changes
  - [x] Implement normalized anchor extraction in `solve_cluster_with_anchors`
  - [x] Validate sub-cluster partitioning on large whole-file diffs
  - [x] Verify golden fidelity and full check gate (`scripts/check.sh`)

---

## Executive Summary

When diffing changesets containing massive hunks with systematic per-line prefix modifications (such as whole-file golden regenerations or global formatting updates, exemplified by `tests/golden/diffuse-2026-08/render.txt` in commit `7e0caab`), the Diffuse TUI and CLI pipeline currently encounter a multi-second streaming delay (~10s in debug/dev builds).

Detailed profiling isolates this latency into three primary bottlenecks:
1. **Redundant & Single-Threaded Syntax Runs (~5.5s)**: In `crates/diffuse-core/src/pipeline.rs`, `process_single_diff_file` executes `colored_runs` sequentially on a single thread when a file contains a single large hunk (because Rayon's `par_iter` only parallelizes over hunks). Furthermore, `build_visible` redundantly invokes `colored_runs` a second time for every row in the file.
2. **Repeated Per-Pair Invariant Recalculation (~0.3s–0.5s)**: In `crates/diffuse-core/src/cluster/dp.rs`, `mod_pair_needs_scoring` iterates through up to hundreds of thousands of candidate cells within the corridor, re-scanning whitespace prefixes and re-trimming strings for each pair rather than reading precomputed properties on `DpRowMeta`.
3. **MUM Anchor Decomposition Failure on Systematic Shifts (~2.0s)**: When 100% of lines experience a systematic prefix change (e.g. ANSI theme escape sequences), exact string matching finds 0 unique anchor MUMs, forcing the engine into a fallback $7{,}183 \times 7{,}183$ banded DP matrix solve.

This plan resolves each bottleneck in sequence, upholding strict test-first discipline, zero golden divergence, and sub-second execution budgets.

---

## Table of Contents

1. [Phase 1 — Deduplicate and Parallelize Post-Pairing Syntax Runs](#1-phase-1--deduplicate-and-parallelize-post-pairing-syntax-runs)
2. [Phase 2 — Precompute Row Invariants for DP Candidate Pruning](#2-phase-2--precompute-row-invariants-for-dp-candidate-pruning)
3. [Phase 3 — Normalized and Semantic MUM Anchor Decomposition](#3-phase-3--normalized-and-semantic-mum-anchor-decomposition)
4. [Verification and Golden Invariants](#4-verification-and-golden-invariants)

---

## 1. Phase 1 — Deduplicate and Parallelize Post-Pairing Syntax Runs

### Objective
Eliminate redundant `colored_runs` evaluations between `hunks_out` and `rows` in `process_single_diff_file`, and introduce intra-hunk parallel processing so files with single monolithic hunks scale linearly across available CPU cores.

### Concrete Steps
1. **Regression Benchmark**:
   - In [`crates/diffuse-core/tests/big_hunk_perf.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/tests/big_hunk_perf.rs), add a dedicated regression test asserting that post-pairing syntax classification and visible row assembly for large hunks completes within a strict budget (< 1.2s on dev profile).
2. **Intra-Hunk Parallelization**:
   - In [`crates/diffuse-core/src/pipeline.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/pipeline.rs), update hunk row processing: when mapping hunk rows to `OutputRow`, parallelize across rows within each hunk using `par_chunks` or `par_iter` if the row count exceeds a minimum threshold (e.g. 64 rows).
3. **Deduplicate `OutputRow` Construction for Visible Rows**:
   - For all change rows (`Row::Mod`, `Row::Add`, `Row::Del`, `Row::MovSrc`, `Row::MovTgt`, `Row::Sep`), the `OutputRow` needed by `rows` (the visible stream) is completely identical to the row created for `hunks_out`.
   - Directly reuse/clone the precomputed rows or construct them in a unified pass, avoiding calling `colored_runs` twice per row.
   - For `Row::Ctx`, preserve the exact context side mapping rules (`vis.side` affecting line numbers and `for_each_ctx_line_token`).
4. **Verification**:
   - Ensure byte-for-byte fidelity across all golden test suites (`cargo test --test golden`).
   - Confirm post-pairing duration drops from ~5.5s to < 0.8s.

---

## 2. Phase 2 — Precompute Row Invariants for DP Candidate Pruning

### Objective
Eliminate redundant character scanning, trimming, and delimiter stripping inside `mod_pair_needs_scoring`, which is evaluated across the DP corridor.

### Concrete Steps
1. **Targeted Micro-Benchmark & Unit Test**:
   - Author a targeted unit test in [`crates/diffuse-core/src/cluster/tests.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/tests.rs) exercising `mod_pair_needs_scoring` across representative rows to verify invariant parity.
2. **Enhance `DpRowMeta`**:
   - In [`crates/diffuse-core/src/cluster/dp.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/dp.rs), add precalculated fields to `DpRowMeta`:
     - Leading whitespace character count (`indent_chars: usize`).
     - Pre-stripped text slice reference or interned ID for `trim_trailing_delims(trimmed, profile)`.
3. **Short-Circuit Candidate Checks**:
   - Compute `is_prefix_containment` lazily only when `max_similarity_ratio < min_ratio` and other fast gates have not already satisfied `could_be_valid_mod`.
   - Compute `common_indent` using `min(old.indent_chars, new.indent_chars)` when indentation prefixes match, avoiding `chars().take_while(is_whitespace)`.
4. **Verification**:
   - Run `cargo test -p diffuse-core` and confirm zero test failures and no golden output changes.

---

## 3. Phase 3 — Normalized and Semantic MUM Anchor Decomposition

### Objective
Enable [`solve_cluster_with_anchors`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/dp.rs) to discover unique matching anchors (MUMs) even when lines carry systematic prefix changes (such as terminal escape sequences or uniform indent changes), decomposing massive monolithic clusters into small independent sub-problems.

### Concrete Steps
1. **Reproduction Unit Test**:
   - Author a test in [`crates/diffuse-core/tests/big_hunk_perf.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/tests/big_hunk_perf.rs) with a large cluster where lines have uniform prefix modifications, proving that `solve_cluster_with_anchors` successfully decomposes the cluster.
2. **Semantic Core Extraction for Anchor Discovery**:
   - In [`crates/diffuse-core/src/cluster/dp.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/dp.rs), provide a normalized line representation for anchor extraction that strips leading ANSI escape codes (`\x1b[...]` or `{48;5;...}` in render fixtures) and excessive whitespace.
   - Guard decomposition with monotonicity and corridor constraints so that invalid crossings are rejected.
3. **Sub-Cluster Decomposition Execution**:
   - When normalized MUM anchors are identified, partition the cluster along the anchors and solve each sub-cluster independently with `solve_cluster_with_arena`.
4. **Verification**:
   - Verify that all golden fixtures pass byte-for-byte.
   - Verify that large replacement hunk pairing time drops to < 0.5s.

---

## 4. Verification and Golden Invariants

1. **Golden Tests**:
   - Run library leg and binary leg across all 16 golden fixtures (`cargo test --test golden`).
   - Every `.wire.jsonl`, `.render.txt`, and `.markdown.md` file must match byte-for-byte.
2. **Quality Invariants**:
   - Nightly formatting: `cargo +nightly fmt --all -- --check`.
   - Clippy: `cargo clippy --all-targets --workspace -- -D warnings`.
   - Full script gate: `./scripts/check.sh` completes cleanly with zero warnings.
