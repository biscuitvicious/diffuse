# Two-Parameter Context Architecture & Carry-Through

- [x] Phase 1: Two-Parameter Context Design (`ENGINE_CONTEXT` vs `visual_context`)
- [x] Phase 2: Context AST Carry-Through (Preserve `Row::Ctx` throughout parsing, shifting, and collapse)
- [x] Phase 3: Context Normalization & Trimming Pipeline (`normalize_context`, budget trimming, and hunk merging)
- [x] Phase 4: Pure Walk Visible Assembly (`build_visible` pure walk without synthetic file reconstruction)
- [x] Phase 5: Reflow & Relocation Semantics in Collapse (Fix same-position vs cross-position moves & run-based reflow absorption)
- [x] Phase 6: Full Verification Across Golden Corpora & Check Suite

---

## Executive Summary

This architecture redesign decouples diff-generation context (`ENGINE_CONTEXT = 4` in `git -U4`) from presentation context (`DEFAULT_CONTEXT_LINES = 4`, visual context in the TUI/CLI).

Previously, the diff engine relied on a context reconstruction heuristic in `buildVisible`, which synthesized missing lines directly from whole-file content arrays. This masked dropping lines in collapse passes and coupled the engine to file contents for presentation.

In this carry-through architecture:
1. `git diff -U4` provides anchoring context (`ENGINE_CONTEXT`).
2. Diff parsing captures context directly as `Row::Ctx` in the diff AST.
3. Hunk shifting and collapse manipulate the diff AST without discarding or synthesizing rows out of thin air.
4. Reflow absorption is strictly scoped to multiline del runs absorbed into their paired `Mod` row; exact moves (`Row::Mov`) across different line positions are preserved; and same-position local moves within a hunk are cleanly handled.
5. A deterministic `normalize_context` pass trims, expands, and merges hunks to match the user's desired visual context budget.
6. `build_visible` is a pure, linear walk over `f.hunks`.

---

## Table of Contents

1. [Phase 1 — Two-Parameter Context Design](#1-phase-1--two-parameter-context-design)
2. [Phase 2 — Context AST Carry-Through](#2-phase-2--context-ast-carry-through)
3. [Phase 3 — Context Normalization & Trimming](#3-phase-3--context-normalization--trimming)
4. [Phase 4 — Pure Walk Visible Assembly](#4-phase-4--pure-walk-visible-assembly)
5. [Phase 5 — Reflow & Relocation Semantics](#5-phase-5--reflow--relocation-semantics)
6. [Phase 6 — Verification & Check Gate](#6-phase-6--verification--check-gate)

---

## 1. Phase 1 — Two-Parameter Context Design

- Defined `ENGINE_CONTEXT = 4` and `DEFAULT_CONTEXT_LINES = 4` in `crates/diffuse-core/src/git.rs`.
- Threaded `visual_context` parameter through `run_pipeline_full`, `stream_diff_pipeline`, `spawn_git_stream_pipeline`, and CLI/TUI drivers.

## 2. Phase 2 — Context AST Carry-Through

- Preserved `Row::Ctx` in both `hunk.rows` and `raw_hunk.rows` during diff parsing (`crates/diffuse-core/src/parse/mod.rs`).
- Shift pass reclassifies `Add` ↔ `Ctx` in place without mutating hunk lengths.

## 3. Phase 3 — Context Normalization & Trimming

- Implemented `normalize_context` in `crates/diffuse-core/src/shift/mod.rs` to trim excess leading/trailing `Ctx` rows beyond the requested visual budget and merge adjacent hunks that fall within proximity threshold.

## 4. Phase 4 — Pure Walk Visible Assembly

- Refactored `build_visible` in `crates/diffuse-core/src/visible.rs` into a pure linear iteration over `f.hunks`, inserting `RowKind::Sep` hidden gap markers between distinct hunks.

## 5. Phase 5 — Reflow & Relocation Semantics

- Replaced brittle global string matching in `pass_filter_reflows` with run-scoped reflow absorption for multiline dels absorbed into a `Mod` row.
- Differentiated same-position reshuffles (`old_line == new_line`) from true relocated moves (`Row::Mov` across line positions), ensuring no missing visible rows or skipped line number gaps in large stress corpora (`diffuse-history-mid`).

## 6. Phase 6 — Verification & Check Gate

- All unit tests in `diffuse-core`, `diffuse-syntax`, `diffuse-render`, `diffuse-tui`, and `diffuse-cli` pass.
- All 8 golden corpora in `tests/golden.rs` pass on both lib leg and bin leg (including `mirage-2025-08`, `diffuse-2025-08`, `opt-1..4`, `untracked-files`, and `diffuse-history-mid`).
- Full `./scripts/check.sh` suite passes cleanly.
- Extension binary installed via `extension/pi/scripts/install.sh`.
