# diffuse — Atomic Cluster Transformation Plan

## Global TODO
- [x] **Phase 1: Semantic Sliding, Dynamic Context & Hunk Merging**
  - [x] Implement bidirectional identity sliding with semantic score weighting (functions, comments, scope braces, blank lines).
  - [x] Implement dynamic context line expansion from `old_content`/`new_content` when sliding past initial hunk boundaries.
  - [x] Implement inline hunk merging when adjacent hunks overlap or touch within the context budget.
  - [x] Implement context normalization to enforce standard context budgets ($N=6$).
- [x] **Phase 2: Syntax-Delimited Section Partitioning**
  - [x] Implement syntax section classifier (doc comments, attributes, function boundaries, statement blocks).
  - [x] Partition change runs by syntax sections to enforce hard semantic barriers.
  - [x] Decompose hunks into isolated, context-bounded $(M \text{ dels}, N \text{ adds})$ change clusters.
- [x] **Phase 3: Cluster Dynamic Programming (DP) Transformation Engine**
  - [x] Define cluster DP table and transition state model ($\text{DP}[i][j]$).
  - [x] Implement 1-to-1 `Mod`, 1-to-$K$ expansion, $K$-to-1 collapse, `Del`, `Add`, and `Mov` transitions.
  - [x] Implement subline `Mov` span detection for cross-line token reflows (e.g. comment text shifting across line breaks).
  - [x] Formulate multi-objective cost function (common tokens, syntax alignment, minimal sprawl, mid-word penalty).
  - [x] Implement DP optimal path traceback with bidirectional string verification ($\Pi_{\text{old}} \equiv \text{old} \land \Pi_{\text{new}} \equiv \text{new}$).
- [x] **Phase 4: Universal Subword Token Stream & Multi-Line Markup**
  - [x] Extend subword run generation to all row kinds (`Mod`, `Del`, `Add`, `Mov`).
  - [x] Implement `RunKind::Mov` subline spans for moved text fragments (source struck-moved, target filled-moved).
  - [x] Implement subword token highlights for absorbed deletions and multi-line expansions.
  - [x] Implement whitespace contraction (8 → 12 spaces = 8 Common + 4 Add) and delimiter snapping.
- [x] **Phase 5: Acceptance Verification & Golden Gate**
  - [x] Extend `apply_oracle` to mechanically prove string and hunk equivalence across all 7 golden corpora.
  - [x] Verify wire contract compatibility and TypeScript TUI rendering.
  - [x] Regenerate visual golden baselines following human sign-off.

---

## Executive Summary

The **Atomic Cluster Transformation** plan replaces diffuse's legacy greedy 1-to-1 collapse heuristics with a mathematically verified, search-based optimization engine. 

Instead of isolating line pairing and word diffing into separate ad-hoc passes, the engine operates on **Context-Delimited Change Clusters** ($M$ deleted lines transforming into $N$ added lines between immutable context barriers). Each cluster is solved via **2D Dynamic Programming (DP)**, computing the globally optimal decomposition into `Mod`, `Del`, `Add`, `Mov`, and `Ctx` rows while guaranteeing the universal bidirectional invariant:
$$\Pi_{\text{old}}(\mathcal{P}) \equiv \text{lines}_{\text{old}} \quad \land \quad \Pi_{\text{new}}(\mathcal{P}) \equiv \text{lines}_{\text{new}}$$

The architecture introduces **subline `Mov` spans**, capturing logical and syntactical moves at the token level (e.g. comment or argument reflows where tokens move from the tail of one line to the head of the next, less shared prefix formatting).

Prior to cluster transformation, two semantic pre-pipeline passes ensure optimal diff framing:
1. **Semantic Sliding Hunks with Dynamic Context & Merging**: Slides diffs along matching file lines to align cleanly with functions and scope braces, dynamically pulling extra context from file buffers and merging adjacent hunks.
2. **Syntax-Delimited Section Partitioning**: Partitions change clusters at syntax boundaries (e.g. doc comments vs code blocks), preventing cross-construct pollution.

---

## Table of Contents
1. [Phase 1 — Semantic Sliding Hunks, Dynamic Context & Hunk Merging](#1-phase-1--semantic-sliding-hunks-dynamic-context--hunk-merging)
2. [Phase 2 — Syntax-Delimited Section Partitioning](#2-phase-2--syntax-delimited-section-partitioning)
3. [Phase 3 — Cluster Dynamic Programming (DP) Transformation Engine](#3-phase-3--cluster-dynamic-programming-dp-transformation-engine)
4. [Phase 4 — Universal Subword Token Stream & Multi-Line Markup](#4-phase-4--universal-subword-token-stream--multi-line-markup)
5. [Phase 5 — Acceptance Verification & Golden Gate](#5-phase-5--acceptance-verification--golden-gate)

---

## 1. Phase 1 — Semantic Sliding Hunks, Dynamic Context & Hunk Merging

Standard git diff algorithms frequently snap edit boundaries at arbitrary points (such as an inner closing brace rather than an outer function boundary). This phase frames hunks semantically before any pairing occurs.

### Step 1.1: Semantic Identity Sliding
- Implement bidirectional sliding in [`src/shift/mod.rs`](file:///home/daniel/lab/diffuse/src/shift/mod.rs):
  - Slide changes up or down along matching file lines as long as the slide is a verified identity transformation ($f_{\text{slide}}(\text{old}) \equiv \text{new}$).
  - Evaluate semantic boundary scores: heavily favor snapping to function headers, blank lines, doc comment starts, and enclosing scope closing braces.

### Step 1.2: Dynamic Context Loading
- When a slide reaches the boundary of the initial diff context, dynamically fetch additional lines from `old_content` and `new_content`.
- Allows diffs to slide arbitrarily far to find the true enclosing function or construct boundary.

### Step 1.3: Inline Hunk Merging
- If sliding hunk $A$ downwards (or hunk $B$ upwards) causes their context windows to overlap or touch, fuse them into a single contiguous hunk.
- Re-index hunk offsets and line headers seamlessly.

### Step 1.4: Context Budget Normalization
- Prune or expand leading and trailing context lines to enforce a uniform context budget ($N=6$ lines).

---

## 2. Phase 2 — Syntax-Delimited Section Partitioning

To guarantee that doc comments never pair with struct fields or distinct functions, change runs are partitioned into syntax-delimited sections prior to DP solving.

### Step 2.1: Syntax Section Classifier
- Implement syntax section boundary detection in [`src/syntax/mod.rs`](file:///home/daniel/lab/diffuse/src/syntax/mod.rs):
  - Classify comment blocks (`///`, `//`, `/* ... */`, `#`).
  - Classify language attributes / decorators (`#[...]`, `@...`).
  - Classify function and type declarations (`fn`, `pub fn`, `struct`, `impl`, `class`).
  - Classify statement blocks and control flow headers.

### Step 2.2: Hard Section Barriers
- Within a hunk, insert hard section boundaries between incompatible syntax constructs.
- Prevents the downstream DP search space from considering pairings across distinct syntactic constructs.

### Step 2.3: Cluster Extraction
- Decompose each hunk into a sequence of isolated change clusters:
  $$\text{Cluster}_k = \Big( [o_1, \dots, o_M], \ [n_1, \dots, n_N] \Big)$$
- Each cluster is strictly bounded by immutable context lines or syntax section barriers.

---

## 3. Phase 3 — Cluster Dynamic Programming (DP) Transformation Engine

Each change cluster is transformed into an optimal visual row stream using 2D Dynamic Programming over the $(M, N)$ line matrix.

### Step 3.1: DP State Space Formulation
- Define the DP table $\text{DP}[i][j]$ representing the optimal decomposition of old lines $o_{1..i}$ into new lines $n_{1..j}$ ($0 \le i \le M, 0 \le j \le N$).

### Step 3.2: Transition Operations
From cell $(i, j)$, evaluate all valid digital transitions:
1. **1-to-1 Modification (`Mod`)** $\to (i+1, j+1)$:
   - Matches $o_{i+1}$ with $n_{j+1}$.
   - Token LCS score + syntax head bonus - character mismatch penalty.
2. **1-to-$K$ Multi-line Expansion (`Mod` + $K-1$ `Add`s)** $\to (i+1, j+K)$:
   - Matches $o_{i+1}$ against new block $n_{j+1..j+K}$.
   - Distributes common tokens across the expansion.
3. **$K$-to-1 Multi-line Collapse (`Mod` + $K-1$ `Del`s)** $\to (i+K, j+1)$:
   - Matches old block $o_{i+1..i+K}$ into single line $n_{j+1}$.
   - Highlights absorbed tokens in deletions.
4. **Subline Move / Reflow (`Mov` Spans)** $\to (i+1, j+1)$ / multi-line:
   - Identifies tokens that moved across lines (e.g. `// words...` moving from line $k$ to line $k+1$ after the common `// ` prefix).
   - Records logical source/target subline coordinates.
5. **Standalone Deletion (`Del`)** $\to (i+1, j)$:
   - Emits $o_{i+1}$ as a pure deletion.
6. **Standalone Addition (`Add`)** $\to (i, j+1)$:
   - Emits $n_{j+1}$ as a pure addition.
7. **Block Move (`Mov`)** $\to (i+1, j+1)$:
   - Emits verified moved line pair identified by move detection.

### Step 3.3: Multi-Objective Cost Function
- Formulate scoring function $\text{Score}(\text{Transition})$:
  $$\text{Score} = \alpha \cdot \text{MatchedChars} - \beta \cdot \text{RowCount} - \gamma \cdot \text{FragmentationPenalty} + \delta \cdot \text{SyntaxAlignment}$$
- Strongly penalize mid-word splits, split identifiers, and unaligned indentation.

### Step 3.4: Traceback & Bidirectional Verification
- Trace back from $\text{DP}[M][N]$ to construct the optimal row sequence $\mathcal{P}^*$.
- Execute verification check: $\Pi_{\text{old}}(\mathcal{P}^*) \equiv \text{lines}_{\text{old}} \land \Pi_{\text{new}}(\mathcal{P}^*) \equiv \text{lines}_{\text{new}}$.

---

## 4. Phase 4 — Universal Subword Token Stream & Multi-Line Markup

Extends subword token diffing and highlight metadata across all diff row kinds.

### Step 4.1: Universal Token Stream & `Mov` Spans
- Update [`src/merge/mod.rs`](file:///home/daniel/lab/diffuse/src/merge/mod.rs) and [`src/bin_pipeline.rs`](file:///home/daniel/lab/diffuse/src/bin_pipeline.rs) to emit structured `Vec<Run>` token streams supporting `RunKind::Common`, `RunKind::Del`, `RunKind::Add`, `RunKind::Ctx`, and `RunKind::Mov`.

### Step 4.2: Multi-Line Highlight Distribution
- In $1 \to K$ expansions, render subsequent `Add` rows with common subword highlights showing tokens reflowed from the original line.
- In $K \to 1$ collapses, render subsequent `Del` rows with absorbed tokens dimmed/struck.
- In comment/code reflows, render moved tokens with distinct subline move styling (`Mov` spans) connecting source and destination.

### Step 4.3: Whitespace & Delimiter Snapping
- Extract shared whitespace prefixes (e.g. 8 → 12 spaces rendered as 8 Common + 4 Add).
- Snap delimiter and comparison operator flips (`<` vs `>`) to their exact token offsets.

---

### Step 4.4: Mov Span Rendering Contract
- `RunKind::Mov` runs render via THEME KEYS (not hardcoded): the renderer's
  palette gains `toolDiffMovedAdded` / `toolDiffMovedRemoved`, defaulting to
  the same muted green/red as added/removed lines (add tint 22 / del tint 52
  in the default theme) — filled-moved (new side) green, struck-moved (old
  side) red. NO subline highlighting: the mov run's text renders one solid
  color (the run's semantic syntax color is ignored for mov runs).

## 5. Phase 5 — Acceptance Verification & Golden Gate

Proves engine correctness, wire schema compatibility, and visual quality across the entire test corpus.

### Step 5.1: Mechanical Oracle Invariant
- Run [`apply_oracle`](file:///home/daniel/lab/diffuse/tests/golden.rs#L526) across all 7 golden corpora:
  - `mirage-2025-08`
  - `diffuse-2025-08`
  - `opt-1-cache-regex-window-slice`
  - `opt-2-cache-markdown-regex`
  - `opt-3-cache-lang-profile`
  - `opt-4-manual-scanner-prefilter`
  - `diffuse-history-mid`
- Assert byte-for-byte mechanical patch application on every file.

### Step 5.2: Wire Contract & Renderer Equivalence
- Validate wire JSON serialization against the TypeScript Pi review renderer.
- Run `./scripts/check.sh` ensuring nightly rustfmt, clippy `-D warnings`, and TypeScript equivalence gates pass with zero warnings.

### Step 5.3: Golden Baseline Regeneration & Sign-Off
- Present rendered visual diffs before/after for human review.
- Regenerate locked golden baselines upon deliberate approval.
