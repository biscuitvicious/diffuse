# Data Structure Optimizations & Algorithm Extraction

- [x] **Phase 1: Bit-Parallel Myers / Hyyrö LCS Engine & Word-Level Parallelism**
  - [x] Implement `BitParallelLcs64` and `BitParallelLcs128` in `crates/diffuse-core/src/lcs/bit_parallel.rs`
  - [x] Support character-weighted score summation without 2D table allocations
  - [x] Author exhaustive differential unit tests comparing bit-parallel output against scalar 2D LCS
  - [x] Wire bit-parallel LCS into `lcs_matched_chars_ids` and the Block-DP `Mod` transition
- [ ] **Phase 2: Token Salience Partitioning & Diagonal Early-Abort**
  - [ ] Classify tokens in `TokenInterner` into Salient (semantic identifiers/literals) vs Delimiter/Format (punctuation, ANSI escapes, whitespace)
  - [ ] Update `token_multiset` bounds in `DpTokens` to enforce salient token correspondence
  - [ ] Implement $K$-bounded diagonal early exit when edit distance exceeds the `next_diag_score` threshold
  - [ ] Verify pruning efficiency and precision on `tests/golden/diffuse-2026-08/render.txt`
- [ ] **Phase 3: Augmented Interval Tree for Coordinate & Span Queries**
  - [ ] Implement array-backed `AugmentedIntervalTree<T>` in `crates/diffuse-core/src/algo/interval_tree.rs`
  - [ ] Implement $O(\log K + M)$ point stabbing and range overlap queries
  - [ ] Replace linear interval scans in subline move tracking and syntax annotation lookups
  - [ ] Verify correctness and benchmark query throughput
- [ ] **Phase 4: Architectural Crate Extraction — `crates/diffuse-algo`**
  - [ ] Create `crates/diffuse-algo` crate in workspace root
  - [ ] Migrate pure algorithms: `SkylineMatrix`, `AdaptiveEnvelope`, `BitParallelLcs`, `AffineGap`, `DyckMatcher`, `PrefixDfa`, `BipartiteMatching`, and `IntervalTree`
  - [ ] Decouple from diff domain models (`Row`, `ChangeCluster`), exposing pure generic interfaces
  - [ ] Re-export algorithms in `diffuse-core` to preserve existing API boundaries
- [ ] **Phase 5: Golden Verification, Performance Benchmarks & Sign-Off**
  - [ ] Benchmark full pipeline throughput on large changesets (`tests/golden/diffuse-2026-08/render.txt` and `opt-1`)
  - [ ] Verify byte-for-byte golden corpus invariance across all formats (`.wire.jsonl`, `.render.txt`, `.markdown.md`)
  - [ ] Run full verification gate (`./scripts/check.sh`) ensuring zero warnings and compliance with performance budgets

---

## Executive Summary

Dynamic programming profiling on massive changesets (such as `tests/golden/diffuse-2026-08/render.txt`, consisting of 14,368 diff lines) confirmed that **93.82% of execution time is consumed by subline token LCS calculations (`lcs_matched_chars_ids`)**. In large diff corridors, evaluating in-place modifications across hundreds of thousands of candidate cell pairings causes the engine to allocate and iterate over billions of scalar DP cells. Furthermore, low-entropy formatting tokens (such as ANSI escape codes, punctuation, and structural delimiters) saturate multiset bounds, triggering false-positive subline alignments.

This plan addresses these performance barriers through principled data structure and algorithmic interventions:

1. **Word-Level Bit Parallelism (Myers 1999 / Hyyrö 2001)**: Subline token sequences ($\le 64$ or $\le 128$ tokens) are evaluated using bit-vector registers. Instead of quadratic $O(M \times N)$ table allocations, the LCS score is computed via $O(N)$ single-cycle bitwise ALU operations (`&`, `|`, `^`, `+`, `<<`), delivering an estimated 30x–40x speedup on token matching.
2. **Token Salience Partitioning**: Tokens are partitioned into high-entropy semantic tokens (identifiers, keywords, literals) versus low-entropy formatting tokens (ANSI escape sequences, single delimiters). Pairwise subline alignment is guarded by salient token presence, preventing ubiquitous formatting markers from triggering expensive evaluations.
3. **Augmented Interval Trees**: Linear coordinate searches in subline move resolution and syntax span tracking are replaced by flat array-backed augmented interval trees, reducing point stabbing queries from $O(K)$ to $O(\log K + M)$.
4. **Crate Decoupling (`diffuse-algo`)**: The mathematical algorithms and generic profile data structures are extracted into a dedicated `diffuse-algo` crate, enforcing strict decoupling between abstract sequence mathematics and diff parsing/rendering.

---

## Table of Contents

1. [Phase 1: Bit-Parallel Myers / Hyyrö LCS Engine & Word-Level Parallelism](#1-phase-1-bit-parallel-myers--hyyrö-lcs-engine--word-level-parallelism)
2. [Phase 2: Token Salience Partitioning & Diagonal Early-Abort](#2-phase-2-token-salience-partitioning--diagonal-early-abort)
3. [Phase 3: Augmented Interval Tree for Coordinate & Span Queries](#3-phase-3-augmented-interval-tree-for-coordinate--span-queries)
4. [Phase 4: Architectural Crate Extraction — `crates/diffuse-algo`](#4-phase-4-architectural-crate-extraction--cratesdiffuse-algo)
5. [Phase 5: Golden Verification, Performance Benchmarks & Sign-Off](#5-phase-5-golden-verification-performance-benchmarks--sign-off)

---

## 1. Phase 1: Bit-Parallel Myers / Hyyrö LCS Engine & Word-Level Parallelism

### 1.1 Mathematical Foundation
Standard dynamic programming for the Longest Common Subsequence computes the recurrence:
$$L[i, j] = \begin{cases} L[i-1, j-1] + 1 & \text{if } A[i] = B[j] \\ \max(L[i-1, j], L[i, j-1]) & \text{otherwise} \end{cases}$$

Myers (1999) and Hyyrö (2001) established that the vertical and horizontal difference vectors $\Delta v_i, \Delta h_j \in \{0, 1\}$ can be encoded as bit-vectors within computer registers. Given an alphabet bitmask pattern $B[c] \in \{0, 1\}^M$ indicating positions where $A[i] = c$:
$$X = B[B_j] \mid V_{\text{in}}$$
$$D = (X - (V_{\text{in}} \ll 1)) \oplus X$$
$$V_{\text{out}} = X \ \& \ (\sim D)$$

For sequences where $M \le 64$, the entire inner DP column evaluates in $\approx 5$ bitwise instructions without memory access or branch mispredictions.

### 1.2 Implementation Steps
1. **Module Creation**:
   Create `crates/diffuse-core/src/lcs/bit_parallel.rs`.
2. **Bit-Vector Structures**:
   Implement `BitParallelLcs64` (for $M \le 64$) and `BitParallelLcs128` (for $65 \le M \le 128$) using native `u64` and `[u64; 2]`.
3. **Pattern Mask Construction**:
   Map token IDs to bitmasks $B[\text{id}]$ using an indexed dense lookup array for the current line $A$.
4. **Weighted Non-Whitespace Summation**:
   Combine the resulting bitmask of matched indices in $B$ with `token_chars` lengths to compute `matched_chars` without traversing a 2D matrix.
5. **Fall-through Strategy**:
   Retain the existing scratch-based 2D algorithm as a fallback for lines exceeding 128 tokens ($< 0.1\%$ of typical code lines).
6. **Differential Testing**:
   Author property-based unit tests asserting that `bit_parallel_lcs(a, b)` produces the exact same matched character counts as `lcs_matched_chars_ids(a, b)` across thousands of synthetic and real token sequences.

---

## 2. Phase 2: Token Salience Partitioning & Diagonal Early-Abort

### 2.1 Problem Analysis
In diff outputs containing formatting (e.g. `render.txt`), lines contain ANSI escape codes like `\x1b[48;5;22m` and punctuation symbols `{`, `}`, `;`, `,`. While these are distinct non-whitespace tokens, their ubiquity causes unrelated lines to share 20–30 characters of formatting, yielding multiset bounds $\ge 25\%$ that trigger unnecessary subline LCS invocations.

### 2.2 Implementation Steps
1. **Token Categorization in `TokenInterner`**:
   Add a classification field or bitflag to `TokenInterner`:
   ```rust
   pub enum TokenSalience {
       Semantic,   // Alphanumeric words, identifiers, literals
       Structural, // Delimiters, brackets, operators
       Formatting, // ANSI escape sequences, whitespace
   }
   ```
2. **Salient Multiset Filtering**:
   In `DpTokens`, maintain:
   - `total_chars`: all non-whitespace token characters.
   - `salient_chars`: characters belonging strictly to `TokenSalience::Semantic`.
   - `salient_multiset`: multiset of semantic token IDs.
3. **Pre-Filter Enforcement**:
   In `could_be_valid_mod`, require that candidate lines share at least one salient token or satisfy a syntactic head match before calling subline LCS:
   ```rust
   let salient_overlap = token_intersection_bound(
       &ot.salient_multiset,
       &nt.salient_multiset,
       &interner.char_lens,
   );
   if salient_overlap == 0 && !is_syntax_match {
       continue;
   }
   ```
4. **$K$-Bounded Diagonal Early Exit**:
   When evaluating cell $(i, j)$, calculate the minimum characters required for the transition to beat the existing score at $(i+1, j+1)$:
   $$K_{\min} = \lceil (next\_diag\_score - base + w.\beta) / w.\alpha \rceil$$
   If the multiset upper bound $\text{bound} < K_{\min}$, abort the transition immediately.

---

## 3. Phase 3: Augmented Interval Tree for Coordinate & Span Queries

### 3.1 Problem Analysis
Subline move tracking (`subline_moves`), syntax section attribution, and user review notes lookups query intervals $[start, end]$ against point coordinates or overlapping spans. Currently, these operations perform linear searches over `Vec<Span>` or multiple binary searches.

### 3.2 Implementation Steps
1. **Data Structure Definition**:
   Implement a flat, cache-conscious array-backed augmented interval tree:
   ```rust
   pub struct IntervalNode<T> {
       pub low: usize,
       pub high: usize,
       pub max: usize,
       pub value: T,
   }
   
   pub struct AugmentedIntervalTree<T> {
       nodes: Vec<IntervalNode<T>>,
   }
   ```
2. **Zero-Allocation Construction**:
   Construct the tree from sorted slices in $O(N)$ time into a contiguous flat vector.
3. **Stabbing Queries**:
   Implement `stabbing_query(point, &mut matches)` in $O(\log N + M)$ without heap allocations, utilizing recycled output buffers.
4. **Range Overlap Queries**:
   Implement `overlapping_query(range, &mut matches)` to detect intersecting relocation blocks and syntax ranges.
5. **Unit Verification**:
   Exhaustively test interval tree operations against brute-force vector scans across boundary conditions: point on edge, fully contained, overlapping, disjoint, and empty trees.

---

## 4. Phase 4: Architectural Crate Extraction — `crates/diffuse-algo`

### 4.1 Architectural Rationale
In accordance with repository guidelines ([`AGENTS.md`](../AGENTS.md) and [`docs/Data Structures.md`](../docs/Data%20Structures.md)), pure algorithms and foundational data structures must be renderer-agnostic, decoupled from I/O, and testable in isolation.

### 4.2 Crate Structure
Create `crates/diffuse-algo`:
```
crates/diffuse-algo/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── skyline.rs          // SkylineMatrix, AdaptiveEnvelope, Jennings profile storage
│   ├── lcs/
│   │   ├── mod.rs
│   │   ├── bit_parallel.rs // Myers/Hyyrö bit-parallel word DP
│   │   └── hirschberg.rs   // Linear-space Myers/Hirschberg
│   ├── affine_gap.rs       // Gotoh three-plane affine alignment
│   ├── dyck.rs             // Dyck-N pushdown stack automaton
│   ├── prefix_dfa.rs       // Aho-Corasick trie
│   ├── bipartite.rs        // Kuhn Hungarian assignment
│   └── interval_tree.rs    // Augmented interval tree
└── tests/
    └── property_tests.rs
```

### 4.3 Workspace Migration Steps
1. Add `diffuse-algo` to root `Cargo.toml` `workspace.members`.
2. Move generic algorithmic implementations from `crates/diffuse-core/src/algo/` to `crates/diffuse-algo/src/`.
3. Add `diffuse-algo = { path = "../diffuse-algo" }` as a dependency of `diffuse-core`.
4. Re-export `pub use diffuse_algo as algo;` in `diffuse-core::lib` to preserve backward compatibility.
5. Ensure zero warnings and verified test passes across the workspace.

---

## 5. Phase 5: Golden Verification, Performance Benchmarks & Sign-Off

### 5.1 Verification Checklist
- [ ] Execute `cargo test -p diffuse-algo` exercising all unit and property tests.
- [ ] Execute `cargo test -p diffuse-core` ensuring all cluster DP tests pass.
- [ ] Benchmark `tests/golden/diffuse-2026-08/render.txt` in release mode, ensuring wall-clock time is $< 5\text{s}$ (down from $1\text{m}48\text{s}$).
- [ ] Verify byte-for-byte identity of all golden test outputs (`tests/golden.rs`).
- [ ] Run full repository verification gate:
  ```bash
  ./scripts/check.sh
  ```
- [ ] Update documentation:
  - [`docs/Data Structures.md`](../docs/Data%20Structures.md) (document `BitParallelLcs` and `AugmentedIntervalTree`).
  - [`docs/Algorithms.md`](../docs/Algorithms.md) (document bit-parallel recurrence and salience partitioning).
