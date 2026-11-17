# Interior Token Donors and Inlining Mod Groups

- [x] **Phase 1: Information-Theoretic Transitive Co-occurrence and Donor Invariants**
- [x] **Phase 2: Interior Donor Detection in `diffuse-core::collapse::assemble`**
- [x] **Phase 3: Sliced Run Assignment & Subline Moved Token Labeling**
- [·] **Phase 4: Golden Corpus Acceptance Analysis, Dual-Leg Verification & Visual Sign-off**

---

## Executive Summary

In structured diff analysis, code modifications often involve expression inlining or extraction (e.g. deleting a temporary variable declaration `let x = expr;` and replacing occurrences of `x` with `expr` in an adjacent conditional or statement).

Currently, *diffuse*'s multi-line grouping engine ([`crates/diffuse-core/src/collapse/assemble.rs`](crates/diffuse-core/src/collapse/assemble.rs)) discovers auxiliary deletion donors (`RowGroup::Aux`) exclusively via 1D linear string reflow (`matches_unmatched_prefix` and `matches_unmatched_suffix`). Because lines with common prefix and suffix context (such as `if ` and ` {`) anchor at both ends, interior token substitutions are assumed to be independent in-place mutations. Consequently, the deleted declaration remains an isolated `Row::Del` (`-`), while the consuming line becomes an independent $1:1$ `Row::Mod` (`~`), falsely asserting that the inlined expression was newly authored rather than relocated.

This plan specifies a language-agnostic, information-theoretic mechanism for identifying **interior token donors**. By detecting high-entropy token conservation and transitive co-occurrence between unpaired deletions and adjacent modification rows, the engine unifies them into a single, cohesive modification group (`RowGroup::Anchor` + `RowGroup::Aux`) without requiring language-specific AST parsing.

---

## Table of Contents

1. [Phase 1 — Information-Theoretic Transitive Co-occurrence and Donor Invariants](#1-phase-1--information-theoretic-transitive-co-occurrence-and-donor-invariants)
2. [Phase 2 — Interior Donor Detection in `diffuse-core::collapse::assemble`](#2-phase-2--interior-donor-detection-in-diffuse-corecollapseassemble)
3. [Phase 3 — Sliced Run Assignment & Subline Moved Token Labeling](#3-phase-3--sliced-run-assignment--subline-moved-token-labeling)
4. [Phase 4 — Golden Corpus Acceptance Analysis, Dual-Leg Verification & Visual Sign-off](#4-phase-4--golden-corpus-acceptance-analysis-dual-leg-verification--visual-sign-off)

---

## 1. Phase 1 — Information-Theoretic Transitive Co-occurrence and Donor Invariants

### 1.1 Mathematical Formulation

Let $M = (L_{\text{old}}, L_{\text{new}})$ be a candidate $1:1$ modification row, and let $D \in \text{Del}$ be an adjacent unpaired deletion row within the same change cluster.
Under Gotoh sub-line alignment, $M$ partitions into common runs $\mathcal{C}$, struck runs $\mathcal{S}$, and filled runs $\mathcal{F}$:
$$L_{\text{old}} = \mathcal{C}_{\text{old}} \cup \mathcal{S}, \quad L_{\text{new}} = \mathcal{C}_{\text{new}} \cup \mathcal{F}$$

A deletion row $D$ is admitted as an **interior donor** to anchor $M$ if and only if it satisfies one of two information-theoretic conservation predicates:

#### Predicate A: Transitive Co-occurrence (Variable Inlining / Extraction)
1. There exists a struck non-whitespace token $A \in \mathcal{S}$ and a filled non-whitespace token $B \in \mathcal{F}$ in $M$.
2. The candidate deletion line $D$ contains both $A$ and $B$:
   $$A \in \text{Tokens}(D) \quad \land \quad B \in \text{Tokens}(D)$$
3. $B$ satisfies the entropy / length floor ($\text{len}(B) \ge 4$ non-whitespace chars, excluding trivia/punctuation).

*Intuition*: Line $D$ defines the semantic equivalence or binding between $A$ and $B$ (e.g. `let A = B;`). Replacing $A$ with $B$ on an adjacent line while deleting $D$ demonstrates that $D$ is the causal donor of $B$.

#### Predicate B: Substantial Information Deficit Conservation
1. $M$ contains an unmatched filled run $B \in \mathcal{F}$ with high entropy ($H(B) \ge \theta$, length $\ge 8$ characters).
2. $B$ occurs verbatim in $D$: $B \subseteq D$.
3. $B$ does not occur in the immediate ambient context ($\pm 3$ lines of unmodified context).

### 1.2 Invariants & Guardrails against False Positives

To prevent greedy or spurious grouping (such as common variable names like `len`, `val`, `i` triggering accidental group absorption):
- **Adjacency Scope**: $D$ must be contiguous with $M$ in the deletion block (distance $\le 2$ rows, no intervening context).
- **Scope Containment**: $D$ and $M$ must belong to the same syntactic block / indentation scope (head indentation of $D$ matches $L_{\text{old}}$).
- **Inversion Duality**: Under diff reversal (swapping pre- and post-images), an inlining collapse ($N \to 1$) must invert symmetrically into an extraction expansion ($1 \to N$).

---

## 2. Phase 2 — Interior Donor Detection in `diffuse-core::collapse::assemble`

### 2.1 Locating Donor Candidates in `assemble.rs`

In [`crates/diffuse-core/src/collapse/assemble.rs`](crates/diffuse-core/src/collapse/assemble.rs#L350-L405):
- Currently, candidate deletions are checked only via:
  ```rust
  matches_unmatched_prefix(add_row.text(), &spans.add, cand_row.text())
  matches_unmatched_suffix(add_row.text(), &spans.add, cand_row.text())
  ```
- Add a new helper:
  ```rust
  fn matches_interior_donor(
      del_row_text: &str,
      add_row_text: &str,
      spans: &Spans,
      cand_del_text: &str,
  ) -> bool
  ```
- When scanning upward deletions from `del_ri`:
  If neither prefix nor suffix matches, evaluate `matches_interior_donor`.
  If true, push `cand_k` to `upward_dels`, bundle into `collapse_blocks`, and claim the deletion.

### 2.2 Preserving the Semantic Anchor Line

In the existing `collapse_blocks` execution (lines 502–533):
- The engine currently assumes `block_dels[0]` (the top-most line) is the anchor row, which is appropriate for top-down reflow.
- For interior donors (such as line 227 donating to line 228), the *primary* anchor row is the line that actually aligned with `add_ri` (line 228), while line 227 is the auxiliary deletion.
- Update the anchor selection in `assemble.rs`:
  ```rust
  // The anchor line is the paired del_ri, NOT necessarily block_dels[0]
  let anchor_di = if block_dels.contains(&del_ri) { del_ri } else { block_dels[0] };
  ```
- All other deletions in `block_dels` (excluding `anchor_di`) are registered in `del_groups` as `RowGroup::Aux { anchor: anchor_nl }`.

---

## 3. Phase 3 — Sliced Run Assignment & Subline Moved Token Labeling

### 3.1 Marking Donated Tokens as Moved

In [`crates/diffuse-core/src/collapse/aux.rs`](crates/diffuse-core/src/collapse/aux.rs):
- Update `slice_multiline_del_runs`:
  When slicing runs for an interior donor block:
  - On the donor row (`RowGroup::Aux`): the donated token `B` (`is_on_scrollbar`) is assigned `RunKind::MovedSrc` (`<`), while the remaining text (`let scrollbar_seek = ;`) remains `RunKind::Del` (`-`).
  - On the anchor row (`RowGroup::Anchor`): the consumed token `B` in `add_row` is assigned `RunKind::MovedTgt` (`>`), while the replaced token `A` (`scrollbar_seek`) remains `RunKind::Del` (`-`).
- Verify character conservation invariants in [`crates/diffuse-core/src/collapse/invariants.rs`](crates/diffuse-core/src/collapse/invariants.rs):
  $$\sum \text{chars}(\text{anchor MovedTgt}) = \sum \text{chars}(\text{aux MovedSrc})$$

---

## 4. Phase 4 — Golden Corpus Acceptance Analysis, Dual-Leg Verification & Visual Sign-off

### 4.1 Golden Fallout Impact Analysis

A survey across all 20 active golden acceptance corpora reveals that interior donor matching targets genuine refactorings with zero negative degradation:

| Golden Corpus | Target Location | Old Del Line | Anchor Mod Line | Impact & Value |
| :--- | :--- | :--- | :--- | :--- |
| **`scrollbar-grab-drag`** | `app/dispatch.rs:227-237` | `let scrollbar_seek = is_on_scrollbar;` | `if scrollbar_seek {` $\to$ `if is_on_scrollbar {` | **Positive**: Unifies isolated deletion with its inlined use into a single mod group. |
| **`scrollbar-grab-drag`** | `app/dispatch.rs:2229-2315` | `let row_relative = r.saturating_sub(...);` | `scrollbar_scroll_for_row(..., row_relative)` $\to$ `scrollbar_scroll_for_offset(..., r)` | **Positive**: Identifies inlined expression `r` from deleted assignment. |
| **`diffcoord-anchoring`** | `tests/golden.rs:1107-1113` | `..MarkdownOptions::default()` | `render_markdown_output(&output, &-opts +MarkdownOptions::default())` | **Positive**: Links inlined default struct initialization with eliminated binding. |

### 4.2 Verification Gate

1. **Unit tests in `diffuse-core`**:
   - Test transitive co-occurrence inlining (`let x = y; if x` $\to$ `if y`).
   - Test rejection of low-entropy / short tokens (`let x = 1; if x` does not steal unrelated `1`s).
   - Test diff reversal involution (extraction matches symmetrically as `1-to-2` expand).
2. **Acceptance suite**:
   - Run `cargo test --test golden`.
   - Carefully review golden diffs (`.wire.jsonl`, `.pager.render.*.txt`, `.tui.*.render.*.txt`) for the 3 affected corpora to confirm that grouping strictly improves visual clarity.
3. **Check gate**:
   - `./scripts/check.sh` passes with zero warnings, nightly formatting, and clippy clean.
