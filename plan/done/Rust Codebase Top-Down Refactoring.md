# Plan — Rust Codebase Top-Down Architectural Refactoring

## Global TODO
- [x] Phase 1 — Wire Contract & Pipeline Decoupling (`src/wire/` & `src/pipeline/`)
  - [x] Step 1.1: Extract serializable wire models into `src/wire/mod.rs`
  - [x] Step 1.2: Decouple AST pipeline runner and introduce `PipelineContext`
  - [x] Step 1.3: Update acceptance tests and verify byte-identical wire output
- [x] Phase 2 — Syntax Engine Modularization (`src/syntax/`)
  - [x] Step 2.1: Decompose `src/syntax/` into `profile`, `classifier`, `fence`, and `languages`
  - [x] Step 2.2: Replace runtime `HashSet<String>` keyword allocations with static sorted slices
  - [x] Step 2.3: Verify syntax highlighting unit tests & golden parity
- [x] Phase 3 — Interactive TUI Subsystem Decomposition (`src/tui/`)
  - [x] Step 3.1: Decompose `AppState` types into `types.rs`
  - [x] Step 3.2: Split frame rendering into `view/` components (`header`, `diff_grid`, `modals`)
  - [x] Step 3.3: Introduce `TuiCommand` / action dispatcher in `src/tui/command.rs` and `event.rs`
  - [x] Step 3.4: Verify all TUI interaction unit tests and full check.sh suite
- [x] Phase 4 — Engine Allocation Optimization & Move Semantics
  - [x] Step 4.1: Eliminate redundant `.clone()` calls across line parsing and context allocation
  - [x] Step 4.2: Optimize DP solver matrix allocations in `src/cluster/dp.rs` with reusable scratch buffers
  - [x] Step 4.3: Verify apply oracle and DP projection invariants
- [x] Phase 5 — Full Acceptance Gate & Documentation
  - [x] Step 5.1: Run complete `./scripts/check.sh` suite across all corpora
  - [x] Step 5.2: Install extension and perform visual review

---

## Executive Summary

Over the course of rapid development, the diffuse engine has reached full functional completeness, native interactive TUI support, subword visual diffing, and 100% parity across all 7 frozen golden corpora. However, rapid iteration concentrated significant complexity into several large, monolithic modules:
1. `src/tui/app.rs` (~3,400 lines) and `src/tui/event.rs` (~840 lines) bundle state management, word-wrapping math, keyboard handling, dialog rendering, and Git actions into a single struct.
2. `src/syntax/mod.rs` (~1,325 lines) mixes language definitions, regex registries, fence scanners, and token classification.
3. `src/bin_pipeline.rs` (~985 lines) combines external JSON wire contracts with internal AST pipeline transformations.

This plan executes a **top-down architectural refactoring** to transform the codebase into a modular, clean, and idiomatic Rust architecture:
- **Clean Separation of Concerns**: Strict boundaries between serialization models, pipeline execution, syntax analysis, diff algorithms, and terminal views.
- **Idiomatic Rust Data Modeling**: Strong types, fine-grained enums, command dispatchers, and state-machine separation.
- **Zero-Copy & Move Semantics**: Eliminating unnecessary heap clones in favor of borrowing, moving, and reusable scratch buffers.
- **Invariance & Equivalence**: Every step guards against behavioral regression with `cargo test`, `npm test`, the `apply_oracle`, and golden acceptance walks.

---

## Table of Contents

1. [Phase 1 — Wire Contract & Pipeline Decoupling](#1-phase-1--wire-contract--pipeline-decoupling)
2. [Phase 2 — Syntax Engine Modularization](#2-phase-2--syntax-engine-modularization)
3. [Phase 3 — Interactive TUI Subsystem Decomposition](#3-phase-3--interactive-tui-subsystem-decomposition)
4. [Phase 4 — Engine Allocation Optimization & Move Semantics](#4-phase-4--engine-allocation-optimization--move-semantics)
5. [Phase 5 — Full Acceptance Gate & Documentation](#5-phase-5--full-acceptance-gate--documentation)

---

## 1. Phase 1 — Wire Contract & Pipeline Decoupling

### Problem & Motivation
`src/bin_pipeline.rs` currently holds the frozen wire contract data structures (`PipelineOutput`, `OutputFile`, `OutputHunk`, `OutputRow`, `OutputRun`, `OutputNote`, `OutputFence`) alongside pipeline orchestration functions (`run_pipeline_full`, `colored_runs`, `classify_ctx_line`) and AST reconstruction utilities. This blurs the boundary between the public serialization contract (consumed by CLI JSON and the pi extension) and internal engine transformations.

### Concrete Steps

#### Step 1.1: Extract Wire Models (`src/wire/mod.rs`)
- Move `OutputRow`, `OutputRun`, `OutputFence`, `OutputNote`, `OutputHunk`, `OutputFile`, and `PipelineOutput` into a dedicated `src/wire/` module.
- Implement explicit conversion traits (`From<&OutputRow> for Row`, `From<&Row> for OutputRow`) and preserve `reverse()` and `reconstruct()` methods.
- Verify that serde serialization attributes (`camelCase`, `skip_serializing_if`) remain 100% byte-identical.

#### Step 1.2: Refactor Pipeline Execution (`src/pipeline/mod.rs`)
- Introduce a strongly-typed `PipelineContext`:
  ```rust
  pub struct PipelineContext<'a> {
      pub diff: &'a str,
      pub contents: &'a HashMap<String, String>,
      pub old_contents: Option<&'a HashMap<String, String>>,
      pub notes_db: Option<&'a NotesDB>,
      pub repo: Option<&'a str>,
  }
  ```
- Modularize the transformation stages into clean functions:
  1. `parse_and_load`: Diff parsing and content attachment.
  2. `detect_moves`: Greedy file-wide relocation pass.
  3. `collapse_and_cluster`: DP transformation and mod assembly.
  4. `synthesize_context`: Visible row generation and fence region detection.
  5. `classify_runs`: Token-level semantic syntax coloring.

#### Step 1.3: Verification Gate
- Run `cargo test` (including `tests/golden.rs` bin and lib legs).
- Assert byte-for-byte identity against all 7 golden fixtures.

---

## 2. Phase 2 — Syntax Engine Modularization

### Problem & Motivation
`src/syntax/mod.rs` (1,325 lines) contains 25+ language definition profiles embedded in a giant `lang_profile` function, along with regex caches, markdown classifiers, and fence boundary detectors. Language definitions also allocate runtime `HashSet<String>`s for keywords on every profile lookup.

### Concrete Steps

#### Step 2.1: Decompose `src/syntax/` into Clean Submodules
- `src/syntax/mod.rs`: Top-level public interface and re-exports.
- `src/syntax/profile.rs`: `LangProfile`, `BlockDelimStyle`, SGR constants.
- `src/syntax/registry.rs`: Profile registry, extension mapping, and regex caches.
- `src/syntax/languages/`: Dedicated modules for language families:
  - `rust.rs`: Rust keywords, types, doc comments, doctest strip prefixes.
  - `python.rs`: Python keywords, hash comments, colon indentation rules.
  - `c_family.rs`: C, C++, C#, Java, Go, Zig.
  - `web.rs`: TypeScript, JavaScript, HTML, CSS, JSON, GraphQL.
  - `config.rs`: YAML, TOML, SQL, Dockerfile, Diff.
  - `shell.rs`: Bash, Zsh, Fish.
  - `markdown.rs`: Prose token classification, inline code backticks, emphasis.
- `src/syntax/classifier.rs`: `classify_tokens`, `classify_line`, `md_inner_tokens`.
- `src/syntax/fence.rs`: `fence_regions`, `region_at`, `fence_lang_ext`.

#### Step 2.2: Static / Const Keyword Lookup
- Replace runtime heap allocations with static sorted arrays:
  ```rust
  pub const RUST_KEYWORDS: &[&str] = &[
      "as", "async", "await", "break", "const", "continue", "crate", "dyn",
      "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in",
      "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
      "self", "Self", "static", "struct", "super", "trait", "true", "type",
      "unsafe", "use", "where", "while", "yield",
  ];
  ```
- Implement fast binary search `keywords.binary_search(&token).is_ok()` (or `phf` compile-time maps) to make syntax profile creation $O(1)$ and zero-allocation.

#### Step 2.3: Verification Gate
- Run all 10 `src/syntax/tests.rs` tests.
- Confirm full ANSI rendering golden walk equivalence.

---

## 3. Phase 3 — Interactive TUI Subsystem Decomposition

### Problem & Motivation
`src/tui/app.rs` is over 3,390 lines long, with `AppState` serving as a monolithic container for every aspect of TUI execution. `src/tui/event.rs` is an 840-line procedural key-matching loop. This makes navigation math, layout rendering, and modal management difficult to audit and test independently.

### Concrete Steps

#### Step 3.1: Decompose State Management (`src/tui/state/`)
- Split `AppState` into cohesive sub-structures:
  - `NavigationState`: `row_idx`, `vis_line`, `anchor`, `confirmed_sel`, `num_buffer`.
  - `ViewportState`: `scroll`, `col_offset`, `terminal_width`, `terminal_height`, wrap line index cache.
  - `TuiOptions`: `syntax_on`, `word_diff`, `raw_diff`, `show_help`.
  - `ModalState`: Strongly typed enum:
    ```rust
    pub enum ActiveModal {
        None,
        Help,
        FileFinder(FileFinderState),
        NoteDraft { file_idx: usize, line: u32, side: Side, text: String },
        ConfirmAction { prompt: String, action: Box<dyn FnOnce(&mut AppState)> },
    }
    ```

#### Step 3.2: Modular View Rendering (`src/tui/view/`)
- Extract frame generation logic from `app.rs`:
  - `src/tui/view/layout.rs`: Top-level viewport split and row clipping.
  - `src/tui/view/header.rs`: Top review header bar, repo status, counts, file selector summary.
  - `src/tui/view/diff_grid.rs`: Visual diff row rendering, cursor highlighting, selection bands, scrollbars.
  - `src/tui/view/modals.rs`: Centered popup rendering for Help, File Finder, and Notes.

#### Step 3.3: Command Pattern Event Dispatching (`src/tui/command.rs`)
- Decouple keyboard/mouse events from state mutation:
  ```rust
  pub enum TuiCommand {
      Scroll(ScrollDirection),
      MoveCursor(CursorMotion),
      JumpHunk(Direction),
      JumpFile(Direction),
      ToggleMode(DisplayMode),
      StartSelection,
      ConfirmSelection,
      OpenEditor,
      StageHunk,
      DiscardHunk,
      OpenModal(ModalKind),
      CloseModal,
      Quit,
  }
  ```
- `src/tui/event.rs` translates `crossterm::Event` into `TuiCommand`, and `AppState::execute(&mut self, cmd: TuiCommand)` executes the state mutation cleanly.

#### Step 3.4: Verification Gate
- Run all 22 TUI unit tests in `src/tui/app.rs`, `src/tui/event.rs`, `src/tui/finder.rs`, `src/tui/actions.rs`, and `src/tui/editor.rs`.
- Verify interactive mode via `diffuse` binary in terminal.

---

## 4. Phase 4 — Engine Allocation Optimization & Move Semantics

### Problem & Motivation
Multiple hot paths perform unnecessary string clones and allocations during line tokenization, context allocation, and DP matrix solving.

### Concrete Steps

#### Step 4.1: Borrowed Slices & Move Optimization
- In `src/visible.rs`: Replace `Vec<String>` allocations for context rows with slice references where possible.
- In `src/parse/`: Use `Cow<'a, str>` or pass `&str` references during hunk and row assembly.
- In `src/git.rs`: Move loaded file contents directly into `DiffFile` structs without cloning intermediate maps.

#### Step 4.2: DP Solver Matrix Optimization (`src/cluster/dp.rs`)
- In `solve_cluster_dp`: Replace `Vec<Vec<DPCell>>` allocations with a flat vector `Vec<DPCell>` or a reusable pre-allocated scratch buffer on `DPState`.
- Avoid re-allocating `HashSet`s and token vectors on every small cluster solve.

#### Step 4.3: Verification Gate
- Run the full test suite including the Phase 6 `apply_oracle` and DP projection assertions.

---

## 5. Phase 5 — Full Acceptance Gate & Documentation

### Concrete Steps

#### Step 5.1: Acceptance Walk
- Run `./scripts/check.sh` (nightly fmt check, clippy with `-D warnings`, full cargo test suite, and npm equivalence smoke tests).
- Validate byte-identical output across all golden corpora:
  - `mirage-2025-08`
  - `diffuse-2025-08`
  - `opt-1-cache-regex-window-slice`
  - `opt-2-cache-markdown-regex`
  - `opt-3-cache-lang-profile`
  - `opt-4-manual-scanner-prefilter`
  - `diffuse-history-mid`
  - `untracked-files`

#### Step 5.2: Extension Packaging & Human Review
- Run `./extension/pi/scripts/install.sh` to bundle release binary into `~/.pi/agent/extensions/diffuse-review/`.
- Present changes and pause for human visual and code review.
