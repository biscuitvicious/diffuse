# Ratatui TUI Layout & Rendering Migration

- [x] Phase 1 — Terminal Backend & Ratatui Shell Integration (`crates/diffuse-tui`)
- [x] Phase 2 — Layout Architecture & Structural Components (Header, Status Bar, Bottom Ribbon)
- [ ] Phase 3 — Modal Overlays (File Finder, Help Modal, Note Draft)
- [ ] Phase 4 — Core Diff Grid & Scrollbar Widget Migration
- [ ] Phase 5 — Mouse Coordinate Hit-Testing & External Editor Suspension
- [ ] Phase 6 — Test Suite Modernization & Acceptance Verification

---

## Executive Summary

[`diffuse-tui`](crates/diffuse-tui) currently manages terminal raw mode, alternate screen buffers, mouse capture, and visual frame assembly by concatenating raw ANSI escape strings into a single terminal frame string written to `stdout`.

While functional, raw ANSI string concatenation requires manual line index math (`height.saturating_sub(4)`), manual floating box coordinate splicing for popups, and full-screen writes. 

This engineering plan migrates `diffuse-tui`'s layout, double-buffering, and rendering primitives to **`ratatui`** (using the `crossterm` backend). 

### Core Architectural Invariants:
1. **Differential Double-Buffering**: Ratatui computes diffs between consecutive frame buffers, updating only changed terminal cells to guarantee flicker-free scrolling across high-frequency mouse and keyboard events.
2. **Strict Engine Decoupling**: `diffuse-core` (pipeline, Myers/LCS diff algorithms, tokenizer, syntax classifier) and `AppState` (flat row indexing, cursor coordinates, selection anchors, navigation) remain 100% pure Rust data models with zero UI framework dependencies.
3. **Subword Highlight & Grapheme Integrity**: Custom subline word-wrapping, Unicode grapheme cluster safety, and multi-color subword highlights are mapped directly into Ratatui `Line` and `Span` primitives without sacrificing any existing visual detail.
4. **Headless Visual Testability**: Terminal frame verification leverages `ratatui::backend::TestBackend` for fast, in-memory visual snapshot assertions without escape-sequence parsing.

---

## Table of Contents

1. [Phase 1 — Terminal Backend & Ratatui Shell Integration](#1-phase-1--terminal-backend--ratatui-shell-integration)
2. [Phase 2 — Layout Architecture & Structural Components](#2-phase-2--layout-architecture--structural-components)
3. [Phase 3 — Modal Overlays](#3-phase-3--modal-overlays)
4. [Phase 4 — Core Diff Grid & Scrollbar Widget Migration](#4-phase-4--core-diff-grid--scrollbar-widget-migration)
5. [Phase 5 — Mouse Coordinate Hit-Testing & External Editor Suspension](#5-phase-5--mouse-coordinate-hit-testing--external-editor-suspension)
6. [Phase 6 — Test Suite Modernization & Acceptance Verification](#6-phase-6--test-suite-modernization--acceptance-verification)

---

## 1. Phase 1 — Terminal Backend & Ratatui Shell Integration

**Files**:
- [`crates/diffuse-tui/Cargo.toml`](crates/diffuse-tui/Cargo.toml)
- [`crates/diffuse-tui/src/terminal.rs`](crates/diffuse-tui/src/terminal.rs)
- [`crates/diffuse-tui/src/lib.rs`](crates/diffuse-tui/src/lib.rs)

1. **Add `ratatui` dependency**:
   - Add `ratatui = { version = "0.29", default-features = false, features = ["crossterm"] }` to `crates/diffuse-tui/Cargo.toml` and workspace root `Cargo.toml`.
2. **Refactor `TerminalGuard`**:
   - Embed `ratatui::Terminal<CrosstermBackend<Stdout>>` into `TerminalGuard`.
   - Expose `terminal.draw(|f| ...)` and `terminal.backend_mut()` lifecycle methods.
   - Maintain `suspend()` and `resume()` hooks for external editor execution ($EDITOR), ensuring alternate screen buffers and raw mode exit/re-entry work seamlessly.

---

## 2. Phase 2 — Layout Architecture & Structural Components

**Files**:
- [`crates/diffuse-tui/src/app/render.rs`](crates/diffuse-tui/src/app/render.rs)
- [`crates/diffuse-tui/src/view/header.rs`](crates/diffuse-tui/src/view/header.rs)
- [`crates/diffuse-tui/src/view/mod.rs`](crates/diffuse-tui/src/view/mod.rs)

1. **Define Top-Level Frame Layout**:
   - Compute top-level layout using `ratatui::layout::Layout`:
     - `HeaderArea`: Fixed 1–2 rows for repository info, branch, and file header.
     - `StatusArea`: Fixed 1 row for hunk badge, stream indicator, raw diff mode, and column scroll offsets.
     - `MainDiffArea`: Flexible constraint (`Constraint::Min(1)`) for diff grid and scrollbars.
     - `RibbonArea`: Fixed 2 rows for bottom keybinding guide and divider line.
2. **Convert Header & Status Bars to Ratatui Widgets**:
   - Refactor `render_header_bar` and `render_status_bar` to produce `ratatui::text::Line` / `Paragraph` widgets.
   - Map theme color keys (e.g. `syntaxKeyword`, `accent`, `syntaxString`) to `ratatui::style::Color::Indexed(n)` and `ratatui::style::Modifier`.
3. **Refactor Bottom Ribbon**:
   - Render keybinding ribbons and horizontal separators using `ratatui::widgets::Paragraph` with styling.

---

## 3. Phase 3 — Modal Overlays

**Files**:
- [`crates/diffuse-tui/src/view/modals.rs`](crates/diffuse-tui/src/view/modals.rs)
- [`crates/diffuse-tui/src/app/render.rs`](crates/diffuse-tui/src/app/render.rs)

1. **Help Modal Widget**:
   - Implement `render_help_modal` using `ratatui::widgets::Clear` to erase background cells, wrapped in a styled `Block` with `Borders::ALL` and `BorderType::Rounded`.
   - Center modal area using `Layout` flex constraints.
2. **File Finder Overlay**:
   - Implement `render_finder_modal` using `Clear`, rendering filtered file paths in a `List` or `Paragraph` widget with highlighted search matches.
3. **Inline Note Draft Editor**:
   - Render note draft prompts (`Note on <path>:<line>: <draft>_`) in an ephemeral block overlay above the main diff area.

---

## 4. Phase 4 — Core Diff Grid & Scrollbar Widget Migration

**Files**:
- [`crates/diffuse-tui/src/view/diff_grid.rs`](crates/diffuse-tui/src/view/diff_grid.rs)
- [`crates/diffuse-tui/src/app/geometry/mod.rs`](crates/diffuse-tui/src/app/geometry/mod.rs)
- [`crates/diffuse-tui/src/app/render.rs`](crates/diffuse-tui/src/app/render.rs)

1. **Diff Grid Widget (`DiffGridWidget`)**:
   - Render flat rows (File Headers, Hunk Dividers, Context, Additions, Deletions, Modified lines) directly into the `MainDiffArea` buffer.
   - For each visible flat row, build `ratatui::text::Line` containing prefix gutters, line numbers, and token spans.
   - Apply active cursor background highlighting (`\x1b[48;5;237m`) and multi-line selection bands (`\x1b[48;5;236m`) via `ratatui::style::Style`.
2. **Scrollbar Indicators**:
   - Render the vertical viewport scrollbar and file navigation indicator along the right edge of `MainDiffArea` using `ratatui::widgets::Scrollbar` or direct buffer cell placement.

---

## 5. Phase 5 — Mouse Coordinate Hit-Testing & External Editor Suspension

**Files**:
- [`crates/diffuse-tui/src/app/dispatch.rs`](crates/diffuse-tui/src/app/dispatch.rs)
- [`crates/diffuse-tui/src/event.rs`](crates/diffuse-tui/src/event.rs)
- [`crates/diffuse-tui/src/actions.rs`](crates/diffuse-tui/src/actions.rs)

1. **Layout-Aware Mouse Hit Testing**:
   - Store sub-area `Rect`s (Header, DiffGrid, Scrollbar, Modals) during frame draw.
   - Dispatch mouse click/drag events using `rect.contains(Position { x, y })`:
     - Header triangle clicks (`▶`/`▼`) toggle file collapse.
     - Divider expansion arrow clicks (`▼▼`/`▲▲`) trigger hunk context expansion.
     - Scrollbar track clicks trigger instant proportional jumps.
2. **Editor Suspension Verification**:
   - Test `$EDITOR` launch action (`o` key) with `terminal.suspend()` and `terminal.resume()` to ensure terminal restoration without artifacts.

---

## 6. Phase 6 — Test Suite Modernization & Acceptance Verification

**Files**:
- [`crates/diffuse-tui/src/app/tests.rs`](crates/diffuse-tui/src/app/tests.rs)
- [`crates/diffuse-tui/tests/app_integration.rs`](crates/diffuse-tui/tests/app_integration.rs)

1. **In-Memory Visual Unit Testing**:
   - Introduce `ratatui::backend::TestBackend` helpers to render frames into headless buffers in unit tests.
   - Verify cell contents, styling attributes, and cursor coordinates without escape string regexes.
2. **Full Regression Gate**:
   - Run `./scripts/check.sh` ensuring all cargo tests (lib, bin, integration, doctests) and pi-review extension equivalence smoke tests pass cleanly.
   - Verify performance invariants: sub-second unit test execution and 100% branch coverage across all new code paths.
