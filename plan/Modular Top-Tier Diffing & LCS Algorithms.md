# Modular Top-Tier Diffing & LCS Algorithms

## Global TODO
- [x] [Phase 1 — Algorithm Module Architecture & Generic Contracts](#1-phase-1--algorithm-module-architecture--generic-contracts)
  - [x] Define abstract, generic trait interfaces in `crates/diffuse-core/src/algo/mod.rs` (or standalone crate `crates/diffuse-algo`)
  - [x] Ensure 100% separation: algorithms take generic slices `&[T]` / `&[u8]` and return index pairs with zero diffuse domain types
- [x] [Phase 2 — Myers' 64-Bit Bit-Parallel LCS Engine](#2-phase-2--myers-64-bit-bit-parallel-lcs-engine)
  - [x] Implement bit-vector dynamic programming across `u64` / `u128` words for token sequences $\le 64 / \le 128$
  - [x] Implement fallback to chunked bit-vector for arbitrarily long sequences
  - [x] Property-based testing & fuzzing against naive $O(M \times N)$ reference LCS
- [ ] [Phase 3 — Myers' $O(ND)$ Difference Engine](#3-phase-3--myers-ond-difference-engine)
  - [ ] Implement diagonal $k$-line search with linear space $O(D)$ trace
  - [ ] Benchmark near-identical line pairing vs full matrix DP
- [ ] [Phase 4 — Hunt-Szymanski / Robinson-Schensted Patience LCS](#4-phase-4--hunt-szymanski--robinson-schensted-patience-lcs)
  - [ ] Implement $O((R + N) \log N)$ sparse LCS matching with binary search
  - [ ] Verify multi-line block and comment reflow anchor fidelity
- [ ] [Phase 5 — Hirschberg Linear-Space Divide-and-Conquer Solver](#5-phase-5--hirschberg-linear-space-divide-and-conquer-solver)
  - [ ] Implement bidirectional linear-space score recursion
  - [ ] Guarantee $O(\min(M, N))$ L1-cache-resident working set for large cluster diffs
- [ ] [Phase 6 — $A^*$ & Diagonal Bounding on Cluster State Graphs](#6-phase-6--a-diagonal-bounding-on-cluster-state-graphs)
  - [ ] Implement admissible heuristic lower bounds and diagonal bandwidth pruning
  - [ ] Verify 100% score and path equivalence against unpruned cluster DP
- [ ] [Phase 7 — Aho-Corasick & Shift-And Direct Pattern Automata](#7-phase-7--aho-corasick--shift-and-direct-pattern-automata)
  - [ ] Implement direct 256-state transition table for prefix matching
  - [ ] Connect to syntax stop-prefixes and language keyword classifiers

---

## Executive Summary

Diffuse requires industry-leading diffing, token matching, and sequence alignment performance. However, diffing algorithms can easily become tightly coupled to higher-level domain representations (ASTs, Hunks, Rows, Syntax regions, Notes).

### Strict Architectural Independence Invariant
> **Total Separation Invariant**:
> 1. **Algorithms are 100% agnostic**: Every algorithm in the `algo` subsystem operates strictly on generic types (e.g., `&[T] where T: PartialEq + Hash`, `&[u8]`, or integer IDs). Algorithms have **zero knowledge** of diffs, hunks, rows, terminal colors, notes, syntax ASTs, or diffuse-specific data structures.
> 2. **Diffuse is 100% algorithm-agnostic**: The diffuse pipeline interacts with sequence matching exclusively through generic trait boundaries (`LcsEngine`, `DiffEngine`, `PrefixMatcher`). The underlying algorithm (Myers bit-parallel, Hirschberg, Hunt-Szymanski, or standard DP) can be swapped or parameterized without modifying a single line of diffuse business logic.

---

## Table of Contents
1. [Phase 1 — Algorithm Module Architecture & Generic Contracts](#1-phase-1--algorithm-module-architecture--generic-contracts)
2. [Phase 2 — Myers' 64-Bit Bit-Parallel LCS Engine](#2-phase-2--myers-64-bit-bit-parallel-lcs-engine)
3. [Phase 3 — Myers' O(ND) Difference Engine](#3-phase-3--myers-ond-difference-engine)
4. [Phase 4 — Hunt-Szymanski / Robinson-Schensted Patience LCS](#4-phase-4--hunt-szymanski--robinson-schensted-patience-lcs)
5. [Phase 5 — Hirschberg Linear-Space Divide-and-Conquer Solver](#5-phase-5--hirschberg-linear-space-divide-and-conquer-solver)
6. [Phase 6 — A* & Diagonal Bounding on Cluster State Graphs](#6-phase-6--a-diagonal-bounding-on-cluster-state-graphs)
7. [Phase 7 — Aho-Corasick & Shift-And Direct Pattern Automata](#7-phase-7--aho-corasick--shift-and-direct-pattern-automata)
8. [Verification Gates & Performance Budgets](#8-verification-gates--performance-budgets)

---

## 1. Phase 1 — Algorithm Module Architecture & Generic Contracts

### Architecture
All algorithms live in an isolated module (e.g. `crates/diffuse-core/src/algo/` or a dedicated crate `crates/diffuse-algo/`).

```rust
pub trait LcsEngine {
    /// Compute index pairings between two sequences: (a_idx, b_idx).
    fn lcs_indices<T: PartialEq + Eq + std::hash::Hash>(
        &self,
        a: &[T],
        b: &[T],
    ) -> Vec<(usize, usize)>;
}

pub trait DiffEngine {
    /// Compute edit script (Keep, Insert, Delete) between two sequences.
    fn diff_slices<T: PartialEq + Eq>(&self, a: &[T], b: &[T]) -> Vec<EditOp>;
}

pub trait PrefixMatcher {
    /// Match any prefix from a compiled dictionary in O(1) per byte.
    fn longest_matching_prefix<'a>(&self, text: &'a str) -> Option<&'a str>;
}
```

---

## 2. Phase 2 — Myers' 64-Bit Bit-Parallel LCS Engine

### Algorithm Overview
Myers (1999) / Hyyrö (2001) bit-vector algorithm computes the LCS DP grid using 64-bit CPU words.
- Each column of the dynamic programming grid is represented as a bit-vector $V$.
- The transition between columns is calculated using bitwise arithmetic:
  $$\Delta = (V + (V \ \& \ M)) \oplus V$$
- For tokenized lines of code ($\le 64$ tokens), the entire LCS table is solved in **$O(N)$ operations with zero heap allocation**.

### Implementation Steps
1. Create `crates/diffuse-core/src/algo/bit_parallel.rs`.
2. Implement `BitParallelLcs::lcs_64<T: PartialEq>(a: &[T], b: &[T]) -> Vec<(usize, usize)>`.
3. Implement block-chunked bit-vectors for inputs exceeding 64 items.
4. Add fuzzing tests proving 100% equivalence against classic 2D matrix LCS.

---

## 3. Phase 3 — Myers' $O(ND)$ Difference Engine

### Algorithm Overview
Myers' $O(ND)$ algorithm searches $D$-contours along diagonals $k = x - y$.
- Runs in $O((M + N) \times D)$ time, where $D$ is the number of differences.
- For lines with few edits ($D \ll N$), the search completes almost instantaneously.

### Implementation Steps
1. Create `crates/diffuse-core/src/algo/myers_ond.rs`.
2. Implement diagonal trace array with linear space backward midpoint recursion.
3. Benchmark against traditional edit distance algorithms.

---

## 4. Phase 4 — Hunt-Szymanski / Robinson-Schensted Patience LCS

### Algorithm Overview
Hunt-Szymanski identifies matches of unique tokens and reduces LCS to finding the Longest Increasing Subsequence (LIS) via binary search (Patience Sorting).
- Time complexity: $O((R + N) \log N)$, where $R$ is the count of matching token pairs.
- Highly effective for sparse diffs and large structural block alignment.

### Implementation Steps
1. Create `crates/diffuse-core/src/algo/hunt_szymanski.rs`.
2. Implement binary-search-driven LIS over candidate match positions.
3. Wire as the backend for patience token anchoring.

---

## 5. Phase 5 — Hirschberg Linear-Space Divide-and-Conquer Solver

### Algorithm Overview
Hirschberg's algorithm computes the exact global alignment in $O(M \times N)$ operations while strictly bounding memory to $O(\min(M, N))$.
- Evaluates forward DP from $(0, 0)$ to row $M/2$ and backward DP from $(M, N)$ to row $M/2$.
- Finds optimal midpoint $(M/2, k)$ and recurses on $(0..M/2, 0..k)$ and $(M/2..M, k..N)$.
- Guarantees complete L1 cache residency ($< 32\text{ KB}$) regardless of changeset scale.

### Implementation Steps
1. Create `crates/diffuse-core/src/algo/hirschberg.rs`.
2. Implement forward/backward score vector passes with two recycled rows.
3. Verify linear space complexity and numerical score equivalence.

---

## 6. Phase 6 — $A^*$ & Diagonal Bounding on Cluster State Graphs

### Algorithm Overview
2D cluster pairing evaluates $(M+1) \times (N+1)$ state transitions.
- By computing an admissible remaining-cost heuristic $h(i, j) = |(M - i) - (N - j)| \cdot \mu$, states that cannot exceed the best known score are immediately pruned.
- Restricts exploration to an adaptive bandwidth diagonal $|i - j| \le K$.

### Implementation Steps
1. Create `crates/diffuse-core/src/algo/bounded_dp.rs`.
2. Implement branch-and-bound search with diagonal bounding.
3. Benchmark cluster solving latency on large multi-line change clusters.

---

## 7. Phase 7 — Aho-Corasick & Shift-And Direct Pattern Automata

### Algorithm Overview
Language stop-prefixes and syntax boundary keywords are compiled into a 256-state direct lookup DFA or Shift-And bitmask.
- Matches keywords in a single memory lookup per byte ($O(1)$) with zero backtracking.

### Implementation Steps
1. Create `crates/diffuse-core/src/algo/prefix_dfa.rs`.
2. Compile immutable static state transition tables for language profile boundaries.
3. Connect to `diffuse-core::scope` and `diffuse-syntax`.

---

## 8. Verification Gates & Performance Budgets

1. **Total Modularity Invariant**: No file in `algo/` may import any type from `diffuse_core::parse`, `wire`, `syntax`, `render`, or `notes`.
2. **Deterministic Parity**: All algorithm implementations must match reference oracle outputs 100%.
3. **Sub-second Suite Budget**: Individual unit tests $< 0.5\text{s}$, test binaries $< 1.5\text{s}$, full `./scripts/check.sh` $< 3\text{s}$.
