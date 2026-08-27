# Markdown Output Format & GFM Diff Exporter

- [x] Phase 1 — Markdown Renderer Core & Options (`diffuse-render/src/markdown.rs`)
- [x] Phase 2 — Facade Re-Exports & Top-Level Engine Integration
- [x] Phase 3 — CLI Argument Parsing & Direct Markdown Dispatch
- [x] Phase 4 — Unit Tests, CLI Integration Tests & Acceptance Verification

---

## Executive Summary

`diffuse` provides an interactive TUI, a pretty-printing ANSI terminal pager (`-p`), and a structured JSON stream (`-j`). For sharing code review summaries in pull request descriptions, issue comments, code review bots, and chat systems, a native **GitHub-Flavored Markdown (GFM)** output format (`-m` / `--markdown`) is needed.

This plan adds a dedicated, high-performance GFM diff exporter in `diffuse-render` that formats diffs with:
1. An **Executive Diff Summary Table** displaying per-file addition, deletion, and modification metrics.
2. **Collapsible `<details open>` File Sections** with syntax-highlighted ```` ```diff ```` code blocks.
3. **Review Note Integration** formatting persistent review comments into GitHub blockquote alerts (`> [!NOTE]`).
4. Direct CLI integration supporting `diffuse -m` and `diffuse --markdown [git-args...]`.

---

## Table of Contents

1. [Phase 1 — Markdown Renderer Core & Options](#1-phase-1--markdown-renderer-core--options)
2. [Phase 2 — Facade Re-Exports & Top-Level Engine Integration](#2-phase-2--facade-re-exports--top-level-engine-integration)
3. [Phase 3 — CLI Argument Parsing & Direct Markdown Dispatch](#3-phase-3--cli-argument-parsing--direct-markdown-dispatch)
4. [Phase 4 — Unit Tests, CLI Integration Tests & Acceptance Verification](#4-phase-4--unit-tests-cli-integration-tests--acceptance-verification)

---

## 1. Phase 1 — Markdown Renderer Core & Options

**File**: [`crates/diffuse-render/src/markdown.rs`](crates/diffuse-render/src/markdown.rs)

1. **Define `MarkdownOptions` Configuration Struct**:
   ```rust
   #[derive(Clone, Debug, PartialEq, Eq)]
   pub struct MarkdownOptions {
       /// Whether to render the summary table at the top (default: true).
       pub summary_table: bool,
       /// Wrap each file diff in collapsible `<details>` blocks (default: true).
       pub collapsible: bool,
       /// Expand all `<details open>` tags by default (default: true).
       pub default_open: bool,
       /// Render attached review notes as GFM callout blockquotes (default: true).
       pub include_notes: bool,
   }

   impl Default for MarkdownOptions {
       fn default() -> Self {
           Self {
               summary_table: true,
               collapsible: true,
               default_open: true,
               include_notes: true,
           }
       }
   }
   ```

2. **Implement `render_markdown_output` and `render_file_markdown`**:
   - `render_markdown_output(output: &PipelineOutput, options: &MarkdownOptions) -> String`
   - Formats summary table with columns: `| File | Status | +Adds | -Dels | ~Mods |`.
   - Iterates through `output.files`, rendering each file via `render_file_markdown`.
   - Emits standard `@@ -old_start,old_count +new_start,new_count @@` hunk headers.
   - For `RowKind::Ctx`: ` {text}`
   - For `RowKind::Add`: `+{text}`
   - For `RowKind::Del`: `-{text}`
   - For `RowKind::Mod`: emits paired `-{del_text}` and `+{text}` rows.
   - Appends review note callouts (`> [!NOTE]\n> 💬 **Line X**: {note.text}`).

---

## 2. Phase 2 — Facade Re-Exports & Top-Level Engine Integration

**Files**:
- [`crates/diffuse-render/src/lib.rs`](crates/diffuse-render/src/lib.rs)
- [`crates/diffuse/src/lib.rs`](crates/diffuse/src/lib.rs)

1. Export `pub mod markdown;` and `pub use markdown::{MarkdownOptions, render_file_markdown, render_markdown_output};` from `diffuse-render`.
2. Re-export in `diffuse::render::*` and `diffuse::prelude::*`.
3. Add doc-test in `diffuse::src/lib.rs` verifying markdown rendering from the prelude.

---

## 3. Phase 3 — CLI Argument Parsing & Direct Markdown Dispatch

**File**: [`crates/diffuse/src/main.rs`](crates/diffuse/src/main.rs)

1. Add CLI flags to `Args`:
   ```rust
   /// Emit diff in GitHub-Flavored Markdown (GFM) format.
   #[arg(short = 'm', long = "markdown", global = true)]
   markdown: bool,
   ```
2. In `main.rs` dispatch:
   - When `args.markdown` is present, treat as non-interactive (`no_tui = true`).
   - Run pipeline or consume streaming file output into `PipelineOutput`.
   - Call `render_markdown_output(&output, &MarkdownOptions::default())`.
   - Print result to `io::stdout()` and exit with code 0.

---

## 4. Phase 4 — Unit Tests, CLI Integration Tests & Acceptance Verification

**Files**:
- [`crates/diffuse-render/src/markdown.rs`](crates/diffuse-render/src/markdown.rs) (unit tests)
- [`crates/diffuse/tests/cli.rs`](crates/diffuse/tests/cli.rs) (integration tests)

1. **Unit Tests**:
   - `test_markdown_summary_table_generation`: verify multi-file table metrics.
   - `test_markdown_collapsible_details`: verify `<details open>` tags.
   - `test_markdown_hunk_and_mod_formatting`: verify diff code fence content.
   - `test_markdown_note_alerts`: verify review notes rendering.
2. **CLI Integration Tests**:
   - Test `diffuse -m` and `diffuse --markdown` execution on git repository fixtures.
3. **Verification Gate**:
   - Run `./scripts/check.sh` ensuring clean format, clippy, cargo tests, and pi-review extension tests.
