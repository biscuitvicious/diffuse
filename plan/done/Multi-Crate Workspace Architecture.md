# Plan — Multi-Crate Workspace Architecture

## Global TODO
- [x] Phase 1 — Workspace Root Setup & `diffuse-syntax` Crate Extraction
  - [x] Step 1.1: Configure root virtual manifest `Cargo.toml` with `[workspace]` and dependency inheritance
  - [x] Step 1.2: Extract `crates/diffuse-syntax` crate with language profiles, tokenizer, classifier, and fence detection
  - [x] Step 1.3: Verify `diffuse-syntax` unit tests compile and pass cleanly
- [x] Phase 2 — `diffuse-core` Engine Crate Extraction
  - [x] Step 2.1: Extract `crates/diffuse-core` containing `wire`, `parse`, `cluster`, `shift`, `collapse`, `scope`, `visible`, `notes`, `git`, and `pipeline`
  - [x] Step 2.2: Establish public API and re-exports in `diffuse-core/src/lib.rs`
  - [x] Step 2.3: Verify engine unit tests and DP projection invariants in `diffuse-core`
- [x] Phase 3 — `diffuse-render` & `diffuse-tui` Crates Extraction
  - [x] Step 3.1: Extract `crates/diffuse-render` with theme models and SGR token styling helpers
  - [x] Step 3.2: Extract `crates/diffuse-tui` with `AppState`, `TuiCommand`, `view/` components, modal dialogues, actions, and crossterm event processing
  - [x] Step 3.3: Verify render and TUI unit tests
- [x] Phase 4 — `diffuse` Binary / CLI Composition Crate & Test Wiring
  - [x] Step 4.1: Configure `crates/diffuse-cli` (named `diffuse`) with `main.rs` and CLI argument handling
  - [x] Step 4.2: Update integration tests in `tests/` (`golden.rs`, `oracle.rs`, `dp_corpus.rs`, `cli.rs`, `render.rs`)
  - [x] Step 4.3: Update extension build and packaging scripts (`extension/pi/scripts/build-bin.sh`, `scripts/check.sh`)
- [x] Phase 5 — Full Workspace Acceptance Gate
  - [x] Step 5.1: Run `./scripts/check.sh` (nightly fmt check, clippy with zero warnings across all crates, all cargo tests, npm smoke suite)
  - [x] Step 5.2: Verify byte-identical wire and render output across all golden corpora

---

## Executive Summary

The `diffuse` engine and reviewer TUI have reached full functional maturity. However, housing all layers—from raw token lexing and DP sequence alignment to terminal SGR formatting, crossterm input loops, and CLI parsing—in a single monolithic crate complicates downstream reuse and creates unnecessary coupling.

This plan restructures the repository into an idiomatic **Cargo Workspace** with centralized dependency management and five distinct, decoupled packages:
1. **`diffuse-syntax`**: Language profiles, syntax token classification, and markdown fence extraction (zero I/O, regex-based).
2. **`diffuse-core`**: The pure diff engine: AST unified diff parsing, bipartite change clustering, dynamic programming token alignment, subword diffing, context allocation, notes DB, wire serialization, and pipeline orchestration.
3. **`diffuse-render`**: SGR terminal styling, color themes, raw unified rendering, and visual column slicing.
4. **`diffuse-tui`**: The interactive terminal reviewer: application state machine, declarative commands, crossterm event handling, view components, file finder overlay, and git staging actions.
5. **`diffuse` (CLI)**: The executable composition root and command-line interface.

---

## Table of Contents

1. [Phase 1 — Workspace Root Setup & diffuse-syntax Crate Extraction](#1-phase-1--workspace-root-setup--diffuse-syntax-crate-extraction)
2. [Phase 2 — diffuse-core Engine Crate Extraction](#2-phase-2--diffuse-core-engine-crate-extraction)
3. [Phase 3 — diffuse-render & diffuse-tui Crates Extraction](#3-phase-3--diffuse-render--diffuse-tui-crates-extraction)
4. [Phase 4 — diffuse Binary / CLI Composition Crate & Test Wiring](#4-phase-4--diffuse-binary--cli-composition-crate--test-wiring)
5. [Phase 5 — Full Workspace Acceptance Gate](#5-phase-5--full-workspace-acceptance-gate)

---

## 1. Phase 1 — Workspace Root Setup & `diffuse-syntax` Crate Extraction

### Motivation & Goals
Establish a clean workspace manifest at the repository root and extract `diffuse-syntax` into an independent crate with no engine dependencies.

### Concrete Steps
1. Create `crates/` directory.
2. Configure root `Cargo.toml`:
   - `[workspace]` with `members = ["crates/*"]`.
   - `[workspace.package]` with version `0.1.0`, edition `2024`, license `MIT`.
   - `[workspace.dependencies]` declaring shared versions for `serde`, `serde_json`, `regex`, `difflib`, `crossterm`, `clap`, `unicode-width`, `unicode-segmentation`.
   - Profile optimizations (`profile.dev.package.regex`, `profile.dev`).
3. Move `src/syntax/` into `crates/diffuse-syntax/src/`:
   - `crates/diffuse-syntax/Cargo.toml` inheriting workspace dependencies.
   - `crates/diffuse-syntax/src/lib.rs` exporting `language/`, `profile.rs`, `registry.rs`, `classifier.rs`, `fence.rs`, and `prelude`.
4. Run `cargo test -p diffuse-syntax` to verify all syntax tests pass independently.

---

## 2. Phase 2 — `diffuse-core` Engine Crate Extraction

### Motivation & Goals
Extract the diff engine, parsing, clustering, alignment, and wire contract into `diffuse-core` with dependencies strictly limited to `diffuse-syntax` and pure data/math crates.

### Concrete Steps
1. Create `crates/diffuse-core/Cargo.toml` depending on `diffuse-syntax = { workspace = true }`, `serde`, `serde_json`, `difflib`, `unicode-segmentation`, `unicode-width`.
2. Move core modules into `crates/diffuse-core/src/`:
   - `wire/`, `parse/`, `cluster/`, `shift/`, `collapse/`, `scope/`, `visible.rs`, `notes.rs`, `git.rs`, `pipeline/`.
3. Expose clean public module interface and preludes in `crates/diffuse-core/src/lib.rs`.
4. Run `cargo test -p diffuse-core` to verify all unit tests and DP projection invariants.

---

## 3. Phase 3 — `diffuse-render` & `diffuse-tui` Crates Extraction

### Motivation & Goals
Separate visual ANSI terminal styling (`diffuse-render`) and interactive TUI event/state management (`diffuse-tui`) into decoupled presentation crates.

### Concrete Steps
1. Create `crates/diffuse-render/`:
   - `Cargo.toml` depending on `diffuse-core`, `diffuse-syntax`, `unicode-width`.
   - `src/lib.rs` and `src/theme.rs` containing `DiffuseTheme`, `styled_prefix`, `render_row_segment`, `render_raw_row`, `slice_by_visual_width`, `display_text`.
2. Create `crates/diffuse-tui/`:
   - `Cargo.toml` depending on `diffuse-core`, `diffuse-render`, `diffuse-syntax`, `crossterm`, `unicode-width`, `unicode-segmentation`.
   - `src/lib.rs`, `src/types.rs`, `src/command.rs`, `src/view/`, `src/app.rs`, `src/event.rs`, `src/finder.rs`, `src/actions.rs`, `src/editor.rs`, `src/terminal.rs`.
3. Verify `cargo test -p diffuse-render` and `cargo test -p diffuse-tui`.

---

## 4. Phase 4 — `diffuse` Binary / CLI Composition Crate & Test Wiring

### Motivation & Goals
Create the CLI executable crate (`crates/diffuse-cli` named `diffuse`) and wire up integration tests in `tests/`.

### Concrete Steps
1. Create `crates/diffuse-cli/`:
   - `Cargo.toml` naming package `diffuse` (with `[[bin]] name = "diffuse"`), depending on `diffuse-core`, `diffuse-render`, `diffuse-tui`, `clap`, `serde_json`.
   - `src/main.rs` orchestrating CLI args, JSON output mode, and interactive TUI mode.
2. Wire up root integration tests in `tests/` (`golden.rs`, `oracle.rs`, `dp_corpus.rs`, `cli.rs`, `render.rs`) against workspace crates.
3. Verify `extension/pi/scripts/build-bin.sh` correctly builds the `diffuse` binary into `target/debug/diffuse` and `target/release/diffuse`.

---

## 5. Phase 5 — Full Workspace Acceptance Gate

### Motivation & Goals
Ensure 100% test coverage, zero clippy warnings across the entire workspace, and full golden corpus equivalence.

### Concrete Steps
1. Run `./scripts/check.sh` (`cargo fmt --all`, `cargo clippy --workspace -- -D warnings`, `cargo test --all`, `npm test`).
2. Verify all 7 golden acceptance corpora match byte-for-byte in both library and binary legs.
3. Bundle release binary via `extension/pi/scripts/install.sh` and perform visual smoke check.
4. Move completed plan to `plan/done/Multi-Crate Workspace Architecture.md`.
