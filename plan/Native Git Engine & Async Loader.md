# Native Git Engine & Asynchronous Repository Loader

- [ ] **Phase 1 — Asynchronous Git Repository Loader & Instant TUI Startup (`diffuse-cli`, `diffuse-tui`)**
- [ ] **Phase 2 — Direct In-Process Git Blob Access via Native Git Crate (`diffuse-core`)**
- [ ] **Phase 3 — Streaming Diff Engine & Benchmark Profiling (`tests/`, `crates/`)**

---

## Executive Summary

When running `diffuse` across multi-commit or wide changesets (e.g. `diffuse HEAD~250..HEAD` on repositories with 200+ modified files), users experience noticeable latency (2–3 seconds) before the TUI appears on screen. Profiling reveals that **~80% of this delay is spent shelling out hundreds of sequential `git show` subprocesses** to fetch old and new blob contents.

This plan solves this latency through a two-fold architectural upgrade:
1. **Asynchronous Git Loader & Instant TUI Display**: Decouples terminal initialization from repository loading. The TUI renders frame 0 instantly (< 2 ms) with live loading status, while git discovery, ref resolution, and diff production run in a dedicated background worker thread.
2. **Direct In-Process Git Blob Access (`gix`)**: Replaces external `git show` subprocess spawning with direct, pure-Rust object database (ODB) and packfile index lookups via the `gix` (Gitoxide) crate. This drops per-file blob retrieval time from ~15 ms down to < 10 µs, reducing 400-blob retrieval from ~2,000 ms to < 10 ms.
3. **End-to-End Pipelined Streaming**: As git diff headers and blobs are read, individual `FileJob`s are immediately pushed into the `PriorityJobQueue`. The worker pool diffs File 0 in under 10 ms, streaming the first file to the active screen while subsequent files populate in the background.

---

## Table of Contents

1. [Phase 1 — Asynchronous Git Repository Loader & Instant TUI Startup (`diffuse-cli`, `diffuse-tui`)](#1-phase-1--asynchronous-git-repository-loader--instant-tui-startup-diffuse-cli-diffuse-tui)
2. [Phase 2 — Direct In-Process Git Blob Access via Native Git Crate (`diffuse-core`)](#2-phase-2--direct-in-process-git-blob-access-via-native-git-crate-diffuse-core)
3. [Phase 3 — Streaming Diff Engine & Benchmark Profiling (`tests/`, `crates/`)](#3-phase-3--streaming-diff-engine--benchmark-profiling-tests-crates)

---

## 1. Phase 1 — Asynchronous Git Repository Loader & Instant TUI Startup (`diffuse-cli`, `diffuse-tui`)

Decouple TUI startup from git repository loading so the TUI displays frame 0 instantly (< 2 ms).

### Step 1.1: Background Git Producer Thread
- **Location**: `crates/diffuse-core/src/git.rs`, `crates/diffuse-core/src/streaming.rs`
- Implement `spawn_git_stream_pipeline`:
  - Spawns a dedicated background thread that runs repository discovery, ref resolution, and diff production.
  - Feeds each discovered and read `FileJob` directly into `PriorityJobQueue`.
  - Connects directly to `StreamingConsumer` without waiting for the full git changeset to complete.

### Step 1.2: Instant TUI Launch & Loading Feedback
- **Location**: `crates/diffuse-cli/src/main.rs`, `crates/diffuse-tui/src/lib.rs`
- In interactive mode:
  - Enter raw terminal mode immediately and render Frame 0 (header bar, branch, and status ribbon displaying `[Loading repository...]`).
  - As files stream in from the background git worker, update the status bar to `[streaming... X files]` and display File 0 as soon as its diff finishes.

---

## 2. Phase 2 — Direct In-Process Git Blob Access via Native Git Crate (`diffuse-core`)

Replace hundreds of sequential `git show` subprocess spawns with direct in-process object database inspection.

### Step 2.1: Add `gix` to Workspace Dependencies
- **Location**: `Cargo.toml`, `crates/diffuse-core/Cargo.toml`
- Add `gix` with minimal required features (`odb`, `revision`, `blob`, `index`).

### Step 2.2: Native Object Database & Tree Inspection
- **Location**: `crates/diffuse-core/src/git.rs`
- Implement in-process tree traversal and blob reader:
  - Resolve `old_ref` and `new_ref` tree objects directly in `gix::Repository`.
  - Fetch file blobs by path or object ID directly from memory-mapped packfiles.
  - Fallback to worktree file reading for uncommitted / dirty working files.
- Benchmark: 400 blob reads drop from ~2,000 ms to < 10 ms.

---

## 3. Phase 3 — Streaming Diff Engine & Benchmark Profiling (`tests/`, `crates/`)

Add benchmark profiling tools and verify end-to-end time-to-first-file render.

### Step 3.1: Large Changeset Integration Tests
- **Location**: `tests/benchmarks.rs`, `crates/diffuse-cli/tests/cli.rs`
- Add integration test running against synthetic and real multi-commit changesets (200+ files).
- Assert that time-to-first-file render is < 20 ms.

### Step 3.2: Verification Gate & Performance Measurements
- Run `./scripts/check.sh` and verify all golden walk suites, unit tests, clippy, and TypeScript extension tests pass cleanly.
- Report before/after latency measurements on `mirage.work` (`HEAD~250..HEAD`).
