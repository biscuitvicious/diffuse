# diffuse — Data Structures Reference

This document provides a comprehensive, rigorous reference of the foundational and specialized data structures implemented across the `diffuse` codebase. Each entry details:
1. **Academic Foundation & Canonical Reference**: Formal citation or structural derivation.
2. **Location in Codebase**: Exact crate and module path.
3. **Problem & Rationale ("Why")**: The memory layout, cache locality, and computational constraints.
4. **Concrete Diff Pipeline Role ("For What")**: How the structure participates in diffing, alignment, and rendering.
5. **Space & Access Complexity**: Memory bounds and lookup/mutation costs.

---

## Table of Contents

1. [Sparse & Banded Profile Matrices](#1-sparse--banded-profile-matrices)
   - [Skyline Profile Matrix (`SkylineMatrix<T>`)](#11-skyline-profile-matrix-skylinematrixt)
   - [Adaptive Wavefront Corridor Envelope (`AdaptiveEnvelope`)](#12-adaptive-wavefront-corridor-envelope-adaptiveenvelope)
2. [Scratch Buffers & Thread-Local Allocation Arenas](#2-scratch-buffers--thread-local-allocation-arenas)
   - [DP Scratch Arena (`DpScratchArena`)](#21-dp-scratch-arena-dpscratcharena)
   - [Affine Gap Recycled Stripe Workspace (`AffineGapWorkspace`)](#22-affine-gap-recycled-stripe-workspace-affinegapworkspace)
3. [Automata, Stacks & Formal Language Trackers](#3-automata-stacks--formal-language-trackers)
   - [Dyck Language Delimiter Stack Automaton (`DyckMatcher`)](#31-dyck-language-delimiter-stack-automaton-dyckmatcher)
   - [Aho–Corasick State Transition Trie & Failure Table (`PrefixDfa`)](#32-ahocorasick-state-transition-trie--failure-table-prefixdfa)
4. [Lexical Pools & Symbol Interning](#4-lexical-pools--symbol-interning)
   - [Hash-Consed Lexical Interner (`TokenInterner`)](#41-hash-consed-lexical-interner-tokeninterner)
   - [Landmark Inverted $N$-Gram Index](#42-landmark-inverted-n-gram-index)
5. [Algebraic & Action Logs](#5-algebraic--action-logs)
   - [Invertible Monoidal Edit Tape (`EditTape`)](#51-invertible-monoidal-edit-tape-edittape)
6. [Graph & Spatial Relocation Structures](#6-graph--spatial-relocation-structures)
   - [Bipartite Spatial Coalescence Graph & Cost Matrix](#61-bipartite-spatial-coalescence-graph--cost-matrix)
7. [Repository Review & Coordinate Indexes](#7-repository-review--coordinate-indexes)
   - [Coordinate-Indexed Notes Database (`NotesDB`)](#71-coordinate-indexed-notes-database-notesdb)
8. [Future Architectural Opportunities](#8-future-architectural-opportunities)
   - [Static Augmented Interval Tree (AST & Note Stabbing Queries)](#81-static-augmented-interval-tree-ast--note-stabbing-queries)
   - [Succinct Suffix Automaton / Directed Acyclic Word Graph (DAWG)](#82-succinct-suffix-automaton--directed-acyclic-word-graph-dawg)
   - [Roaring Bitmaps for Token Set Operations](#83-roaring-bitmaps-for-token-set-operations)
   - [Chunked Arena String Interner (`Symbol32`)](#84-chunked-arena-string-interner-symbol32)

---

## 1. Sparse & Banded Profile Matrices

### 1.1 Skyline Profile Matrix (`SkylineMatrix<T>`)

- **Canonical Reference**:
  Alan Jennings (1966). *"A Compact Storage Scheme for Simultaneous Linear Equations"*. The Computer Journal, 9(3), 281–285.  
  [Oxford Academic DOI:10.1093/comjnl/9.3.281](https://doi.org/10.1093/comjnl/9.3.281)
- **Location**:
  [`crates/diffuse-core/src/algo/skyline.rs`](../crates/diffuse-core/src/algo/skyline.rs) (`SkylineMatrix<T>`, `SkylineRow`)
- **Why It Is Used**:
  Dynamic programming alignment over large hunks ($M, N \approx 7,000$ lines) requires evaluating cells near an optimal path. A full dense matrix requires $M \times N \approx 50,000,000$ cells, incurring tens of megabytes of memory allocations and catastrophic CPU L3 cache thrashing. General-purpose sparse matrices (Compressed Sparse Row / CSR or Dictionary of Keys / DOK) incur expensive $O(N)$ row shifts on sequential writes or pointer indirection. The Skyline (envelope profile) scheme stores only the active column interval $[j_{\min}(i) \dots j_{\max}(i)]$ for each row $i$ packed contiguously in a single flat contiguous `Vec<T>`.
- **For What It Is Used**:
  Serves as the grid container for `solve_cluster_with_arena` in `diffuse-core::cluster::dp`, supporting fast cell writes and backward traceback while consuming only the cell volume bounded by the adaptive corridor.
- **Complexity**:
  - Memory: $O(\sum_i (j_{\max}(i) - j_{\min}(i) + 1)) = O(K \cdot M)$ cells, where $K \ll N$.
  - Random Access (`get`, `get_mut`): $O(1)$ time via base row offset calculation: `offset + (j - start_j)`.
  - Row Slices (`row`, `row_mut`): Direct contiguous memory slice access (`&[T]`, `&mut [T]`).

---

### 1.2 Adaptive Wavefront Corridor Envelope (`AdaptiveEnvelope`)

- **Canonical Reference**:
  Santiago Marco-Sola et al. (2021). *"Fast Gap-Affine Alignment of Long Reads Using the Wavefront Alignment Algorithm"*. Bioinformatics, 37(4), 456–463.  
  [Oxford Academic DOI:10.1093/bioinformatics/btaa777](https://doi.org/10.1093/bioinformatics/btaa777)  
  Kun-Mao Chao, William R. Pearson, and Michael W. Miller (1992). *"Aligning Two Sequences Within a Specified Dynamic Programming Corridor"*. SAGMB, 4(1), 1–18.
- **Location**:
  [`crates/diffuse-core/src/algo/skyline.rs`](../crates/diffuse-core/src/algo/skyline.rs) (`AdaptiveEnvelope`, `AdaptiveEnvelopeBuilder`)
- **Why It Is Used**:
  Code hunk modifications are frequently asymmetric (e.g. inserting or deleting 50 lines within a 200-line hunk). A fixed diagonal band centered on $i = j$ drifts away from the true alignment corridor, causing downstream aligned lines to be clipped outside the matrix. The `AdaptiveEnvelope` calculates an asymmetric corridor tracking the true affine diagonal $j = i \cdot \frac{N}{M}$ with bandwidth $K = \max(K_{\min}, |M - N| + K_{\text{slack}})$, with safe cell budget clamping.
- **For What It Is Used**:
  Constructs the row boundary definitions (`SkylineRow`) passed to `SkylineMatrix::new_with_envelope` before executing the Block-DP recurrence.
- **Complexity**:
  - Construction Time: $O(M)$ time to compute row bounds.
  - Space: $O(M)$ metadata storage for row descriptors.

---

## 2. Scratch Buffers & Thread-Local Allocation Arenas

### 2.1 DP Scratch Arena (`DpScratchArena`)

- **Location**:
  [`crates/diffuse-core/src/cluster/dp.rs`](../crates/diffuse-core/src/cluster/dp.rs) (`DpScratchArena`, `solve_cluster_with_arena`)
- **Why It Is Used**:
  Diff processing executes across thousands of hunk clusters per second across worker threads. Repeatedly allocating and deallocating dynamic programming vectors, token match maps, and line metadata causes severe memory fragmentation and lock contention in the heap allocator (`glibc malloc` / `jemalloc`).
- **For What It Is Used**:
  Maintains thread-local, recycled buffers (`SkylineMatrix<Cell>`, `Vec<usize>`, `Vec<RowMetaData>`) across successive cluster solves. Reuses allocated capacity via `clear()` without reallocating underlying heap buffers.
- **Complexity**:
  - Amortized Allocation Cost: $O(1)$ per cluster after initial high-water mark sizing.
  - Memory: Bounded by the single largest cluster encountered by each thread.

---

### 2.2 Affine Gap Recycled Stripe Workspace (`AffineGapWorkspace`)

- **Canonical Reference**:
  Osamu Gotoh (1982). *"An Improved Algorithm for Matching Biological Sequences"*. Journal of Molecular Biology, 162(3), 705–708.
- **Location**:
  [`crates/diffuse-core/src/algo/affine_gap.rs`](../crates/diffuse-core/src/algo/affine_gap.rs) (`AffineGapWorkspace`, `align_affine`)
- **Why It Is Used**:
  Sub-line Gotoh alignment computes three concurrent recurrence planes: match/mismatch ($M$), deletion gap ($I_x$), and insertion gap ($I_y$). When evaluating token sequences of length $N, M$, maintaining full 2D matrices requires $3 \times N \times M \times 8$ bytes. The recycled stripe workspace retains only the current and previous row vectors, recycling memory across token comparisons.
- **For What It Is Used**:
  Computes sub-line token edit scripts without allocating temporary vectors per modified line pair.
- **Complexity**:
  - Space: $O(\min(N, M))$ working memory.
  - Allocation: Zero heap allocations during repeated invocations on the same thread.

---

## 3. Automata, Stacks & Formal Language Trackers

### 3.1 Dyck Language Delimiter Stack Automaton (`DyckMatcher`)

- **Canonical Reference**:
  Walther von Dyck (1882). *"Gruppentheoretische Studien"*. Mathematische Annalen, 20(1), 1–44.  
  John E. Hopcroft and Jeffrey D. Ullman (1979). *"Introduction to Automata Theory, Languages, and Computation"*. Addison-Wesley.
- **Location**:
  [`crates/diffuse-core/src/algo/dyck.rs`](../crates/diffuse-core/src/algo/dyck.rs) (`DyckMatcher`, `DelimiterFrame`, `NestingDepth`)
- **Why It Is Used**:
  Programming languages represent balanced Dyck-1/Dyck-N languages over nested delimiters: `()`, `[]`, `{}`. Unbalanced delimiters across hunk boundaries produce syntactic fractures. A pushdown stack automaton tracks open delimiter frames, line-origin coordinates, and nesting depth to ensure diff hunk expansions maintain delimiter integrity.
- **For What It Is Used**:
  Validates syntax boundaries, assists hunk shifting, and supplies opening/closing brace proximity bonuses in the Block-DP scoring function.
- **Complexity**:
  - Time: $O(N)$ linear single-pass scan over tokens or characters.
  - Space: $O(D)$ where $D$ is the maximum nesting depth ($D \le N$).

---

### 3.2 Aho–Corasick State Transition Trie & Failure Table (`PrefixDfa`)

- **Canonical Reference**:
  Alfred V. Aho and Margaret J. Corasick (1975). *"Efficient String Matching: An Aid to Bibliographic Search"*. Communications of the ACM, 18(6), 333–340.  
  [ACM Digital Library](https://doi.org/10.1145/360825.360855)
- **Location**:
  [`crates/diffuse-core/src/algo/prefix_dfa.rs`](../crates/diffuse-core/src/algo/prefix_dfa.rs) (`PrefixDfa`)
- **Why It Is Used**:
  Detecting comment markers, heredoc delimiters, directive prefixes, or statement terminators across hundreds of language profiles requires matching multiple keywords simultaneously. Naive substring scanning takes $O(P \cdot N)$ where $P$ is the number of keyword patterns. The Aho–Corasick automaton compiles keyword dictionaries into a deterministic trie with failure links.
- **For What It Is Used**:
  Dispatches lexical classifications, line-type identification, and comment stripping in linear time regardless of the number of target keywords.
- **Complexity**:
  - Construction Time: $O(\sum_{p \in P} |p|)$.
  - Scan Time: $O(N)$ strictly linear in the input text length.
  - Space: $O(S \cdot |\Sigma|)$ where $S$ is the total number of trie states.

---

## 4. Lexical Pools & Symbol Interning

### 4.1 Hash-Consed Lexical Interner (`TokenInterner`)

- **Canonical Reference**:
  Andrei P. Ershov (1958). *"On Programming of Arithmetic Operations"*. Communications of the ACM, 1(8), 3–6.  
  [ACM Digital Library](https://doi.org/10.1145/368892.368907)
- **Location**:
  [`crates/diffuse-core/src/cluster/similarity.rs`](../crates/diffuse-core/src/cluster/similarity.rs) (`TokenInterner`, `TokenId`)
- **Why It Is Used**:
  Comparing code lines in dynamic programming requires computing token set overlaps and character-level LCS across thousands of line pairs. Storing token strings repeatedly produces exorbitant heap allocations and expensive string equality comparisons. The `TokenInterner` hash-conses every unique token string into a dense 32-bit `TokenId`, precomputing whitespace flags, alphanumeric classifications, and character counts.
- **For What It Is Used**:
  Transforms string slice comparisons into single-instruction integer comparisons (`id1 == id2`), accelerating line-similarity and token-LCS scoring by up to $10\times$.
- **Complexity**:
  - Lookup/Intern: $O(L)$ where $L$ is token length (using `FxHasher`).
  - Comparison: $O(1)$ 32-bit integer equality.
  - Space: $O(U)$ where $U$ is the number of unique lexical tokens in the cluster.

---

### 4.2 Landmark Inverted $N$-Gram Index

- **Location**:
  [`crates/diffuse-core/src/algo/landmarks.rs`](../crates/diffuse-core/src/algo/landmarks.rs)
- **Why It Is Used**:
  Detecting Maximal Unique Matches (MUMs) between pre-image and post-image files requires identifying matching phrase anchors that occur exactly once in both documents.
- **For What It Is Used**:
  Builds inverted index hash maps from $N$-gram hashes to line coordinates. Coordinates with posting frequency equal to 1 in both sequences form unique anchor pairs around which diff alignments are anchored.
- **Complexity**:
  - Indexing Time: $O(N + M)$ linear passes over sequences.
  - Space: $O(N + M)$ hash table storage for k-mer postings lists.

---

## 5. Algebraic & Action Logs

### 5.1 Invertible Monoidal Edit Tape (`EditTape`)

- **Canonical Reference**:
  Saunders Mac Lane (1971). *"Categories for the Working Mathematician"*. Springer-Verlag.
- **Location**:
  [`crates/diffuse-core/src/algo/tape.rs`](../crates/diffuse-core/src/algo/tape.rs) (`EditTape`, `EditOp`)
- **Why It Is Used**:
  Diff operations (`Keep`, `Insert`, `Delete`, `Replace`) form a free monoid under sequence concatenation with identity element $\varepsilon$ (empty tape). Maintaining edits as an algebraic tape allows compositional transformations, slicing, inversion ($T^{-1}$), and replay without reparsing text.
- **For What It Is Used**:
  Represents intermediate edit transformations during sequence alignment, sub-token diffing, and wire protocol emission.
- **Complexity**:
  - Composition: $O(A + B)$ contiguous append.
  - Inversion: $O(L)$ linear-time single pass reversing operation roles.
  - Space: $O(L)$ where $L$ is the number of atomic edit operations.

---

## 6. Graph & Spatial Relocation Structures

### 6.1 Bipartite Spatial Coalescence Graph & Cost Matrix

- **Canonical Reference**:
  Harold W. Kuhn (1955). *"The Hungarian Method for the Assignment Problem"*. Naval Research Logistics Quarterly, 2(1–2), 83–97.
- **Location**:
  [`crates/diffuse-core/src/algo/bipartite_matching.rs`](../crates/diffuse-core/src/algo/bipartite_matching.rs)
- **Why It Is Used**:
  Identifying moved code blocks requires pairing deleted line clusters with inserted line clusters across disparate regions of a file or commit. Greedy matching easily locks onto false positives (e.g. repeated boilerplate functions).
- **For What It Is Used**:
  Encodes similarity scores, spatial continuity penalties, and displacement metrics into an edge-weighted bipartite graph, solving for optimal relocation pairings while penalizing fragmented split relocations.
- **Complexity**:
  - Matrix Construction: $O(|D| \cdot |A|)$ where $|D|, |A|$ are deleted and added candidate blocks.
  - Matching: $O(V^2 E)$ max-flow / min-cost assignment.

---

## 7. Repository Review & Coordinate Indexes

### 7.1 Coordinate-Indexed Notes Database (`NotesDB`)

- **Location**:
  [`crates/diffuse-core/src/notes.rs`](../crates/diffuse-core/src/notes.rs) (`NotesDB`, `ReviewNote`, `Side`)
- **Why It Is Used**:
  Interactive code reviews associate notes and inline comments with specific repository file paths, diff sides (`Old` vs `New`), and line numbers. During hunk rendering and cursor navigation, the TUI must query the presence of comments for screen rows in real time.
- **For What It Is Used**:
  Provides serialized and queryable review comments across repositories, persisting notes in `.diffuse/notes.json` and exposing fast coordinate lookup.
- **Complexity**:
  - Query by File & Coordinate: $O(C)$ where $C$ is the count of notes on that file.
  - Persistence: JSON serialization over atom-locked storage files.

---

## 8. Future Architectural Opportunities

The following high-leverage data structures represent identified opportunities to further optimize memory consumption, cache locality, and algorithmic throughput across `diffuse`:

### 8.1 Static Augmented Interval Tree (AST & Note Stabbing Queries)

- **Target Scenario**:
  During syntax-aware token diffing and TUI viewport rendering, the engine frequently performs point queries (e.g. "which AST syntax scopes or user review notes cover line $L$ or byte offset $O$?"). Currently, this scans vector slices or runs binary searches across start offsets.
- **Proposed Data Structure**:
  A **Static Augmented Interval Tree** (or flat array-backed Centered Interval Tree / Fenwick tree).
- **Projected Advantage**:
  Reduces interval stabbing queries from $O(K)$ linear scan to strictly $O(\log K + M)$ time (where $K$ is the number of intervals and $M$ is the number of overlapping matches). For deeply nested syntax trees (50+ AST nodes covering an expression), this yields zero-allocation, cache-friendly lookups during high-frequency interactive cursor movement.

---

### 8.2 Succinct Suffix Automaton / Directed Acyclic Word Graph (DAWG)

- **Target Scenario**:
  Maximal Unique Match (MUM) detection and moved block relocation across massive files (e.g. 50,000-line generated files or data dumps). The current inverted $N$-gram table allocates hash table entries and posting vectors for every $k$-mer.
- **Proposed Data Structure**:
  A **Succinct Suffix Automaton (DAWG)** or **Compressed Suffix Array (CSA) / FM-Index** utilizing bitvectors with $O(1)$ Rank/Select primitives.
- **Projected Advantage**:
  A Suffix Automaton represents all distinct substrings of a sequence of length $N$ using at most $2N - 1$ states and $3N - 4$ transitions. This shrinks memory consumption by $4\times$ to $8\times$ compared to $N$-gram hash tables, while enabling substring lookup in strictly $O(L)$ time where $L$ is query length, independent of file size.

---

### 8.3 Roaring Bitmaps for Token Set Operations

- **Target Scenario**:
  Line-level token set presence checks, vocabulary uniqueness filters, and relocation candidate filtering currently use `rustc_hash::FxHashSet<u32>` or `Vec<bool>`.
- **Proposed Data Structure**:
  **Roaring Bitmaps** (compressed bitsets dividing integer space into 65,536-integer chunks, dynamically selecting between uncompressed bitsets, sorted 16-bit integer arrays, and run-length encoded runs).
- **Projected Advantage**:
  Permits bitwise set intersections (`AND`), unions (`OR`), and differences (`ANDNOT`) to execute using SIMD CPU instructions (AVX-512 / AVX2 / NEON) across cache lines without pointer indirection, speeding up large-scale candidate pruning.

---

### 8.4 Chunked Arena String Interner (`Symbol32`)

- **Target Scenario**:
  AST nodes, lexical tokens, and file path representations currently allocate individual `String`s or `Box<str>` in pipeline stages. In Rust on 64-bit systems, `String` takes 24 bytes (pointer, capacity, length) on the stack, plus heap allocator header overhead.
- **Proposed Data Structure**:
  A **Chunked Arena Interner** (similar to `lasso` or `rodeo`), returning 4-byte `NonZeroU32` handles (`Symbol`).
- **Projected Advantage**:
  Reduces token descriptor sizes from 24 bytes to 4 bytes ($6\times$ reduction in stack footprint). Maximizes CPU L1/L2 cache occupancy during dynamic programming alignments and syntax token traversals, completely eliminating heap allocation pauses during tokenization.
