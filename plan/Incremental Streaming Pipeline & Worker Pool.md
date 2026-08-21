# Incremental Streaming Pipeline & Asynchronous Worker Pool

- [x] **Phase 1 — Core Streaming & Priority Worker Pool (`diffuse-core`)**
- [x] **Phase 2 — CLI Line-by-Line Streaming Output (`diffuse-cli`)**
- [ ] **Phase 3 — Incremental File Ingestion & TUI Live Rendering (`diffuse-tui`)**
- [ ] **Phase 4 — Open-Source Dataset Harvesting & Large-Scale Benchmarks (`tests/`)**

---

## Executive Summary

Currently, Diffuse operates as an all-or-nothing pipeline: it blocks until all files in a changeset are completely parsed, cluster-diffed, and syntax-classified before emitting output to the terminal or TUI. On massive changesets (hundreds or thousands of files in large repositories), this results in noticeable initial latency before anything appears on screen.

This plan introduces an **Asynchronous Incremental Streaming Pipeline & Priority Worker Pool**:
1. **Producer**: Reads the raw git diff and immediately emits self-contained `FileJob`s into a synchronized Priority Queue.
2. **Worker Pool**: A scoped pool of `N = (num_cpus / 2).max(1)` worker threads processes jobs ordered by priority (`priority = -file_idx <= 0`), ensuring File 0, File 1, and early files on the screen are computed first.
3. **In-Order Sequencer**: An in-memory reassembly buffer collects completed `OutputFile`s, ensuring that files and diff lines are emitted strictly in sequence (`0, 1, 2, ...`) without skipping or out-of-order anomalies.
4. **Incremental Consumers**:
   - **TUI Mode**: Appends files dynamically to `AppState`, updating visual line geometry and rendering live while preserving the user's active cursor and scroll position.
   - **CLI / Pager Mode (`--no-tui`)**: Renders and streams rows line-by-line immediately to `stdout` as each sequential file completes.
5. **Harvesting Suite**: Harvests multi-file git diffs from major open-source repositories (Linux kernel, Git, ripgrep) to guard streaming correctness and sub-10ms time-to-first-render.

---

## Table of Contents

1. [Phase 1 — Core Streaming & Priority Worker Pool (`diffuse-core`)](#1-phase-1--core-streaming--priority-worker-pool-diffuse-core)
2. [Phase 2 — CLI Line-by-Line Streaming Output (`diffuse-cli`)](#2-phase-2--cli-line-by-line-streaming-output-diffuse-cli)
3. [Phase 3 — Incremental File Ingestion & TUI Live Rendering (`diffuse-tui`)](#3-phase-3--incremental-file-ingestion--tui-live-rendering-diffuse-tui)
4. [Phase 4 — Open-Source Dataset Harvesting & Large-Scale Benchmarks (`tests/`)](#4-phase-4--open-source-dataset-harvesting--large-scale-benchmarks-tests)

---

## 1. Phase 1 — Core Streaming & Priority Worker Pool (`diffuse-core`)

Decompose the monolithic diff processing pipeline into a prioritized worker pool and in-order reassembly sequencer.

### Step 1.1: Single-File Pipeline Function
- **Location**: `crates/diffuse-core/src/pipeline/mod.rs`
- Extract `process_single_file(job: FileJob, notes_db: Option<&NotesDB>, repo: Option<&str>) -> OutputFile` from `run_pipeline_full`.
- Ensure each file job is completely independent, carrying its own `path`, `status`, raw diff chunks, and new/old content buffers.

### Step 1.2: Priority Queue & Scoped Worker Pool
- **Location**: `crates/diffuse-core/src/streaming.rs`
- Define `FileJob`:
  ```rust
  pub struct FileJob {
      pub idx: usize,
      pub diff_chunk: String,
      pub new_content: Option<String>,
      pub old_content: Option<String>,
  }
  ```
- Implement `PriorityWorkerPool`:
  - Worker count: `(std::thread::available_parallelism().map(|p| p.get()).unwrap_or(2) / 2).max(1)`.
  - Shared job queue: `Arc<Mutex<BinaryHeap<Reverse<FileJob>>>>` (or `crossbeam_channel`).
  - Workers pop lowest `file_idx` available, execute `process_single_file`, and send `ProcessedFile { idx, file: OutputFile }` to completion channel.

### Step 1.3: In-Order Sequencer
- **Location**: `crates/diffuse-core/src/streaming.rs`
- Implement `FileSequencer`:
  ```rust
  pub struct FileSequencer {
      next_expected_idx: usize,
      buffer: BTreeMap<usize, OutputFile>,
  }
  ```
- As `ProcessedFile` arrives, insert into `buffer`. While `buffer.contains_key(&next_expected_idx)`, pop and yield to output iterator/channel.

### Step 1.4: Unit & Stress Testing
- Write targeted unit tests verifying:
  - In-order delivery when workers finish out-of-order (e.g. File 1 finishes before File 0).
  - Proper termination when producer closes job stream.
  - Zero dropped or duplicated files.

---

## 2. Phase 2 — CLI Line-by-Line Streaming Output (`diffuse-cli`)

Integrate streaming pipeline into the CLI binary for instant pager and `--no-tui` rendering.

### Step 2.1: Streaming JSON Lines & Wire Output
- **Location**: `crates/diffuse-cli/src/main.rs`
- Support progressive emission of `OutputFile`s over JSON Lines (`--stream-jsonl`).

### Step 2.2: Line-by-Line Terminal Pager Streaming
- **Location**: `crates/diffuse-cli/src/main.rs`
- In non-interactive mode (`--no-tui` or piped to pager), as each `OutputFile` is emitted by `FileSequencer`:
  - Pass the file to `diffuse_render::render_file_rows`.
  - Stream rendered ANSI rows directly to `io::stdout()` without buffering remaining files.
- Verify that piping large diffs to `diffuse | head -n 20` exits immediately without crunching unrendered files.

---

## 3. Phase 3 — Incremental File Ingestion & TUI Live Rendering (`diffuse-tui`)

Enable live streaming ingestion within the interactive TUI.

### Step 3.1: Incremental File Append in `AppState`
- **Location**: `crates/diffuse-tui/src/app.rs`
- Implement `AppState::append_file(&mut self, file: OutputFile)`:
  - Appends to `self.output.files`.
  - Flattens rows into `self.flat` with correct `file_idx`.
  - Appends new hunk coordinates to `self.hunk_starts`.
  - Marks untracked files as collapsed by default.
  - Recomputes visual geometry while preserving `self.row_idx`, `self.vis_line`, and `self.scroll`.

### Step 3.2: Asynchronous Event Loop Ingestion
- **Location**: `crates/diffuse-tui/src/event.rs`, `crates/diffuse-tui/src/lib.rs`
- Connect streaming receiver `Receiver<OutputFile>` to the event loop.
- Drain ready files with non-blocking `try_recv()` before rendering each frame.
- Display background streaming progress indicator in status bar (e.g. `[Loading: 4/120 files...]` until stream completes).

### Step 3.3: Interactive Navigation & Search During Streaming
- Ensure file finder (`@`), line jumps (`:123`), hunk navigation (`J`/`K`), and file stepping (`n`/`p`) operate safely and seamlessly as the file set expands live.

---

## 4. Phase 4 — Open-Source Dataset Harvesting & Large-Scale Benchmarks (`tests/`)

Build a robust harvesting harness and stress-test suite from real open-source repositories.

### Step 4.1: Dataset Harvesting Harness
- **Location**: `tests/harvest/`
- Add automated script/harness to pull real multi-file commits and diffs from:
  - Linux kernel (complex C refactors, multi-thousand line diffs).
  - Git repository (mixed C, scripts, docs).
  - Ripgrep / Rust compiler (Rust generics, macros, indentation reflows).

### Step 4.2: Streaming Latency & Performance Benchmarks
- Benchmark time-to-first-file render on:
  - 10-file diff (<2ms).
  - 100-file diff (<5ms).
  - 1,000+ file massive changeset (<10ms time-to-first-file).
