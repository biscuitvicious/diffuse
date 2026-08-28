# Inline Test Decoupling & Test Suite Performance Optimization

- [x] Phase 1 — Inline Test Module Decoupling Across Workspace Crates
- [x] Phase 2 — Test Suite Profiling, Hot-Spot Elimination & Latency Budget Enforcement
- [·] Phase 3 — Golden Fixture JSON Format Deconstruction & Dedicated Per-Corpus File Storage

---

## Executive Summary

This plan addresses two critical aspects of developer velocity and codebase hygiene in the `diffuse` repository:

1. **Inline Test Module Decoupling**: In accordance with the Rust codebase component layout conventions ([`CODE_STYLE.md`](../CODE_STYLE.md) and [`GUIDELINES.md`](../GUIDELINES.md)), component source files should present clean, top-down abstractions without being cluttered by hundred-to-thousand-line `#[cfg(test)] mod tests` blocks at the bottom of single files. All inline test blocks will be decoupled into dedicated `tests.rs` companion files following the standard submodule pattern (`<module>/mod.rs` + `<module>/tests.rs`).

2. **Test Suite Latency Optimization**: As specified in [`AGENTS.md`](../AGENTS.md), the test suite enforces strict performance budgets:
   - Any individual `#[test]` must execute in **`< 0.5s`**.
   - Any test binary (e.g. `diffuse-tui`, `golden.rs`, `diffuse-core`) must complete in **`< 1.5s`**.
   - The entire `./scripts/check.sh` test run must execute rapidly without stalling developer iteration loops.
   
   Currently, `diffuse-tui` unit tests take ~6–8s and `golden.rs` takes ~6s. We will profile each individual test, identify algorithmic hot spots (e.g., redundant uncompressed golden runs, synthetic heavy diff generation in unit tests, or expensive rendering allocations), and optimize or appropriately gate them to bring execution times strictly under the budget.

---

## Table of Contents

1. [Phase 1 — Inline Test Module Decoupling Across Workspace Crates](#1-phase-1--inline-test-module-decoupling-across-workspace-crates)
2. [Phase 2 — Test Suite Profiling, Hot-Spot Elimination & Latency Budget Enforcement](#2-phase-2--test-suite-profiling-hot-spot-elimination--latency-budget-enforcement)
3. [Phase 3 — Golden Fixture JSON Format Deconstruction & Dedicated Per-Corpus File Storage](#3-phase-3--golden-fixture-json-format-deconstruction--dedicated-per-corpus-file-storage)

---

## 1. Phase 1 — Inline Test Module Decoupling Across Workspace Crates

**Goal:** Decouple all remaining inline `#[cfg(test)] mod tests { ... }` blocks across `diffuse-core`, `diffuse-render`, `diffuse-tui`, and `diffuse` into dedicated `<module>/mod.rs` and `<module>/tests.rs` files.

### 1.1 `diffuse-core`
- `src/apply.rs` $\rightarrow$ `src/apply/mod.rs` + `src/apply/tests.rs`
- `src/git.rs` $\rightarrow$ `src/git/mod.rs` + `src/git/tests.rs`
- `src/old_content.rs` $\rightarrow$ `src/old_content/mod.rs` + `src/old_content/tests.rs`
- `src/streaming.rs` $\rightarrow$ `src/streaming/mod.rs` + `src/streaming/tests.rs`
- `src/visible.rs` $\rightarrow$ `src/visible/mod.rs` + `src/visible/tests.rs`

### 1.2 `diffuse-render`
- `src/diff.rs` $\rightarrow$ `src/diff/mod.rs` + `src/diff/tests.rs`
- `src/markdown.rs` $\rightarrow$ `src/markdown/mod.rs` + `src/markdown/tests.rs`
- `src/row.rs` $\rightarrow$ `src/row/mod.rs` + `src/row/tests.rs`
- `src/theme.rs` $\rightarrow$ `src/theme/mod.rs` + `src/theme/tests.rs`

### 1.3 `diffuse-tui`
- `src/actions.rs` $\rightarrow$ `src/actions/mod.rs` + `src/actions/tests.rs`
- `src/app/collapse.rs` $\rightarrow$ `src/app/collapse/mod.rs` + `src/app/collapse/tests.rs`
- `src/app/notes.rs` $\rightarrow$ `src/app/notes/mod.rs` + `src/app/notes/tests.rs`
- `src/app/selection.rs` $\rightarrow$ `src/app/selection/mod.rs` + `src/app/selection/tests.rs`
- `src/app/sync.rs` $\rightarrow$ `src/app/sync/mod.rs` + `src/app/sync/tests.rs`
- `src/editor.rs` $\rightarrow$ `src/editor/mod.rs` + `src/editor/tests.rs`
- `src/event.rs` $\rightarrow$ `src/event/mod.rs` + `src/event/tests.rs`
- `src/finder.rs` $\rightarrow$ `src/finder/mod.rs` + `src/finder/tests.rs`
- `src/terminal.rs` $\rightarrow$ `src/terminal/mod.rs` + `src/terminal/tests.rs`
- `src/view/diff_grid.rs` $\rightarrow$ `src/view/diff_grid/mod.rs` + `src/view/diff_grid/tests.rs`
- `src/view/header.rs` $\rightarrow$ `src/view/header/mod.rs` + `src/view/header/tests.rs`
- `src/view/modals.rs` $\rightarrow$ `src/view/modals/mod.rs` + `src/view/modals/tests.rs`

### 1.4 `diffuse`
- `src/lib.rs` $\rightarrow$ `src/tests.rs`
- `src/main.rs` $\rightarrow$ `src/main_tests.rs`

### Verification Gate
- Run `cargo check --all-targets` and `cargo test` to verify 100% compilation and pass rate with identical coverage.

---

## 2. Phase 2 — Test Suite Profiling, Hot-Spot Elimination & Latency Budget Enforcement

**Goal:** Profile all test binaries with high-resolution test timers (`cargo test -- --nocapture` and time reporting), identify all slow test executions, optimize hot loops, and enforce the latency budgets mandated in `AGENTS.md`.

### 2.1 Latency Diagnostics & Profiling
- Run `cargo test -- -Z unstable-options --report-time` or per-crate timing breakdowns.
- Log every test taking $> 50\text{ms}$ and find the root cause (e.g. repeated git commands, massive unbuffered string IO, large pipeline matrix runs, or un-ignored history corpora).

### 2.2 Optimize `diffuse-tui` Tests
- Identify which tests in `diffuse-tui` account for the 6–8s runtime.
- Replace slow repetitive pipeline/git mock setups with lightweight fixtures or shared state.

### 2.3 Optimize `golden.rs` Acceptance Walk
- Profile the lib leg vs bin leg across all corpora.
- Ensure large history stress tests (e.g. `diffuse-history-mid`) are marked `#[ignore = "..."]` for the default developer loop per `AGENTS.md § Test Performance Invariants`.

### 2.4 Verification Gate
- Verify each individual `#[test]` completes in `< 0.5s` (except unignored heavy whole-evolution benchmarks).
- Verify `diffuse-tui` and `diffuse-core` test binaries complete in `< 1.5s`.
- Verify full `./scripts/check.sh` warm gate passes cleanly.

---

## 3. Phase 3 — Golden Fixture Format Deconstruction & Plain File Storage

**Goal:** Deconstruct monolithic `<corpus>.json[.gz]` input fixtures into dedicated directories per corpus with plain, uncompressed file storage for diffs and file snapshots for maximum git visibility and reviewability.

### 3.1 Directory Structure & Plain File Storage
Replace all gzip-compressed monolithic files with plain uncompressed text files under `tests/golden/<corpus>/`:
```
tests/golden/<corpus>/
  ├── diff.patch (raw unified diff patch)
  ├── meta.json (branch, commit range, path -> snapshot mappings)
  ├── files/
  │   ├── crates-diffuse-tui-src-main.rs.old
  │   └── crates-diffuse-tui-src-main.rs.new
  ├── wire.json
  ├── render.txt
  └── markdown.md
```
Where all path slashes `/` in repository file paths are normalized to dashes `-` for flat storage inside `files/`.

### 3.2 Fixture Loader & Harvest Tool Modernization
- **Update `load_fixture` in `golden.rs` and `diffuse` tests**: Support loading from uncompressed files (`diff.patch`, `meta.json`, `files/*.old`, `files/*.new`, `wire.json`, `render.txt`, `markdown.md`) with transparent fallback to `.gz` if present.
- **Update `diffuse-harvest` tool (`crates/diffuse/src/bin/harvest.rs`)**: Emits new fixtures directly in the uncompressed directory structure.
- **Update Pi Extension Acceptance Runner**: Verify compatibility with uncompressed golden paths.

### 3.3 Corpus Migration & Verification
- Unpack and store all 8 existing corpora (`mirage-2025-08`, `diffuse-2025-08`, `opt-1-cache-regex-window-slice`, `opt-2-cache-markdown-regex`, `opt-3-cache-lang-profile`, `opt-4-manual-scanner-prefilter`, `diffuse-history-mid`, `untracked-files`) as uncompressed plain text files.
- Verify byte-for-byte equivalence of pipeline outputs and golden wire / render / markdown acceptance walks across both Rust (`tests/golden.rs`) and TypeScript tests.

---

## 4. Phase 4 — Wire JSON Format Optimization

**Goal:** Audit, streamline, and optimize the `PipelineOutput` / wire JSON contract:
- Eliminate redundant or dead fields in `OutputFile`, `OutputHunk`, `OutputRow`, and `OutputRun` (such as unused pre-render structures superseded by native Rust TUI).
- Optimize serialization representation and token runs to reduce payload sizes and serialization overhead.
- Ensure bidirectional round-trip (`reconstruct()`, `reverse()`) remains strictly invariant.

---

## 5. Phase 5 — Streaming Line-by-Line JSONL Protocol & Granular Section Headers

**Goal:** Deconstruct JSONL output format down to individual file lines with section header envelopes:
- Define granular JSONL record types:
  1. `FileHeader` record: path, old path, status, line counts.
  2. `HunkHeader` record: old start/count, new start/count, section title.
  3. `Line` record: single row representation (row kind, line numbers, classified runs, spans).
  4. `FileFooter` / `Summary` record: per-file stats and notes.
- Update `stream_diff_pipeline` to emit progressive line-level JSONL events for low-latency streaming consumption.
- Add comprehensive unit tests and benchmarks verifying 100% branch coverage and latency guarantees.

---

## 6. Phase 6 — Replace Temp File Handling in Tests with `tempfile` Crate

**Goal:** Standardize and modernize temporary file and directory handling across all test suites using the `tempfile` crate:
- Replace manual temporary file path generation (e.g. `std::env::temp_dir().join(...)` and ad-hoc temp paths) with `tempfile::NamedTempFile` and `tempfile::tempdir()`.
- Ensure automatic cleanup on test drop, avoiding file collisions, leaks, or leftover files in `/tmp` across concurrent test executions.
- Audit all unit and integration tests across `diffuse-core`, `diffuse-tui`, `diffuse-render`, `diffuse-syntax`, and `diffuse`.




