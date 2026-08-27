# Markdown Syntax Classifier & Pulldown-Cmark Integration

- [x] Phase 1 — Add `pulldown-cmark` Dependency to `diffuse-syntax`
- [x] Phase 2 — Implement Offset-Based Markdown Token Classifier in `diffuse-syntax`
- [x] Phase 3 — Expand Table, Task List, Alert, and HTML Syntax Styles
- [x] Phase 4 — Unit Tests, Fixtures & Equivalence Gate Verification

---

## Executive Summary

`diffuse` provides syntax-highlighted diffs across programming languages and Markdown documents. Currently, the Markdown classifier in `diffuse-syntax` relies on basic ad-hoc regular expressions that miss GitHub-Flavored Markdown (GFM) features such as tables (`|`), task lists (`- [ ]`, `- [x]`), blockquotes (`>`), GitHub alerts (`> [!NOTE]`), HTML tags (`<details>`, `<code>`), and thematic breaks (`---`).

This plan integrates the standard Rust CommonMark/GFM pull-parser **`pulldown-cmark`** into `diffuse-syntax`. 

### Key Invariant: Exact Raw Text Preservation
As a diff viewer, `diffuse` must never alter, strip, or replace underlying Markdown characters (such as `` ` ``, `**`, `*`, `~~`, `|`, `[ ]`, `>`). The raw diff text is preserved 100% character-for-character so changes in punctuation and formatting are precisely displayed. `pulldown-cmark`'s `into_offset_iter()` will be used to map exact byte ranges `(usize, usize)` onto styled token runs with semantic colors and SGR attributes (bold, italic, underline, strikethrough, accents).

---

## Table of Contents

1. [Phase 1 — Add pulldown-cmark Dependency to diffuse-syntax](#1-phase-1--add-pulldown-cmark-dependency-to-diffuse-syntax)
2. [Phase 2 — Implement Offset-Based Markdown Token Classifier in diffuse-syntax](#2-phase-2--implement-offset-based-markdown-token-classifier-in-diffuse-syntax)
3. [Phase 3 — Expand Table, Task List, Alert, and HTML Syntax Styles](#3-phase-3--expand-table-task-list-alert-and-html-syntax-styles)
4. [Phase 4 — Unit Tests, Fixtures & Equivalence Gate Verification](#4-phase-4--unit-tests-fixtures--equivalence-gate-verification)

---

## 1. Phase 1 — Add pulldown-cmark Dependency to diffuse-syntax

**Files**:
- [`Cargo.toml`](Cargo.toml)
- [`crates/diffuse-syntax/Cargo.toml`](crates/diffuse-syntax/Cargo.toml)

1. Add `pulldown-cmark` to root `[workspace.dependencies]` with `default-features = false` and GFM features enabled (`simd` optional):
   ```toml
   pulldown-cmark = { version = "0.13", default-features = false, features = ["gfm"] }
   ```
2. Add `pulldown-cmark = { workspace = true }` in `crates/diffuse-syntax/Cargo.toml`.

---

## 2. Phase 2 — Implement Offset-Based Markdown Token Classifier in diffuse-syntax

**Files**:
- [`crates/diffuse-syntax/src/classifier.rs`](crates/diffuse-syntax/src/classifier.rs)
- [`crates/diffuse-syntax/src/profile.rs`](crates/diffuse-syntax/src/profile.rs)

1. **Leverage `pulldown_cmark::Parser::new_ext(text, Options::all()).into_offset_iter()`**:
   - For any line or block of Markdown text, `pulldown-cmark` produces an iterator of `(Event, Range<usize>)`.
   - Maintain a current byte cursor spanning `0..text.len()`.
   - As events arrive (`Start(Tag)`, `End(TagEnd)`, `Code`, `Text`, `Html`, `InlineHtml`, `TaskListMarker`), map the exact byte slices into `ClassifiedToken<'a>`.
2. **Text Preservation Invariant**:
   - The token stream's concatenated `text` must exactly equal the input slice `text` byte-for-byte (`assert_eq!(tokens.concat(), input_text)`).
   - Any gaps between recognized Markdown AST elements (e.g. delimiters `**`, `*`, `~~`, `` ` ``, `[`, `](`, `)`, `|`, `>`, `#`) are emitted as `accent` or delimiter tokens, preserving their raw characters in place.

---

## 3. Phase 3 — Expand Table, Task List, Alert, and HTML Syntax Styles

**Files**:
- [`crates/diffuse-syntax/src/classifier.rs`](crates/diffuse-syntax/src/classifier.rs)
- [`crates/diffuse-render/src/theme.rs`](crates/diffuse-render/src/theme.rs)

1. **Table Syntax**:
   - Pipes `|` and alignment dashes `|---|---|` styled as muted borders (`border` / `accent`).
   - Table header text styled with bold accents.
2. **Task List Checkboxes**:
   - `[ ]` and `[x]` / `[X]` styled with distinct accent/status colors while retaining exact text.
3. **Blockquotes & GitHub Alerts**:
   - `>` markers styled with vertical bar accents.
   - `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]` styled with dedicated theme colors (`info`, `success`, `warning`, `danger`).
4. **HTML Tags**:
   - Inline HTML and HTML blocks (`<details>`, `<summary>`, `<code>`, `<div>`) highlighted with tag colors (`syntaxKeyword` / `syntaxType`).
5. **Thematic Breaks & Frontmatter**:
   - `---`, `***`, `___` styled as horizontal dividers.

---

## 4. Phase 4 — Unit Tests, Fixtures & Equivalence Gate Verification

**Files**:
- [`crates/diffuse-syntax/src/tests.rs`](crates/diffuse-syntax/src/tests.rs)
- [`crates/diffuse/tests/golden.rs`](crates/diffuse/tests/golden.rs)

1. **Unit Tests**:
   - Verify exact text byte reconstruction on tables, headings, code spans, links, lists, task checkboxes, alerts, and blockquotes.
   - Test doc-comment markdown parsing in Rust/TS code files.
2. **Acceptance Verification**:
   - Run `./scripts/check.sh` to ensure all tests, doctests, and acceptance gates pass cleanly.
