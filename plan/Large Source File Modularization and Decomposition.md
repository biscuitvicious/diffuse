# Plan — Large Source File Modularization and Decomposition

## Global TODO
- [x] Phase 1 — Dynamic Programming Solver Engine (`crates/diffuse-core/src/cluster/dp.rs` · 3,823 lines)
- [x] Phase 2 — Hunk Modification Assembly & Reflow Engine (`crates/diffuse-core/src/collapse.rs` · 2,682 lines)
- [x] Phase 3 — CLI Entry Point, Args & Command Dispatch (`crates/diffuse/src/main.rs` · 1,916 lines)
- [x] Phase 4 — Line Tokenization, LCS Alignment & Word Spans (`crates/diffuse-core/src/lcs.rs` · 1,697 lines)
- [x] Phase 5 — Diff Pipeline Orchestration & Run Classification (`crates/diffuse-core/src/pipeline.rs` · 1,671 lines)
- [x] Phase 6 — Public Wire Contract & Serialization Models (`crates/diffuse-core/src/wire.rs` · 1,664 lines)
- [x] Phase 7 — Bit-Parallel LCS Vector Search (`crates/diffuse-core/src/lcs/bit_parallel.rs` · 1,535 lines)
- [x] Phase 8 — Token Classification & Markdown Token Streaming (`crates/diffuse-syntax/src/classifier.rs` · 1,443 lines)
- [x] Phase 9 — Markdown Document AST & Event Reducer (`crates/diffuse-syntax/src/markdown.rs` · 1,433 lines)
- [x] Phase 10 — Review Notes Data Store & Anchor Matching (`crates/diffuse-core/src/notes.rs` · 1,375 lines)
- [x] Phase 11 — TUI Viewport Diff Lines & Dual Frame Renderer (`crates/diffuse-tui/src/app/render.rs` · 1,344 lines)
- [x] Phase 12 — Terminal Markdown Flow, Table & Inline Renderer (`crates/diffuse-render/src/markdown/terminal.rs` · 1,198 lines)
- [x] Phase 13 — Unified Diff Parser & Core AST Models (`crates/diffuse-parse/src/lib.rs` · 1,178 lines)
- [x] Phase 14 — Invertible Alignment Tape & Run Reconstruction (`crates/diffuse-core/src/tape.rs` · 1,086 lines)
- [x] Phase 15 — Syntactic Language Profiles & Delimiter Tables (`crates/diffuse-syntax/src/profile.rs` · 1,032 lines)
- [ ] Phase 16 — Dyck Language Pushdown Automata & Containers (`crates/diffuse-algo/src/dyck.rs` · 1,017 lines)
- [ ] Phase 17 — Review Notes CLI Action Handlers & Formatters (`crates/diffuse/src/notes_cli.rs` · 1,015 lines)
- [ ] Phase 18 — Token Semantics, Entropy & Container Affinities (`crates/diffuse-core/src/token.rs` · 996 lines)
- [ ] Phase 19 — Interactive TUI Modals & Dialog Views (`crates/diffuse-tui/src/view/modals.rs` · 994 lines)
- [ ] Phase 20 — TUI Review Notes Lifecycle & Disk Sync (`crates/diffuse-tui/src/app/notes.rs` · 935 lines)
- [ ] Phase 21 — Interactive Text Buffer & History Editor (`crates/diffuse-tui/src/editor.rs` · 932 lines)
- [ ] Phase 22 — Terminal Row Segments, Gutters & Raw Diff Formatting (`crates/diffuse-render/src/row.rs` · 859 lines)
- [ ] Phase 23 — Golden Test Harvesting & Fixture Ingestion CLI (`crates/diffuse/src/bin/harvest.rs` · 854 lines)
- [ ] Phase 24 — Visual Line Geometry & Scrollbar Indicators (`crates/diffuse-tui/src/app/geometry.rs` · 775 lines)
- [ ] Phase 25 — Cluster DP Pairing & Surplus Survival (`crates/diffuse-core/src/cluster/pairing.rs` · 771 lines)
- [ ] Phase 26 — Spatial Neighborhood & Relocation Matching (`crates/diffuse-algo/src/bipartite_matching.rs` · 760 lines)
- [ ] Phase 27 — Syntactic Profile Registry & Extension Mapping (`crates/diffuse-syntax/src/registry.rs` · 744 lines)
- [ ] Phase 28 — Context Row Synthesis & Hunk Folding State (`crates/diffuse-tui/src/app/context.rs` · 731 lines)
- [ ] Phase 29 — ANSI Escape Sequence & Color Styling Models (`crates/diffuse-render/src/ansi.rs` · 702 lines)
- [ ] Phase 30 — Git Domain Models, Repositories & Review Targets (`crates/diffuse-git/src/types.rs` · 693 lines)
- [ ] Phase 31 — File Header Banners & Diff Hunk Rendering (`crates/diffuse-render/src/diff.rs` · 686 lines)
- [ ] Phase 32 — Markdown Table Formatting & Side-by-Side Diffing (`crates/diffuse-render/src/markdown.rs` · 667 lines)
- [ ] Phase 33 — Theme Color Palettes & Contrast Specifications (`crates/diffuse-render/src/theme/palette.rs` · 663 lines)
- [ ] Phase 34 — TUI Top Header & Revision Status Bar (`crates/diffuse-tui/src/view/header.rs` · 597 lines)
- [ ] Phase 35 — TUI Diff Grid Cell Layout & Column Math (`crates/diffuse-tui/src/view/diff_grid.rs` · 580 lines)
- [ ] Phase 36 — Native Git Tree & Working Copy Diffing (`crates/diffuse-git/src/diff.rs` · 571 lines)
- [ ] Phase 37 — Working Tree Live Reload & Note Synchronization (`crates/diffuse-tui/src/app/sync.rs` · 569 lines)
- [ ] Phase 38 — Native Git Engine & Content Loader (`crates/diffuse-core/src/git.rs` · 565 lines)
- [ ] Phase 39 — Affine-Gap Cost Matrix & Dynamic Programming Traceback (`crates/diffuse-algo/src/affine_gap.rs` · 562 lines)
- [ ] Phase 40 — TUI Application State Lifecycle & Construction (`crates/diffuse-tui/src/app.rs` · 533 lines)
- [ ] Phase 41 — Visual Row & Semantic Hunk Navigation (`crates/diffuse-tui/src/app/navigation.rs` · 529 lines)
- [ ] Phase 42 — Keyboard Event Dispatch & TuiAction Mapping (`crates/diffuse-tui/src/app/dispatch.rs` · 527 lines)
- [ ] Phase 43 — Working Tree Hunk Staging & Mutation Engine (`crates/diffuse-git/src/mutation.rs` · 514 lines)

---

## Executive Summary

An audit of source files across the `diffuse` workspace reveals 43 production Rust modules exceeding the threshold of 500 lines, with 26 files exceeding 750 lines, and 7 monoliths exceeding 1,500 lines. The largest source file, `crates/diffuse-core/src/cluster/dp.rs`, stands at 3,823 lines.

This plan defines a modular decomposition strategy to refactor each large source file into focused submodules under a directory module pattern (`<module>/mod.rs` + `<module>/<submodule>.rs`).

### Architectural Principle: Local Definition of Public Types and Interfaces

A central architectural requirement governs this decomposition: **a module should define locally the `pub` types, structs, enums, and trait interfaces required to use the module.**

1. **Colocation of Types and Behavior**:
   - Avoid artificial `types.rs`, `model.rs`, or `models/` dumping grounds that separate data definitions from the algorithms that operate upon them.
   - A type belongs at the top of the cohesive submodule that implements its operations, constructors, and business logic.
   - For example, `ReflowFilter` is defined locally in `collapse/reflow.rs` alongside `pass_filter_reflows`; `Row` and its line variants are defined locally in `diffuse-parse/src/row.rs` alongside row methods; `OutputRow` and `RowFlags` are defined locally in `wire/row.rs` alongside their custom `Serialize` implementation.
2. **Top-Down Design Pattern**:
   - Within every newly created submodule, components follow strict top-down ordering per [`CODE_STYLE.md`](../CODE_STYLE.md):
     1. Module doc comment (`//!`) and grouped `use` declarations.
     2. Primary public types, structs, enums, and trait definitions required to use the module.
     3. Primary constructors (`new`, `with_*`) and high-level public methods.
     4. Trait implementations (`Display`, `From`, `Serialize`, etc.).
     5. Internal supporting structures, private types, and helper algorithms.
     6. Companion unit tests in dedicated `tests.rs` files.
3. **Cohesive Facade Re-exports**:
   - The parent module (`mod.rs`) acts as an architectural boundary, defining any overarching orchestration entry points and re-exporting (`pub use submodule::{PrimaryType, ...};`) public types so callers consume a unified API without breaking call sites.

Every phase guarantees 100% preservation of public API surfaces, zero behavioral regressions, zero compiler or clippy warnings, and byte-for-byte acceptance verification across all 41 golden corpora.

---

## Table of Contents

### Tier 1 — Monolithic Engines (>1,500 lines)
1. [Phase 1 — Dynamic Programming Solver Engine (`cluster/dp.rs`)](#1-phase-1--dynamic-programming-solver-engine-cratesdiffuse-coresrcclusterdprs--3823-lines)
2. [Phase 2 — Hunk Modification Assembly & Reflow Engine (`collapse.rs`)](#2-phase-2--hunk-modification-assembly--reflow-engine-cratesdiffuse-coresrccollapsers--2682-lines)
3. [Phase 3 — CLI Entry Point, Args & Command Dispatch (`diffuse/main.rs`)](#3-phase-3--cli-entry-point-args--command-dispatch-cratesdiffusesrcmainrs--1916-lines)
4. [Phase 4 — Line Tokenization, LCS Alignment & Word Spans (`lcs.rs`)](#4-phase-4--line-tokenization-lcs-alignment--word-spans-cratesdiffuse-coresrclcsrs--1697-lines)
5. [Phase 5 — Diff Pipeline Orchestration & Run Classification (`pipeline.rs`)](#5-phase-5--diff-pipeline-orchestration--run-classification-cratesdiffuse-coresrcpipeliners--1671-lines)
6. [Phase 6 — Public Wire Contract & Serialization Models (`wire.rs`)](#6-phase-6--public-wire-contract--serialization-models-cratesdiffuse-coresrcwirers--1664-lines)
7. [Phase 7 — Bit-Parallel LCS Vector Search (`lcs/bit_parallel.rs`)](#7-phase-7--bit-parallel-lcs-vector-search-cratesdiffuse-coresrclcsbit_parallelrs--1535-lines)

### Tier 2 — Major Large Files (1,000–1,500 lines)
8. [Phase 8 — Token Classification & Markdown Token Streaming (`syntax/classifier.rs`)](#8-phase-8--token-classification--markdown-token-streaming-cratesdiffuse-syntaxsrcclassifierrs--1443-lines)
9. [Phase 9 — Markdown Document AST & Event Reducer (`syntax/markdown.rs`)](#9-phase-9--markdown-document-ast--event-reducer-cratesdiffuse-syntaxsrcmarkdownrs--1433-lines)
10. [Phase 10 — Review Notes Data Store & Anchor Matching (`core/notes.rs`)](#10-phase-10--review-notes-data-store--anchor-matching-cratesdiffuse-coresrcnotesrs--1375-lines)
11. [Phase 11 — TUI Viewport Diff Lines & Dual Frame Renderer (`tui/app/render.rs`)](#11-phase-11--tui-viewport-diff-lines--dual-frame-renderer-cratesdiffuse-tuisrcapprenderrs--1344-lines)
12. [Phase 12 — Terminal Markdown Flow, Table & Inline Renderer (`render/markdown/terminal.rs`)](#12-phase-12--terminal-markdown-flow-table--inline-renderer-cratesdiffuse-rendersrcmarkdownterminalrs--1198-lines)
13. [Phase 13 — Unified Diff Parser & Core AST Models (`diffuse-parse/src/lib.rs`)](#13-phase-13--unified-diff-parser--core-ast-models-cratesdiffuse-parsesrclibrs--1178-lines)
14. [Phase 14 — Invertible Alignment Tape & Run Reconstruction (`core/tape.rs`)](#14-phase-14--invertible-alignment-tape--run-reconstruction-cratesdiffuse-coresrctapers--1086-lines)
15. [Phase 15 — Syntactic Language Profiles & Delimiter Tables (`syntax/profile.rs`)](#15-phase-15--syntactic-language-profiles--delimiter-tables-cratesdiffuse-syntaxsrcprofilers--1032-lines)
16. [Phase 16 — Dyck Language Pushdown Automata & Containers (`algo/dyck.rs`)](#16-phase-16--dyck-language-pushdown-automata--containers-cratesdiffuse-algosrcdyckrs--1017-lines)
17. [Phase 17 — Review Notes CLI Action Handlers & Formatters (`diffuse/notes_cli.rs`)](#17-phase-17--review-notes-cli-action-handlers--formatters-cratesdiffusesrcnotes_clirs--1015-lines)

### Tier 3 — Moderately Large Modules (750–1,000 lines)
18. [Phase 18 — Token Semantics, Entropy & Container Affinities (`core/token.rs`)](#18-phase-18--token-semantics-entropy--container-affinities-cratesdiffuse-coresrctokenrs--996-lines)
19. [Phase 19 — Interactive TUI Modals & Dialog Views (`tui/view/modals.rs`)](#19-phase-19--interactive-tui-modals--dialog-views-cratesdiffuse-tuisrcviewmodalsrs--994-lines)
20. [Phase 20 — TUI Review Notes Lifecycle & Disk Sync (`tui/app/notes.rs`)](#20-phase-20--tui-review-notes-lifecycle--disk-sync-cratesdiffuse-tuisrcappnotesrs--935-lines)
21. [Phase 21 — Interactive Text Buffer & History Editor (`tui/editor.rs`)](#21-phase-21--interactive-text-buffer--history-editor-cratesdiffuse-tuisrceditorrs--932-lines)
22. [Phase 22 — Terminal Row Segments, Gutters & Raw Diff Formatting (`render/row.rs`)](#22-phase-22--terminal-row-segments-gutters--raw-diff-formatting-cratesdiffuse-rendersrcrowrs--859-lines)
23. [Phase 23 — Golden Test Harvesting & Fixture Ingestion CLI (`diffuse/bin/harvest.rs`)](#23-phase-23--golden-test-harvesting--fixture-ingestion-cli-cratesdiffusesrcbinharvestrs--854-lines)
24. [Phase 24 — Visual Line Geometry & Scrollbar Indicators (`tui/app/geometry.rs`)](#24-phase-24--visual-line-geometry--scrollbar-indicators-cratesdiffuse-tuisrcappgeometryrs--775-lines)
25. [Phase 25 — Cluster DP Pairing & Surplus Survival (`core/cluster/pairing.rs`)](#25-phase-25--cluster-dp-pairing--surplus-survival-cratesdiffuse-coresrcclusterpairingrs--771-lines)
26. [Phase 26 — Spatial Neighborhood & Relocation Matching (`algo/bipartite_matching.rs`)](#26-phase-26--spatial-neighborhood--relocation-matching-cratesdiffuse-algosrcbipartite_matchingrs--760-lines)

### Tier 4 — Threshold Modules (500–750 lines)
27. [Phase 27 — Syntactic Profile Registry & Extension Mapping (`syntax/registry.rs`)](#27-phase-27--syntactic-profile-registry--extension-mapping-cratesdiffuse-syntaxsrcregistryrs--744-lines)
28. [Phase 28 — Context Row Synthesis & Hunk Folding State (`tui/app/context.rs`)](#28-phase-28--context-row-synthesis--hunk-folding-state-cratesdiffuse-tuisrcappcontextrs--731-lines)
29. [Phase 29 — ANSI Escape Sequence & Color Styling Models (`render/ansi.rs`)](#29-phase-29--ansi-escape-sequence--color-styling-models-cratesdiffuse-rendersrcansirs--702-lines)
30. [Phase 30 — Git Domain Models, Repositories & Review Targets (`git/types.rs`)](#30-phase-30--git-domain-models-repositories--review-targets-cratesdiffuse-gitsrctypesrs--693-lines)
31. [Phase 31 — File Header Banners & Diff Hunk Rendering (`render/diff.rs`)](#31-phase-31--file-header-banners--diff-hunk-rendering-cratesdiffuse-rendersrcdiffrs--686-lines)
32. [Phase 32 — Markdown Table Formatting & Side-by-Side Diffing (`render/markdown.rs`)](#32-phase-32--markdown-table-formatting--side-by-side-diffing-cratesdiffuse-rendersrcmarkdownrs--667-lines)
33. [Phase 33 — Theme Color Palettes & Contrast Specifications (`render/theme/palette.rs`)](#33-phase-33--theme-color-palettes--contrast-specifications-cratesdiffuse-rendersrcthemepaletters--663-lines)
34. [Phase 34 — TUI Top Header & Revision Status Bar (`tui/view/header.rs`)](#34-phase-34--tui-top-header--revision-status-bar-cratesdiffuse-tuisrcviewheaderrs--597-lines)
35. [Phase 35 — TUI Diff Grid Cell Layout & Column Math (`tui/view/diff_grid.rs`)](#35-phase-35--tui-diff-grid-cell-layout--column-math-cratesdiffuse-tuisrcviewdiff_gridrs--580-lines)
36. [Phase 36 — Native Git Tree & Working Copy Diffing (`git/diff.rs`)](#36-phase-36--native-git-tree--working-copy-diffing-cratesdiffuse-gitsrcdiffrs--571-lines)
37. [Phase 37 — Working Tree Live Reload & Note Synchronization (`tui/app/sync.rs`)](#37-phase-37--working-tree-live-reload--note-synchronization-cratesdiffuse-tuisrcappsyncrs--569-lines)
38. [Phase 38 — Native Git Engine & Content Loader (`core/git.rs`)](#38-phase-38--native-git-engine--content-loader-cratesdiffuse-coresrcgitrs--565-lines)
39. [Phase 39 — Affine-Gap Cost Matrix & Dynamic Programming Traceback (`algo/affine_gap.rs`)](#39-phase-39--affine-gap-cost-matrix--dynamic-programming-traceback-cratesdiffuse-algosrcaffine_gaprs--562-lines)
40. [Phase 40 — TUI Application State Lifecycle & Construction (`tui/app.rs`)](#40-phase-40--tui-application-state-lifecycle--construction-cratesdiffuse-tuisrcapprs--533-lines)
41. [Phase 41 — Visual Row & Semantic Hunk Navigation (`tui/app/navigation.rs`)](#41-phase-41--visual-row--semantic-hunk-navigation-cratesdiffuse-tuisrcappnavigationrs--529-lines)
42. [Phase 42 — Keyboard Event Dispatch & TuiAction Mapping (`tui/app/dispatch.rs`)](#42-phase-42--keyboard-event-dispatch--tuiaction-mapping-cratesdiffuse-tuisrcappdispatchrs--527-lines)
43. [Phase 43 — Working Tree Hunk Staging & Mutation Engine (`git/mutation.rs`)](#43-phase-43--working-tree-hunk-staging--mutation-engine-cratesdiffuse-gitsrcmutationrs--514-lines)

---

## 1. Phase 1 — Dynamic Programming Solver Engine (`crates/diffuse-core/src/cluster/dp.rs` · 3,823 lines)

### Dependency Surface
- **Inbound Calls**: `cluster::pairing` (`dp_pairing`), `cluster::mod` (`solve_cluster_with_tls`, `DpSolution`, `DpTokens`, `Weights`, `DpOp`, `set_cluster_budget`).
- **Outbound Dependencies**: `diffuse-algo::block_dp`, `diffuse-algo::interner::TokenInterner`, `diffuse-syntax::profile::LangProfile`, `diffuse_parse::{Hunk, Row}`, `smallvec::SmallVec`.
- **Public API Surface**: `Weights`, `DpOp`, `DpSolution`, `DpTokens`, `BlockMatch`, `RowValidBlocks`, `DpScratchArena`, `solve_cluster`, `solve_cluster_with`, `solve_cluster_with_tls`, `verify_projection`, global budget and cap mutators.

### Separation of Concerns & Local Type Definitions
Decompose `cluster/dp.rs` into a dedicated submodule directory `crates/diffuse-core/src/cluster/dp/`:
1. `config.rs` (~120 lines):
   - **Local Public Types**: `Weights` (cost weights for insertions, deletions, substitutions, and anchor rewards).
   - **Functions**: Global atomic tuning functions (`set_cluster_budget`, `cluster_budget`, `set_cluster_bandwidth_slack`, `cluster_bandwidth_slack`, `set_cluster_product_cap`, `cluster_product_cap`).
2. `tokens.rs` (~220 lines):
   - **Local Public Types**: `DpTokens` (token multiset representation and character span lengths).
   - **Functions**: `dp_tokens`, `merge_multisets`, and delimiter trimming helpers (`trim_trailing_delims`, `calc_common_indent`).
3. `bounds.rs` (~260 lines):
   - **Functions**: Salient pruning filters and fast intersection bounds (`mask_overlap`, `token_intersection_bound`, `graded_salient_bound`, `is_prefix_at_boundary`).
4. `block.rs` (~480 lines):
   - **Local Public Types**: `BlockMatch`, `RowValidBlocks`.
   - **Functions**: `block_lcs`, `precompute_valid_blocks`, `block_lcs_fast_fixed_walk`, `block_lcs_fast_fixed`, `block_lcs_fast_precomputed`, `block_anchor`.
5. `arena.rs` (~150 lines):
   - **Local Public Types**: `DpScratchArena`.
   - **Internal Types**: `Cell`, `OpKind`.
   - **Functions**: `with_tls_arena`, `solve_cluster_with_tls`.
6. `solver.rs` (~950 lines):
   - **Local Public Types**: `DpOp` (enum of dynamic programming operations), `DpSolution` (optimal path and score).
   - **Functions**: `solve_cluster`, `solve_cluster_with`, `AnchorTokenSlice`, `extract_anchor_tokens`, `score_lcs_candidates`.
7. `verify.rs` (~110 lines):
   - **Functions**: Formal verification and projection integrity checks (`verify_projection`).
8. `mod.rs` (~80 lines): Top-level facade re-exporting all locally defined public types and solver entry points.

---

## 2. Phase 2 — Hunk Modification Assembly & Reflow Engine (`crates/diffuse-core/src/collapse.rs` · 2,682 lines)

### Dependency Surface
- **Inbound Calls**: `pipeline::process_single_diff_file`, unit tests.
- **Outbound Dependencies**: `diffuse_parse::{AddLine, CtxLine, DelLine, Hunk, Row, RowKind}`, `diffuse_syntax::LangProfile`, `diffuse_algo::tape`.
- **Public API Surface**: `collapse_hunk_rows`, `PairingResults`, `AssembledMods`, `PairingContext`, `ReflowFilter`, `pass_assemble_mods`, `pass_filter_reflows`, `pass_sort_ordered_walk`, DBC invariant functions (`check_reflow_cluster_confinement`, `check_no_orphaned_continuation_dels`, etc.).

### Separation of Concerns & Local Type Definitions
Decompose `collapse.rs` into `crates/diffuse-core/src/collapse/`:
1. `mod.rs` (~140 lines):
   - **Local Public Types**: `PairingResults` (paired rows and unmatched remainder), `PairingContext<'a>` (borrowed hunk context and profiles).
   - **Functions**: Top-down entry point `collapse_hunk_rows` orchestrating the multi-pass pipeline.
2. `assemble.rs` (~850 lines):
   - **Local Public Types**: `AssembledMods` (resulting assembled rows and modification maps).
   - **Functions**: Core pairing and modification assembly pass (`pass_assemble_mods`).
3. `aux.rs` (~380 lines):
   - **Internal Types**: `SlicedMultilineRuns`, `PendingAux`.
   - **Functions**: Multiline run slicing and auxiliary row synthesis (`slice_multiline_runs`, `build_aux_runs`, `join_collapse_lines`, `normalize_ranges`, `trailing_delimiters`, `supplied_occurrence`, `consume_supply`).
4. `reflow.rs` (~220 lines):
   - **Local Public Types**: `ReflowFilter` (reflow classification parameters).
   - **Functions**: Word boundary affinity and reflow pass (`pass_filter_reflows`, `is_word_char`, `has_opening_affinity`, `has_closing_affinity`).
5. `walk.rs` (~180 lines):
   - **Functions**: Topological row sorting and ordered walk (`pass_sort_ordered_walk`, `is_contiguous_in_del`).
6. `invariants.rs` (~250 lines):
   - **Functions**: Design-by-Contract postcondition checks (`check_reflow_cluster_confinement`, `check_no_orphaned_continuation_dels`, `check_context_old_line_monotonicity`, `check_del_add_topological_order`, `assert_collapsed_reflow_invariants`).

---

## 3. Phase 3 — CLI Entry Point, Args & Command Dispatch (`crates/diffuse/src/main.rs` · 1,916 lines)

### Dependency Surface
- **Inbound Calls**: Cargo binary entry point `diffuse`.
- **Outbound Dependencies**: `clap`, `diffuse::{core, render, syntax, tui}`, `diffuse_git`.
- **Public API Surface**: `Args`, `RevisionArgs`, `SubCommand`, `NotesArgs`, `NotesAction`, `resolve_theme`, `init_color_mode`.

### Separation of Concerns & Local Type Definitions
Decompose `crates/diffuse/src/main.rs` into dedicated modules under `crates/diffuse/src/cli/`:
1. `args.rs` (~650 lines):
   - **Local Public Types**: Clap declarative command-line structures (`Args`, `RevisionArgs`, `SubCommand`, `NotesArgs`, `NotesAction`).
2. `theme.rs` (~120 lines):
   - **Functions**: ANSI color detection, terminal mode initialization, and theme resolution (`resolve_theme`, `init_color_mode`).
3. `input.rs` (~280 lines):
   - **Local Types**: `Input`, `InputPayload`.
   - **Functions**: Stdin detection, git revision resolution, and payload loading (`target_for_input`, `process_input`).
4. `dispatch.rs` (~350 lines):
   - **Functions**: Execution dispatch for review, parse, and diff modes (`route_mode`, `run_review_mode`, `run_parse_mode`).
5. `main.rs` (~90 lines): Minimal top-down binary entry point handling logging setup, working directory change, thread pool initialization, and calling `cli::dispatch`.

---

## 4. Phase 4 — Line Tokenization, LCS Alignment & Word Spans (`crates/diffuse-core/src/lcs.rs` · 1,697 lines)

### Dependency Surface
- **Inbound Calls**: `pipeline`, `collapse`, `tape`, `cluster::dp`.
- **Outbound Dependencies**: `diffuse_algo::hirschberg`, `diffuse_syntax::LangProfile`, `rustc_hash::{FxHashMap, FxHashSet}`.
- **Public API Surface**: `char_slice`, `char_len`, `LineTokenizer`, `tokenize_line`, `token_is_ws`, `token_is_alnum`, `token_is_salient`, `raw_lcs_pairs`, `Weights`, `LcsScratchArena`, `lcs_pairs`, `word_spans`, `word_spans_with_profile`.

### Separation of Concerns & Local Type Definitions
Decompose `lcs.rs` into `crates/diffuse-core/src/lcs/`:
1. `tokenizer.rs` (~380 lines):
   - **Local Public Types**: `LineTokenizer<'a>`.
   - **Functions**: `char_slice`, `char_len`, `tokenize_line`, `token_is_ws`, `token_is_alnum`, `token_is_salient`, `line_has_salient_token`, `unique_token_anchors`, `compound_identifier_root_and_stem`.
2. `arena.rs` (~150 lines):
   - **Local Public Types**: `Weights`, `LcsScratchArena`.
   - **Functions**: Global product cap tuning (`set_token_product_cap`, `token_product_cap`).
3. `raw.rs` (~320 lines):
   - **Functions**: Raw DP token alignment algorithms (`raw_lcs_pairs`, `raw_lcs_pairs_with_lookahead`, `raw_lcs_pairs_with_arena`, `raw_lcs_pairs_with_arena_and_weights`, `raw_lcs_pairs_with_context`, `is_container_delim`, `token_match_reward`).
4. `pairs.rs` (~300 lines):
   - **Functions**: High-level pair extractors and recursion (`lcs_pairs`, `lcs_pairs_with_profile`, `lcs_pairs_recursive`, `lcs_pairs_anchored`, `lcs_pairs_ids`, `lcs_matched_chars_ids`).
5. `spans.rs` (~180 lines):
   - **Functions**: Word spans and character range synthesis (`spans_from`, `word_spans`, `word_spans_with_profile`, `word_spans_and_runs_with_profile`).
6. `mod.rs` (~100 lines): Public API re-exports and module facade.

---

## 5. Phase 5 — Diff Pipeline Orchestration & Run Classification (`crates/diffuse-core/src/pipeline.rs` · 1,671 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse::main`, `diffuse::notes_cli`, `diffuse_tui`.
- **Outbound Dependencies**: `diffuse_core::parse`, `diffuse_core::wire`, `diffuse_core::notes`, `diffuse_syntax`.
- **Public API Surface**: `PipelineOptions`, `PipelineInput`, `run_pipeline`, `run_pipeline_full`, `process_single_diff_file`, `colored_runs`, `context_line_runs`, `synthesize_note_context_hunks`, `markdown_file_rows`.

### Separation of Concerns & Local Type Definitions
Decompose `pipeline.rs` into `crates/diffuse-core/src/pipeline/`:
1. `mod.rs` (~280 lines):
   - **Local Public Types**: `PipelineOptions`, `PipelineInput`.
   - **Functions**: Pipeline entry points (`run_pipeline`, `run_pipeline_diff`, `run_pipeline_with_notes`, `run_pipeline_arc`, `run_pipeline_full`).
2. `file.rs` (~420 lines):
   - **Functions**: Single-file transformation pipeline (`process_single_diff_file`, `render_file_from_source`, `is_initially_collapsed`).
3. `classify.rs` (~350 lines):
   - **Functions**: Token syntax classification and SGR run mapping (`colored_runs`, `colored_runs_with_next`, `token_cow`, `push_output_run`, `classify_ctx_line`, `for_each_sliced_token`, `context_line_runs`).
4. `context.rs` (~320 lines):
   - **Functions**: Review note context synthesis and plain Markdown view generation (`synthesize_note_context_hunks`, `synthesize_plain_common_context_hunk`, `render_plain_common_context`, `markdown_file_rows`, `note_runs`, `build_hunk_output_row`).

---

## 6. Phase 6 — Public Wire Contract & Serialization Models (`crates/diffuse-core/src/wire.rs` · 1,664 lines)

### Dependency Surface
- **Inbound Calls**: Consumed by CLI JSON/JSONL emitters, interactive TUI, render crate, and the TypeScript extension.
- **Outbound Dependencies**: `diffuse_parse::Row`, `diffuse_syntax::color::TokenColor`, `serde::{Serialize, Deserialize}`.
- **Public API Surface**: `MovSide`, `PipelineOutput`, `ByteSize`, `OutputFile`, `RawFileSource`, `format_rename_path*`, `OutputHunk`, `RowFlags`, `OutputRow`, `OutputRun`, `OutputNote`, `OutputFence`, span derivation utilities.

### Separation of Concerns & Local Type Definitions
Decompose `crates/diffuse-core/src/wire.rs` into existing directory `crates/diffuse-core/src/wire/`:
1. `pipeline.rs` (~180 lines):
   - **Local Public Types**: `PipelineOutput`, `ByteSize`.
   - **Functions**: `format_byte_size`.
2. `file.rs` (~350 lines):
   - **Local Public Types**: `OutputFile`, `RawFileSource`.
   - **Functions**: Rename path formatters (`format_rename_path`, `format_rename_path_styled`, `format_rename_path_markdown`).
3. `hunk.rs` (~220 lines):
   - **Local Public Types**: `OutputHunk`, `MovSide`.
   - **Methods**: Hunk-level metadata methods and span computations.
4. `row.rs` (~400 lines):
   - **Local Public Types**: `OutputRow`, `RowFlags`.
   - **Trait Impls**: Custom `Serialize` implementation, bidirectional `From` conversions to `diffuse_parse::Row`, `reverse()` and `reconstruct()` methods.
5. `run.rs` (~240 lines):
   - **Local Public Types**: `OutputRun`.
   - **Trait Impls & Functions**: Custom `Serialize` implementation, `derive_spans_from_runs`, `runs_new_text`, and `runs_old_text`.
6. `note.rs` (~120 lines):
   - **Local Public Types**: `OutputNote`, `OutputFence`.
   - **Trait Impls**: `From<&OutputNote> for ReviewNote`.
7. `jsonl.rs`: Existing JSONL protocol streaming module.
8. `mod.rs` (~120 lines): Re-exports ensuring exact wire contract compatibility.

---

## 7. Phase 7 — Bit-Parallel LCS Vector Search (`crates/diffuse-core/src/lcs/bit_parallel.rs` · 1,535 lines)

### Dependency Surface
- **Inbound Calls**: `lcs/pairs.rs` (`lcs_matched_chars_ids`).
- **Outbound Dependencies**: Core bitwise primitives.
- **Public API Surface**: `bit_parallel_lcs_len_64`, `bit_parallel_lcs_len_128`, `bit_parallel_matched_chars`, `SuffixPattern`, `BitVector`.

### Separation of Concerns & Local Type Definitions
Decompose `lcs/bit_parallel.rs` into `crates/diffuse-core/src/lcs/bit_parallel/`:
1. `mod.rs` (~180 lines):
   - **Local Public Types**: `SuffixPattern<'a>`.
   - **Functions**: Dynamic dispatch entry points (`bit_parallel_matched_chars`, `bit_parallel_suffix_matched_chars`).
2. `vec64.rs` (~280 lines):
   - **Local Public Types**: `SuffixPattern64<'a>`.
   - **Functions**: `bit_parallel_lcs_len_64`, `bit_parallel_matched_chars_64`, `bit_parallel_suffix_walk_64`.
3. `vec128.rs` (~260 lines):
   - **Functions**: `bit_parallel_lcs_len_128`, `bit_parallel_matched_chars_128`, `bit_parallel_suffix_walk_128`.
4. `fixed.rs` (~280 lines):
   - **Local Public Types**: `BitVector<const K: usize>`, `SuffixPatternFixed<'a, const K: usize>`.
   - **Functions**: `bit_parallel_suffix_walk_fixed_inner`.
5. `dyn_width.rs` (~240 lines):
   - **Functions**: Dynamic width arbitrary fallback (`bit_parallel_suffix_matched_chars_dyn`).

---

## 8. Phase 8 — Token Classification & Markdown Token Streaming (`crates/diffuse-syntax/src/classifier.rs` · 1,443 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse-core::pipeline`, `diffuse-render::markdown`.
- **Outbound Dependencies**: `diffuse-syntax::profile`, `diffuse-syntax::color`, `pulldown_cmark`.
- **Public API Surface**: `ClassifiedToken`, `for_each_classified_token`, `classify_tokens`, `for_each_markdown_token_with_header`, `for_each_line_classified_token`, `classify_line`.

### Separation of Concerns & Local Type Definitions
Decompose `classifier.rs` into `crates/diffuse-syntax/src/classifier/`:
1. `token.rs` (~220 lines):
   - **Local Public Types**: `ClassifiedToken<'a>`.
   - **Functions**: Generic token iteration (`for_each_classified_token`, `classify_tokens`), and backtick scanning in comments (`for_each_comment_with_backticks`).
2. `markdown.rs` (~750 lines):
   - **Internal Types**: `StyleStack`, `StyleLayer`, `LayerTag`.
   - **Functions**: Nondestructive Markdown token scanner (`for_each_markdown_token_with_header`, `emit_styled_token`, `emit_table_line_slice`, `for_each_html_token`, `markdown_tokens`).
3. `line.rs` (~260 lines):
   - **Functions**: Line-level classified token driver (`for_each_line_classified_token`, `classify_line`, `markdown_document_tokens`, `is_line_alignment_row`).
4. `mod.rs` (~80 lines): Facade re-exports.

---

## 9. Phase 9 — Markdown Document AST & Event Reducer (`crates/diffuse-syntax/src/markdown.rs` · 1,433 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse-render::markdown::terminal`.
- **Outbound Dependencies**: `pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd}`.
- **Public API Surface**: `Flow`, `Block`, `Span`, `ListItem`, `Inline`, `TaskState`, `ListMarker`, `ColumnAlign`, `blocks`, `spans`.

### Separation of Concerns & Local Type Definitions
Decompose `crates/diffuse-syntax/src/markdown.rs` into `crates/diffuse-syntax/src/markdown/`:
1. `inline.rs` (~420 lines):
   - **Local Public Types**: `Inline<'a>`, `Span<'a>`.
   - **Internal Types**: `InlineBuilder`.
   - **Functions**: `spans`, `coalesce_text_events`, `split_paragraph_task_lists`.
2. `block.rs` (~750 lines):
   - **Local Public Types**: `Flow<'a>`, `Block<'a>`, `ListItem<'a>`, `TaskState`, `ListMarker<'a>`, `ColumnAlign`.
   - **Internal Types**: `Frame<'a>`, `ListFrame<'a>`, `ItemFrame<'a>`, `ParaFrame<'a>`, `TableFrame<'a>`.
   - **Functions**: `blocks`, frame stack event reduction, container marker stripping.
3. `mod.rs` (~90 lines): Parser options configuration (`md_options`), re-exports of `blocks`, `spans`, and AST types.

---

## 10. Phase 10 — Review Notes Data Store & Anchor Matching (`crates/diffuse-core/src/notes.rs` · 1,375 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse::notes_cli`, `diffuse_tui::app::notes`, `pipeline`.
- **Outbound Dependencies**: `serde`, `std::fs`, `diffuse_git::Repo`.
- **Public API Surface**: `ReviewNote`, `DiffCoord`, `NoteLocation`, `NoteStatus`, `LineHash`, `NotesDB`, `load_notes`, `save_notes`, `add_note`, `note_anchors_in_rows`.

### Separation of Concerns & Local Type Definitions
Decompose `notes.rs` into `crates/diffuse-core/src/notes/`:
1. `note.rs` (~260 lines):
   - **Local Public Types**: `ReviewNote`, `NoteLocation`, `NoteStatus`.
   - **Methods**: Status transitions, location query methods (`note_line_span`).
2. `anchor.rs` (~340 lines):
   - **Local Public Types**: `DiffCoord`, `LineHash`, `StableHasher`.
   - **Functions**: Context hunk anchoring and row coordinate matching (`coord_matches`, `anchors_by_coord`, `note_anchors_in_rows`, `note_anchors_in_context_lines`).
3. `store.rs` (~420 lines):
   - **Local Public Types**: `NotesDB` (type alias).
   - **Functions**: Database persistence, JSON serialization, and queries (`load_notes`, `save_notes`, `add_note`, `remove_notes_in_range`, `remove_file_notes`, `notes_for_file`, `all_notes`, `without_resolved`).
4. `path.rs` (~140 lines):
   - **Functions**: Path helpers and identifier generation (`default_state_dir`, `default_notes_path`, `generate_note_id`, `now_millis`, `head_commit`, `notes_commit`).
5. `mod.rs` (~90 lines): Facade re-exports.

---

## 11. Phase 11 — TUI Viewport Diff Lines & Dual Frame Renderer (`crates/diffuse-tui/src/app/render.rs` · 1,344 lines)

### Dependency Surface
- **Inbound Calls**: `tui::app`, `tui::event`.
- **Outbound Dependencies**: `ratatui`, `diffuse-render`.
- **Public API Surface**: `AppState::render_frame`, `AppState::render_ratatui_frame`, `AppState::render_diff_viewport_lines`, `AppState::sticky_header_info`, `AppState::viewport_row_to_visual`.

### Separation of Concerns & Local Type Definitions
Decompose `app/render.rs` into `crates/diffuse-tui/src/app/render/`:
1. `ansi.rs` (~360 lines):
   - **Functions**: Headless ANSI string frame generator (`render_frame`, `blank_status_ansi`, raw diff toggling).
2. `ratatui.rs` (~450 lines):
   - **Functions**: Terminal UI widget tree rendering (`render_ratatui_frame`, `blank_status_widget`).
3. `viewport.rs` (~420 lines):
   - **Local Public Types**: `StickyHeaderInfo` (sticky header text and column offset).
   - **Functions**: Diff viewport visual line generation and sticky header calculation (`render_diff_viewport_lines`, `sticky_header_info`, `viewport_row_to_visual`).
4. `mod.rs` (~60 lines): AppState method implementations delegating to submodules.

---

## 12. Phase 12 — Terminal Markdown Flow, Table & Inline Renderer (`crates/diffuse-render/src/markdown/terminal.rs` · 1,198 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse-render::markdown`, `diffuse-tui::editor`.
- **Outbound Dependencies**: `diffuse-syntax::markdown`, `yansi`.
- **Public API Surface**: `MarkdownLine`, `MarkdownRun`, `markdown_lines`, `write_markdown_line`, `render_markdown_line`.

### Separation of Concerns & Local Type Definitions
Decompose `markdown/terminal.rs` into `crates/diffuse-render/src/markdown/terminal/`:
1. `mod.rs` (~150 lines):
   - **Local Public Types**: `MarkdownLine`, `MarkdownRun`.
   - **Functions**: Public entry points (`markdown_lines`, `write_markdown_line`, `render_markdown_line`).
2. `inline.rs` (~160 lines):
   - **Internal Types**: `Run`, `InlineStyle`.
   - **Functions**: Inline run styling and task checkbox glyphs (`inline_runs`, `inline_runs_into`, `task_glyph`).
3. `table.rs` (~240 lines):
   - **Functions**: Terminal table layout budget math, cell wrapping, and table borders (`squash_table_widths`, `wrap_table_cell`, `table_rule`).
4. `flow.rs` (~420 lines):
   - **Internal Types**: `Renderer`.
   - **Functions**: Block and flow renderer (headings, lists, blockquotes, code fences).
5. `wrap.rs` (~140 lines):
   - **Internal Types**: `VisualLine`.
   - **Functions**: Visual wrapping, continuation indent, and run merging (`wrap_content`, `push_merged`, `flatten_line`).

---

## 13. Phase 13 — Unified Diff Parser & Core AST Models (`crates/diffuse-parse/src/lib.rs` · 1,178 lines)

### Dependency Surface
- **Inbound Calls**: Consumed universally by `diffuse-core`, `diffuse-render`, `diffuse-tui`, and `diffuse`.
- **Outbound Dependencies**: Standard library only.
- **Public API Surface**: `Row`, `RowKind`, `RunKind`, `RowGroup`, `DelLine`, `AddLine`, `CtxLine`, `Spans`, `Hunk`, `DiffFile`, `FileStatus`, `parse_diff`, `parse_diff_for_each`.

### Separation of Concerns & Local Type Definitions
Decompose `crates/diffuse-parse/src/lib.rs` into dedicated submodules under `crates/diffuse-parse/src/`:
1. `row.rs` (~360 lines):
   - **Local Public Types**: `Row`, `RowKind`, `RunKind`, `RowGroup`, `DelLine`, `AddLine`, `CtxLine`, and `Spans`.
   - **Methods & Inherent Impls**: Row constructors (`DelLine::new`, `AddLine::new`, `CtxLine::new`), coordinate queries, and predicate methods.
2. `hunk.rs` (~180 lines):
   - **Local Public Types**: `Hunk`.
   - **Functions**: `add_count`, `del_count`, and hunk header parsing (`parse_hunk_header`).
3. `file.rs` (~240 lines):
   - **Local Public Types**: `DiffFile`, `FileStatus`.
   - **Functions**: Path unquoting and git path header extraction (`unquote_git_path`, `hunk_path`, `parse_diff_git_paths`, `is_valid_diff_file`).
4. `diff.rs` (~280 lines):
   - **Functions**: Streaming and batch diff parser (`parse_diff`, `parse_diff_for_each`, `zero_hunk_section`).
5. `lib.rs` (~90 lines): Top-level re-exports maintaining complete backwards compatibility.

---

## 14. Phase 14 — Invertible Alignment Tape & Run Reconstruction (`crates/diffuse-core/src/tape.rs` · 1,086 lines)

### Dependency Surface
- **Inbound Calls**: `pipeline`, `collapse`.
- **Outbound Dependencies**: `diffuse-algo::tape`, `diffuse-syntax::profile`.
- **Public API Surface**: `TapeChunk`, `AlignmentTape`, projections (`project_old`, `project_new`), algebraic tape conversion, `to_merged_runs`, `to_spans`, `from_texts`.

### Separation of Concerns & Local Type Definitions
Decompose `tape.rs` into `crates/diffuse-core/src/tape/`:
1. `chunk.rs` (~80 lines):
   - **Local Public Types**: `TapeChunk<'a>` enum.
   - **Methods**: Chunk character length, text accessor, and classification predicates.
2. `core.rs` (~180 lines):
   - **Local Public Types**: `AlignmentTape<'a>` struct.
   - **Methods**: Projection iterators (`project_old`, `project_new`), and algebraic tape conversions (`to_algebraic_tape`, `from_algebraic_tape`).
3. `spans.rs` (~420 lines):
   - **Functions**: Merged run synthesis, span construction, delimiter affinity rules, and whitespace adjustment (`to_merged_runs`, `to_spans`).
4. `builder.rs` (~180 lines):
   - **Functions**: Fast construction from pre- and post-image texts (`from_texts`, `from_texts_with_cap`).
5. `mod.rs` (~80 lines): Re-exports.

---

## 15. Phase 15 — Syntactic Language Profiles & Delimiter Tables (`crates/diffuse-syntax/src/profile.rs` · 1,032 lines)

### Dependency Surface
- **Inbound Calls**: Widely consumed across `diffuse-syntax`, `diffuse-core`, and `diffuse-algo`.
- **Outbound Dependencies**: Standard library only.
- **Public API Surface**: `BlockDelimStyle`, `CloseDelimiter`, `PropertySyntax`, `UnicodeEscapeStyle`, `CharLiteralProfile`, `SingleQuoteKind`, `LangProfile`, delimiter character predicates.

### Separation of Concerns & Local Type Definitions
Decompose `profile.rs` into `crates/diffuse-syntax/src/profile/`:
1. `lang.rs` (~450 lines):
   - **Local Public Types**: `LangProfile`.
   - **Methods**: Profile builders and property accessors.
2. `block.rs` (~180 lines):
   - **Local Public Types**: `BlockDelimStyle`, `CloseDelimiter`, `PropertySyntax`.
   - **Functions**: Statement boundary and continuation checks (`is_statement_terminator_char`, `default_is_continuation_phrase`).
3. `literal.rs` (~160 lines):
   - **Local Public Types**: `CharLiteralProfile`, `SingleQuoteKind`, `UnicodeEscapeStyle`.
   - **Methods**: Character literal escape matching and validation.
4. `delimiters.rs` (~160 lines):
   - **Functions**: Delimiter character and phrase testing functions (`is_opening_delimiter_char`, `is_closing_delimiter_char`, `is_delimiter_char`, `is_delimiter_run`).
5. `mod.rs` (~70 lines): Re-exports.

---

## 16. Phase 16 — Dyck Language Pushdown Automata & Containers (`crates/diffuse-algo/src/dyck.rs` · 1,017 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse-core::token`, `diffuse-core::cluster::dp`.
- **Outbound Dependencies**: Standard library only.
- **Public API Surface**: `SmallStack`, `DelimiterCategory`, `DelimiterClassifier`, `UniversalDelimiterClassifier`, `compute_delimiter_partners`, `trim_balanced_boundaries`, `track_enclosing_containers`, `track_nesting_depths`, `DyckBalanced`, `DiffuseDyckExt`.

### Separation of Concerns & Local Type Definitions
Decompose `dyck.rs` into `crates/diffuse-algo/src/dyck/`:
1. `stack.rs` (~100 lines):
   - **Local Public Types**: `SmallStack<T: Copy + Default, const N: usize>`.
2. `classifier.rs` (~110 lines):
   - **Local Public Types**: `DelimiterCategory`, `DelimiterClassifier<T>` trait, and `UniversalDelimiterClassifier`.
3. `partners.rs` (~180 lines):
   - **Functions**: Delimiter pairing and boundary trimming algorithms (`compute_delimiter_partners_into`, `compute_delimiter_partners`, `trim_balanced_boundaries`).
4. `containers.rs` (~260 lines):
   - **Functions**: Surrounding and enclosing container tracking (`track_enclosing_containers`, `track_surrounding_containers`, `track_nesting_depths`).
5. `iter.rs` (~160 lines):
   - **Local Public Types**: `DyckBalanced<I, C, T>`, `DiffuseDyckExt` iterator extension trait.
6. `mod.rs` (~80 lines): Re-exports.

---

## 17. Phase 17 — Review Notes CLI Action Handlers & Formatters (`crates/diffuse/src/notes_cli.rs` · 1,015 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse::main`.
- **Outbound Dependencies**: `diffuse_core::notes`, `diffuse_render::markdown`.
- **Public API Surface**: `run_notes_command`, `format_rows_with_context`, `format_note_text`, `parse_line_span`.

### Separation of Concerns & Local Type Definitions
Decompose `notes_cli.rs` into `crates/diffuse/src/notes_cli/`:
1. `format.rs` (~240 lines):
   - **Functions**: Context row formatting, terminal width wrapping, and note text rendering (`format_rows_with_context`, `print_rows_with_context`, `format_note_text`, `print_note_text`, `plain_row_texts`, `provisional_ctx_row`).
2. `span.rs` (~90 lines):
   - **Functions**: Line span parsing and bounds validation (`parse_line_span`, `check_span_bounds`, `missing_file_exit`).
3. `actions.rs` (~480 lines):
   - **Functions**: Action handlers for `List`, `Show`, `Add`, `Resolve`, `Unresolve`, `Remove`, and `Clear`.
4. `mod.rs` (~90 lines): `run_notes_command` entry point and repository target resolution.

---

## 18. Phase 18 — Token Semantics, Entropy & Container Affinities (`crates/diffuse-core/src/token.rs` · 996 lines)

### Dependency Surface
- **Inbound Calls**: `pipeline`, `cluster`.
- **Outbound Dependencies**: `diffuse-syntax::profile`, `diffuse-algo::dyck`.
- **Public API Surface**: `TokenClass`, `WhitespaceKind`, `classify_token`, `classify_whitespace`, `token_self_information`, `entropy_anchors`, `ContainerKind`, `compute_token_containers`, `multi_token_phrase_anchors`.

### Separation of Concerns & Local Type Definitions
Decompose `token.rs` into `crates/diffuse-core/src/token/`:
1. `class.rs` (~260 lines):
   - **Local Public Types**: `TokenClass`.
   - **Functions**: Literals, keywords, operator predicates, and `classify_token`.
2. `whitespace.rs` (~90 lines):
   - **Local Public Types**: `WhitespaceKind`.
   - **Functions**: `classify_whitespace`, `classify_token_whitespace`.
3. `entropy.rs` (~140 lines):
   - **Functions**: `token_self_information`, `entropy_anchors`.
4. `container.rs` (~350 lines):
   - **Local Public Types**: `ContainerKind`.
   - **Functions**: `compute_token_containers`, `multi_token_phrase_anchors`, and `compute_parenthesis_depth`.
5. `mod.rs` (~70 lines): Re-exports.

---

## 19. Phase 19 — Interactive TUI Modals & Dialog Views (`crates/diffuse-tui/src/view/modals.rs` · 994 lines)

### Dependency Surface
- **Inbound Calls**: `tui::app::render`, `tui::event`.
- **Outbound Dependencies**: `ratatui`, `diffuse-render`.
- **Public API Surface**: `centered_rect`, `wrap_words`, `render_help_modal`, `render_help_modal_widget`, `render_finder_modal`, `render_finder_modal_widget`, `render_note_modal_widget`.

### Separation of Concerns & Local Type Definitions
Decompose `view/modals.rs` into existing directory `crates/diffuse-tui/src/view/modals/`:
1. `layout.rs` (~160 lines):
   - **Functions**: Area math and styling conversions (`centered_rect`, `wrap_words`, `ansi_to_ratatui_line`, `parse_sgr_to_style`).
2. `help.rs` (~180 lines):
   - **Functions**: Help dialog view (`render_help_modal`, `render_help_modal_widget`).
3. `finder.rs` (~360 lines):
   - **Functions**: File fuzzy finder modal and path highlighting (`render_finder_modal`, `render_finder_modal_widget`, `format_path_matches*`).
4. `note.rs` (~120 lines):
   - **Functions**: Review note editor modal widget (`render_note_modal_widget`).
5. `mod.rs` (~60 lines): Facade re-exports.

---

## 20. Phase 20 — TUI Review Notes Lifecycle & Disk Sync (`crates/diffuse-tui/src/app/notes.rs` · 935 lines)

### Dependency Surface
- **Inbound Calls**: `tui::event`, `tui::actions`.
- **Outbound Dependencies**: `diffuse-core::notes`.
- **Public API Surface**: `NotesChangeSummary`, `VisualTarget`, AppState methods (`start_note_draft`, `commit_note_draft`, `delete_note`, `cycle_note_status`, `reload_notes_from_disk`, `visual_target_at`).

### Separation of Concerns & Local Type Definitions
Decompose `app/notes.rs` into `crates/diffuse-tui/src/app/notes/`:
1. `target.rs` (~200 lines):
   - **Local Public Types**: `VisualTarget<'a>`.
   - **Methods on AppState**: `note_target_row`, `visual_target_at`.
2. `sync.rs` (~180 lines):
   - **Local Public Types**: `NotesChangeSummary`.
   - **Methods on AppState**: `diff_notes_db`, `reload_notes_from_disk`.
3. `draft.rs` (~180 lines):
   - **Methods on AppState**: `start_note_draft`, `commit_note_draft`.
4. `mutations.rs` (~220 lines):
   - **Methods on AppState**: `delete_note`, `cycle_note_status`, `toggle_show_resolved_notes`, `toggle_show_note_source`, `visible_notes_db`.
5. `mod.rs` (~60 lines): AppState impl delegation facade.

---

## 21. Phase 21 — Interactive Text Buffer & History Editor (`crates/diffuse-tui/src/editor.rs` · 932 lines)

### Dependency Surface
- **Inbound Calls**: `tui::app::notes`.
- **Outbound Dependencies**: `ratatui`, `crossterm`.
- **Public API Surface**: `Editor`, `EditorAction`, `markdown_highlighter`.

### Separation of Concerns & Local Type Definitions
Decompose `editor.rs` into `crates/diffuse-tui/src/editor/`:
1. `buffer.rs` (~480 lines):
   - **Local Public Types**: `Editor`, `EditorAction`.
   - **Methods**: Buffer navigation, character insertion, line deletion, and word wrapping.
2. `history.rs` (~120 lines):
   - **Internal Types**: `EditorSnapshot`.
   - **Methods**: Undo/redo stacks and snapshot restoration.
3. `render.rs` (~160 lines):
   - **Local Types**: `HighlightingCallback`.
   - **Functions**: `markdown_highlighter`, `slice_spans`.
4. `mod.rs` (~70 lines): Re-exports.

---

## 22. Phase 22 — Terminal Row Segments, Gutters & Raw Diff Formatting (`crates/diffuse-render/src/row.rs` · 859 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse-render::diff`, `diffuse-tui::app::render`.
- **Outbound Dependencies**: `yansi`, `diffuse-core::wire`.
- **Public API Surface**: `pad4`, `prefix_of`, `styled_prefix`, `display_text`, `expand_tabs`, `write_row_segment`, `render_row_segment`, `write_note_segment`, `write_raw_row`.

### Separation of Concerns & Local Type Definitions
Decompose `row.rs` into `crates/diffuse-render/src/row/`:
1. `gutter.rs` (~180 lines):
   - **Functions**: Line number zero-allocation padding (`pad4`, `pad4_bytes`), diff symbol prefixes (`prefix_of`, `styled_prefix`).
2. `text.rs` (~120 lines):
   - **Functions**: Tab expansion and visual width column slicing (`display_text`, `expand_tabs`, `expand_runs_tabs`, `slice_by_visual_width`).
3. `segment.rs` (~260 lines):
   - **Functions**: Diff row segment ANSI rendering (`write_row_segment`, `render_row_segment`, `is_common_only_row`, `is_pure_deletion_mod`).
4. `note.rs` (~90 lines):
   - **Functions**: Note block segment emitter (`write_note_segment`, `render_note_segment`).
5. `raw.rs` (~110 lines):
   - **Functions**: Raw unified diff row renderer (`write_raw_row`, `render_raw_row`).
6. `mod.rs` (~60 lines): Re-exports.

---

## 23. Phase 23 — Golden Test Harvesting & Fixture Ingestion CLI (`crates/diffuse/src/bin/harvest.rs` · 854 lines)

### Dependency Surface
- **Inbound Calls**: Developer CLI binary `cargo run --bin harvest`.
- **Outbound Dependencies**: `diffuse::core`, `git2`/`gix`.
- **Public API Surface**: Binary entry point.

### Separation of Concerns & Local Type Definitions
Decompose `harvest.rs` into submodules under `crates/diffuse/src/bin/harvest/`:
1. `args.rs` (~140 lines):
   - **Local Types**: `Args`.
   - **Functions**: Path exclusion pattern matchers.
2. `fixture.rs` (~220 lines):
   - **Local Types**: `FixtureOutput`.
   - **Functions**: Snapshot file hashing (`fnv1a`), snapshot naming, and golden directory resolution.
3. `git.rs` (~180 lines):
   - **Functions**: Git revision resolution and endpoint queries.
4. `main.rs` (~220 lines): Ingestion orchestration loop.

---

## 24. Phase 24 — Visual Line Geometry & Scrollbar Indicators (`crates/diffuse-tui/src/app/geometry.rs` · 775 lines)

### Dependency Surface
- **Inbound Calls**: `tui::app::navigation`, `tui::app::render`.
- **Outbound Dependencies**: `ratatui`.
- **Public API Surface**: AppState geometry methods (`row_seg_count`, `visual_geometry`, `cursor_visual_line`, `hunk_divider_vis_line`), `ScrollbarCell`, `ScrollbarSubcellThumb`.

### Separation of Concerns & Local Type Definitions
Decompose `app/geometry.rs` into `crates/diffuse-tui/src/app/geometry/`:
1. `scrollbar.rs` (~180 lines):
   - **Local Public Types**: `ScrollbarCell`, `ScrollbarSubcellThumb`.
   - **Methods**: Subcell rendering and thumb character selection.
2. `vis.rs` (~260 lines):
   - **Methods on AppState**: Visual row count and coordinate projections (`row_seg_count`, `row_nav_vis_count`, `visual_geometry`, `cursor_visual_line`).
3. `divider.rs` (~220 lines):
   - **Methods on AppState**: Hunk divider lines and bottom gap heights (`hunk_div_seg_count`, `bottom_gap_seg_count`, `hunk_divider_vis_line`, `bottom_divider_vis_line`, hunk gaps).
4. `mod.rs` (~60 lines): AppState impl delegation facade.

---

## 25. Phase 25 — Cluster DP Pairing & Surplus Survival (`crates/diffuse-core/src/cluster/pairing.rs` · 771 lines)

### Dependency Surface
- **Inbound Calls**: `cluster::mod`.
- **Outbound Dependencies**: `diffuse-algo::bipartite_matching`.
- **Public API Surface**: `dp_pairing`.

### Separation of Concerns & Local Type Definitions
Decompose `cluster/pairing.rs` into `crates/diffuse-core/src/cluster/pairing/`:
1. `survival.rs` (~360 lines):
   - **Functions**: Surplus survival dynamic programming and candidate memoization (`surplus_survival`, `memo_fn_of_str`).
2. `filter.rs` (~120 lines):
   - **Functions**: Fragment pair rejection heuristics (`is_fragment_pair_rejected`).
3. `mod.rs` (~180 lines): `dp_pairing` orchestrator and legacy index mapping.

---

## 26. Phase 26 — Spatial Neighborhood & Relocation Matching (`crates/diffuse-algo/src/bipartite_matching.rs` · 760 lines)

### Dependency Surface
- **Inbound Calls**: `diffuse-core::cluster::pairing`, `diffuse-core::moves`.
- **Outbound Dependencies**: Standard library only.
- **Public API Surface**: `SpatialNeighborhood`, `PositionedItem`, `HunkGridNeighborhood`, `RelocationMatch`, `RelocationMatches`, `greedy_multiset_match`, `expand_neighborhood_matches`, `BipartiteMatcher`, `DiffuseBipartiteExt`.

### Separation of Concerns & Local Type Definitions
Decompose `bipartite_matching.rs` into `crates/diffuse-algo/src/bipartite_matching/`:
1. `neighborhood.rs` (~140 lines):
   - **Local Public Types & Traits**: `SpatialNeighborhood<P>` trait, `PositionedItem<K, P>`, and `HunkGridNeighborhood<'a>`.
2. `matches.rs` (~180 lines):
   - **Local Public Types**: `RelocationMatch<P>`, `RelocationMatches<P>`.
   - **Functions**: `filter_cohesion_runs`.
3. `matcher.rs` (~320 lines):
   - **Local Public Types & Traits**: `BipartiteMatcher<K, P>`, `DiffuseBipartiteExt<K, P>` trait.
   - **Functions**: `greedy_multiset_match`, `expand_neighborhood_matches`.
4. `mod.rs` (~60 lines): Re-exports.

---

## 27. Phase 27 — Syntactic Profile Registry & Extension Mapping (`crates/diffuse-syntax/src/registry.rs` · 744 lines)

### Dependency Surface & Local Type Definitions
Decompose `registry.rs` into `crates/diffuse-syntax/src/registry/`:
- `map.rs`: Extension to language profile mapping table.
- `cache.rs`:
  - **Local Public Types**: `ProfileRegistry`.
  - **Functions**: Compiled regex caches and profile lookup.
- `mod.rs`: Facade re-exports.

---

## 28. Phase 28 — Context Row Synthesis & Hunk Folding State (`crates/diffuse-tui/src/app/context.rs` · 731 lines)

### Dependency Surface & Local Type Definitions
Decompose `app/context.rs` into `crates/diffuse-tui/src/app/context/`:
- `fold.rs`: File and hunk collapse tracking methods on `AppState`.
- `synthesis.rs`: Context row generation around active notes.
- `mod.rs`: Facade re-exports.

---

## 29. Phase 29 — ANSI Escape Sequence & Color Styling Models (`crates/diffuse-render/src/ansi.rs` · 702 lines)

### Dependency Surface & Local Type Definitions
Decompose `ansi.rs` into `crates/diffuse-render/src/ansi/`:
- `color.rs`:
  - **Local Public Types**: `AnsiColor`.
  - **Trait Impls**: Bidirectional `From` implementations between `AnsiColor` and `yansi::Color`.
- `style.rs`:
  - **Local Public Traits**: `ColorExt` trait.
  - **Constants**: SGR reset sequences (`SGR_RESET`, `FG_RESET`, `BG_RESET`).
- `mod.rs`: Facade re-exports.

---

## 30. Phase 30 — Git Domain Models, Repositories & Review Targets (`crates/diffuse-git/src/types.rs` · 693 lines)

### Dependency Surface & Local Type Definitions
Decompose `diffuse-git/src/types.rs` into `crates/diffuse-git/src/types/`:
- `error.rs`:
  - **Local Public Types**: `GitError`.
- `options.rs`:
  - **Local Public Types**: `GitDiffOptions`, `GitBlob`.
- `repo.rs`:
  - **Local Public Types**: `Repo`.
  - **Methods**: Repository discovery, root canonicalization, and repo key methods.
- `target.rs`:
  - **Local Public Types**: `ReviewTarget`, `RevisionRange`, `RefPair`.
  - **Functions**: `parse_git_rev_and_pathspecs`.
- `mod.rs`: Facade re-exports.

---

## 31. Phase 31 — File Header Banners & Diff Hunk Rendering (`crates/diffuse-render/src/diff.rs` · 686 lines)

### Dependency Surface & Local Type Definitions
Decompose `diff.rs` into `crates/diffuse-render/src/diff/`:
- `header.rs`: File banner, status badges, and rename arrow rendering.
- `hunk.rs`: Hunk header emitter and divider styling.
- `mod.rs`: Public diff rendering entry points.

---

## 32. Phase 32 — Markdown Table Formatting & Side-by-Side Diffing (`crates/diffuse-render/src/markdown.rs` · 667 lines)

### Dependency Surface & Local Type Definitions
Decompose `markdown.rs` into `crates/diffuse-render/src/markdown/`:
- `table.rs`: GitHub-flavored Markdown table diff emitter (`render_markdown_table_diff`).
- `highlight.rs`: Shared nondestructive Markdown line highlighter (`write_markdown_line_highlighted`, `render_markdown_lines`, `render_markdown_lines_with_width`).
- `mod.rs`: Re-exports.

---

## 33. Phase 33 — Theme Color Palettes & Contrast Specifications (`crates/diffuse-render/src/theme/palette.rs` · 663 lines)

### Dependency Surface & Local Type Definitions
Decompose `theme/palette.rs` into `crates/diffuse-render/src/theme/palette/`:
- `mod.rs`:
  - **Local Public Types**: `ThemePalette`.
  - **Methods**: Inherent constructors and color lookup methods.
- `dark.rs`: Default dark border constants and dark theme palette values (`DEFAULT_BORDER_COLOR_DARK`, `dark_palette`).
- `light.rs`: Default light border constants and light theme palette values (`DEFAULT_BORDER_COLOR_LIGHT`, `light_palette`).

---

## 34. Phase 34 — TUI Top Header & Revision Status Bar (`crates/diffuse-tui/src/view/header.rs` · 597 lines)

### Dependency Surface & Local Type Definitions
Decompose `view/header.rs` into `crates/diffuse-tui/src/view/header/`:
- `status.rs`: Top commit/branch/revision header bar widget.
- `path.rs`: Current file path bar widget.
- `mod.rs`: Header view entry points.

---

## 35. Phase 35 — TUI Diff Grid Cell Layout & Column Math (`crates/diffuse-tui/src/view/diff_grid.rs` · 580 lines)

### Dependency Surface & Local Type Definitions
Decompose `view/diff_grid.rs` into `crates/diffuse-tui/src/view/diff_grid/`:
- `layout.rs`: Column coordinate math and gutter spacing calculations.
- `render.rs`: Row widget rendering and scrollbar placement.
- `mod.rs`: Diff grid view entry points.

---

## 36. Phase 36 — Native Git Tree & Working Copy Diffing (`crates/diffuse-git/src/diff.rs` · 571 lines)

### Dependency Surface & Local Type Definitions
Decompose `diff.rs` into `crates/diffuse-git/src/diff/`:
- `tree.rs`: Commit-to-commit tree diffing via `gix`.
- `worktree.rs`: Working copy vs index diffing.
- `mod.rs`: Public diff generator entry points.

---

## 37. Phase 37 — Working Tree Live Reload & Note Synchronization (`crates/diffuse-tui/src/app/sync.rs` · 569 lines)

### Dependency Surface & Local Type Definitions
Decompose `app/sync.rs` into `crates/diffuse-tui/src/app/sync/`:
- `worktree.rs`: Automatic filesystem reload methods on `AppState`.
- `notes.rs`: Disk note polling and change synchronization methods.
- `mod.rs`: Facade re-exports.

---

## 38. Phase 38 — Native Git Engine & Content Loader (`crates/diffuse-core/src/git.rs` · 565 lines)

### Dependency Surface & Local Type Definitions
Decompose `git.rs` into `crates/diffuse-core/src/git/`:
- `blob.rs`: Blob extraction and snapshot loading.
- `runner.rs`: Git diff command executor.
- `mod.rs`: Facade re-exports.

---

## 39. Phase 39 — Affine-Gap Cost Matrix & Dynamic Programming Traceback (`crates/diffuse-algo/src/affine_gap.rs` · 562 lines)

### Dependency Surface & Local Type Definitions
Decompose `affine_gap.rs` into `crates/diffuse-algo/src/affine_gap/`:
- `matrix.rs`:
  - **Local Public Types**: `AffineWeights`.
  - **Functions**: Cost matrix allocation and cell forward DP computation.
- `traceback.rs`: Optimal path reconstruction and alignment tape emission.
- `mod.rs`: Public `affine_gap_align` entry point.

---

## 40. Phase 40 — TUI Application State Lifecycle & Construction (`crates/diffuse-tui/src/app.rs` · 533 lines)

### Dependency Surface & Local Type Definitions
Decompose `app.rs` into `crates/diffuse-tui/src/app/`:
- `mod.rs`:
  - **Local Public Types**: `AppState` struct with its fields, accessors, and re-exported subsystem methods.
- `init.rs`: `AppState::new(...)` constructor and initial terminal geometry setup.

---

## 41. Phase 41 — Visual Row & Semantic Hunk Navigation (`crates/diffuse-tui/src/app/navigation.rs` · 529 lines)

### Dependency Surface & Local Type Definitions
Decompose `app/navigation.rs` into `crates/diffuse-tui/src/app/navigation/`:
- `row.rs`: Visual line and row cursor movement methods on `AppState`.
- `hunk.rs`: Hunk and file jumping methods on `AppState`.
- `mod.rs`: Navigation facade.

---

## 42. Phase 42 — Keyboard Event Dispatch & TuiAction Mapping (`crates/diffuse-tui/src/app/dispatch.rs` · 527 lines)

### Dependency Surface & Local Type Definitions
Decompose `app/dispatch.rs` into `crates/diffuse-tui/src/app/dispatch/`:
- `keymap.rs`: Keyevent translation to `TuiAction`.
- `handler.rs`: Action execution methods on `AppState`.
- `mod.rs`: Facade re-exports.

---

## 43. Phase 43 — Working Tree Hunk Staging & Mutation Engine (`crates/diffuse-git/src/mutation.rs` · 514 lines)

### Dependency Surface & Local Type Definitions
Decompose `mutation.rs` into `crates/diffuse-git/src/mutation/`:
- `stage.rs`: Hunk and file staging/unstaging routines.
- `discard.rs`: Patch reversion and discard routines.
- `mod.rs`: Facade re-exports.

---

## Verification & Golden Gate Protocol

Every phase must pass the strict repository verification gate before acceptance:
1. **Compilation & Formatting**: Zero warnings under `cargo +nightly fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`.
2. **Unit Test Coverage**: 100% branch coverage on all newly structured modules with test execution times $< 0.5s$ per unit test and $< 1.5s$ per test binary.
3. **Golden Fixture Equivalence**: All 41 frozen golden corpora in [`tests/golden/`](../tests/golden/) must match byte-for-byte on both the in-process library leg and the external CLI binary leg (`./scripts/check.sh`).
