# Monotonic Invertible Reflow & Syntax-Bounded Alignment

- [x] Global Execution Checklist
  - [x] Phase 1: Reproduction Test Harness & Reflow Invertibility Oracle
  - [x] Phase 2: Syntactic Affinity & Aux Row Qualification Gate
  - [x] Phase 3: Monotonic Multi-Line Alignment & Delimiter Anchoring
  - [x] Phase 4: Group Reverse Invertibility & Golden Verification Gate

---

## Executive Summary

When a single line reflows into a multi-line expansion (such as a single `use std::sync::{Arc, Mutex};` import expanding into a multi-line `use std::{ ... };` block), `diffuse` designates the first line as an anchor modification row (`~`) and subsequent addition lines as auxiliary continuation rows (`~+`).

However, the current reflow engine in [`crates/diffuse-core/src/collapse.rs`](crates/diffuse-core/src/collapse.rs) suffers from two critical algorithmic defects:
1. **Unscoped Auxiliary Ingestion**: Every addition within an enclosing delimiter block (e.g., `{ ... }`) is bundled into the aux group, regardless of whether it shares lexical affinity with the donor line.
2. **Non-Monotonic "Bag-of-Tokens" Scavenging**: Deleted characters from the donor line are pooled into an unordered supply. Token claims are evaluated purely by token length and line number, allowing earlier, unrelated lines (e.g. `io::{self, Write},`) to steal structural delimiters (`::{`, `,`, `}`) from later continuation lines (e.g. `sync::{Arc, Mutex, MutexGuard},`).

This breaks the #1 invariant of the diff engine: **monotonic invertibility**. When concatenating non-add runs across the reflow group in visual order, tokens are scrambled out of sequence (e.g. producing `"use std::::{, }syncArcMutex;"`).

This plan transitions multi-line reflow in `collapse.rs` to an **affinity-filtered, monotonically ordered alignment model** where:
- Unrelated additions remain clean `Row::Add` rows without bogus `MovedTgt` tags.
- Structural delimiters cannot be scavenged in isolation from their enclosing identifiers.
- Character allocations across rows are strictly monotonic, guaranteeing that the original source line can be reconstructed faithfully.

---

## Table of Contents

1. [Phase 1 — Reproduction Test Harness & Reflow Invertibility Oracle](#1-phase-1--reproduction-test-harness--reflow-invertibility-oracle)
2. [Phase 2 — Syntactic Affinity & Aux Row Qualification Gate](#2-phase-2--syntactic-affinity--aux-row-qualification-gate)
3. [Phase 3 — Monotonic Multi-Line Alignment & Delimiter Anchoring](#3-phase-3--monotonic-multi-line-alignment--delimiter-anchoring)
4. [Phase 4 — Group Reverse Invertibility & Golden Verification Gate](#4-phase-4--group-reverse-invertibility--golden-verification-gate)

---

## 1. Phase 1 — Reproduction Test Harness & Reflow Invertibility Oracle

Establish automated regression tests capturing the exact failure mode and define an invariant check for group reflow reconstructibility.

### Concrete Steps
1. **Targeted Unit Test in `collapse/tests.rs`**:
   - Add `test_import_block_expansion_delimiter_hijacking` in [`crates/diffuse-core/src/collapse/tests.rs`](crates/diffuse-core/src/collapse/tests.rs).
   - Input:
     ```rust
     -use std::sync::{Arc, Mutex};
     +use std::{
     +    fs,
     +    io::{self, Write},
     +    sync::{Arc, Mutex, MutexGuard},
     +};
     ```
   - Assert:
     - Line 2 (`fs,`) and Line 3 (`io::{...}`) must **not** claim `::{`, `,`, or `}` from `sync::{Arc, Mutex};`.
     - Line 4 (`sync::{Arc, Mutex, MutexGuard},`) must preserve its own delimiters `::{` and content `Arc`, `Mutex` in monotonic order.
     - Concatenating context (`_`) and moved source (`<`) / target (`>`) across the reflow block must reconstruct the exact original string `"use std::sync::{Arc, Mutex};"` without permutation or duplication.
2. **Reflow Invertibility Oracle**:
   - In [`crates/diffuse-core/src/merge/tests.rs`](crates/diffuse-core/src/merge/tests.rs) or [`crates/diffuse-core/src/reflow/tests.rs`](crates/diffuse-core/src/reflow/tests.rs), author an oracle function `assert_reflow_group_invertible(anchor: &Row, aux_rows: &[Row], expected_old: &str)`.
   - Confirm initial failure on the unmodified codebase.

---

## 2. Phase 2 — Syntactic Affinity & Aux Row Qualification Gate

Prevent unrelated addition rows from being indiscriminately absorbed into the donor line's auxiliary group.

### Concrete Steps
1. **Affinity Filtering in `collapse.rs`**:
   - Inspect [`crates/diffuse-core/src/collapse.rs`](crates/diffuse-core/src/collapse.rs#L501-L532) (`opens_delim` loop).
   - Rather than admitting every line ending with `,` or `(` into `next_adds`, evaluate candidate rows for **lexical affinity** with the donor line:
     - Candidate lines must either match an alphanumeric identifier present in the donor line or match the donor line's closing delimiter.
     - Addition lines with zero content overlap (e.g. `fs,` and `io::{self, Write},`) are excluded from `next_adds` and remain clean `Row::Add` rows without `RowGroup::Aux`.
2. **Closing Row Separation**:
   - Ensure the closing row (e.g. `};`) is only paired with the donor if the donor line contains the matching closing delimiter and trailing punctuation (e.g. `;`).

---

## 3. Phase 3 — Monotonic Multi-Line Alignment & Delimiter Anchoring

Enforce monotonic character ordering and bind structural delimiters to their associated identifiers during token allocation.

### Concrete Steps
1. **Delimiter Anchoring Rule**:
   - In [`crates/diffuse-core/src/collapse.rs`](crates/diffuse-core/src/collapse.rs#L663-L705) (claim collection):
   - Prohibit bare delimiter claims (`::{`, `(`, `)`, `{`, `}`, `,`) unless accompanied by an adjacent content token that matches the donor line.
   - Structural syntax envelopes cannot be scavenged across lines in isolation.
2. **Monotonic Allocation Gate**:
   - In the greedy allocation loop ([`crates/diffuse-core/src/collapse.rs`](crates/diffuse-core/src/collapse.rs#L919-L936)):
   - Track the latest consumed source offset in `del_text` per row and across rows.
   - Enforce the monotonicity invariant: a claim on an earlier line index $i_1 < i_2$ cannot consume source characters located *after* characters consumed by line $i_2$.
   - Any claim that violates source-order monotonicity is rejected and remains a normal addition (`+`), preventing out-of-order token scattering.

---

## 4. Phase 4 — Group Reverse Invertibility & Golden Verification Gate

Verify that the reflow changes preserve all existing diff semantics while strictly fixing the reconstruction anomaly.

### Concrete Steps
1. **Invertibility Verification**:
   - Verify that all unit tests in `crates/diffuse-core` pass, including `block_reflow_involution` and the new `test_import_block_expansion_delimiter_hijacking`.
2. **Golden Acceptance Suite**:
   - Execute `cargo test --test golden`.
   - Review any golden test diffs per [`tests/AGENTS.md`](tests/AGENTS.md). Confirm that any fixture changes strictly eliminate delimiter hijacking and preserve correct alignment.
3. **Verification Gate**:
   - Run `scripts/check.sh` to ensure formatting, clippy, and all workspace tests pass with zero warnings in under 3 seconds.
