# diffuse — Algorithms & Data Structures Reference

This document provides a comprehensive, rigorous reference of the algorithms, data structures, mathematical models, and formal systems implemented across the `diffuse` codebase. Each entry details:
1. **Academic Foundation & Original Paper**: Formal citation with links to the paper or canonical specification.
2. **Location in Codebase**: Exact crate and module path.
3. **Problem & Rationale ("Why")**: The design challenge and mathematical necessity.
4. **Concrete Diff Pipeline Role ("For What")**: How it transforms tokens, lines, hunks, or terminals.
5. **Complexity Profile**: Asymptotic time and space complexity.

---

## Table of Contents

1. [Sequence Differencing & Minimal Edit Scripts](#1-sequence-differencing--minimal-edit-scripts)
   - [Myers' $O(ND)$ Difference Algorithm](#11-myers-ond-difference-algorithm)
   - [Hirschberg's Linear-Space LCS Algorithm](#12-hirschbergs-linear-space-lcs-algorithm)
   - [Hunt–Szymanski Fast LCS Algorithm](#13-huntszymanski-fast-lcs-algorithm)
   - [Bit-Parallel Dynamic Programming (Myers–Hyyrö Bit-Vector)](#14-bit-parallel-dynamic-programming-myershyyrö-bit-vector)
   - [Banded / Corridor-Bounded Dynamic Programming](#15-banded--corridor-bounded-dynamic-programming)
2. [Sequence Alignment & Anti-Fragmentation](#2-sequence-alignment--anti-fragmentation)
   - [Gotoh's Affine Gap Cost Dynamic Programming](#21-gotohs-affine-gap-cost-dynamic-programming)
3. [Landmark Indexing, Suffix Search & Phrase Anchoring](#3-landmark-indexing-suffix-search--phrase-anchoring)
   - [Maximal Unique Matches (MUMs) & $N$-Gram Phrase Anchoring](#31-maximal-unique-matches-mums--n-gram-phrase-anchoring)
   - [Aho–Corasick Trie & Deterministic Finite Automata (Prefix DFA)](#32-ahocorasick-trie--deterministic-finite-automata-prefix-dfa)
4. [Formal Languages, Containers & Delimiter Balancing](#4-formal-languages-containers--delimiter-balancing)
   - [Dyck Language Delimiter Balancer & Stack Automaton](#41-dyck-language-delimiter-balancer--stack-automaton)
5. [Lattice Optimization & Multi-Objective Dynamic Programming](#5-lattice-optimization--multi-objective-dynamic-programming)
   - [2D Multi-Objective Block Transformation Lattice (Cluster DP)](#51-2d-multi-objective-block-transformation-lattice-cluster-dp)
6. [Information Theory & Statistical Weighting](#6-information-theory--statistical-weighting)
   - [Shannon Information Entropy & Term Specificity (TF-IDF Weighting)](#61-shannon-information-entropy--term-specificity-tf-idf-weighting)
7. [Similarity Metrics, Assignment & Relocation](#7-similarity-metrics-assignment--relocation)
   - [Jaccard Metric & Normalized Levenshtein Token Ratio](#71-jaccard-metric--normalized-levenshtein-token-ratio)
   - [Bipartite Matching & Spatial Coalescence (Move Detection)](#72-bipartite-matching--spatial-coalescence-move-detection)
   - [Semantic Boundary Disambiguation (Hunk Shifting)](#73-semantic-boundary-disambiguation-hunk-shifting)
8. [Algebraic Systems & Memory Structures](#8-algebraic-systems--memory-structures)
   - [Invertible Monoidal Edit Tape Algebra](#81-invertible-monoidal-edit-tape-algebra)
   - [Hash-Consing & Symbol Interning](#82-hash-consing--symbol-interning)
   - [Robin Hood & Cache-Optimized Hash Tables (FxHash)](#83-robin-hood--cache-optimized-hash-tables-fxhash)
9. [Perception, Accessibility & Color Systems](#9-perception-accessibility--color-systems)
   - [Oklab Perceptually Uniform Color Space](#91-oklab-perceptually-uniform-color-space)
   - [WCAG 2.1 Relative Luminance & Contrast Ratio Formulation](#92-wcag-21-relative-luminance--contrast-ratio-formulation)
10. [Parsing, Syntax & Verification](#10-parsing-syntax--verification)
    - [Generalized LR (GLR) Incremental Parsing (Tree-sitter)](#101-generalized-lr-glr-incremental-parsing-tree-sitter)
    - [Design by Contract (DbC) & Hoare Logic Invariant Enforcement](#102-design-by-contract-dbc--hoare-logic-invariant-enforcement)

---

## 1. Sequence Differencing & Minimal Edit Scripts

### 1.1 Myers' $O(ND)$ Difference Algorithm

- **Paper / Canonical Reference**:
  Eugene W. Myers (1986). *"An $O(ND)$ Difference Algorithm and Its Variations"*. Algorithmica, 1(2), 251–266.  
  [Paper PDF](http://www.xmailserver.org/diff2.pdf) · [Springer DOI:10.1007/BF01840446](https://doi.org/10.1007/BF01840446)
- **Location**:
  [`crates/diffuse-core/src/algo/myers_ond.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/myers_ond.rs)
- **Why It Is Used**:
  Computes the shortest edit script (SES) between two sequences with runtime proportional to the number of differences $D$ rather than sequence length $N$. In typical code diffs where most lines or tokens are unchanged ($D \ll N$), Myers runs in near-linear time ($O(N + D^2)$) instead of standard quadratic dynamic programming ($O(N^2)$).
- **For What It Is Used**:
  Serves as a primary diff engine implementation for the generic `DiffEngine` trait, generating elementary edit operations (`Keep`, `Insert`, `Delete`) across arbitrary item sequences.
- **Complexity**:
  - Time: $O((N + M) D)$, where $N, M$ are sequence lengths and $D$ is the size of the minimal edit script.
  - Space: $O(D^2)$ standard, or $O(N + M)$ with divide-and-conquer linear-space variant.

---

### 1.2 Hirschberg's Linear-Space LCS Algorithm

- **Paper / Canonical Reference**:
  Daniel S. Hirschberg (1975). *"A Linear Space Algorithm for Computing Maximal Common Subsequences"*. Communications of the ACM, 18(6), 341–343.  
  [ACM Digital Library](https://doi.org/10.1145/360825.360861) · [CMU Paper PDF](https://www.cs.cmu.edu/~ckingsf/bio/resources/hirschberg-cacm.pdf)
- **Location**:
  [`crates/diffuse-core/src/algo/hirschberg.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/hirschberg.rs)
- **Why It Is Used**:
  Standard Wagner–Fischer dynamic programming requires an $O(N \cdot M)$ matrix, which causes prohibitive memory allocations and cache misses when diffing large files (e.g. 50,000 lines or tokens). Hirschberg combines forward and backward 1D dynamic programming sweeps with recursive divide-and-conquer to determine the midpoint crossover, slashing memory consumption to strictly $O(\min(N, M))$.
- **For What It Is Used**:
  Provides a linear-memory LCS engine (`HirschbergLcs`) for large sequences where allocating a full 2D scoring matrix would exceed memory budgets.
- **Complexity**:
  - Time: $O(N \cdot M)$
  - Space: $O(\min(N, M))$

---

### 1.3 Hunt–Szymanski Fast LCS Algorithm

- **Paper / Canonical Reference**:
  James W. Hunt and Thomas G. Szymanski (1977). *"A Fast Algorithm for Computing Longest Common Subsequences"*. Communications of the ACM, 20(5), 350–353.  
  [ACM Digital Library](https://doi.org/10.1145/359581.359603) · [Dartmouth Paper PDF](https://www.cs.dartmouth.edu/~doug/hunt.pdf)
- **Location**:
  [`crates/diffuse-core/src/algo/hunt_szymanski.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/hunt_szymanski.rs)
- **Why It Is Used**:
  When the sequence alphabet is large and identical matches between the two sequences are sparse (low match count $r$), standard dynamic programming does wasteful work on non-matching cells. Hunt–Szymanski indexes identical pairs using an inverted index and computes the LCS via Patience-sorting / Longest Increasing Subsequence (LIS) with binary search in $O((r + N) \log N)$ time.
- **For What It Is Used**:
  Implements `HuntSzymanskiLcs`, offering sub-quadratic performance on token sequences with high vocabulary diversity.
- **Complexity**:
  - Time: $O((r + N) \log N)$, where $r$ is the number of matching pairs ($r \le N \cdot M$).
  - Space: $O(r + N)$

---

### 1.4 Bit-Parallel Dynamic Programming (Myers–Hyyrö Bit-Vector)

- **Paper / Canonical Reference**:
  Gene Myers (1999). *"A Fast Bit-Vector Algorithm for Approximate String Matching Based on Dynamic Programming"*. Journal of the ACM, 46(3), 395–415.  
  [ACM Digital Library](https://doi.org/10.1145/316542.316550)  
  Heikki Hyyrö (2004). *"Bit-Parallel Computation of Local Similarity Scores"*. In: Combinatorial Pattern Matching (CPM 2004), LNCS 3109, 211–225.  
  [Springer DOI:10.1007/978-3-540-27801-6_16](https://doi.org/10.1007/978-3-540-27801-6_16)
- **Location**:
  [`crates/diffuse-core/src/algo/bit_parallel.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/bit_parallel.rs)
- **Why It Is Used**:
  Calculates the exact LCS using bitwise CPU word operations (AND, OR, XOR, shifts, additions). By packing 64 dynamic programming matrix cells into a single 64-bit integer (`u64`), the algorithm processes DP diagonals in parallel, achieving a $w = 64\times$ speedup without loss of precision.
- **For What It Is Used**:
  Provides `BitParallelLcs` for ultra-fast LCS computation on line-level and token-level diffs whose lengths fit within CPU register word widths (or small arrays of words).
- **Complexity**:
  - Time: $O(\lceil M/w \rceil \cdot N)$, where $w = 64$.
  - Space: $O(\lceil M/w \rceil \cdot |\Sigma| + \lceil M/w \rceil \cdot N)$ for traceback.

---

### 1.5 Banded / Corridor-Bounded Dynamic Programming

- **Paper / Canonical Reference**:
  Esko Ukkonen (1985). *"Algorithms for Approximate String Matching"*. Information and Control, 64(1–3), 100–118.  
  [ScienceDirect DOI:10.1016/S0019-9958(85)80046-2](https://doi.org/10.1016/S0019-9958(85)80046-2)
- **Location**:
  [`crates/diffuse-core/src/algo/bounded_dp.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/bounded_dp.rs)
- **Why It Is Used**:
  When two sequences are known to be similar or when changes are localized, exploring cells far from the matrix main diagonal ($|i - j| > k$) is mathematically guaranteed to yield suboptimal paths. Banded DP restricts evaluation strictly to a corridor of width $2k + 1$ centered on the diagonal.
- **For What It Is Used**:
  Provides `BoundedDpLcs`, delivering guaranteed $O(k \cdot N)$ runtime for lines and hunks where divergence is constrained within a maximum displacement parameter $k$.
- **Complexity**:
  - Time: $O(k \cdot \min(N, M))$
  - Space: $O(k \cdot \min(N, M))$

---

## 2. Sequence Alignment & Anti-Fragmentation

### 2.1 Gotoh's Affine Gap Cost Dynamic Programming

- **Paper / Canonical Reference**:
  Osamu Gotoh (1982). *"An Improved Algorithm for Matching Biological Sequences"*. Journal of Molecular Biology, 162(3), 705–708.  
  [ScienceDirect DOI:10.1016/0022-2836(82)90398-9](https://doi.org/10.1016/0022-2836(82)90398-9) · [Paper PDF](https://www.cs.cmu.edu/~ckingsf/bio/resources/gotoh-gap-penalty.pdf)
- **Location**:
  Architecture plan: [`plan/Principled Data-Plane Alignment & Generalized Anti-Fragmentation.md`](file:///home/daniel/lab/diffuse/plan/Principled%20Data-Plane%20Alignment%20&%20Generalized%20Anti-Fragmentation.md) (Phase 2); replacing unweighted LCS in [`crates/diffuse-core/src/lcs.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/lcs.rs).
- **Why It Is Used**:
  Standard LCS assigns uniform cost to every gap character or token ($c(\text{gap}) = k \cdot g$). This causes anti-fragmentation artifacts where contiguous deletions/insertions are broken up to preserve isolated, coincidental punctuation or single characters (e.g. aligning underscores across unrelated identifiers). Gotoh's algorithm introduces affine gap penalties:
  $$\text{Cost}(\text{gap of length } k) = \alpha + \beta \cdot k \quad (\text{where } \alpha > \beta)$$
  Opening a gap pays a high initial penalty $\alpha$, while extending an existing gap pays a smaller incremental penalty $\beta$. This mathematically favors contiguous replacement blocks over scattered multi-run token shredding.
- **For What It Is Used**:
  Core alignment engine for subword and token-level diffing, ensuring replaced expressions, method calls, and paths remain visually cohesive rather than fragmented into alternating micro-common spans.
- **Complexity**:
  - Time: $O(N \cdot M)$ using 3 recurrence matrices ($M$, $I_x$, $I_y$).
  - Space: $O(N \cdot M)$ or $O(\min(N, M))$ with linear-space row recycling.

---

## 3. Landmark Indexing, Suffix Search & Phrase Anchoring

### 3.1 Maximal Unique Matches (MUMs) & $N$-Gram Phrase Anchoring

- **Paper / Canonical Reference**:
  Arthur L. Delcher, Simon Kasif, Robert D. Fleischmann, Jeremy Peterson, Owen White, Steven L. Salzberg (1999). *"Alignment of Whole Genomes"*. Nucleic Acids Research, 27(11), 2369–2376.  
  [Nucleic Acids Research](https://doi.org/10.1093/nar/27.11.2369) · [NCBI PMC148464](https://www.ncbi.nlm.nih.gov/pmc/articles/PMC148464/)  
  Richard M. Karp, Raymond E. Miller, Arnold L. Rosenberg (1972). *"Rapid Identification of Repeated Patterns in Strings, Trees and Arrays"*. STOC '72, 125–136.  
  [ACM Digital Library](https://doi.org/10.1145/800152.804905)
- **Location**:
  [`crates/diffuse-core/src/algo/landmarks.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/landmarks.rs) (`find_mums`, `find_monotonic_chain`, `find_monotonic_mums`, `Landmarks`, `DiffuseSequenceExt`); consumed in [`crates/diffuse-core/src/token.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/token.rs) (`multi_token_phrase_anchors`, `entropy_anchors`).
- **Why It Is Used**:
  Micro-token LCS algorithms easily get trapped in local combinatorial optima (e.g. matching single quotes, dots, or common keywords in reversed or misaligned positions). In contrast, multi-token phrases that occur *exactly once* in both the old and new text (Maximal Unique Matches) provide unambiguous, mathematically provable macro-anchors.
- **For What It Is Used**:
  Forms Phase 1 of diffuse's sequence alignment: extracts high-entropy $n$-gram phrases ($\ge 2$ non-whitespace tokens) that are unique on both sides, balances their structural delimiters, and locks them as fixed landmarks before fine-grained subword LCS runs in between.
- **Complexity**:
  - Time: $O(N \cdot K)$, where $K$ is maximum phrase window size ($K \le 8$), or $O(N)$ with generalized suffix trees.
  - Space: $O(N)$ inverted hash index.

---

### 3.2 Aho–Corasick Trie & Deterministic Finite Automata (Prefix DFA)

- **Paper / Canonical Reference**:
  Alfred V. Aho and Margaret J. Corasick (1975). *"Efficient String Matching: An Aid to Bibliographic Search"*. Communications of the ACM, 18(6), 333–340.  
  [ACM Digital Library](https://doi.org/10.1145/360825.360855) · [Paper PDF](https://cr.yp.to/bib/1975/aho.pdf)  
  Edward Fredkin (1960). *"Trie Memory"*. Communications of the ACM, 3(9), 490–499.  
  [ACM Digital Library](https://doi.org/10.1145/367390.367400)
- **Location**:
  [`crates/diffuse-core/src/algo/prefix_dfa.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/prefix_dfa.rs)
- **Why It Is Used**:
  Testing line prefixes against hundreds of language profile declaration keywords (`fn`, `pub fn`, `async fn`, `class`, `interface`, etc.) naively causes $O(P \cdot L)$ string comparisons per line. A deterministic finite prefix automaton compiles all candidate dictionary prefixes into a trie with failure transitions, matching the longest prefix in a single linear pass over the line text.
- **For What It Is Used**:
  Implements the `PrefixMatcher` trait, providing instant dictionary lookups for language declaration prefixes, comment markers, and scope keywords.
- **Complexity**:
  - Construction Time: $O(\sum |p_i|)$ over dictionary patterns.
  - Query Time: $O(L)$ where $L$ is the length of the inspected line (independent of dictionary size).
  - Space: $O(\text{states} \times |\Sigma|)$.

---

## 4. Formal Languages, Containers & Delimiter Balancing

### 4.1 Dyck Language Delimiter Balancer & Stack Automaton

- **Paper / Canonical Reference**:
  Walther von Dyck (1882). *"Gruppentheoretische Studien"*. Mathematische Annalen, 20(1), 1–44.  
  [Springer DOI:10.1007/BF01443322](https://doi.org/10.1007/BF01443322)  
  Noam Chomsky and Marcel-Paul Schützenberger (1963). *"The Algebraic Theory of Context-Free Languages"*. In: Computer Programming and Formal Systems, Studies in Logic and the Foundations of Mathematics, 35, 118–161.  
  [ScienceDirect DOI:10.1016/S0049-237X(08)72019-6](https://doi.org/10.1016/S0049-237X(08)72019-6)
- **Location**:
  [`crates/diffuse-core/src/algo/dyck.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/dyck.rs) (`trim_balanced_boundaries`, `track_enclosing_containers`, `DyckBalanced`, `SmallStack`, `DelimiterClassifier`, `AsciiDelimiterClassifier`); consumed in [`crates/diffuse-core/src/token.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/token.rs) (`trim_to_balanced_boundaries`, `compute_token_containers`).
- **Why It Is Used**:
  Greedy phrase matching across boundaries frequently captures opening delimiters (e.g. `(` or backtick `` ` ``) without their closing partners, or vice-versa. Slicing open an atomic container breaks the structural semantics of code expressions. The Dyck language formalizes well-nested brackets ($\mathcal{D}_n$). A deterministic pushdown automaton verifies container parity and trims dangling boundaries.
- **For What It Is Used**:
  Guarantees that anchored $n$-gram phrases never slice across unclosed delimiters (parentheses, brackets, braces, double quotes, single quotes, backticks), preserving atomic code expressions like `` `default` ``.
- **Complexity**:
  - Time: $O(N)$ single-pass pushdown traversal.
  - Space: $O(D)$ stack depth where $D \le 16$.

---

## 5. Lattice Optimization & Multi-Objective Dynamic Programming

### 5.1 2D Multi-Objective Block Transformation Lattice (Cluster DP)

- **Paper / Canonical Reference**:
  Richard Bellman (1957). *"Dynamic Programming"*. Princeton University Press.  
  [Princeton University Press](https://press.princeton.edu/books/hardcover/9780691146683/dynamic-programming)  
  Robert A. Wagner and Michael J. Fischer (1974). *"The String-to-String Correction Problem"*. Journal of the ACM, 21(1), 168–173.  
  [ACM Digital Library](https://doi.org/10.1145/321796.321811)
- **Location**:
  [`crates/diffuse-core/src/cluster/dp.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/dp.rs)
- **Why It Is Used**:
  Diff hunks often contain reflows where a single deleted line expands into multiple added lines ($1\text{-to-}K$), or multiple deleted lines collapse into one ($K\text{-to-}1$). Standard line-by-line diffing misaligns these into disconnected deletion/addition blocks. The Cluster DP formulates the problem as a 2D transformation grid where transitions include block expansions, block collapses, in-place modifications, moves, and standalone adds/deletes.
- **For What It Is Used**:
  Computes the Pareto-optimal decomposition of each isolated change cluster $(M \text{ del}, N \text{ add})$ using a multi-objective cost function:
  $$\text{Score} = \alpha \cdot \text{matched} - \mu \cdot \text{unmatched} - \beta \cdot \text{rows} - \gamma \cdot \text{fragmentation} + \delta \cdot \text{syntax\_alignment}$$
- **Complexity**:
  - Time: $O(M \cdot N \cdot K)$, where $K \le 10$ is the maximum reflow block window (`MAX_BLOCK_K`).
  - Space: $O(M \cdot N)$ DP grid with traceback pointers.

---

## 6. Information Theory & Statistical Weighting

### 6.1 Shannon Information Entropy & Term Specificity (TF-IDF Weighting)

- **Paper / Canonical Reference**:
  Claude E. Shannon (1948). *"A Mathematical Theory of Communication"*. Bell System Technical Journal, 27(3), 379–423.  
  [IEEE Xplore](https://ieeexplore.ieee.org/document/6773024) · [Bell Labs Paper PDF](https://people.math.harvard.edu/~ctm/home/text/others/shannon/entropy/entropy.pdf)  
  Karen Spärck Jones (1972). *"A Statistical Interpretation of Term Specificity and its Application in Retrieval"*. Journal of Documentation, 28(1), 11–21.  
  [Emerald DOI:10.1108/eb026526](https://doi.org/10.1108/eb026526)
- **Location**:
  [`crates/diffuse-core/src/algo/entropy.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/algo/entropy.rs) (`FrequencyRanker`, `EntropyFiltered`, `DiffuseEntropyExt`); consumed in [`crates/diffuse-core/src/token.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/token.rs) (`entropy_anchors`, `token_entropy_score`).
- **Why It Is Used**:
  Not all matching tokens carry equal significance. Frequent tokens like punctuation (`(`, `)`, `;`, `,`) and generic keywords (`let`, `fn`, `return`) occur dozens of times and easily create false-positive alignments. Rare tokens (long identifier names, distinctive literals) carry high information content:
  $$I(x) = -\log_2 p(x)$$
- **For What It Is Used**:
  Filters and ranks candidate anchors by information entropy and uniqueness within the diff hunk, ensuring only tokens with high semantic uniqueness serve as alignment anchors.
- **Complexity**:
  - Time: $O(N)$ histogram accumulation and entropy filtering.
  - Space: $O(V)$ where $V \le N$ is unique token vocabulary size.

---

## 7. Similarity Metrics, Assignment & Relocation

### 7.1 Jaccard Metric & Normalized Levenshtein Token Ratio

- **Paper / Canonical Reference**:
  Paul Jaccard (1912). *"The Distribution of the Flora in the Alpine Zone"*. New Phytologist, 11(2), 37–50.  
  [Wiley DOI:10.1111/j.1469-8137.1912.tb05611.x](https://doi.org/10.1111/j.1469-8137.1912.tb05611.x)  
  Vladimir I. Levenshtein (1966). *"Binary Codes Capable of Correcting Deletions, Insertions, and Reversals"*. Soviet Physics Doklady, 10(8), 707–710.  
  [Math-Net Archive](https://www.mathnet.ru/php/archive.phtml?wnode=18128&jrn=dan&paperid=31464&option_lang=eng)
- **Location**:
  [`crates/diffuse-core/src/similarity.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/similarity.rs) (`line_similarity`, `line_similarity_parts`)
- **Why It Is Used**:
  Determining whether a deleted line and an added line represent an in-place modification (`Mod`) requires an objective similarity coefficient that is normalized between $[0.0, 1.0]$ and invariant to leading indentation.
- **For What It Is Used**:
  Calculates token-LCS shared-character fractions and add-side coverage ratios (`SimilarityParts`) to distinguish true in-place modifications from fragment insertions or coincidental token overlaps.
- **Complexity**:
  - Time: $O(|A| \cdot |B|)$ over non-whitespace token strings.
  - Space: $O(\min(|A|, |B|))$.

---

### 7.2 Bipartite Matching & Spatial Coalescence (Move Detection)

- **Paper / Canonical Reference**:
  Harold W. Kuhn (1955). *"The Hungarian Method for the Assignment Problem"*. Naval Research Logistics Quarterly, 2(1–2), 83–97.  
  [Wiley DOI:10.1002/nav.3800020109](https://doi.org/10.1002/nav.3800020109)  
  James Munkres (1957). *"Algorithms for the Assignment and Transportation Problems"*. Journal of the SIAM, 5(1), 32–38.  
  [SIAM DOI:10.1137/0105003](https://doi.org/10.1137/0105003)
- **Location**:
  [`crates/diffuse-core/src/moves.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/moves.rs) (`mark_moves`, `mark_moves_with_profile`)
- **Why It Is Used**:
  When blocks of code or renumbered sections are moved elsewhere in a file, displaying them as pure deletions and insertions inflates change statistics and forces reviewers to re-read identical code. Detecting moved lines across hunks requires bipartite matching with spatial clustering to avoid false-positive matches on common trivial lines (like closing braces).
- **For What It Is Used**:
  Performs multiset matching of deleted and added lines by content key (`t.trim_start()`), marks exact relocations as `Row::MovSrc` and `Row::MovTgt`, and suppresses moved lines from net addition/deletion counts.
- **Complexity**:
  - Time: $O(L \log L)$ or $O(L)$ with hash multiset indexing, where $L$ is the total row count in the file.
  - Space: $O(L)$ hash map of row positions.

---

### 7.3 Semantic Boundary Disambiguation (Hunk Shifting)

- **Paper / Canonical Reference**:
  Fitash Nurul Nugroho, Hideaki Hata, Kenichi Matsumoto (2019). *"How Different are Different Diff Algorithms in Git?"*. Empirical Software Engineering, 25, 790–823.  
  [Springer DOI:10.1007/s10664-019-09772-z](https://doi.org/10.1007/s10664-019-09772-z)  
  Michael D. Ernst (2001). *"Research Directions in Software Evolution: Quick and Clean Diffs"*. ACM SIGSOFT Software Engineering Notes.
- **Location**:
  [`crates/diffuse-core/src/shift.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/shift.rs)
- **Why It Is Used**:
  Diff hunks generated by Git are often mathematically valid but semantically unnatural (e.g. slicing an added function midway through its signature or splitting between closing braces). Shifting a hunk boundary up or down by absorbing and releasing identical lines preserves mathematical identity while aligning boundaries to natural semantic breaks.
- **For What It Is Used**:
  Computes semantic boundary scores (`semantic_boundary_score`) based on blank lines, language declaration headers, comment lines, and scope closing braces to slide hunk windows into their most readable position.
- **Complexity**:
  - Time: $O(C)$ where $C$ is the number of context shift lines.
  - Space: $O(1)$ in-place row slice rotation.

---

## 8. Algebraic Systems & Memory Structures

### 8.1 Invertible Monoidal Edit Tape Algebra

- **Paper / Canonical Reference**:
  Saunders Mac Lane (1971). *"Categories for the Working Mathematician"*. Graduate Texts in Mathematics 5, Springer-Verlag.  
  [Springer DOI:10.1007/978-1-4757-4721-8](https://doi.org/10.1007/978-1-4757-4721-8)  
  Rolf Landauer (1961). *"Irreversibility and Heat Generation in the Computing Process"*. IBM Journal of Research and Development, 5(3), 183–191.  
  [IBM JRD DOI:10.1147/rd.53.0183](https://doi.org/10.1147/rd.53.0183)
- **Location**:
  [`crates/diffuse-core/src/tape.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/tape.rs) (`AlignmentTape`, `TapeChunk`)
- **Why It Is Used**:
  Diff outputs frequently suffer from coordinate desynchronization and non-invertibility (reversing the diff order does not yield symmetric results). The Alignment Tape models edits as an invertible monoid with typed chunks (`Keep`, `Strike`, `Fill`, `Relocate`) and provable projection functors:
  $$\pi_{\text{old}}(\text{tape}) = \text{OldText}, \quad \pi_{\text{new}}(\text{tape}) = \text{NewText}$$
- **For What It Is Used**:
  Serves as the canonical data currency for token and subword alignments, ensuring lossless round-trip transformations and symmetric involution under diff reversal.
- **Complexity**:
  - Time: $O(N)$ tape construction and projection.
  - Space: $O(N)$ contiguous chunk vector.

---

### 8.2 Hash-Consing & Symbol Interning

- **Paper / Canonical Reference**:
  Andrei P. Ershov (1958). *"On Programming of Arithmetic Operations"*. Communications of the ACM, 1(8), 3–6.  
  [ACM Digital Library](https://doi.org/10.1145/368892.368907)  
  John McCarthy (1960). *"Recursive Functions of Symbolic Expressions and Their Computation by Machine, Part I"*. Communications of the ACM, 3(4), 184–195.  
  [ACM Digital Library](https://doi.org/10.1145/367177.367199)
- **Location**:
  [`crates/diffuse-core/src/similarity.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/similarity.rs) (`TokenInterner`)
- **Why It Is Used**:
  Comparing strings during token LCS and DP transitions repeatedly evaluates string equality and pointer chasing. Hash-consing maps every unique string token to a compact `u32` integer symbol upon first ingestion.
- **For What It Is Used**:
  Allows `lcs_pairs_ids` and Cluster DP to perform token comparisons as single-cycle integer equality checks (`a_id == b_id`), improving cache locality and vectorization.
- **Complexity**:
  - Interning Time: $O(L)$ per token string length on first insertion; $O(1)$ amortized lookup.
  - Comparison Time: $O(1)$ integer compare.
  - Space: $O(U)$ where $U$ is the number of distinct token symbols.

---

### 8.3 Robin Hood & Cache-Optimized Hash Tables (FxHash)

- **Paper / Canonical Reference**:
  Pedro Celis (1986). *"Robin Hood Hashing"*. PhD Thesis, University of Waterloo, Computer Science Department.  
  [Waterloo Archive PDF](https://cs.uwaterloo.ca/research/tr/1986/CS-86-14.pdf)
- **Location**:
  Used across [`diffuse-core`](file:///home/daniel/lab/diffuse/crates/diffuse-core/) via `rustc_hash::{FxHashMap, FxHashSet}`.
- **Why It Is Used**:
  Standard SipHash (Rust's default `std::collections::HashMap`) protects against hash-flooding DoS attacks at the expense of computational overhead per key lookup. In a diff engine processing local git commits, cryptographic security is unnecessary. `FxHash` (originating from Firefox and `rustc`) uses fast multiplication and bit-shifting to maximize throughput and CPU cache friendliness.
- **For What It Is Used**:
  Powers inverted token indices, move detection hash tables, and syntax profile lookup tables.
- **Complexity**:
  - Lookup/Insertion: $O(1)$ average time with minimal variance due to Robin Hood variance reduction.

---

## 9. Perception, Accessibility & Color Systems

### 9.1 Oklab Perceptually Uniform Color Space

- **Paper / Canonical Reference**:
  Björn Ottosson (2020). *"A Perceptual Color Space for Image Processing"*.  
  [Specification & Research Post](https://bottosson.github.io/posts/oklab/)  
  CIE (1976). *"Official Recommendations on Uniform Color Spaces, Color Difference Equations, Metric Color Terms"*. Color Research & Application, 2(1), 5–6.  
  [Wiley DOI:10.1002/j.1520-6378.1977.tb00102.x](https://doi.org/10.1002/j.1520-6378.1977.tb00102.x)
- **Location**:
  [`crates/diffuse-render/src/theme/`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/theme/)
- **Why It Is Used**:
  Standard sRGB and HSL color spaces are perceptually non-uniform: hues shift dramatically as lightness or chroma changes (e.g. pure yellow appears far brighter to the human eye than pure blue of identical nominal lightness). Oklab maps colors into a 3D coordinate space $(L, a, b)$ matching human visual cone perception, ensuring that hue rotations and contrast scaling remain perceptually linear and uniform.
- **For What It Is Used**:
  Derives theme palette variations, generating soft syntax highlighting, dimmed context backgrounds, and visually balanced deletion/addition shades across light and dark terminals.
- **Complexity**:
  - Time: $O(1)$ matrix transformation via linear RGB $\to$ LMS cone coordinates $\to$ non-linear cube root $\to$ Lab space.
  - Space: $O(1)$.

---

### 9.2 WCAG 2.1 Relative Luminance & Contrast Ratio Formulation

- **Paper / Canonical Reference**:
  World Wide Web Consortium (W3C) (2018). *"Web Content Accessibility Guidelines (WCAG) 2.1 — Guideline 1.4.3 Contrast (Minimum) and 1.4.6 Contrast (Enhanced)"*.  
  [W3C Recommendation](https://www.w3.org/TR/WCAG21/#contrast-minimum)  
  ITU-R Recommendation BT.709-6 (2015). *"Parameter Values for the HDTV Standards for Production and International Programme Exchange"*.  
  [ITU-R BT.709-6](https://www.itu.int/rec/R-REC-BT.709)
- **Location**:
  [`crates/diffuse-render/src/theme/palette.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/theme/palette.rs)
- **Why It Is Used**:
  Terminal diff tools must remain legible under diverse monitor conditions, ambient lighting, and for users with visual impairments. WCAG 2.1 defines relative luminance $Y$:
  $$Y = 0.2126 \cdot R_{\text{lin}} + 0.7152 \cdot G_{\text{lin}} + 0.0722 \cdot B_{\text{lin}}$$
  and the contrast ratio:
  $$\text{Ratio} = \frac{L_1 + 0.05}{L_2 + 0.05}$$
- **For What It Is Used**:
  Programmatically guarantees minimum contrast thresholds ($\ge 4.5:1$ for normal text, $\ge 7:1$ for enhanced text) between foreground token styles, strikethrough strikes, filled additions, and background terminal colors.
- **Complexity**:
  - Time: $O(1)$ per palette entry.
  - Space: $O(1)$.

---

## 10. Parsing, Syntax & Verification

### 10.1 Generalized LR (GLR) Incremental Parsing (Tree-sitter)

- **Paper / Canonical Reference**:
  Masaru Tomita (1985). *"Efficient Parsing for Natural Language: A Generalized LR Algorithm"*. Kluwer Academic Publishers.  
  [Springer DOI:10.1007/978-1-4613-2559-8](https://doi.org/10.1007/978-1-4613-2559-8)  
  Max Brunsfeld (2018). *"Tree-sitter: An Incremental Parsing System for Programming Tools"*. Strange Loop.  
  [Tree-sitter Documentation](https://tree-sitter.github.io/tree-sitter/)
- **Location**:
  [`crates/diffuse-syntax/src/`](file:///home/daniel/lab/diffuse/crates/diffuse-syntax/src/) (`tree_sitter.rs`)
- **Why It Is Used**:
  Regex-based syntax highlighters lack semantic understanding of nested grammars, template literals, multiline comments, and language scopes. Tree-sitter generates concrete syntax trees (CSTs) using Tomita's Generalized LR parsing with graph-structured stacks (GSS), with incremental re-parsing that updates trees in $O(\log N)$ time upon edits.
- **For What It Is Used**:
  Provides syntax highlighting tokens, semantic section boundary detection, and scope analysis for supported programming languages.
- **Complexity**:
  - Parsing Time: $O(N)$ full parse, $O(\log N)$ incremental re-parse per line edit.
  - Space: $O(N)$ CST node allocation.

---

### 10.2 Design by Contract (DbC) & Hoare Logic Invariant Enforcement

- **Paper / Canonical Reference**:
  C. A. R. Hoare (1969). *"An Axiomatic Basis for Computer Programming"*. Communications of the ACM, 12(10), 576–580.  
  [ACM Digital Library](https://doi.org/10.1145/363235.363259)  
  Bertrand Meyer (1992). *"Applying 'Design by Contract'"*. IEEE Computer, 25(10), 40–51.  
  [IEEE Xplore DOI:10.1109/2.161279](https://doi.org/10.1109/2.161279)
- **Location**:
  [`crates/diffuse-core/src/dbc.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/dbc.rs) (`require!`, `ensure!`, `invariant!`)
- **Why It Is Used**:
  Diff engine pipelines are prone to silent coordinate drift, index off-by-one errors, and corrupt tape states if edge cases go unverified. DbC integrates formal Hoare triples $\{P\}\;S\;\{Q\}$ into runtime code:
  - $\{P\}$ Preconditions (`require!`)
  - $\{Q\}$ Postconditions (`ensure!`)
  - Structural Invariants (`invariant!`)
- **For What It Is Used**:
  Enforces bidirectional projection invariants across alignment tapes ($\pi_{\text{old}} = \text{OldText}$, $\pi_{\text{new}} = \text{NewText}$), valid index bounds across hunks, and non-empty coordinate ranges during debug and test runs (`cfg!(debug_assertions)`), while compiling to zero-cost no-ops in optimized release builds.
- **Complexity**:
  - Debug Builds: $O(1)$ boolean assertion check.
  - Release Builds: Zero runtime overhead ($0\text{ ns}$, completely eliminated by compiler).
