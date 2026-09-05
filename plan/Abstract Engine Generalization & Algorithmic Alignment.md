# Abstract Engine Generalization & Algorithmic Alignment

- [x] 1. [Phase 1 — Engine Parameterization & Symmetrical Block Reflow (`crates/diffuse-core/src/cluster/dp.rs`)](#1-phase-1--engine-parameterization--symmetrical-block-reflow-cratesdiffuse-coresrcclusterdprs)
- [ ] 2. [Phase 2 — Declarative Delimiters & Language Profile Cleanliness (`crates/diffuse-syntax/`)](#2-phase-2--declarative-delimiters--language-profile-cleanliness-cratesdiffuse-syntax)
- [ ] 3. [Phase 3 — Bounded Maximal Contiguous Token Affinity in LCS (`crates/diffuse-core/src/lcs/mod.rs`)](#3-phase-3--bounded-maximal-contiguous-token-affinity-in-lcs-cratesdiffuse-coresrclcsmodrs)
- [ ] 4. [Phase 4 — Generalized Prefix Containment & Indentation Scoping (`crates/diffuse-core/src/cluster/dp.rs`)](#4-phase-4--generalized-prefix-containment--indentation-scoping-cratesdiffuse-coresrcclusterdprs)
- [ ] 5. [Phase 5 — Comprehensive Verification & Acceptance Gate](#5-phase-5--comprehensive-verification--acceptance-gate)

---

## Executive Summary

The diffuse diff engine aims to be strictly language-agnostic, mathematically symmetric, and renderer-independent. Recent bug fixes and behavioral improvements (preventing delimiter/parenthesis splitting, receiver-to-statement pairing across collapsed multiline expressions, and direct tail closing delimiter matching) introduced targeted adjustments and speculative functions that fall short of a clean, top-level architectural design.

Specifically, the current change set contains:
1. **Dead Speculative Residue**: `is_method_continuation_match` in `crates/diffuse-syntax/src/profile.rs` was conceived as a workaround for matching method continuations, but when wired into dynamic programming it caused regressions (e.g. `opt_1` where `.push(...)` stole an entire statement line from its receiver), leading to its removal from `dp.rs` while remaining as dead code in `diffuse-syntax`.
2. **Unbounded Indentation Shift**: Completely removing `om.indent == nm.indent` from prefix containment permits any two lines sharing an 8-character prefix to receive full containment bonuses across arbitrary nesting scopes.
3. **Expand vs Collapse Asymmetry**: 1-to-K expansions require `om.indent == nm.indent`, while K-to-1 collapses do not, violating mathematical involution and symmetry under hunk reversal.
4. **Hardcoded Magic Numbers & Ad-Hoc Aborts**: `count == 0` aborts in `block_lcs_fast_precomputed` and hardcoded character minimums (`8`) in `is_direct_tail_match` bypass the engine's central `Weights` configuration.
5. **Unbounded Lookahead Scans in LCS**: Combining backward and forward run walks in `raw_lcs_pairs_with_lookahead` performs quadratic $O(n^2)$ token scans on repeated delimiters.

This plan refactors these targeted workarounds into a cohesive, top-tier engine architecture grounded in four foundational pillars:
- **Tunable Engine Weights**: Expose indentation shift bounds, tail match minimums, and block reflow coverage thresholds directly in `Weights`.
- **Declarative Language Profiles**: Purge dead speculative method-matching methods and define syntax delimiters declaratively.
- **Symmetric Block Reflow**: Unify 1-to-K and K-to-1 reflow conditions under identical indentation penalty semantics.
- **Bounded Contiguous Token Affinity**: Replace unconstrained token searches with a windowed contiguous affinity metric in LCS backtracing.

---

## Table of Contents

1. [Phase 1 — Engine Parameterization & Symmetrical Block Reflow (`crates/diffuse-core/src/cluster/dp.rs`)](#1-phase-1--engine-parameterization--symmetrical-block-reflow-cratesdiffuse-coresrcclusterdprs)
2. [Phase 2 — Declarative Delimiters & Language Profile Cleanliness (`crates/diffuse-syntax/`)](#2-phase-2--declarative-delimiters--language-profile-cleanliness-cratesdiffuse-syntax)
3. [Phase 3 — Bounded Maximal Contiguous Token Affinity in LCS (`crates/diffuse-core/src/lcs/mod.rs`)](#3-phase-3--bounded-maximal-contiguous-token-affinity-in-lcs-cratesdiffuse-coresrclcsmodrs)
4. [Phase 4 — Generalized Prefix Containment & Indentation Scoping (`crates/diffuse-core/src/cluster/dp.rs`)](#4-phase-4--generalized-prefix-containment--indentation-scoping-cratesdiffuse-coresrcclusterdprs)
5. [Phase 5 — Comprehensive Verification & Acceptance Gate](#5-phase-5--comprehensive-verification--acceptance-gate)

---

## 1. Phase 1 — Engine Parameterization & Symmetrical Block Reflow (`crates/diffuse-core/src/cluster/dp.rs`)

### Objective
Eliminate magic constants and restore mathematical symmetry between 1-to-K expansions and K-to-1 collapses by promoting alignment parameters into [`Weights`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/dp.rs#L67-L106).

### Steps
1. **Extend `Weights` with Missing Alignment Parameters**:
   Add the following fields to `Weights` in [`crates/diffuse-core/src/cluster/dp.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/dp.rs):
   ```rust
   pub struct Weights {
       // ... existing fields ...
       /// Maximum indentation shift (in characters) permitted for prefix containment
       /// and block reflow before being disqualified.
       pub max_indent_shift: usize,
       /// Minimum character count required for direct closing delimiter tail pairing.
       pub min_tail_matched_chars: usize,
       /// Minimum ratio of lines in a 1-to-K or K-to-1 block that must have matching content.
       pub min_block_line_coverage: f64,
   }
   ```
   Provide default values:
   - `max_indent_shift: 16` (accommodates up to 4 indentation levels of 4 spaces, or 2 of 8 spaces).
   - `min_tail_matched_chars: 8`.
   - `min_block_line_coverage: 0.50`.

2. **Refactor `block_lcs_fast_precomputed`**:
   Replace the hardcoded `if count == 0 { return (0, 0, 0); }` abort with a coverage check:
   ```rust
   let lines_with_matches = line_matched_counts[..k].iter().filter(|&&c| c > 0).count();
   if (lines_with_matches as f64 / k as f64) < min_block_line_coverage {
       return (0, 0, 0);
   }
   ```
   This ensures multi-line reflow blocks are evaluated systematically rather than abruptly aborted on empty or whitespace-only formatting lines.

3. **Enforce Involution & Symmetry in Expand vs Collapse**:
   - In **Expand (1-to-K)** around line 1337, replace `&& om.indent == nm.indent` with:
     ```rust
     let indent_diff = (om.indent as isize - nm.indent as isize).unsigned_abs();
     indent_diff <= w.max_indent_shift
         && om.comment == nm.comment
         && om.is_reflow
     ```
   - In **Collapse (K-to-1)** around line 1421, apply the exact same precondition:
     ```rust
     let indent_diff = (om.indent as isize - nm.indent as isize).unsigned_abs();
     indent_diff <= w.max_indent_shift
         && om.comment == nm.comment
         && nm.is_reflow
     ```
   - Both directions penalize the indentation difference using `w.gamma * indent_diff * w.indent_penalty_factor`, ensuring inverse operations have identical objective scores.

---

## 2. Phase 2 — Declarative Delimiters & Language Profile Cleanliness (`crates/diffuse-syntax/`)

### Objective
Clean up the syntax profile interface by removing speculative method-continuation heuristics, parameterizing closing delimiter thresholds, and formalizing delimiter definitions declaratively.

### Steps
1. **Purge Speculative Method Continuation Functions**:
   Remove the following dead/unused functions from [`crates/diffuse-syntax/src/profile.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-syntax/src/profile.rs):
   - `is_method_continuation`
   - `method_continuation_name`
   - `contains_method_call`
   - `is_method_continuation_match`
   - `CloseDelimiter::default_is_method_continuation_match`
   Statement and receiver evolution (such as `unmark_positions` $\to$ `unmark_positions.extend(...)`) is handled universally through Prefix Containment (Phase 4), rendering these ad-hoc method heuristics obsolete.

2. **Parameterize `is_direct_tail_match`**:
   Update `CloseDelimiter::default_is_direct_tail_match` and `LangProfile::is_direct_tail_match` to accept `min_chars: usize` as a parameter rather than hardcoding `8`:
   ```rust
   pub fn is_direct_tail_match(
       &self,
       tail_a: CloseDelimiter,
       tail_b: CloseDelimiter,
       matched_chars: usize,
       min_chars: usize,
   ) -> bool
   ```

3. **Declarative Syntax Delimiter Tables**:
   Structure closing delimiters and trailing statement delimiters as clean tables or slice constants on `LangProfile` rather than procedural `if/else` ladders:
   - Provide `exact_delimiters: &'static [(&'static str, CloseDelimiter)]`
   - Provide `tail_delimiters: &'static [(&'static str, CloseDelimiter)]`
   - Provide `trailing_delimiters: &'static [char]`
   Ensure `trim_trailing_delimiters` and `classify_close_tail` operate over these declarative specifications.

4. **Update Syntax Unit Tests**:
   Update [`crates/diffuse-syntax/src/tests.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-syntax/src/tests.rs) to exercise the cleaned API and maintain 100% branch coverage across all `CloseDelimiter` and `LangProfile` methods.

---

## 3. Phase 3 — Bounded Maximal Contiguous Token Affinity in LCS (`crates/diffuse-core/src/lcs/mod.rs`)

### Objective
Prevent delimiter/parenthesis splitting (e.g. `!run.is_empty()` splitting `()`) while eliminating unbounded linear scans and out-of-path backward walks in LCS tie-breaking.

### Steps
1. **Define Bounded Lookahead Window**:
   In [`crates/diffuse-core/src/lcs/mod.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/lcs/mod.rs), define `const MAX_TIE_LOOKAHEAD: usize = 16`.
   Limit the search range for better candidates to `(i + 1..(i + 1 + MAX_TIE_LOOKAHEAD).min(n))`.

2. **Formalize Contiguous Token Affinity Metric**:
   Replace ad-hoc backward/forward run summation with a formal **Contiguous Token Affinity** function:
   ```rust
   /// Measures the contiguous non-whitespace token cluster length centered at (i, j)
   /// along the current backtrace path.
   fn contiguous_affinity(a: &[&str], b: &[&str], i: usize, j: usize) -> usize {
       let backward = contiguous_non_ws_run_backward(a, b, i, j);
       let forward = contiguous_non_ws_run(a, b, i, j);
       backward + forward
   }
   ```
   For candidate alternative positions `k > i`:
   - Backward context from `k` must only count tokens matching `b` that do not overlap with tokens already committed prior to `i`.
   - Bounding `k` to `MAX_TIE_LOOKAHEAD` guarantees strict $O(n)$ backtrace latency.

3. **Unit Testing**:
   Add targeted tests in [`crates/diffuse-core/src/lcs/tests.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/lcs/mod.rs):
   - Preserving intact parentheses in `if !run.is_empty() {` when alternative identical closing parentheses follow.
   - Preserving contiguous chained method calls without splitting operator punctuation.

---

## 4. Phase 4 — Generalized Prefix Containment & Indentation Scoping (`crates/diffuse-core/src/cluster/dp.rs`)

### Objective
Establish Prefix Containment as the primary, general mechanism for multiline statement splits and single-line statement collapses, bounded by language-aware indentation scoping.

### Steps
1. **Apply Scoped Indentation Bounds**:
   In [`crates/diffuse-core/src/cluster/dp.rs:1255`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/dp.rs#L1241-L1259), evaluate prefix containment using `w.max_indent_shift`:
   ```rust
   let indent_shift = (om.indent as isize - nm.indent as isize).unsigned_abs();
   let is_prefix_containment = {
       let ot_strip = trim_trailing_delims(ot_trim, profile);
       let nt_strip = trim_trailing_delims(nt_trim, profile);
       let min_len = ot_strip.len().min(nt_strip.len());
       let is_prefix_at_boundary = |longer: &str, shorter: &str| -> bool {
           if let Some(rest) = longer.strip_prefix(shorter) {
               rest.is_empty()
                   || rest.starts_with(|c: char| {
                       c.is_whitespace() || (!c.is_alphanumeric() && c != '_')
                   })
           } else {
               false
           }
       };
       min_len >= w.min_containment_len
           && indent_shift <= w.max_indent_shift
           && (om.comment == nm.comment)
           && (is_prefix_at_boundary(ot_strip, nt_strip)
               || is_prefix_at_boundary(nt_strip, ot_strip))
   };
   ```

2. **Penalty Formulation**:
   - The core unmatched characters of the prefix are discounted via `min_chars`:
     `let core_unmatched = min_chars.saturating_sub(matched);`
     `let length_diff = (total - min_chars) as f64;`
     `let pen = w.mu * core_unmatched as f64 + w.gamma * length_diff;`
   - Indentation shift is penalized proportionally by the existing term:
     `let indent_penalty = w.gamma * indent_diff * w.indent_penalty_factor;`
   - This ensures statements shifted by 1 or 2 indentation levels (such as `unmark_positions` $\to$ `unmark_positions.extend(...)` shifting by 8 spaces) match cleanly while distant cross-scope shifts receive heavy cumulative penalties.

---

## 5. Phase 5 — Comprehensive Verification & Acceptance Gate

### Objective
Verify that all clean abstractions preserve exact golden output across all 14 acceptance suites, achieve 100% branch coverage, and adhere to strict performance invariants.

### Steps
1. **Targeted Unit Suite**:
   Run `cargo test -p diffuse-core test_head_commit_hunk_render` to confirm:
   - Line 150 pairs with the old if statement as `Mod` without split parentheses.
   - Line 156 is a clean `Add`.
   - Line 166 pairs with old receiver line 148 as `Mod`.
2. **100% Branch Coverage**:
   Ensure all match arms, closure branches, and edge cases in `diffuse-syntax` and `diffuse-core` have dedicated unit tests.
3. **Golden Acceptance Suite**:
   Run `cargo test -p diffuse --test golden` to verify byte-for-byte fidelity across all 14 golden integration test corpora (both lib and bin legs).
4. **Full Workspace Gate**:
   Execute `./scripts/check.sh` to confirm formatting, clippy (`-D warnings`), all cargo test suites, and npm extension smoke tests pass cleanly under the latency budget.
5. **Dev Binary Installation**:
   Confirm dev binary (`PROFILE=dev`) is built and installed to `~/.cargo/bin/diffuse` and the pi review extension directory with DbC invariants active.
