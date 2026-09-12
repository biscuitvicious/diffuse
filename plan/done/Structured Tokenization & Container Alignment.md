# Structured Tokenization, Hierarchical Container Alignment & Affine Gap Scoring

- [x] Phase 1 — Structural Tokenization & Delimited Spans (Lexical Layer)
- [x] Phase 2 — Affine Gap Scoring & Fragmentation Penalties (Optimization Layer)
- [x] Phase 3 — Hierarchical Container Alignment on AlignmentTape (Algebra Layer)
- [x] Phase 4 — Golden Acceptance, Verification & Visual Review Signoff

---

## Executive Summary

Diffuse's word-level alignment engine produces token streams by aligning old and new lines. When lines contain structured entities—such as delimited inline code spans (`` `...` ``), quoted strings (`"..."`), or file paths (`a/b/c.d`)—raw, unweighted Longest Common Subsequence (LCS) over flat character classes can produce fragmented "chimera" diffs. A prominent example is line 117 of `AGENTS.md` where replacing `` `extension/pi/scripts/install.sh` `` with `` `./scripts/check.sh` `` was spliced into `` `extension/pi/./scripts/installcheck.sh` `` because intermediate directories (`scripts/`) and extensions (`.sh`) matched with zero penalty for the intervening edit gaps.

This plan establishes a mathematically principled, three-tier architecture at the data and algorithmic level to eliminate fragmented splices while preserving fine-grained subword diffing where appropriate:

1. **Structural Tokenization (Lexical Layer)**: Tokenization preserves structural boundaries (such as backtick-delimited code spans and unspaced file paths) instead of reducing all text to undifferentiated flat characters.
2. **Affine Gap Scoring (Optimization Layer)**: The dynamic programming sequence alignment replaces flat token counting with affine gap penalties, mathematically penalizing edit boundary alternations so scattered, coincidental matches cannot overcome the cost of opening multiple edit gaps.
3. **Hierarchical Container Alignment (Algebra Layer)**: On the `AlignmentTape`, structural containers (e.g. backticks or quotes) enforce containment coherence: clean single-point edits inside containers are preserved, while fragmented multi-chunk alternations are coalesced into atomic replacements within intact container delimiters.

### Core Mathematical & Architectural Invariants

1. **Container Delimiter Invariance**: Enclosing delimiters (such as matching backticks `` ` `` ... `` ` ``) remain common (`Keep`) whenever both sides share the container construct.
2. **Anti-Fragmentation Guarantee**: The alignment score $S$ penalizes gap openings:
   $$S = \sum \text{MatchReward}(t) - \sum \text{GapOpenCost} - \sum \text{GapExtendCost}$$
   A fragmented match alternating between common and changed text pays multiple gap opening costs, mathematically ensuring that scattered coincidental tokens (like intermediate directories or isolated punctuation) cannot tear apart different semantic units.
3. **Container Coherence**: For any aligned container pair, if the interior edit is not a clean single-point modification (such as a single prefix/suffix change or clean insertion), the entire interior is coalesced into atomic `Strike(old_inner)` and `Fill(new_inner)` chunks, preventing interleaved Frankenstein splices.
4. **Latency Ceiling Invariant**: All tokenization, scoring, and tape operations must remain zero-allocation on the hot path, preserving sub-millisecond per-line diffing and the `< 0.5s` unit test latency budget.

---

## Table of Contents

1. [Phase 1 — Structural Tokenization & Delimited Spans (Lexical Layer)](#1-phase-1--structural-tokenization--delimited-spans-lexical-layer)
2. [Phase 2 — Affine Gap Scoring & Fragmentation Penalties (Optimization Layer)](#2-phase-2--affine-gap-scoring--fragmentation-penalties-optimization-layer)
3. [Phase 3 — Hierarchical Container Alignment on AlignmentTape (Algebra Layer)](#3-phase-3--hierarchical-container-alignment-on-alignmenttape-algebra-layer)
4. [Phase 4 — Golden Acceptance, Verification & Visual Review Signoff](#4-phase-4--golden-acceptance-verification--visual-review-signoff)

---

## 1. Phase 1 — Structural Tokenization & Delimited Spans (Lexical Layer)

**Goal:** Enhance line tokenization to recognize structural containers (such as backtick code spans `` `...` `` in Markdown/doc-comments) and compound path literals (`foo/bar/baz`), providing structural context to the alignment engine.

### Key Deliverables & Affected Files
- `crates/diffuse-core/src/lcs.rs` & `crates/diffuse-core/src/token.rs`:
  - Enhance `tokenize_line` / `tokenize_line_with_profile` to produce tokens with container boundary tags or recognize delimited code spans.
  - Prevent accidental punctuation fusing (such as `./` vs `.` and `/`) while maintaining compound path affinity.
  - Ensure zero allocations on ASCII lines via zero-copy string slices.

### Verification Gate
- Write targeted unit tests in `crates/diffuse-core/src/lcs/tests.rs`:
  - Test tokenization of code spans containing file paths: `` `extension/pi/scripts/install.sh` ``.
  - Test tokenization of compound paths and ensure delimiters are accurately tracked.
- Confirm all tests pass with 100% branch coverage.

---

## 2. Phase 2 — Affine Gap Scoring & Fragmentation Penalties (Optimization Layer)

**Goal:** Implement affine gap penalties / edit boundary cost in the dynamic programming sequence alignment, ensuring that the optimal alignment mathematically rejects fragmented, noisy matches.

### Key Deliverables & Affected Files
- `crates/diffuse-core/src/lcs.rs`:
  - Replace or enhance `raw_lcs_pairs_with_context` with affine gap scoring:
    - Match rewards scaled by token length / character count.
    - Gap open penalty incurred on state transitions between match and del/add.
    - Gap extend penalty for continuing a contiguous deletion or addition.
  - Bounded lookahead and state transitions maintaining linear memory scratchpad and sub-millisecond latency.
  - Preserve `unique_token_anchors` Patience diff acceleration for unambiguous anchors.

### Verification Gate
- Write targeted unit tests in `crates/diffuse-core/src/lcs/tests.rs`:
  - Verify that two different paths sharing only intermediate directories (like `extension/pi/scripts/install.sh` and `./scripts/check.sh`) do NOT match intermediate segments.
  - Verify that contiguous runs are preferred over fragmented matches.
  - Verify latency benchmarks remain `< 0.5s`.

---

## 3. Phase 3 — Hierarchical Container Alignment on AlignmentTape (Algebra Layer)

**Goal:** Implement container coherence on `AlignmentTape`, ensuring that delimited regions (such as backtick spans) maintain structural integrity.

### Key Deliverables & Affected Files
- `crates/diffuse-core/src/tape.rs`:
  - Detect aligned container pairs (e.g. matching opening `` ` `` and closing `` ` ``).
  - Inspect the inner tape chunks between container boundaries:
    - If the inner chunks represent a clean single-point edit (e.g. single addition like `cargo clippy` → `cargo +nightly clippy`, or single prefix deletion like `resolved_layers` → `layers`), retain the granular inner diff.
    - If the inner chunks contain multiple alternating edits/matches (fragmentation), coalesce the interior into a single atomic replacement:
      `TapeChunk::Strike(old_inner)`, `TapeChunk::Fill(new_inner)`.
  - Maintain tape invertibility: $\text{tape.reverse().reverse()} \equiv \text{tape}$.

### Verification Gate
- Write targeted unit tests in `crates/diffuse-core/src/tape/tests.rs`:
  - Verify line 117 scenario: `extension/pi/scripts/install.sh` → `./scripts/check.sh` inside backticks produces:
    `TapeChunk::Keep("`")`, `TapeChunk::Strike("extension/pi/scripts/install.sh")`, `TapeChunk::Fill("./scripts/check.sh")`, `TapeChunk::Keep("`")`.
  - Verify that `cargo clippy` → `cargo +nightly clippy` inside backticks retains clean insertion.
  - Verify that `resolved_layers` → `layers` inside backticks retains prefix deletion.

---

## 4. Phase 4 — Golden Acceptance, Verification & Visual Review Signoff

**Goal:** Run the full verification suite, evaluate wire/golden changes, and verify visual rendering.

### Key Deliverables & Affected Files
- `tests/golden/`:
  - Run `cargo test --test golden` to verify the lib and bin legs across all 8 golden corpora.
  - Audit any modified goldens per `tests/AGENTS.md` (strictly better or equivalent).
  - Verify that `theme-scrollbar-palette` line 117 cleanly renders the replaced path without splitting.
- `./scripts/check.sh`:
  - Run nightly formatting, clippy (`-D warnings`), and all unit/integration tests.
- Human review signoff on visual TUI diff rendering.
