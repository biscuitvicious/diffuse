# Tokio Tracing & Structured Logging

- [x] **Phase 1: Workspace Dependencies & Logging Subsystem** — Add `tracing` and `tracing-subscriber` to workspace, create `crates/diffuse-core/src/logging/mod.rs` with custom formatters, span truncation, and multi-target log writer (`DIFFUSE_LOG_FILE`, TCP, Unix domain socket) ported from `mirage`.
- [x] **Phase 2: CLI/Binary Logging Initialization & Guard Rails** — Initialize tracing in `crates/diffuse/src/main.rs`, ensuring default silent execution (`LevelFilter::OFF`) and zero TUI / pipe corruption when logging to external files or sockets.
- [x] **Phase 3: Core Pipeline & Git Engine Instrumentation** — Instrument `load_git_repo`, `run_pipeline_full`, `stream_diff_pipeline`, and worker pool methods with `#[instrument]`, skipping bulk data (diff strings, whole file contents, ASTs).
- [x] **Phase 4: Alignment, Moves, Reflow & Syntax Instrumentation** — Instrument sequence alignment algorithms (Myers, Hirschberg, Patience), move detection, reflow pairing, and tokenizers with structured fields (row counts, slice bounds, heuristics).
- [x] **Phase 5: Interactive TUI & Streaming Event Instrumentation** — Instrument TUI event loop, key handling, file switching, and streaming channel receivers.
- [x] **Phase 6: Verification, Golden Gate & Zero-Overhead Benchmarks** — Verify `./scripts/check.sh` passes cleanly with <0.5s unit test latency and zero overhead when tracing is disabled.

---

## Executive Summary

`diffuse` operates as an interactive terminal UI, a high-throughput CLI pager, and a streaming JSON/JSONL protocol engine embedded in editor extensions. When analyzing complex multi-file diffs, performance bottlenecks, or tricky alignment edge cases, structured observability is essential.

Standard stderr logging is unsuitable for `diffuse` because it corrupts interactive Ratatui alternate-screen rendering and stdout JSON protocol streams.

This plan integrates a zero-overhead, structured tracing infrastructure adapted from `mirage.work/src/logging.rs`:
1. **Multi-Target Dispatch**: Logs route dynamically via `DIFFUSE_LOG_FILE` to local append-only files, Unix domain sockets (`unix:<path>`), or TCP sockets (`tcp:<addr>`), allowing developers to live-stream detailed trace output in a secondary terminal (`DIFFUSE_LOG_FILE=/tmp/diffuse.log RUST_LOG=trace diffuse`).
2. **Compact Span Hierarchies**: A custom `LogFormat` renders scoped span chains with head/tail truncation (e.g. `pipeline:worker:...:align:lcs: message`) and clean `key:value` field visitors.
3. **Selective Parameter Logging, Detail Extraction & Return/Error Tracking**: `#[instrument]` macros capture contextual metadata while skipping bulky memory payloads (`diff`, `old_content`, `new_content`, large string buffers). When handling verbose arguments, we drill down into them to record high-value specific details (e.g. lengths, counts, indices, options, file paths) via structured `fields(...)`. In general, methods will use `ret` and `err` (e.g. `#[instrument(ret, err, ...)]`) or log summary return metadata (e.g. counts of returned files/rows/moves) upon exit.
4. **Zero Overhead by Default**: The default filter directive is `LevelFilter::OFF`, resulting in single-branch bypass when unconfigured.

---

## Table of Contents

1. [Phase 1 — Workspace Dependencies & Logging Subsystem](#1-phase-1--workspace-dependencies--logging-subsystem)
2. [Phase 2 — CLI/Binary Logging Initialization & Guard Rails](#2-phase-2--clibinary-logging-initialization--guard-rails)
3. [Phase 3 — Core Pipeline & Git Engine Instrumentation](#3-phase-3--core-pipeline--git-engine-instrumentation)
4. [Phase 4 — Alignment, Moves, Reflow & Syntax Instrumentation](#4-phase-4--alignment-moves-reflow--syntax-instrumentation)
5. [Phase 5 — Interactive TUI & Streaming Event Instrumentation](#5-phase-5--interactive-tui--streaming-event-instrumentation)
6. [Phase 6 — Verification, Golden Gate & Zero-Overhead Benchmarks](#6-phase-6--verification-golden-gate--zero-overhead-benchmarks)

---

## 1. Phase 1 — Workspace Dependencies & Logging Subsystem

### 1.1 Workspace Dependency Configuration
Add `tracing` and `tracing-subscriber` to `Cargo.toml` workspace dependencies:
```toml
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter", "registry"] }
```
Add `tracing` to all workspace crates (`diffuse-core`, `diffuse-syntax`, `diffuse-render`, `diffuse-tui`, `diffuse`). Add `tracing-subscriber` to `diffuse-core` and `diffuse`.

### 1.2 Port and Refine Logging Subsystem
Create `crates/diffuse-core/src/logging/mod.rs` and `crates/diffuse-core/src/logging/tests.rs` based on `mirage.work/src/logging.rs`:
- **`init_tracing_subscriber()`**:
  - Checks `DIFFUSE_LOG_FILE` (with fallback to `RUST_LOG_FILE`).
  - Supports `tcp:<addr>`, `unix:<path>`, and standard file paths.
  - Defaults to `io::stderr` if unconfigured.
  - Sets default directive to `LevelFilter::OFF`, overridden by `DIFFUSE_LOG` / `RUST_LOG`.
- **`LogFormat`**:
  - Truncates file paths to last 2 segments (e.g. `diffuse-core/src/pipeline.rs:42: INFO`).
  - Hierarchical span formatting with configurable `max_head_spans: 2` and `max_tail_spans: 2`.
- **`LogFieldFormatter` & `LogFieldVisitor`**:
  - Clean `key:value` output, formatting messages directly without redundant `message:` prefixes.
- **Unit Tests (`crates/diffuse-core/src/logging/tests.rs`)**:
  - Port full suite verifying formatting of nested spans, empty span fields, file/line headers, truncated span chains, and struct debugging.

---

## 2. Phase 2 — CLI/Binary Logging Initialization & Guard Rails

### 2.1 Initialization in `main.rs`
- In `crates/diffuse/src/main.rs`:
  - Call `diffuse::core::logging::init_tracing_subscriber()?;` at the very beginning of `main()`.
  - Ensure initialization failure does not abort CLI unless `DIFFUSE_LOG_FILE` was explicitly configured with an invalid path/socket.

### 2.2 Re-export Facades
- Export `pub mod logging;` in `crates/diffuse-core/src/lib.rs`.
- Re-export `pub use core::logging;` in top-level `diffuse` crate.

---

## 3. Phase 3 — Core Pipeline & Git Engine Instrumentation

### 3.1 Git Repository Loader
- In `crates/diffuse-core/src/git/`:
  - `load_git_repo`: Drill down into repository options and paths while skipping full diff/blob contents:
    `#[instrument(skip(paths), fields(repo_root = %repo_root.display(), base_args = ?base_args, path_count = paths.len(), cached))]`
  - `stream_git_diff`: `#[instrument(skip(meta_tx), fields(repo_root = %repo_root.display(), base_args = ?base_args, cached, context = engine_context))]`
  - Trace milestone events: git discovery duration, patch parse time, file inventory counts.

### 3.2 Pipeline Orchestration & Worker Pool
- In `crates/diffuse-core/src/pipeline/`:
  - `run_pipeline_full`: Extract diff byte size, dictionary file counts, and visual context settings:
    `#[instrument(skip(diff, new_content, old_content, opts), fields(diff_bytes = diff.len(), new_file_count = new_content.as_ref().map(|m| m.len()), old_file_count = old_content.as_ref().map(|m| m.len()), visual_context = opts.visual_context, expand_lines = opts.expand_lines))]`
  - `process_file`: Extract path, status, hunk count, and file buffer lengths:
    `#[instrument(skip(diff_text, old_file_content, new_file_content, opts), fields(path = %file.path, status = ?file.status, hunks = file.hunks.len(), old_bytes = old_file_content.map(|s| s.len()), new_bytes = new_file_content.map(|s| s.len())))]`
- In `crates/diffuse-core/src/streaming/`:
  - `stream_diff_pipeline`: `#[instrument(skip(diff, new_content, old_content, opts), fields(diff_bytes = diff.len(), visual_context = opts.visual_context))]`
  - `spawn_git_stream_pipeline`: `#[instrument(skip(opts, pool), fields(repo_root = %repo_root.display(), base_args = ?base_args, cached, visual_context = opts.visual_context))]`

---

## 4. Phase 4 — Alignment, Moves, Reflow & Syntax Instrumentation

### 4.1 Sequence Alignment & LCS
- In `crates/diffuse-core/src/lcs/` and `crates/diffuse-core/src/align/`:
  - `align_lines`: `#[instrument(skip(old_lines, new_lines), fields(old_count = old_lines.len(), new_count = new_lines.len()))]`
  - `myers_diff`: `#[instrument(skip_all, fields(n = a.len(), m = b.len()))]`
  - `hirschberg`: `#[instrument(skip_all, fields(n = a.len(), m = b.len()))]`
  - `patience_diff`: `#[instrument(skip_all, fields(n = a.len(), m = b.len()))]`

### 4.2 Move & Reflow Detection
- In `crates/diffuse-core/src/moves/`:
  - `detect_moves`: Extract file and hunk counts:
    `#[instrument(skip(files), fields(file_count = files.len(), total_hunks = files.iter().map(|f| f.hunks.len()).sum::<usize>()))]`
- In `crates/diffuse-core/src/reflow/`:
  - `reflow_hunk`: Extract line bounds and row counts:
    `#[instrument(skip(hunk), fields(old_start = hunk.old_start, new_start = hunk.new_start, row_count = hunk.rows.len()))]`

### 4.3 Syntax Tokenization & Language Profiles
- In `crates/diffuse-syntax/`:
  - `classify_syntax`: Extract detected language tag, extension, and character length:
    `#[instrument(skip(text), fields(lang = ?lang, char_len = text.len()))]`

---

## 5. Phase 5 — Interactive TUI & Streaming Event Instrumentation

### 5.1 App State & Navigation
- In `crates/diffuse-tui/src/app/`:
  - `toggle_raw_diff`: `#[instrument(skip(self), fields(cur_raw = self.raw_diff, cur_row = self.row_idx, cur_file = self.current_file_idx()))]`
  - `toggle_file_collapse`: `#[instrument(skip(self), fields(file_idx, is_collapsed = self.collapsed_files.contains(&file_idx)))]`
  - `jump_to_hunk`: `#[instrument(skip(self), fields(hunk_idx, cur_file = self.current_file_idx()))]`

### 5.2 Event Dispatch & Background Ingestion
- In `crates/diffuse-tui/src/app/events.rs`:
  - `handle_key_event`: `#[instrument(skip(self), fields(code = ?key.code, modifiers = ?key.modifiers))]`
- In streaming receiver ingestion:
  - Trace incoming `OutputFile` arrivals, recording path, chunk byte size, and flat indexing invalidation.

---

## 6. Phase 6 — Verification, Golden Gate & Zero-Overhead Benchmarks

### 6.1 Performance Invariant Verification
- Run test suite with `LevelFilter::OFF` to ensure all tests execute within latency budgets (<0.5s per unit test, <1.5s per test binary).
- Run benchmarks to verify zero measurable regression in hot tokenization or alignment loops.

### 6.2 Test Suite & Check Gate
- Verify full `./scripts/check.sh` gate passes cleanly (Nightly fmt, clippy with zero warnings, all cargo test suites, and extension smoke tests).
- Add integration test in `crates/diffuse/tests/cli.rs` verifying that `DIFFUSE_LOG_FILE` creates structured log records without writing to stderr or stdout.
