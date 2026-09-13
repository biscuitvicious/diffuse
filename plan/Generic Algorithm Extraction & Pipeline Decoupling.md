# Generic Algorithm Extraction & Pipeline Decoupling

- [x] Phase 1: Landmark Anchoring & Entropy Engines (`Mums` & `EntropyRanker`)
- [x] Phase 2: Formal Container Balancing (`DyckBalancer` & Sequence Geometry)
- [ ] Phase 3: Gotoh Affine Gap Aligner with Zero-Allocation Workspace
- [ ] Phase 4: Generic 2D Multi-Objective Block Transformation Solver (`BlockAligner`)
- [ ] Phase 5: Bipartite Matching & Spatial Coalescence Engine (`BipartiteMatcher`)
- [ ] Phase 6: Monoidal Invertible Edit Tape & Generic Symbol Interning
- [ ] Phase 7: Lean Pipeline Integration & Verification Gate

---

## Executive Summary

The `diffuse` diff engine relies on sophisticated algorithms from bioinformatics, information theory, formal language theory, and dynamic programming. While elementary sequence differencing engines ([`MyersOnD`](crates/diffuse-core/src/algo/myers_ond.rs), [`HuntSzymanskiLcs`](crates/diffuse-core/src/algo/hunt_szymanski.rs), [`HirschbergLcs`](crates/diffuse-core/src/algo/hirschberg.rs), [`BitParallelLcs`](crates/diffuse-core/src/algo/bit_parallel.rs), and [`PrefixDfa`](crates/diffuse-core/src/algo/prefix_dfa.rs)) reside in [`crates/diffuse-core/src/algo/`](crates/diffuse-core/src/algo.rs), several high-level mathematical algorithms currently live intertwined with diff domain objects (`Row`, `Hunk`, `DiffFile`, AST strings, syntax profiles) throughout the pipeline.

This architectural coupling introduces several limitations:
1. **Hidden Allocations & Missing Streams**: Dynamic programming matrices, inverted indices, and intermediate token buffers are allocated repeatedly across hunks rather than processing streams lazily via zero-cost iterators.
2. **Domain Pollution**: Pure mathematical algorithms (such as the 2D transformation lattice solver in `cluster/dp.rs` or phrase landmark indexing in `token.rs`) contain branches querying AST keywords, comment flags, or terminal row models.
3. **Restricted Testability & Verification**: Testing algorithmic edge cases currently requires assembling mock diffs or AST strings, rather than testing mathematical properties over pure generic sequences (`&[T]`, `&[u32]`).
4. **Violates Architectural Separation**: Contravenes [`GUIDELINES.md § Algorithm & Core Separation`](../GUIDELINES.md#algorithm--core-separation), which mandates that algorithms operate strictly on generic types and interfaces decoupled from diffuse domain objects.

### Governing Architectural Principles

1. **Idiomatic Iterator Trait Structs & Free Combinatorials**:
   Algorithms are exposed as dedicated iterator adapter structs (e.g. `Landmarks<I1, I2>`, `DyckBalanced<I>`, `EntropyFiltered<I>`, `TapeEvents<I>`) implementing standard `Iterator`, `FusedIterator`, and `ExactSizeIterator`.
   - **Free Combinatorials**: Chained seamlessly with standard library and `itertools` combinators (`.map()`, `.filter()`, `.zip()`, `.chunks()`, `.peekable()`, `.coalesce()`).
   - **Lazy Streaming Execution**: Items are produced on-demand via `.next()` without collecting into intermediate vectors, enabling bounded-memory streaming across large git diffs.
   - **Multithreaded Parallelism (Rayon)**: By conforming to standard iterator and indexed slice contracts, algorithms bridge cleanly to Rayon (`par_iter`, `IntoParallelIterator`, `par_chunks`) for multithreaded processing across hunks and files.
2. **Zero-Cost Abstractions & Workspace Recycling**: Algorithms operate over generic slices (`&[T]`), indexed iterators (`impl IntoIterator<Item = T>`), and comparator/evaluator traits. Zero heap allocation in tight inner loops through reusable scratch workspaces.
3. **Integer Symbol Boundary (`u32`)**: Convert strings, tokens, and words into compact `u32` symbol IDs at the pipeline boundary, enabling CPU cache locality, SIMD vectorization, and $O(1)$ integer equality.
4. **Policy-Engine Decoupling**: Algorithms solve mathematical optimization lattices; domain policies (what constitutes a keyword, syntax weights, comment barriers) are supplied externally through trait evaluators or closures.
5. **Zero Design Regression**: Golden acceptance corpora, sub-second latency budgets, and 100% branch coverage invariants remain strictly guarded at every phase.

---

## Table of Contents

1. [Phase 1 — Landmark Anchoring & Entropy Engines](#1-phase-1--landmark-anchoring--entropy-engines)
2. [Phase 2 — Formal Container Balancing & Sequence Geometry](#2-phase-2--formal-container-balancing--sequence-geometry)
3. [Phase 3 — Gotoh Affine Gap Aligner with Zero-Allocation Workspace](#3-phase-3--gotoh-affine-gap-aligner-with-zero-allocation-workspace)
4. [Phase 4 — Generic 2D Multi-Objective Block Transformation Solver](#4-phase-4--generic-2d-multi-objective-block-transformation-solver)
5. [Phase 5 — Bipartite Matching & Spatial Coalescence Engine](#5-phase-5--bipartite-matching--spatial-coalescence-engine)
6. [Phase 6 — Monoidal Invertible Edit Tape & Generic Symbol Interning](#6-phase-6--monoidal-invertible-edit-tape--generic-symbol-interning)
7. [Phase 7 — Lean Pipeline Integration & Verification Gate](#7-phase-7--lean-pipeline-integration--verification-gate)

---

## 1. Phase 1 — Landmark Anchoring & Entropy Engines

- [x] Extract Maximal Unique Matches (MUMs) / $N$-Gram Landmark Indexing into `crates/diffuse-core/src/algo/landmarks.rs`.
- [x] Extract Shannon Information Content & Empirical Frequency Filter into `crates/diffuse-core/src/algo/entropy.rs`.
- [x] Provide lazy iterator adapter structs (`Landmarks<I1, I2>`, `EntropyFiltered<I>`) and `DiffuseIterExt` trait.
- [x] Implement zero-allocation index scratch buffer recycling.
- [x] Add pure unit tests over integer slices (`&[u32]`) verifying $100\%$ branch coverage.

### Problem
`multi_token_phrase_anchors` and `entropy_anchors` currently live in [`crates/diffuse-core/src/token.rs`](crates/diffuse-core/src/token.rs), tightly coupled to string slices (`&str`), token classifications (`token_is_ws`), and specific punctuation literals, allocating intermediate `Vec` collections.

### Architecture & Iterator Structs
Create [`crates/diffuse-core/src/algo/landmarks.rs`](crates/diffuse-core/src/algo/landmarks.rs) and [`crates/diffuse-core/src/algo/entropy.rs`](crates/diffuse-core/src/algo/entropy.rs):

```rust
pub struct MatchInterval {
    pub a_start: usize,
    pub b_start: usize,
    pub len: usize,
}

/// Dedicated iterator struct for streaming maximal unique match landmarks.
pub struct Landmarks<I1, I2> {
    stream_a: I1,
    stream_b: I2,
    min_len: usize,
    max_len: usize,
    // internally buffered window / inverted index
}

impl<I1, I2, T> Iterator for Landmarks<I1, I2>
where
    I1: Iterator<Item = T>,
    I2: Iterator<Item = T>,
    T: std::hash::Hash + Eq + Clone,
{
    type Item = MatchInterval;

    fn next(&mut self) -> Option<Self::Item> { ... }
}

/// Dedicated iterator adapter struct for filtering items by information entropy.
pub struct EntropyFiltered<I, T> {
    iter: I,
    min_entropy_bits: f64,
    _marker: std::marker::PhantomData<T>,
}

impl<I, T> Iterator for EntropyFiltered<I, T>
where
    I: Iterator<Item = T>,
    T: std::hash::Hash + Eq + Copy,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> { ... }
}

pub trait DiffuseSequenceExt: Iterator + Sized {
    fn find_landmarks<I2: Iterator<Item = Self::Item>>(
        self,
        other: I2,
        min_len: usize,
    ) -> Landmarks<Self, I2>;

    fn filter_entropy(self, threshold_bits: f64) -> EntropyFiltered<Self, Self::Item>;
}
```

### Verification
- Test-first: Pure unit tests in `algo/landmarks.rs` and `algo/entropy.rs` using numeric streams (`0..100`).
- Validate lazy evaluation: `.take(3)` stops processing early without building full indices.

---

## 2. Phase 2 — Formal Container Balancing & Sequence Geometry

- [x] Extract Dyck Language pushdown automaton into `crates/diffuse-core/src/algo/dyck.rs`.
- [x] Create `DelimiterClassifier<T>` trait and lazy `DyckBalanced<I, C>` iterator adapter.
- [x] Decouple bracket/quote balancing from token strings using stack-allocated small-vector storage.
- [x] Verify 100% branch coverage for container boundary trimming.

### Problem
`trim_to_balanced_boundaries` in [`crates/diffuse-core/src/token.rs`](crates/diffuse-core/src/token.rs) embeds hardcoded string checks (`"("`, `"]"`, `"\""`, etc.) and allocates a dynamic `Vec<&str>` stack on every phrase candidate check.

### Architecture & Iterator Structs
Create [`crates/diffuse-core/src/algo/dyck.rs`](crates/diffuse-core/src/algo/dyck.rs):

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelimiterCategory {
    Open(u8),
    Close(u8),
    Symmetric(u8),
    Separator,
}

pub trait DelimiterClassifier<T> {
    fn classify(&self, item: &T) -> Option<DelimiterCategory>;
}

/// Dedicated iterator struct yielding balanced container ranges.
pub struct DyckBalanced<I, C> {
    iter: I,
    classifier: C,
    inline_stack: tinyvec::ArrayVec<[u8; 16]>,
}

impl<I, C, T> Iterator for DyckBalanced<I, C>
where
    I: Iterator<Item = T>,
    C: DelimiterClassifier<T>,
{
    type Item = (usize, usize);

    fn next(&mut self) -> Option<Self::Item> { ... }
}
```

### Verification
- Unit tests asserting nesting integrity across nested parentheses, brackets, braces, and quotes using synthetic test enums.
- Confirm zero heap allocations for nested containers up to depth 16.

---

## 3. Phase 3 — Gotoh Affine Gap Aligner with Zero-Allocation Workspace

- [ ] Implement generic 3-matrix Gotoh sequence alignment in `crates/diffuse-core/src/algo/affine_gap.rs`.
- [ ] Expose lazy alignment step iterator `AffineAlignmentSteps<T>`.
- [ ] Implement linear-space / banded affine gap DP with row buffer recycling.
- [ ] Parameterize alignment with generic gap opening $\alpha$, gap extension $\beta$, and match/mismatch scoring matrices.
- [ ] Verify sub-millisecond execution over long token sequences.

### Problem
Current subword and token LCS in [`crates/diffuse-core/src/lcs.rs`](crates/diffuse-core/src/lcs.rs) assigns uniform penalty to all gaps, causing token fragmentation across punctuation and identifier boundaries.

### Architecture & Iterator Structs
Create [`crates/diffuse-core/src/algo/affine_gap.rs`](crates/diffuse-core/src/algo/affine_gap.rs):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AffineGapPenalties<S> {
    pub match_score: S,
    pub mismatch_penalty: S,
    pub gap_open_penalty: S,
    pub gap_extend_penalty: S,
}

/// Streaming iterator over affine gap alignment operations.
pub struct AffineAlignmentSteps<'a, T> {
    items_a: &'a [T],
    items_b: &'a [T],
    traceback: &'a [u8],
    cursor: (usize, usize),
}

impl<'a, T: Clone> Iterator for AffineAlignmentSteps<'a, T> {
    type Item = EditOp;

    fn next(&mut self) -> Option<Self::Item> { ... }
}

pub struct AffineGapWorkspace<S> {
    m_matrix: Vec<S>,
    ix_matrix: Vec<S>,
    iy_matrix: Vec<S>,
    traceback: Vec<u8>,
}
```

### Verification
- Prove mathematically that affine gap penalties favor contiguous replacement blocks over fragmented alternating matches.
- Benchmark: alignment of 500-token lines completes in $< 0.1\text{ ms}$.

---

## 4. Phase 4 — Generic 2D Multi-Objective Block Transformation Solver

- [ ] Extract the 2D DP grid and traceback solver from `cluster/dp.rs` into `crates/diffuse-core/src/algo/block_dp.rs`.
- [ ] Define the `BlockTransitionEvaluator` trait for 1-to-1, 1-to-K, K-to-1, Insert, and Delete transitions.
- [ ] Decouple `ChangeCluster`, `Row`, `Hunk`, and AST syntax checks from the DP lattice.
- [ ] Expose `BlockTransitionsIter` for streaming row construction.
- [ ] Implement reusable matrix workspace (`BlockDpWorkspace`).

### Problem
[`crates/diffuse-core/src/cluster/dp.rs`](crates/diffuse-core/src/cluster/dp.rs) is currently 1,748 lines of code where matrix indexing, traceback decoding, Pareto score calculation, and diff domain row models are intertwined in a single monolithic implementation.

### Architecture & Iterator Structs
Create [`crates/diffuse-core/src/algo/block_dp.rs`](crates/diffuse-core/src/algo/block_dp.rs):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockTransition {
    Mod,
    Mov,
    Expand { k: usize, anchor_offset: usize },
    Collapse { k: usize, anchor_offset: usize },
    Del,
    Add,
}

pub trait BlockTransitionEvaluator {
    type ItemA;
    type ItemB;
    type Score: Copy + Ord + Default;

    fn score_mod(&self, a: &Self::ItemA, b: &Self::ItemB) -> Option<Self::Score>;
    fn score_mov(&self, a: &Self::ItemA, b: &Self::ItemB) -> Option<Self::Score>;
    fn score_expand(&self, a: &Self::ItemA, b_slice: &[Self::ItemB]) -> Option<(Self::Score, usize)>;
    fn score_collapse(&self, a_slice: &[Self::ItemA], b: &Self::ItemB) -> Option<(Self::Score, usize)>;
    fn score_del(&self, a: &Self::ItemA) -> Self::Score;
    fn score_add(&self, b: &Self::ItemB) -> Self::Score;
}

pub struct BlockTransitionsIter {
    transitions: Vec<BlockTransition>,
    cursor: usize,
}

impl Iterator for BlockTransitionsIter {
    type Item = BlockTransition;
    fn next(&mut self) -> Option<Self::Item> { ... }
}
```

### Verification
- Unit test the generic lattice with mock numeric nodes and cost functions.
- Verify that `cluster::dp::solve_cluster_dp` becomes a thin domain wrapper around `BlockAligner`, preserving exact behavior across all golden corpora.

---

## 5. Phase 5 — Bipartite Matching & Spatial Coalescence Engine

- [ ] Extract multiset matching and spatial block clustering from `moves.rs` into `crates/diffuse-core/src/algo/bipartite_matching.rs`.
- [ ] Parameterize exact matching and spatial neighborhood coalescence over generic keyed items.
- [ ] Expose `RelocationMatches<I>` streaming iterator.
- [ ] Decouple `DiffFile`, `Hunk`, and `Row` mutations from the matching algorithm.

### Problem
[`crates/diffuse-core/src/moves.rs`](crates/diffuse-core/src/moves.rs) directly iterates over hunks and row vectors while maintaining nested position tuples `(usize, usize)`, mixing hunk mutation logic with the bipartite relocation algorithm.

### Architecture & Iterator Structs
Create [`crates/diffuse-core/src/algo/bipartite_matching.rs`](crates/diffuse-core/src/algo/bipartite_matching.rs):

```rust
pub struct PositionedItem<K, P> {
    pub key: K,
    pub position: P,
}

pub struct RelocationMatch<P> {
    pub source_pos: P,
    pub target_pos: P,
}

pub trait SpatialNeighborhood<P> {
    fn is_contiguous_successor(&self, prev: &P, next: &P) -> bool;
}

/// Iterator struct yielding detected move relocations.
pub struct RelocationMatches<P> {
    matches: Vec<RelocationMatch<P>>,
    cursor: usize,
}

impl<P: Copy> Iterator for RelocationMatches<P> {
    type Item = RelocationMatch<P>;
    fn next(&mut self) -> Option<Self::Item> { ... }
}
```

### Verification
- Test-first: Pure unit tests with generic coordinate systems `(usize, usize)` and integer keys.
- Ensure all move detection behaviors in `moves::tests` pass with zero regressions.

---

## 6. Phase 6 — Monoidal Invertible Edit Tape & Generic Symbol Interning

- [ ] Generalize `AlignmentTape` in `crates/diffuse-core/src/tape.rs` to an algebraic `Tape<T>` or zero-copy slice tape.
- [ ] Expose streaming projection iterators: `tape.project_left_iter()` and `tape.project_right_iter()`.
- [ ] Generalize `TokenInterner` in `similarity.rs` to a high-performance generic `SymbolTable<T>`.
- [ ] Provide formal algebraic verification of monoidal properties (associativity, identity, left/right projections).

### Problem
`AlignmentTape` in [`crates/diffuse-core/src/tape.rs`](crates/diffuse-core/src/tape.rs) and `TokenInterner` in [`crates/diffuse-core/src/similarity.rs`](crates/diffuse-core/src/similarity.rs) are specialized for diff text strings, preventing their reuse in line-level symbol interning or generic AST node diffing.

### Architecture & Iterator Structs
Create [`crates/diffuse-core/src/algo/interner.rs`](crates/diffuse-core/src/algo/interner.rs) and [`crates/diffuse-core/src/algo/tape.rs`](crates/diffuse-core/src/algo/tape.rs):

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SymbolId(pub u32);

pub struct SymbolTable<T> {
    item_to_id: rustc_hash::FxHashMap<T, SymbolId>,
    id_to_item: Vec<T>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapeChunk<T> {
    Keep(T),
    Strike(T),
    Fill(T),
    Relocate { from_span: (usize, usize), to_span: (usize, usize), item: T },
}

pub struct Tape<T> {
    chunks: Vec<TapeChunk<T>>,
}

pub struct TapeProjection<'a, T, F> {
    chunks: &'a [TapeChunk<T>],
    cursor: usize,
    projector: F,
}

impl<'a, T, F> Iterator for TapeProjection<'a, T, F>
where
    F: Fn(&'a TapeChunk<T>) -> Option<&'a T>,
{
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> { ... }
}

impl<T: Clone> Tape<T> {
    pub fn project_left_iter(&self) -> TapeProjection<'_, T, impl Fn(&TapeChunk<T>) -> Option<&T>>;
    pub fn project_right_iter(&self) -> TapeProjection<'_, T, impl Fn(&TapeChunk<T>) -> Option<&T>>;
}
```

### Verification
- QuickCheck / property-based unit tests verifying monoidal invariants:
  - Identity: $T \circ \emptyset = T = \emptyset \circ T$
  - Associativity: $(A \circ B) \circ C = A \circ (B \circ C)$
  - Involution: $\text{invert}(\text{invert}(T)) = T$

---

## 7. Phase 7 — Lean Pipeline Integration & Verification Gate

- [ ] Connect diffuse pipeline stages to the decoupled algorithmic engines in `crates/diffuse-core/src/algo/`.
- [ ] Verify that diff pipeline modules become lean orchestrators:
  $$\text{Parse} \longrightarrow \text{Intern} \longrightarrow \text{Algorithmic Optimization} \longrightarrow \text{Wire/Render}$$
- [ ] Run full golden acceptance corpus (`cargo test --test golden`).
- [ ] Run full repository verification suite (`./scripts/check.sh`).
- [ ] Measure and assert test latency budgets (< 0.5s per test, < 1.5s per test binary).

### Verification Gates
1. `cargo test --lib -p diffuse-core`: 100% passing.
2. `cargo test --test golden`: 100% byte-for-byte fidelity across all 8 pinned corpora.
3. `cargo +nightly clippy --all-targets --all-features -- -D warnings`: Clean.
4. `cargo +nightly fmt --all -- --check`: Clean.
5. Latency validation: Sub-second total test runtime on warm cache.
