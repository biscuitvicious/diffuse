# Pairing and Assembly Performance Optimization

## Global TODO
- [x] [Phase 1 — Parallelized Subword Span Assembly](#1-phase-1--parallelized-subword-span-assembly)
  - [x] Author targeted performance regression test exercising multi-core span generation
  - [x] Extract paired `(del_text, add_text)` payload extraction into pre-pass
  - [x] Precompute `word_spans_with_profile` and wholesale replacement classification via Rayon `par_iter`
  - [x] Retain deterministic sequential assembly order in `pass_assemble_mods`
  - [x] Verify byte-identical wire outputs across all golden test suites
- [x] [Phase 2 — Precomputed Tail Filters in Guarded Deletions](#2-phase-2--precomputed-tail-filters-in-guarded-deletions)
  - [x] Author test verifying tail-closing candidate guard behavior on large synthetic hunks
  - [x] Eliminate $O(D \times A)$ nested loop in `guarded_dels` by indexing add-tail candidates
  - [x] Pre-filter candidate add tails and associated function scopes once per hunk
  - [x] Verify zero regressions across golden fixtures and assert pairing time drop
- [x] [Phase 3 — Common Prefix & Suffix Token Stripping in AlignmentTape](#3-phase-3--common-prefix--suffix-token-stripping-in-alignmenttape)
  - [x] Author unit tests in `tape.rs` asserting exact tape equivalence for prefix/suffix peeling
  - [x] Implement fast token-level common prefix and suffix run peeling in `AlignmentTape::from_texts_with_cap`
  - [x] Restrict dynamic programming matrix to the differing middle window
  - [x] Enforce alignment tape invariants: `project_old`, `project_new`, and invertibility
- [x] [Phase 4 — Token DP Matrix Scratch Arena](#4-phase-4--token-dp-matrix-scratch-arena)
  - [x] Benchmark heap allocation overhead in `raw_lcs_pairs_with_context`
  - [x] Implement thread-local or pooled scratch arena for DP matrices (`m_matrix`, `x_matrix`, `y_matrix`, `tb_m`, `tb_x`, `tb_y`, `run_len`)
  - [x] Replace per-call `vec![0; (n + 1) * (m + 1)]` heap allocations with re-sliced arena vectors
  - [x] Verify thread-safety and exact LCS backtrace identity
- [·] [Phase 5 — Full Gate Verification & Performance Locking](#5-phase-5--full-gate-verification--performance-locking)
  - [ ] Execute `scripts/check.sh` with zero warnings and zero golden regressions
  - [ ] Record end-to-end speedup on 7,174-line benchmark in `tests/big_hunk_perf.rs`
  - [ ] Lock new performance thresholds in test assertions

---

## Executive Summary

A comprehensive performance profile of the 7,174-line whole-file replacement benchmark (`tests/big_hunk_perf.rs`) revealed that while cluster DP pairing was successfully accelerated from 36.7s down to ~1.08s via adaptive corridor narrowing and inverted token phrase anchors, the remaining execution time is dominated by two bottlenecks:

1. **Sequential Subword Diffing in Single Hunks (630 ms / 58.4%)**: `pass_assemble_mods` runs sequentially on a single thread. For hunks with thousands of modifications, `word_spans_with_profile` is invoked ~2,400 times serially on 1 core, performing full tokenization and matrix DP for each line pair.
2. **Unindexed Tail Scans in `guarded_dels` (156 ms / 14.4%)**: In `guarded_dels`, an inner `h.rows.iter().any(...)` loop repeatedly scans all 14,000 hunk rows for every candidate deletion, executing expensive scope queries (`is_tail_before_closing` and `fn_of`) repeatedly in an $O(D \times A)$ nested loop.
3. **Full-Line Matrix Diffing on Localized Edits**: `AlignmentTape::from_texts_with_cap` does not peel common prefix and suffix token runs, computing full $O(N \times M)$ token matrices even when only a single token differs at the end of a 500-token line.
4. **Dynamic Vector Allocation Churn**: Each subword DP invocation in `raw_lcs_pairs_with_context` allocates 7 distinct heap vectors, generating over 16,800 allocations in a single hunk.

This plan systematically eliminates the quadratic scan, parallelizes subword span generation across available CPU cores via Rayon, strips common prefix/suffix tokens before matrix DP, and reuses memory via a scratch buffer arena.

---

## Table of Contents
1. [Phase 1 — Parallelized Subword Span Assembly](#1-phase-1--parallelized-subword-span-assembly)
2. [Phase 2 — Precomputed Tail Filters in Guarded Deletions](#2-phase-2--precomputed-tail-filters-in-guarded-deletions)
3. [Phase 3 — Common Prefix & Suffix Token Stripping in AlignmentTape](#3-phase-3--common-prefix--suffix-token-stripping-in-alignmenttape)
4. [Phase 4 — Token DP Matrix Scratch Arena](#4-phase-4--token-dp-matrix-scratch-arena)
5. [Phase 5 — Full Gate Verification & Performance Locking](#5-phase-5--full-gate-verification--performance-locking)

---

## 1. Phase 1 — Parallelized Subword Span Assembly

### Problem
[`crates/diffuse-core/src/collapse.rs:207`](crates/diffuse-core/src/collapse.rs#L207) (`pass_assemble_mods`) iterates through `h.rows` sequentially:
```rust
for (ri, r) in h.rows.iter().enumerate() {
    ...
    let spans = word_spans_with_profile(&del_text, add.text(), profile);
    ...
}
```
In whole-file replacements or large single hunks (e.g. 7,174 lines), Rayon hunk-level parallelism in [`pipeline.rs`](crates/diffuse-core/src/pipeline.rs#L432) has no effect because there is only one hunk. As a result, all 2,392 subword alignments run on a single CPU core, consuming **630 ms (58.4% of the entire pipeline runtime)**.

### Implementation Steps
1. In `crates/diffuse-core/src/collapse.rs`:
   - Prior to the assembly loop, identify all paired deletion row indices `ri` in `ctx.paired_dels`.
   - Extract the pair items: `(ri, add_ri, del_text, add.text())`.
   - Using `rayon::prelude::*`, compute `(ri, word_spans_with_profile(&del_text, add_text, profile))` in parallel across all worker threads.
   - Collect the results into a fast lookup table `FxHashMap<usize, WordSpans>` (or a pre-indexed vector).
2. Inside `pass_assemble_mods`:
   - Query the precomputed spans for each pair, avoiding any inline blocking call to `word_spans_with_profile`.
   - Maintain the identical sequential order, `kept_dels`, `pure_adds`, and anchor insertion order into `out`.
3. Invariant: Wire and Markdown output must remain 100% byte-for-byte identical.

### Verification Gate
- Unit test ensuring `pass_assemble_mods` output with precomputed spans is identical to baseline.
- Full `scripts/check.sh` passes with zero golden changes.
- Benchmark: `test_stress_full_replace_pairing_time` assemble phase drops from ~630 ms to <100 ms on multi-core hardware.

---

## 2. Phase 2 — Precomputed Tail Filters in Guarded Deletions

### Problem
[`crates/diffuse-core/src/cluster/pairing.rs:145`](crates/diffuse-core/src/cluster/pairing.rs#L145) (`guarded_dels`) checks whether a candidate deletion is a tail before closing:
```rust
let del_is_tail = is_tail_before_closing(r.old_line(), true, old_content, new_content, profile);
let tail_candidate = del_is_tail && h.rows.iter().any(|a| {
    matches!(a, Row::Add(_) | Row::MovTgt(_))
        && is_tail_before_closing(a.new_line(), false, old_content, new_content, profile)
        && (fn_of(old_content, r.old_line(), profile).is_none()
            || fn_of(old_content, r.old_line(), profile) == fn_of(new_content, a.new_line(), profile))
});
```
On a hunk with 7,174 deletions and 7,174 additions, this inner `any(...)` scan checks up to 7,174 rows for every deletion, evaluating string matching and syntax function scopes over and over, taking **156 ms (45% of `dp_pairing`)**.

### Implementation Steps
1. In `crates/diffuse-core/src/cluster/pairing.rs`:
   - In `guarded_dels`, pre-filter all additions in `h.rows` that satisfy `is_tail_before_closing(a.new_line(), false, ...)`.
   - Store their function scopes in a precomputed structure (e.g. `has_unscoped_add_tail: bool`, `add_tail_fn_scopes: FxHashSet<String>`).
   - Replace the $O(D \times A)$ inner linear search with an $O(1)$ set lookup.
2. Invariant: The set of guarded deletions produced by `guarded_dels` must be identical to the baseline.

### Verification Gate
- Unit test comparing `guarded_dels` output before and after refactoring on synthetic hunks with functions and closing delimiters.
- Full `scripts/check.sh` passes with zero golden diffs.
- Benchmark: `guarded_dels` execution time drops from 156 ms to <5 ms.

---

## 3. Phase 3 — Common Prefix & Suffix Token Stripping in AlignmentTape

### Problem
[`crates/diffuse-core/src/tape.rs:495`](crates/diffuse-core/src/tape.rs#L495) (`AlignmentTape::from_texts_with_cap`) converts the old and new text into tokens:
```rust
let ot: Vec<&str> = tokenize_line(old_text).collect();
let nt: Vec<&str> = tokenize_line(new_text).collect();
```
Even if lines have 500 identical tokens and only differ by a single character or token at the end (such as `{l}~` in the stress test), the entire token array is passed to `lcs_pairs_anchored` or `raw_lcs_pairs`, building full $(N+1) \times (M+1)$ 2D matrices.

### Implementation Steps
1. In `crates/diffuse-core/src/tape.rs`:
   - Identify common prefix tokens: `while prefix < ot.len() && prefix < nt.len() && ot[prefix] == nt[prefix] { prefix += 1; }`.
   - Identify common suffix tokens: `while suffix < (ot.len() - prefix) && suffix < (nt.len() - prefix) && ...`.
   - Emit `TapeChunk::Keep` for the common prefix.
   - Run the anchor and LCS dynamic programming solely on the remaining inner token slices: `&ot[prefix..ot.len() - suffix]` and `&nt[prefix..nt.len() - suffix]`.
   - Emit `TapeChunk::Keep` for the common suffix.
2. Respect subword and whitespace boundary invariants so token peeling does not split compound identifiers across disparate classifications.
3. Invariant: Algebraic tape involution `tape.reverse().reverse() == tape` and `project_old` / `project_new` must hold exactly.

### Verification Gate
- Property unit test exercising `AlignmentTape` prefix/suffix peeling across arbitrary perturbed lines.
- All golden test suites pass byte-for-byte.

---

## 4. Phase 4 — Token DP Matrix Scratch Arena

### Problem
[`crates/diffuse-core/src/lcs.rs:680`](crates/diffuse-core/src/lcs.rs#L680) (`raw_lcs_pairs_with_context`) allocates 7 heap vectors per invocation:
```rust
let mut m_matrix = vec![0i32; (n + 1) * (m + 1)];
let mut x_matrix = vec![0i32; (n + 1) * (m + 1)];
let mut y_matrix = vec![0i32; (n + 1) * (m + 1)];
let mut tb_m = vec![0u8; (n + 1) * (m + 1)];
let mut tb_x = vec![0u8; (n + 1) * (m + 1)];
let mut tb_y = vec![0u8; (n + 1) * (m + 1)];
let mut run_len = vec![0u16; (n + 1) * (m + 1)];
```
Over 2,400 line alignments trigger **16,800 heap allocations**, creating allocator contention and cache pollution.

### Implementation Steps
1. In `crates/diffuse-core/src/lcs.rs`:
   - Introduce `LcsScratchArena` encapsulating reusable contiguous backing storage for the 7 matrices.
   - Provide `raw_lcs_pairs_with_arena` which resizes and zeroes the scratch slices as needed without reallocating if capacity suffices.
   - Use thread-local scratch or pass arena references from the worker threads during Rayon parallel execution.
2. Invariant: Identical LCS pairs and backtrace paths for all inputs.

### Verification Gate
- Unit tests verifying exact equivalence between `raw_lcs_pairs` and `raw_lcs_pairs_with_arena`.
- Zero golden deviations across all acceptance suites.

---

## 5. Phase 5 — Full Gate Verification & Performance Locking

### Steps
1. Execute full `./scripts/check.sh` verification suite across all 16 golden test suites, unit tests, clippy, and rustfmt.
2. Run `cargo test --release --test big_hunk_perf -- --nocapture` and assert:
   - 1,100-line test executes in `< 50 ms`.
   - 7,174-line stress test executes in `< 300 ms` (down from 36.7s baseline, a >120× overall optimization).
3. Document performance results and archive plan.
