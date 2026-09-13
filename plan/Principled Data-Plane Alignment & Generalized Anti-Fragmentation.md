# Principled Data-Plane Alignment & Generalized Anti-Fragmentation

- [x] Phase 1 — Generalized Multi-Token $n$-Gram Phrase Anchoring (Topology Layer)
- [x] Phase 2 — Affine Gap Sequence Alignment & Path Heuristic Elimination (Optimization Layer)
- [x] Phase 3 — Delimiter-Agnostic Container Coherence (Algebra Layer)
- [·] Phase 4 — Golden Acceptance, Verification & Full Suite Signoff

---

## Executive Summary

Diffuse's word-level alignment engine produces token streams by aligning old and new lines. Over recent iterations, several targeted heuristics and character-specific checks were introduced to prevent Longest Common Subsequence (LCS) from generating fragmented "chimera" diffs:
1. Backtick-specific parity tracking (`a_in_code`, `code_open`) and standalone checks (`is_standalone_code`) in `entropy_anchors`.
2. Hardcoded backward expansion over backticks in `lcs_pairs_anchored` (`|| a[back_a - 1] == "\`"`).
3. Path-specific dot heuristics and file-extension tables (`is_file_extension`, `is_path_dot`, `path_or_file_root_and_stem` in `tokens_can_match`).
4. Backtick-specific container coherence checking (`TapeChunk::Keep("\`")` in `tape.rs`).

While these targeted patches fixed immediate visual defects, they violate the core architecture of Diffuse: **diff engines should operate on a principled, high-level data plane rather than ad-hoc character heuristics**.

This plan refactors Diffuse's alignment pipeline into a three-tier, mathematically grounded data plane:
1. **Topology Layer ($n$-Gram / Phrase Anchoring)**: Upgrades Patience diff from unigrams to high-entropy multi-token $n$-grams ($n \ge 2$), atomically binding structured phrases (such as code spans, string literals, and compound expressions) to prevent delimiter tearing.
2. **Optimization Layer (Affine Gap Sequence Alignment)**: Upgrades residue DP from flat unweighted token counting ($+1$) to affine gap scoring (Gotoh's algorithm), mathematically penalizing edit boundary alternations so coincidental token matches cannot fragment different semantic units.
3. **Algebra Layer (Delimiter-Agnostic Container Coherence)**: Generalizes `AlignmentTape` container coherence to any matched delimiter pair (backticks, quotes, parentheses, brackets), guaranteeing structural integrity without hardcoding specific symbols.

### Existing Heuristics Inventory & Elimination Strategy

| Current Heuristic / Check | File | Purpose | Replacement Model |
|---|---|---|---|
| `a_in_code`, `code_open` | `token.rs` | Prevent unquoted words from matching code spans | **Phase 1**: Multi-token $n$-gram phrase anchoring (`["\`", word, "\`"]` is a unique $n$-gram) |
| `is_standalone_code` | `token.rs` | Prevent single tokens inside code from misaligning | **Phase 1**: Delimiter-bound phrase anchors |
| `\|\| a[back_a - 1] == "\`"` | `lcs.rs` | Re-attach opening backtick to anchored word | **Phase 1**: Multi-token anchor includes delimiters directly |
| `is_file_extension` | `lcs.rs` | 20+ hardcoded file extensions (`.sh`, `.rs`, ...) | **Phase 2**: Affine gap penalties penalize scattered dot matches |
| `is_path_dot` | `lcs.rs` | Disallow matching dots in paths without neighbors | **Phase 2**: Affine gap penalties |
| `path_or_file_root_and_stem` | `lcs.rs` | Path root/stem match guard in `tokens_can_match` | **Phase 2**: Affine gap penalties |
| `TapeChunk::Keep("\`")` | `tape.rs` | Hardcoded backtick container check | **Phase 3**: Delimiter-agnostic container matcher (`"`, `'`, `` ` ``, `(`, `[`, `{`) |

---

## Table of Contents

1. [Phase 1 — Generalized Multi-Token $n$-Gram Phrase Anchoring (Topology Layer)](#1-phase-1--generalized-multi-token-n-gram-phrase-anchoring-topology-layer)
2. [Phase 2 — Affine Gap Sequence Alignment & Path Heuristic Elimination (Optimization Layer)](#2-phase-2--affine-gap-sequence-alignment--path-heuristic-elimination-optimization-layer)
3. [Phase 3 — Delimiter-Agnostic Container Coherence (Algebra Layer)](#3-phase-3--delimiter-agnostic-container-coherence-algebra-layer)
4. [Phase 4 — Golden Acceptance, Verification & Full Suite Signoff](#4-phase-4--golden-acceptance-verification--full-suite-signoff)

---

## 1. Phase 1 — Generalized Multi-Token $n$-Gram Phrase Anchoring (Topology Layer)

**Goal:** Implement true multi-token $n$-gram phrase anchoring in `crates/diffuse-core/src/token.rs`, integrating it into `entropy_anchors` and completely removing backtick-specific heuristics from `token.rs` and `lcs.rs`.

### Detailed Steps

1. **Refactor Multi-Token Phrase Detection (`crates/diffuse-core/src/token.rs`)**:
   - Update `multi_token_phrase_anchors`:
     - Trim only leading and trailing whitespace (`token_is_ws`), preserving enclosing and internal delimiters (quotes, backticks, brackets, punctuation).
     - Require that the phrase contains at least one content token (identifier, keyword, prose word, literal, or operator anchor) with Shannon self-information.
     - Enforce uniqueness in both token sequences ($A$ count = 1, $B$ count = 1) and cumulative entropy $\ge \text{MIN\_PHRASE\_ENTROPY}$ (2.0).
     - Emit **all** constituent tokens within the selected phrase as anchor pairs `(a_idx, b_idx)`. This atomically locks opening and closing delimiters directly to the content.
2. **Integrate Multi-Token Landmarks into `entropy_anchors` (`crates/diffuse-core/src/token.rs`)**:
   - Invoke `multi_token_phrase_anchors` as the first topological pass.
   - For unanchored intervals between phrase landmarks, collect unique single-token anchors meeting `MIN_SINGLE_TOKEN_ENTROPY`.
   - Merge phrase anchors and single-token anchors monotonically.
3. **Eliminate Backtick-Specific Heuristics**:
   - Remove `a_in_code`, `b_in_code`, `code_open`, and `is_standalone_code` from `crates/diffuse-core/src/token.rs`.
   - Revert `|| a[back_a - 1] == "\`"` in `crates/diffuse-core/src/lcs.rs` back to pure subword expansion (`is_subword_token`).
4. **Verification Gate**:
   - Verify `test_theme_table_code_span_alignment` passes with 6 clean runs.
   - Verify `test_multi_token_phrase_detection` and all unit tests in `diffuse-core` pass.
   - Verify all 8 golden corpora pass with zero regressions.

---

## 2. Phase 2 — Affine Gap Sequence Alignment & Path Heuristic Elimination (Optimization Layer)

**Goal:** Implement affine gap penalties in `raw_lcs_pairs_with_context` (`crates/diffuse-core/src/lcs.rs`), mathematically penalizing fragmented edit alternations so coincidental island matches are rejected without path/extension heuristics.

### Detailed Steps

1. **Affine Gap Alignment DP (`crates/diffuse-core/src/lcs.rs`)**:
   - Implement 3-state Gotoh dynamic programming:
     - Match state $M(i, j)$: token match reward proportional to token character length / information content.
     - Delete state $X(i, j)$: opening penalty $G_{\text{open}}$ + extension penalty $G_{\text{extend}}$.
     - Insert state $Y(i, j)$: opening penalty $G_{\text{open}}$ + extension penalty $G_{\text{extend}}$.
   - Ensure linear or scratchpad memory allocations satisfying the `< 0.5s` test performance budget.
2. **Audit & Deprecate Ad-Hoc Path Checks (`crates/diffuse-core/src/lcs.rs`)**:
   - Test removal of `is_file_extension`, `is_path_dot`, and `path_or_file_root_and_stem`.
   - Verify that paths like `extension/pi/scripts/install.sh` vs `./scripts/check.sh` naturally align as atomic replacements under affine gap costs without needing extension tables.
3. **Verification Gate**:
   - Run unit tests covering path replacements, identifier diffs, and punctuation edits.
   - Verify test suite runtime stays within latency ceilings.

---

## 3. Phase 3 — Delimiter-Agnostic Container Coherence (Algebra Layer)

**Goal:** Generalize `enforce_container_coherence` in `crates/diffuse-core/src/tape.rs` to support all paired containers (quotes, parentheses, brackets, braces, backticks) symmetrically.

### Detailed Steps

1. **General Container Recognition (`crates/diffuse-core/src/tape.rs`)**:
   - Define paired delimiter matching:
     - Quotes: `"..."`, `'...'`, `` `...` ``
     - Enclosures: `(...)`, `[...]`, `{...}`
   - Detect when an aligned pair of matching delimiters contains fragmented interior edits ($\ge 2$ edit groups separated by common text).
   - Coalesce fragmented interiors into atomic `Strike(old_inner)` and `Fill(new_inner)`.
2. **Invertibility Guarantee**:
   - Prove algebraic involution: `tape.reverse().reverse() == tape`.
3. **Verification Gate**:
   - Add unit tests for parenthesized, quoted, and bracketed fragmented edits.
   - Verify zero regressions across golden corpora.

---

## 4. Phase 4 — Golden Acceptance, Verification & Full Suite Signoff

**Goal:** Validate the complete, heuristic-free data-plane pipeline across all corpora and developer toolchains.

### Detailed Steps

1. **Automated Verification**:
   - Run `./scripts/check.sh` (nightly fmt check, clippy with zero warnings, all cargo unit and integration tests).
   - Run all 16 legs in `tests/golden.rs` across all 8 corpora (`diffuse-2026-08`, `diffuse-history-mid`, `theme-scrollbar-palette`, `untracked-files`, `opt-1`–`opt-4`).
2. **Performance Benchmarking**:
   - Validate that total test suite latency remains $< 3\text{s}$ warm.
3. **Human Visual Review**:
   - Present clean before-and-after diffs to the human reviewer for final signoff before commit.
