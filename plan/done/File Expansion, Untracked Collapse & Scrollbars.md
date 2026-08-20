# File Expansion, Untracked File Collapse & Viewport Scrollbars

- [x] **Phase 1 — File Collapse & Expand Model (`src/tui/app.rs`, `src/tui/event.rs`)**
  - [x] Step 1.1: `collapsed_files: HashSet<usize>` state in `AppState`, initialized with `f.status == "untracked"` collapsed by default
  - [x] Step 1.2: Flat row construction (`build_flat` and `build_raw_flat`) respecting file collapse state + file header banner collapse indicator (`▸` vs `▾` / count)
  - [x] Step 1.3: `Enter` keybinding on file header (or within file) toggles collapse state with cursor coordinate preservation
- [x] **Phase 2 — Untracked File Detection, Pipeline & Goldens (`src/git.rs`, `src/bin_pipeline.rs`, `tests/golden/`)**
  - [x] Step 2.1: Untracked file pipeline representation in `PipelineOutput` and `run_pipeline_full`
  - [x] Step 2.2: Add untracked file corpus fixture in `tests/golden/` locking wire JSON and terminal render golden
- [x] **Phase 3 — Visual Viewport & File Navigation Scrollbar (`src/tui/app.rs`)**
  - [x] Step 3.1: Viewport scrollbar calculation and rendering on rightmost column
  - [x] Step 3.2: Verification and integration test suite across visual and raw diff modes

---

## Executive Summary

This plan introduces interactive **file-level collapse/expand**, default **untracked file collapse**, golden test corpus integration for untracked changes, and **visual viewport scrollbars** in the diffuse native Rust TUI.

### Architectural Invariants:
1. **Untracked files are collapsed by default**: When diffuse loads a repository diff containing untracked files (`f.status == "untracked"`), they are initially rendered as single-row collapsed header banners (`▸ path (untracked) · +N lines · press Enter to expand`), preventing large untracked additions from blowing out the viewport.
2. **File Header Toggle with Cursor Invariant**: Pressing `Enter` on a file header (or a dedicated toggle action) flips the collapse state of that file, immediately rebuilding the visual flat-row sequence and re-anchoring the cursor to the file header.
3. **Scrollbar Column Slicing**: The scrollbar is rendered strictly within the rightmost column of the viewport, calculating thumb position and size proportionally from `(viewport_height, scroll, flat.len())` without interfering with row tokens or ANSI styles.
4. **Wire and Render Golden Parity**: Untracked file additions are represented in the pipeline and golden corpora, verifying byte-identical rendering.

---

## Table of Contents

1. [Phase 1 — File Collapse & Expand Model](#1-phase-1--file-collapse--expand-model)
   - [Step 1.1: State Tracking & Untracked Default Collapse](#step-11-state-tracking--untracked-default-collapse)
   - [Step 1.2: Flat Sequence Construction & Header Banner Visuals](#step-12-flat-sequence-construction--header-banner-visuals)
   - [Step 1.3: Interactive Toggle & Cursor Re-anchoring](#step-13-interactive-toggle--cursor-re-anchoring)
2. [Phase 2 — Untracked File Detection, Pipeline & Goldens](#2-phase-2--untracked-file-detection-pipeline--goldens)
   - [Step 2.1: Untracked Files in Pipeline Output](#step-21-untracked-files-in-pipeline-output)
   - [Step 2.2: Golden Corpus Untracked Fixture](#step-22-golden-corpus-untracked-fixture)
3. [Phase 3 — Visual Viewport & File Navigation Scrollbar](#3-phase-3--visual-viewport--file-navigation-scrollbar)
   - [Step 3.1: Proportional Scrollbar Geometry & Slicing](#step-31-proportional-scrollbar-geometry--slicing)
   - [Step 3.2: Automated Verification & Integration Tests](#step-32-automated-verification--integration-tests)

---

## 1. Phase 1 — File Collapse & Expand Model

### Step 1.1: State Tracking & Untracked Default Collapse
- **Affected Files**: `src/tui/app.rs`
- **Specification**:
  - Add `pub collapsed_files: std::collections::HashSet<usize>` to `AppState`.
  - In `AppState::new`, scan `output.files`: for every file where `f.status == "untracked"`, insert its file index into `collapsed_files`.
- **Verification**: Unit tests asserting `s.collapsed_files` contains untracked files on initial state creation.

### Step 1.2: Flat Sequence Construction & Header Banner Visuals
- **Affected Files**: `src/tui/app.rs`, `src/render/row.rs`
- **Specification**:
  - Update `build_flat` and `build_raw_flat`: if `collapsed_files.contains(&fi)`, emit only the `file_header_row` for that file, skipping all hunk rows.
  - Update `file_header_row` and header rendering: include `▸` (collapsed) vs `▾` (expanded), plus line/hunk summary stats (`[collapsed · +A −D]`).
- **Verification**: `test_collapsed_file_emits_only_header_row`.

### Step 1.3: Interactive Toggle & Cursor Re-anchoring
- **Affected Files**: `src/tui/app.rs`, `src/tui/event.rs`
- **Specification**:
  - Implement `AppState::toggle_file_collapse(&mut self, fi: usize)`.
  - In `handle_key_event`: `KeyCode::Enter` on a file header row (or when `state.current_flat_row().is_file_header`) toggles the collapse state of that file.
  - When collapsing/expanding, rebuild `self.flat` / `self.hunk_starts` and keep the cursor anchored cleanly on the file header row.
- **Verification**: Unit tests testing `Enter` key toggle, flat row expansion/collapse, and cursor position preservation.

---

## 2. Phase 2 — Untracked File Detection, Pipeline & Goldens

### Step 2.1: Untracked Files in Pipeline Output
- **Affected Files**: `src/bin_pipeline.rs`, `src/git.rs`
- **Specification**:
  - Verify untracked files are correctly parsed as pure add hunks in `run_pipeline_full` with `status: "untracked"`.
- **Verification**: Unit test `test_pipeline_untracked_file_full`.

### Step 2.2: Golden Corpus Untracked Fixture
- **Affected Files**: `tests/golden.rs`, `tests/golden/`
- **Specification**:
  - Add a dedicated golden corpus fixture containing untracked files alongside tracked modifications.
  - Verify lib leg, bin leg, and terminal render golden walks match byte-for-byte.
- **Verification**: `cargo test --test golden`.

---

## 3. Phase 3 — Visual Viewport & File Navigation Scrollbar

### Step 3.1: Proportional Scrollbar Geometry & Slicing
- **Affected Files**: `src/tui/app.rs`
- **Specification**:
  - When rendering viewport rows in `render_frame`, compute scrollbar track height, thumb size, and thumb offset based on `viewport_height`, `scroll`, and `flat.len()`.
  - Overlay a subtle scrollbar character (`│` for track, `█` for thumb) on the rightmost column of each viewport line.
- **Verification**: `test_scrollbar_thumb_geometry`.

### Step 3.2: Automated Verification & Integration Tests
- **Affected Files**: Full workspace
- **Specification**:
  - Run `./scripts/check.sh` ensuring all format checks, Clippy lints, unit tests, golden walks, and extension smoke tests pass.
