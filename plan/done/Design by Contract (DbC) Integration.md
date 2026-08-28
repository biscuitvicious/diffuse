# Design by Contract (DbC) Integration Plan

- [x] **Phase 1 — Core DBC Module & Zero-Cost Macro Infrastructure**
  - [x] Implement `crates/diffuse-core/src/dbc/mod.rs` with `require!`, `ensure!`, `invariant!`, and `check!` macros.
  - [x] Support custom diagnostic formatting with expression stringification, caller location, and structured tracing integration.
  - [x] Add comprehensive unit tests in `crates/diffuse-core/src/dbc/tests.rs` asserting zero-overhead release behavior and failure formatting.
  - [x] Export `pub mod dbc` and re-export `require!`, `ensure!`, `invariant!` across the workspace.
- [x] **Phase 2 — Algorithm & LCS Contract Layer**
  - [x] Instrument `MyersOnD::diff` with preconditions, snake grid bounds invariants, and shortest edit script postconditions.
  - [x] Instrument `HirschbergLcs::lcs` with strict pairing monotonicity and subsequence equality postconditions.
  - [x] Instrument `word_spans` & `lcs_pairs` with char-offset validity, non-overlapping span partition, and whitespace boundary postconditions.
- [x] **Phase 3 — Diff Parsing, Hunk Shifting & Row Collapse Contracts**
  - [x] Add contract specifications to `parse_diff` (valid line numbers, non-empty hunk rows, status consistency).
  - [x] Add contract specifications to `pass_shift_hunk` (semantic diff operation conservation, line delta invariance).
  - [x] Add contract specifications to `collapse_hunk_rows` (non-overlapping synthesized context, exact row coverage).
- [x] **Phase 4 — Move Detection, Reversibility & Count Contracts**
  - [x] Instrument `mark_moves` ensuring move-pair content identity and add/del balance conservation.
  - [x] Instrument `file_counts` and `FileCounts::reverse` with count arithmetic invariants and involution identity ($f(f(x)) = x$).
- [x] **Phase 5 — Streaming Pipeline & Priority Queue State Contracts**
  - [x] Instrument `PriorityJobQueue` with heap order invariants and monotonic sequence delivery postconditions.
  - [x] Instrument `process_single_diff_file` and `stream_diff_pipeline_arc` with job index sequencing contracts.
- [x] **Phase 6 — Syntax Tokenization & Boundary Partition Contracts**
  - [x] Instrument `classify_tokens` and `for_each_classified_token` with strict slice bounds and gapless partition postconditions.
- [x] **Phase 7 — TUI Coordinate Navigation & Screen Invariant Contracts**
  - [x] Instrument `flat_rows` indexing and coordinate lookups with row range invariants.
  - [x] Instrument `toggle_raw_diff` with cursor coordinate roundtrip invariance.
- [x] **Phase 8 — Test Suite, Performance Budget & Acceptance Gate Verification**
  - [x] Ensure 100% branch coverage on all new DBC infrastructure.
  - [x] Verify test execution latency invariants (<0.5s per test, <3s warm check gate).
  - [x] Confirm full `./scripts/check.sh` passes cleanly with zero warnings.

---

## Executive Summary

This plan integrates **Design by Contract (DbC)** deeply into `diffuse`. By introducing declarative preconditions (`require!`), postconditions (`ensure!`), and internal invariants (`invariant!`), we establish mathematically verifiable runtime boundaries for all core algorithms, AST transformations, streaming channels, syntax classifiers, and TUI coordinate mappers.

### Core Architectural Principles:
1. **Zero-Cost Release Guarantees**: All contract assertions compile down to `debug_assert!` equivalents in `--release` builds, guaranteeing zero runtime overhead in production.
2. **Mathematical Invariant Verification**: Verifies formal properties at runtime during tests and dev builds (e.g., LCS monotonic index order, edit script equivalence, non-overlapping span partitions, and diff reversibility involution).
3. **Structured Diagnostics & Tracing Synergy**: When a contract is violated, the macro provides file, line, expression stringification, formatted diagnostic context, and logs to the active `tracing` subscriber before failing.

---

## Table of Contents

1. [Phase 1 — Core DBC Module & Zero-Cost Macro Infrastructure](#1-phase-1--core-dbc-module--zero-cost-macro-infrastructure)
2. [Phase 2 — Algorithm & LCS Contract Layer](#2-phase-2--algorithm--lcs-contract-layer)
3. [Phase 3 — Diff Parsing, Hunk Shifting & Row Collapse Contracts](#3-phase-3--diff-parsing-hunk-shifting--row-collapse-contracts)
4. [Phase 4 — Move Detection, Reversibility & Count Contracts](#4-phase-4--move-detection-reversibility--count-contracts)
5. [Phase 5 — Streaming Pipeline & Priority Queue State Contracts](#5-phase-5--streaming-pipeline--priority-queue-state-contracts)
6. [Phase 6 — Syntax Tokenization & Boundary Partition Contracts](#6-phase-6--syntax-tokenization--boundary-partition-contracts)
7. [Phase 7 — TUI Coordinate Navigation & Screen Invariant Contracts](#7-phase-7--tui-coordinate-navigation--screen-invariant-contracts)
8. [Phase 8 — Test Suite, Performance Budget & Acceptance Gate Verification](#8-phase-8--test-suite-performance-budget--acceptance-gate-verification)

---

## 1. Phase 1 — Core DBC Module & Zero-Cost Macro Infrastructure

### Objectives
- Create a dedicated, native `crates/diffuse-core/src/dbc/mod.rs` module providing `require!`, `ensure!`, `invariant!`, and `check!` macros.
- Provide clean, developer-friendly diagnostic formatting that prints the exact contract type (`PRECONDITION`, `POSTCONDITION`, `INVARIANT`), expression text, file/line, and optional user-provided details.
- Ensure seamless export across all workspace crates (`diffuse-core`, `diffuse-syntax`, `diffuse-render`, `diffuse-tui`, `diffuse`).

### Implementation Steps
1. **Create `crates/diffuse-core/src/dbc/mod.rs`**:
   ```rust
   #[macro_export]
   macro_rules! require {
       ($cond:expr $(,)?) => {
           if cfg!(debug_assertions) && !$cond {
               $crate::dbc::fail_contract("PRECONDITION", stringify!($cond), file!(), line!(), None);
           }
       };
       ($cond:expr, $($arg:tt)+) => {
           if cfg!(debug_assertions) && !$cond {
               let msg = format!($($arg)+);
               $crate::dbc::fail_contract("PRECONDITION", stringify!($cond), file!(), line!(), Some(&msg));
           }
       };
   }

   #[macro_export]
   macro_rules! ensure {
       ($cond:expr $(,)?) => {
           if cfg!(debug_assertions) && !$cond {
               $crate::dbc::fail_contract("POSTCONDITION", stringify!($cond), file!(), line!(), None);
           }
       };
       ($cond:expr, $($arg:tt)+) => {
           if cfg!(debug_assertions) && !$cond {
               let msg = format!($($arg)+);
               $crate::dbc::fail_contract("POSTCONDITION", stringify!($cond), file!(), line!(), Some(&msg));
           }
       };
   }

   #[macro_export]
   macro_rules! invariant {
       ($cond:expr $(,)?) => {
           if cfg!(debug_assertions) && !$cond {
               $crate::dbc::fail_contract("INVARIANT", stringify!($cond), file!(), line!(), None);
           }
       };
       ($cond:expr, $($arg:tt)+) => {
           if cfg!(debug_assertions) && !$cond {
               let msg = format!($($arg)+);
               $crate::dbc::fail_contract("INVARIANT", stringify!($cond), file!(), line!(), Some(&msg));
           }
       };
   }
   ```
2. **Failure Handler (`fail_contract`)**:
   - Logs an `ERROR` event to `tracing` (if tracing is initialized).
   - Panics with a standardized diagnostic message:
     ```text
     [DBC: PRECONDITION FAILED] at crates/diffuse-core/src/algo/hirschberg.rs:27
       Condition: a_idx < a.len() && b_idx < b.len()
       Details:   Index out of bounds during LCS pairing
     ```
3. **Unit Tests in `crates/diffuse-core/src/dbc/tests.rs`**:
   - Test passing assertions for `require!`, `ensure!`, `invariant!`.
   - Test `#[should_panic]` behavior and formatted error messages on violations.

---

## 2. Phase 2 — Algorithm & LCS Contract Layer

### Objectives
- Enforce formal contracts on core dynamic programming algorithms.

### Implementation Steps
1. **`crates/diffuse-core/src/algo/hirschberg.rs` (`HirschbergLcs`)**:
   - `require!`: Valid slices passed to `lcs`.
   - `ensure!`:
     - Monotonicity: For all $(k_1, k_2)$ with $k_1 < k_2$, $a_{k_1} < a_{k_2}$ and $b_{k_1} < b_{k_2}$.
     - Equality: For all $(a_i, b_j) \in \text{pairs}$, $a[a_i] == b[b_j]$.
     - Bound: `pairs.len() <= a.len().min(b.len())`.
2. **`crates/diffuse-core/src/algo/myers_ond.rs` (`MyersOnD`)**:
   - `ensure!`:
     - Applying the returned `Vec<EditOp>` to slice `A` reconstructs slice `B`.
     - Count of `Keep` matches `a.len() - deletions` and `b.len() - insertions`.
3. **`crates/diffuse-core/src/lcs/mod.rs` (`word_spans` & `lcs_pairs`)**:
   - `require!`: Valid UTF-8 char slices.
   - `ensure!`:
     - Spans in `WordSpans.del` are within `[0, char_len(old_text)]`.
     - Spans in `WordSpans.add` are within `[0, char_len(new_text)]`.
     - Spans are strictly sorted and non-overlapping: `span[k].1 <= span[k+1].0`.

---

## 3. Phase 3 — Diff Parsing, Hunk Shifting & Row Collapse Contracts

### Objectives
- Guard diff AST transformations against invalid states or corrupted context syntheses.

### Implementation Steps
1. **`crates/diffuse-core/src/parse/mod.rs` (`parse_diff`)**:
   - `ensure!`:
     - Every parsed `DiffFile` has valid line counts (`old_line_count >= 0`, `new_line_count >= 0`).
     - Every `Hunk` has `new_start >= 1` (or 0 for empty files) and `old_start >= 1`.
2. **`crates/diffuse-core/src/collapse/mod.rs` (`collapse_hunk_rows` & `pass_shift_hunk`)**:
   - `require!`: Hunk row count corresponds to diff operations.
   - `ensure!`:
     - Output row sequence has non-decreasing line numbers on both old and new sides.
     - Collapsed `Mod` rows carry non-empty `old_line` and `new_line`.
     - Synthesized context rows match original old/new content buffers when provided.

---

## 4. Phase 4 — Move Detection, Reversibility & Count Contracts

### Objectives
- Verify move detection consistency and arithmetic conservation of change counts.

### Implementation Steps
1. **`crates/diffuse-core/src/moves/mod.rs` (`mark_moves`)**:
   - `ensure!`:
     - Any row marked as `Mov` has identical trimmed text on both old and new sides.
     - Move pairing is balanced: each moved addition has an exact corresponding moved deletion.
2. **`crates/diffuse-core/src/counts/mod.rs` (`file_counts`, `reverse`)**:
   - `ensure!`:
     - `file_counts.adds + file_counts.dels + file_counts.mods + file_counts.movs` accurately partitions all changed rows in the file.
     - Involution contract: `counts.reverse().reverse() == counts`.

---

## 5. Phase 5 — Streaming Pipeline & Priority Queue State Contracts

### Objectives
- Enforce concurrency and ordering invariants across the streaming pipeline.

### Implementation Steps
1. **`crates/diffuse-core/src/streaming/mod.rs` (`PriorityJobQueue`)**:
   - `invariant!`: Heap property maintained across `push` and `pop`.
   - `ensure!`: Sequencer outputs files in strictly ascending order of `idx` (`0, 1, 2, ...`).
2. **`crates/diffuse-core/src/pipeline/mod.rs` (`process_single_diff_file`)**:
   - `ensure!`: `OutputFile.path == f.path` and hunk counts match processed output.

---

## 6. Phase 6 — Syntax Tokenization & Boundary Partition Contracts

### Objectives
- Ensure syntax token streams cleanly partition code lines without gaps or overlapping tokens.

### Implementation Steps
1. **`crates/diffuse-syntax/src/lib.rs` (`classify_tokens`)**:
   - `require!`: Line text is valid UTF-8.
   - `ensure!`:
     - Emitted token byte ranges `[start, end)` are strictly ordered: $start_k < end_k \le start_{k+1}$.
     - Sum of token lengths equals line length (or covers non-whitespace tokens exactly).

---

## 7. Phase 7 — TUI Coordinate Navigation & Screen Invariant Contracts

### Objectives
- Guard cursor positioning, flat row indexing, and view toggle roundtrips.

### Implementation Steps
1. **`crates/diffuse-tui/src/app/render.rs` (`toggle_raw_diff`)**:
   - `invariant!`: Flat row array is non-empty when files are present.
   - `ensure!`: `self.row_idx < self.flat_rows.len()`.
   - `ensure!`: Visual scroll offset satisfies `self.scroll <= self.row_idx`.

---

## 8. Phase 8 — Test Suite, Performance Budget & Acceptance Gate Verification

### Objectives
- Validate that all contracts pass across the full test suite and latency budgets remain intact.

### Verification Steps
1. **Test-First Discipline**:
   - Write targeted contract violation tests in `dbc/tests.rs` and verify clean panics in debug mode.
2. **Latency Budget Verification**:
   - Run `cargo test` to ensure all unit tests complete in `<0.5s` and the test binary completes in `<1.5s`.
   - Run `./scripts/check.sh` and ensure warm execution finishes in `<3s`.
3. **Release Mode Verification**:
   - Compile in `--release` and verify that contract checks are completely eliminated.
