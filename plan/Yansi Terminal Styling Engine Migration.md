# Yansi Terminal Styling Engine Migration

- [x] **Phase 1: Workspace & Dependency Foundation**
  - [x] Add `yansi = "1.0"` to workspace `Cargo.toml` and crate manifests
  - [x] Re-export core `yansi` types in `crates/diffuse-render/src/ansi.rs`
- [x] **Phase 2: Theme Palette & Row Style Transformation**
  - [x] Replace custom `AnsiColor` with `yansi::Color` in `crates/diffuse-render/src/theme/palette.rs`
  - [x] Replace `RowStyle` background string generators with `yansi::Style`
  - [x] Update `DiffuseTheme` and syntax token styling to use `yansi::Style`
- [·] **Phase 3: Zero-Allocation Painting in Render Pipeline**
  - [ ] Refactor `crates/diffuse-render/src/row.rs` to style via `yansi::Paint`
  - [ ] Refactor `crates/diffuse-render/src/diff.rs` to format with `yansi`
  - [ ] Preserve golden markup converters `encode_sgr`, `decode_sgr`, and `strip_ansi`
- [ ] **Phase 4: Ratatui & TUI Bridge Integration**
  - [ ] Update `crates/diffuse-tui/src/view.rs` to map from `yansi::Color` / `yansi::Style` to `ratatui::style::Color` / `Style`
  - [ ] Update header, footer, divider, and grid rendering to use native `yansi` types
- [ ] **Phase 5: CLI, Color Conditions & Verification**
  - [ ] Add support for `yansi::Condition` (`CLICOLOR`, `NO_COLOR`, TTY detection) in the CLI
  - [ ] Run `./scripts/check.sh` and verify all unit tests and golden corpora pass byte-for-byte

---

## Executive Summary

The `diffuse` rendering subsystem currently relies on a custom `AnsiColor` enum, an `AnsiStyle` builder, and string formatting that concatenates raw ANSI escape codes (`\x1b[...m`). While functional, this manual approach requires custom string allocations, ad-hoc ANSI code tracking, and custom reset handling.

This plan executes a deep migration to `yansi = "1.0"`. `yansi` becomes the primary color and styling currency across the entire workspace:
1. `AnsiColor` is replaced with `yansi::Color` (`Fixed(u8)`, `Rgb(u8, u8, u8)`, `Default`).
2. `AnsiStyle` is replaced with `yansi::Style` and `yansi::Paint`.
3. Row and token rendering leverage `yansi::Paint` for zero-allocation formatting directly into stream buffers.
4. Terminal environment detection (`NO_COLOR`, `CLICOLOR`, non-TTY streams) is handled natively via `yansi::Condition`.
5. The Ratatui TUI view cleanly bridges from `yansi::Color` to `ratatui::style::Color`.
6. All existing golden terminal output fixtures (`.render.txt`) remain byte-identical.

---

## Table of Contents

1. [Phase 1: Workspace & Dependency Foundation](#1-phase-1-workspace--dependency-foundation)
2. [Phase 2: Theme Palette & Row Style Transformation](#2-phase-2-theme-palette--row-style-transformation)
3. [Phase 3: Zero-Allocation Painting in Render Pipeline](#3-phase-3-zero-allocation-painting-in-render-pipeline)
4. [Phase 4: Ratatui & TUI Bridge Integration](#4-phase-4-ratatui--tui-bridge-integration)
5. [Phase 5: CLI, Color Conditions & Verification](#5-phase-5-cli-color-conditions--verification)

---

## 1. Phase 1: Workspace & Dependency Foundation

- **Goal**: Declare `yansi = "1.0"` at the root workspace level and equip `crates/diffuse-render` with the dependency.
- **Files**:
  - `Cargo.toml`
  - `crates/diffuse-render/Cargo.toml`
  - `crates/diffuse-render/src/ansi.rs`
- **Actions**:
  1. Add `yansi = "1.0"` to `[workspace.dependencies]` in root `Cargo.toml`.
  2. Add `yansi = { workspace = true }` in `crates/diffuse-render/Cargo.toml`.
  3. In `crates/diffuse-render/src/ansi.rs`, re-export `yansi::{Color, Style, Paint, Condition}` and establish compatibility aliases during transition.
- **Verification**: `cargo check --workspace` compiles cleanly with the new dependency.

---

## 2. Phase 2: Theme Palette & Row Style Transformation

- **Goal**: Replace custom `AnsiColor` in `ThemePalette` and `RowStyle` with `yansi::Color` and `yansi::Style`.
- **Files**:
  - `crates/diffuse-render/src/theme/palette.rs`
  - `crates/diffuse-render/src/theme.rs`
- **Actions**:
  1. Update `ThemePalette` struct fields to store `yansi::Color`.
  2. Update palette constant definitions (`THEME_MAGIC`, etc.) to use `yansi::Color::Fixed(idx)` and `yansi::Color::Rgb(r, g, b)`.
  3. Transform `RowStyle` to store native `yansi::Style` for tints, cursors, and fills.
  4. Implement helper methods for converting/writing styles to formatters without intermediate heap strings.
- **Verification**: Unit tests in `crates/diffuse-render/src/theme/tests.rs` pass.

---

## 3. Phase 3: Zero-Allocation Painting in Render Pipeline

- **Goal**: Adopt `yansi::Paint` in `row.rs` and `diff.rs` for subword spans, gutters, and diff lines.
- **Files**:
  - `crates/diffuse-render/src/row.rs`
  - `crates/diffuse-render/src/diff.rs`
  - `crates/diffuse-render/src/ansi.rs`
- **Actions**:
  1. Refactor `render_row_segment` to use `yansi::Paint` for token colors, deletions with strikethroughs, and additions.
  2. Refactor `render_raw_row_themed` and file header bars in `diff.rs` using `yansi::Style` and `Paint`.
  3. Retain `encode_sgr`, `decode_sgr`, and `strip_ansi` in `ansi.rs` to ensure `.render.txt` golden fixtures remain fully verifiable.
- **Verification**: Golden tests in `crates/diffuse/tests/golden.rs` and unit tests in `diffuse-render` pass with zero output regression.

---

## 4. Phase 4: Ratatui & TUI Bridge Integration

- **Goal**: Bridge native `yansi` colors directly to `ratatui::style::Color` in `diffuse-tui`.
- **Files**:
  - `crates/diffuse-tui/src/view.rs`
  - `crates/diffuse-tui/src/view/header.rs`
  - `crates/diffuse-tui/src/view/diff_grid.rs`
  - `crates/diffuse-tui/src/app/render.rs`
- **Actions**:
  1. Update `to_color` in `crates/diffuse-tui/src/view.rs` to take `yansi::Color` and produce `ratatui::style::Color`:
     - `yansi::Color::Default => Color::Reset`
     - `yansi::Color::Fixed(i) => Color::Indexed(i)`
     - `yansi::Color::Rgb(r, g, b) => Color::Rgb(r, g, b)`
  2. Update any direct references to `AnsiColor` in TUI modules to `yansi::Color`.
- **Verification**: `cargo test -p diffuse-tui` passes all unit and integration tests.

---

## 5. Phase 5: CLI, Color Conditions & Verification

- **Goal**: Hook global terminal styling controls to `yansi::whenever(Condition::...)` and enforce full test & style gates.
- **Files**:
  - `crates/diffuse/src/main.rs`
  - `TODO.md`
- **Actions**:
  1. Wire `--color` / `--no-color` CLI flags to `yansi::enable()` / `yansi::disable()`.
  2. Mark completed item in `TODO.md`.
  3. Run `./scripts/check.sh` (nightly fmt, clippy, unit tests, golden tests, install extension).
- **Verification**: All workspace gates pass cleanly without warnings or test failures.
