# Diff Viewer Markdown Table Rendering

- [x] Phase 1 — Composite SGR Encoding in `diffuse-syntax` and `diffuse-render`
- [x] Phase 2 — Table Pipe and Alignment Row Syntax Classification
- [x] Phase 3 — Table Header Cell Syntax Styling
- [x] Phase 4 — Unit Tests, Golden Corpus Review & Verification Gate

---

## Executive Summary

`diffuse` provides structured diff analysis and interactive terminal review with syntax highlighting across programming languages and Markdown documents. Currently, Markdown tables in the regular diff viewer (file diffs of `.md` files and doc comments) display with basic accents or raw border classification:
- Table vertical delimiters (`|`) and alignment rows (`| :--- | :---: | ---: |`) use bright accent colors that compete visually with table cell content.
- Table headers lack distinct hierarchy compared to body rows.

In the notes and terminal Markdown reviewer, tables display with clean visual hierarchy:
1. Table vertical delimiters (`|`) styled in the muted border color (`"muted"`).
2. Alignment / separator rows (`| :--- | :---: | ---: |`) styled in the muted border color (`"muted"`).
3. Table header cells styled in wheat yellow (`"mdItalic"` / `Color::Fixed(229)`), bold, and underlined (`\x1b[1;4m`).

This plan aligns the regular Markdown diff viewer with the terminal table styling, elevating Markdown table legibility while strictly preserving two key invariants:
- **Wire Contract Immutability** ([`crates/diffuse-core/src/wire.rs`](../crates/diffuse-core/src/wire.rs)): `OutputRun.sgr` remains `Option<u32>` without breaking serialized schemas. Composite styles are represented via a single dedicated SGR identifier (`MD_SGR_BOLD_UNDERLINE = 14`).
- **Exact Raw Text Preservation** ([`plan/done/Markdown Syntax Classifier & Pulldown Cmark.md`](done/Markdown%20Syntax%20Classifier%20&%20Pulldown%20Cmark.md)): The character-for-character raw diff text is preserved 100% byte-for-byte; only token colors and SGR attributes change.

---

## Table of Contents

1. [Phase 1 — Composite SGR Encoding in `diffuse-syntax` and `diffuse-render`](#1-phase-1--composite-sgr-encoding-in-diffuse-syntax-and-diffuse-render)
2. [Phase 2 — Table Pipe and Alignment Row Syntax Classification](#2-phase-2--table-pipe-and-alignment-row-syntax-classification)
3. [Phase 3 — Table Header Cell Syntax Styling](#3-phase-3--table-header-cell-syntax-styling)
4. [Phase 4 — Unit Tests, Golden Corpus Review & Verification Gate](#4-phase-4--unit-tests-golden-corpus-review--verification-gate)

---

## 1. Phase 1 — Composite SGR Encoding in `diffuse-syntax` and `diffuse-render`

### Objective

Enable `OutputRun.sgr: Option<u32>` to represent stacked bold and underline attributes without altering the frozen JSON wire contract.

### Files

- [`crates/diffuse-syntax/src/profile.rs`](../crates/diffuse-syntax/src/profile.rs)
- [`crates/diffuse-syntax/src/lib.rs`](../crates/diffuse-syntax/src/lib.rs)
- [`crates/diffuse-render/src/ansi.rs`](../crates/diffuse-render/src/ansi.rs)
- [`crates/diffuse-render/src/theme.rs`](../crates/diffuse-render/src/theme.rs)

### Steps

1. **Define Composite SGR Attribute**:
   - Add `pub const MD_SGR_BOLD_UNDERLINE: u32 = 14;` to `diffuse-syntax::profile` and re-export in `diffuse-syntax::lib`.
   - Value `14` uniquely represents the combination of SGR 1 (bold) and SGR 4 (underline) in a single `u32` slot.

2. **Update ANSI Sequence Emission and Reset**:
   - In `diffuse-render::ansi::sgr_on_str`:
     ```rust
     14 => Some("\x1b[1;4m"),
     ```
   - In `diffuse-render::ansi::sgr_off`:
     ```rust
     14 => Some("\x1b[24;22m"), // underline off, bold off
     ```
   - In `diffuse-render::theme::write_token`: `sgr_on_str` and `sgr_off` seamlessly emit `\x1b[1;4m` and reset with `\x1b[24;22m`.

---

## 2. Phase 2 — Table Pipe and Alignment Row Syntax Classification

### Objective

Classify table delimiters (`|`) and alignment rows as semantic token `"muted"` instead of bright accents or generic borders.

### Files

- [`crates/diffuse-syntax/src/classifier.rs`](../crates/diffuse-syntax/src/classifier.rs)

### Steps

1. **Table Pipe Delimiter Tokens**:
   - When scanning Markdown table rows in `for_each_markdown_token`:
     - Pipe delimiters (`|`) between table cells are emitted as `ClassifiedToken { color: "muted", sgr: None, text: "|" }`.
2. **Alignment Row Classification**:
   - Recognize delimiter/alignment rows (e.g. `| :--- | :---: | ---: |` or `|---|---|`).
   - Emit delimiter lines and alignment dashes with `color: "muted"`.

---

## 3. Phase 3 — Table Header Cell Syntax Styling

### Objective

Style table header cell content with wheat yellow italic color (`"mdItalic"` / `Color::Fixed(229)`), bold, and underlined.

### Files

- [`crates/diffuse-syntax/src/classifier.rs`](../crates/diffuse-syntax/src/classifier.rs)

### Steps

1. **Table Head Tag Handling**:
   - On `Tag::TableHead`:
     - Set `is_table_head = true`.
     - Set `active_color = "mdItalic"`.
     - Set `active_sgr = Some(MD_SGR_BOLD_UNDERLINE)`.
   - On `TagEnd::TableHead`:
     - Set `is_table_head = false`.
     - Reset `active_color = "text"`.
     - Reset `active_sgr = None`.

---

## 4. Phase 4 — Unit Tests, Golden Corpus Review & Verification Gate

### Objective

Verify that all capabilities are tested under strict test-first discipline and that all verification gates pass cleanly.

### Files

- [`crates/diffuse-syntax/src/tests.rs`](../crates/diffuse-syntax/src/tests.rs)
- [`crates/diffuse-render/src/theme/tests.rs`](../crates/diffuse-render/src/theme/tests.rs)
- [`crates/diffuse-render/src/row/tests.rs`](../crates/diffuse-render/src/row/tests.rs)
- [`tests/golden.rs`](../tests/golden.rs)

### Steps

1. **Unit Tests**:
   - Add unit tests verifying `for_each_markdown_token` classifies table pipes as `"muted"` and headers as `"mdItalic"` with `MD_SGR_BOLD_UNDERLINE`.
   - Add unit tests verifying `render_token` and `render_row_segment` emit `\x1b[1;4m` with wheat yellow foreground for table headers and reset with `\x1b[24;22m`.
2. **Golden Corpus Sign-off**:
   - Inspect any golden fixtures containing Markdown tables (`tests/golden/diffuse-2026-08/`, `tests/golden/theme-scrollbar-palette/`, `tests/golden/diffuse-history-mid/`).
   - If diffs occur, verify they strictly improve visual hierarchy per [`tests/AGENTS.md`](../tests/AGENTS.md) and obtain sign-off.
3. **Acceptance Verification Gate**:
   - Run `./scripts/check.sh` ensuring all tests, clippy checks, and formatting pass cleanly with 0 warnings.
