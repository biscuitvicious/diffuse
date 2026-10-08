# Compact Wire Format & Run Stream Simplification

- [x] Global TODO
  - [x] **Phase 1 — RunKind & Color Consolidation**
    - [x] Unify `RunKind` to 3 variants (`Ctx`, `Del`, `Add`) and eliminate `Common` and `Mov` across `crates/diffuse-core`
    - [x] Introduce canonical single-char diff signs (`'_'`, `'-'`, `'+'`) for serialization/display
    - [x] Introduce 3-letter semantic color abbreviations (`sop`, `skw`, `sfn`, `sty`, `str`, `snb`, `scm`, `bld`, `ita`, `lnk`, `url`, `rul`, `dlm`, `stk`, `cde`, `tbo`, `thd`, `lbo`, `acc`, `mut`) with default `txt` omitted (each markdown/table code additionally accepts its `UPPER` and long-form alias: `CDE`/`code`, `TBO`/`table_border`, `THD`/`table_header`, `LBO`/`link_border`)
    - [x] Implement `new_text(&self)` and `old_text(&self)` projection methods on runs
    - [x] Add unit test suite for consolidation and text projection
  - [x] **Phase 2 — Core Wire Model Refactoring & Redundancy Elimination**
    - [x] Drop `spans` from `OutputRow` serialization and wire contract (derive on demand via `OutputRow::spans()` if needed)
    - [x] Remove `text` and `del_text` from serialized wire row representation (derive via `new_text()` and `old_text()`)
    - [x] Refactor `OutputRow` to compute `text()` and `del_text()` from `runs` without backwards compatibility
    - [x] Implement compact array-based tuple encoding for `OutputRun` and `OutputRow`
    - [x] Add serialization and round-trip deserialization unit tests in `diffuse-core`
  - [x] **Phase 3 — Renderer & Consumer Adaptation**
    - [x] Update `crates/diffuse-render` to consume `runs`, `new_text()`, and `old_text()`
    - [x] Update `crates/diffuse-tui` and modals to use updated wire models
    - [x] Verify `extension/pi` smoke tests and backend integration
  - [x] **Phase 4 — Golden Corpus & CLI Harvest Migration**
    - [x] Update `crates/diffuse/src/bin/harvest.rs` to generate new compact wire records
    - [x] Re-harvest all `tests/golden/*/outputs/wire.jsonl` files in `tests/golden` submodule
    - [x] Assert `.render.*.txt` and `.markdown.md` outputs remain 100% byte-identical
    - [x] Update `crates/diffuse/tests/golden.rs` and CLI verification tests
  - [x] **Phase 5 — Full Verification & Benchmark Review**
    - [x] Execute `scripts/check.sh` with zero warnings
    - [x] Measure exact payload reduction across golden corpus (target >60% reduction, achieved 70.8%)
    - [x] Commit and sync submodules (completed in commit a11d526)

---

## Executive Summary

Diffuse's wire contract (`OutputRow`, `OutputRun`) currently suffers from severe duplication and verbosity:
1. **Triplicated Text**: Every row serializes the complete line text three times (`row.text`, `row.del_text`, and sliced across `row.runs`).
2. **Obsolete Spans**: `row.spans` (`{"del":[],"add":[]}`) is emitted on every line despite 98.8% being empty and renderers ignoring it.
3. **Redundant Run Kinds**: `common` and `ctx` both mean unchanged text; `mov` is never emitted for runs.
4. **Key Overhead**: Every run and row repeats verbose map keys (`"kind"`, `"color"`, `"text"`, `"newLine"`, `"oldLine"`, `"runs"`).

This plan refactors the wire data model without backwards compatibility:
- Eliminates `common` and `mov` from token classifications, consolidating to 3 canonical diff states: `Ctx` (`'_'`), `Del` (`'-'`), `Add` (`'+'`).
- Replaces verbose color names with 3-char semantic abbreviations, omitting the default `txt` color.
- Drops `text`, `del_text`, and `spans` from wire rows. Line text is projected directly from runs via `new_text()` and `old_text()`.
- Implements compact array-based tuple encoding for runs and rows.
- Slashes golden wire JSONL payloads by **60% to 75%** (~20–25 MB reduction across the test suite) while preserving 100% byte-identical terminal and markdown rendering.

---

## Table of Contents

1. [Phase 1 — RunKind & Color Consolidation](#1-phase-1--runkind--color-consolidation)
2. [Phase 2 — Core Wire Model Refactoring & Redundancy Elimination](#2-phase-2--core-wire-model-refactoring--redundancy-elimination)
3. [Phase 3 — Renderer & Consumer Adaptation](#3-phase-3--renderer--consumer-adaptation)
4. [Phase 4 — Golden Corpus & CLI Harvest Migration](#4-phase-4--golden-corpus--cli-harvest-migration)
5. [Phase 5 — Full Verification & Benchmark Review](#5-phase-5--full-verification--benchmark-review)

---

## 1. Phase 1 — RunKind & Color Consolidation

### Objectives
- Simplify `RunKind` to 3 canonical states: `Ctx`, `Del`, `Add`.
- Encode diff signs as single ASCII characters (`' '`, `'-'`, `'+'`).
- Compress semantic colors into 3-character abbreviations, omitting default `txt`.
- Provide clean `new_text()` and `old_text()` projection functions across slices of runs.

### Affected Modules
- [`crates/diffuse-core/src/merge.rs`](crates/diffuse-core/src/merge.rs)
- [`crates/diffuse-core/src/cluster/render.rs`](crates/diffuse-core/src/cluster/render.rs)
- [`crates/diffuse-core/src/tape.rs`](crates/diffuse-core/src/tape.rs)
- [`crates/diffuse-core/src/pipeline.rs`](crates/diffuse-core/src/pipeline.rs)
- [`crates/diffuse-render/src/theme/palette.rs`](crates/diffuse-render/src/theme/palette.rs)

### Detailed Steps

1. **Refactor `RunKind` Enum**:
   ```rust
   #[derive(Clone, Copy, PartialEq, Eq, Debug)]
   pub enum RunKind {
       Ctx,
       Del,
       Add,
   }

   impl RunKind {
       #[inline]
       pub fn sign_char(self) -> char {
           match self {
               RunKind::Ctx => '_',
               RunKind::Del => '-',
               RunKind::Add => '+',
           }
       }
   }
   ```
   Remove `RunKind::Common` and `RunKind::Mov`. Replace all internal tape and cluster emissions of `Common` with `Ctx`.

2. **Implement `new_text()` and `old_text()` on Run Slices**:
   ```rust
   pub fn runs_new_text<'a, I>(runs: I) -> String
   where
       I: IntoIterator<Item = &'a OutputRun>,
   {
       runs.into_iter()
           .filter(|r| r.kind != RunKind::Del)
           .map(|r| r.text.as_ref())
           .collect()
   }

   pub fn runs_old_text<'a, I>(runs: I) -> String
   where
       I: IntoIterator<Item = &'a OutputRun>,
   {
       runs.into_iter()
           .filter(|r| r.kind != RunKind::Add)
           .map(|r| r.text.as_ref())
           .collect()
   }
   ```

3. **Define Canonical 3-Letter Semantic Color Table** (final scheme as
   implemented in `crates/diffuse-syntax/src/color.rs`; the earlier
   `m*`-prefixed draft was superseded during implementation):
   - Syntax: `sop` (operator), `skw` (keyword), `sfn` (function), `sty` (type), `str` (string), `snb` (number), `scm` (comment).
   - Markdown: `bld` (bold), `ita` (italic), `lnk` (link), `url` (url), `rul` (rule), `dlm` (delim), `stk` (strike), `cde` (inline code span), `tbo` (table border), `thd` (table header), `lbo` (link border).
   - Base UI: `acc` (accent), `mut` (muted).
   - Default: `txt` (omitted on the wire when not modified by SGR).
   - Aliases: `cde` also accepts `CDE`/`code`, `tbo` accepts `TBO`/`table_border`, `thd` accepts `THD`/`table_header`, `lbo` accepts `LBO`/`link_border` — each resolving to its own `ThemePalette` key (`code`, `table_border`, `table_header`, `link_border`, each with `add_`/`del_` row overrides). Inline code spans migrated off `str`, table pipes/headers off `mut`/`ita`, link parens off `border` (kept only for quote/fence bars).

---

## 2. Phase 2 — Core Wire Model Refactoring & Redundancy Elimination

### Objectives
- Eliminate `text`, `del_text`, and `spans` fields from serialization in `OutputRow`.
- Implement compact array-based serialization for `OutputRun` and `OutputRow`.
- Add serde serialization and deserialization implementations without backwards compatibility fallbacks.

### Affected Modules
- [`crates/diffuse-core/src/wire.rs`](crates/diffuse-core/src/wire.rs)
- [`crates/diffuse-core/src/wire/jsonl.rs`](crates/diffuse-core/src/wire/jsonl.rs)
- [`crates/diffuse-core/src/parse.rs`](crates/diffuse-core/src/parse.rs)

### Detailed Steps

1. **Refactor `OutputRow` Struct**:
   ```rust
   #[derive(Clone, PartialEq, Eq, Debug)]
   pub struct OutputRow {
       pub kind: RowKind,
       pub mov_side: Option<MovSide>,
       pub new_line: Option<u32>,
       pub old_line: Option<u32>,
       pub runs: Vec<OutputRun>,
       pub wholesale: bool,
       pub group: Option<RowGroup>,
   }

   impl OutputRow {
       #[inline]
       pub fn new_text(&self) -> String {
           runs_new_text(&self.runs)
       }

       #[inline]
       pub fn old_text(&self) -> String {
           runs_old_text(&self.runs)
       }

       /// Derives character spans on demand for callers needing intra-line offsets.
       pub fn spans(&self) -> Spans {
           derive_spans_from_runs(&self.runs)
       }
   }
   ```

2. **Custom Array Serialization for `OutputRun`**:
   - If no color/sgr: serialize as string scalar `"+text"`, `" text"`, `"-text"`.
   - If color: serialize as `["+text", "skw"]`.
   - If SGR present: serialize as `["+text", "acc", 1]`.

3. **Row Record Serialization in `jsonl.rs`**:
   Streamline line records in JSONL to lean representations:
   `["line", kind, old_line, new_line, runs]` or lean object without duplicate text/spans.

---

## 3. Phase 3 — Renderer & Consumer Adaptation

### Objectives
- Update all terminal rendering, diff grid, and modal components to use `new_text()` and `old_text()`.
- Eliminate unused `_spans` parameters in `diffuse-render`.
- Verify TypeScript client in `extension/pi/`.

### Affected Modules
- [`crates/diffuse-render/src/row.rs`](crates/diffuse-render/src/row.rs)
- [`crates/diffuse-render/src/diff.rs`](crates/diffuse-render/src/diff.rs)
- [`crates/diffuse-tui/src/view/`](crates/diffuse-tui/src/view/)
- [`extension/pi/`](extension/pi/)

### Detailed Steps
1. Update `render_row_segment_into` in `crates/diffuse-render/src/row.rs` to take `runs` directly, removing the unused `_spans` argument.
2. Update callers that previously accessed `r.text` or `r.del_text` to call `r.new_text()` and `r.old_text()`.
3. Update `extension/pi` smoke test if wire contract parsing is exercised.

---

## 4. Phase 4 — Golden Corpus & CLI Harvest Migration

### Objectives
- Update `harvest.rs` to emit the new compact wire schema.
- Re-harvest all `tests/golden/*/outputs/wire.jsonl` files in the golden submodule.
- Verify that terminal text renders (`.render.*.txt`) and `.markdown.md` remain 100% byte-identical to current golden fixtures — with one reviewed exception (see below).

> **Reviewed deviation**: inline code spans were given a dedicated `code`
> palette color distinct from `string` (dark `38;5;81` cyan vs `38;5;120`
> green; light `38;2;12;100;125` teal vs `38;2;20;115;40` green). The
> re-harvest therefore touched 8 `render.*.txt` fixtures; agent review
> confirmed plain-text output is byte-identical everywhere and the only ANSI
> delta is the code-span hue. All other new palette keys (`tbo`, `thd`,
> `lbo`) duplicate their predecessor values, so their renders are unchanged.
> No `.markdown.md` fixture changed.

### Affected Modules
- [`crates/diffuse/src/bin/harvest.rs`](crates/diffuse/src/bin/harvest.rs)
- [`crates/diffuse/tests/golden.rs`](crates/diffuse/tests/golden.rs)
- [`tests/golden/`](tests/golden/)

### Detailed Steps
1. Run `cargo run -p diffuse --bin harvest -- --all` to regenerate `wire.jsonl` files.
2. Run `git diff tests/golden/` to confirm that:
   - `outputs/wire.jsonl` files changed (compact encoding + color-code splits).
   - All `outputs/render.*.txt` and `outputs/markdown.md` files are **completely untouched** — except the reviewed code-color deviation above (8 `render.*.txt` files, ANSI hue only, plain text identical).
3. Update `crates/diffuse/tests/golden.rs` to parse the new wire records.

---

## 5. Phase 5 — Full Verification & Benchmark Review

### Objectives
- Pass all unit tests, integration tests, and verification scripts.
- Verify memory and latency budgets.

### Verification Gates
1. Run `scripts/check.sh` — must pass cleanly with 0 warnings.
2. Verify golden acceptance tests in `crates/diffuse/tests/golden.rs`.
3. Measure final wire payload sizes across all golden corpora.
