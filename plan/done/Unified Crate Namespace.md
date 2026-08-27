# Unified Crate Namespace Refactoring (`diffuse::core`, `diffuse::tui`, etc.)

- [x] Phase 1 — Top-Level Library Facade & Workspace Crate Manifest
- [x] Phase 2 — Downstream Inter-Crate Facades & Re-Exports
- [x] Phase 3 — Binary & CLI Entrypoint Migration
- [x] Phase 4 — Integration Test Suites & Benchmarks Migration
- [x] Phase 5 — Documentation, Code Style Alignment & Final Verification

---

## Executive Summary

The `diffuse` workspace is composed of five modular crates: `diffuse-syntax`, `diffuse-core`, `diffuse-render`, `diffuse-tui`, and `diffuse-cli` (package name `diffuse`). While this multi-crate structure enforces strict dependency decoupling and modular compilation boundaries, the top-level `diffuse` package currently only builds binary targets (`diffuse` and `diffuse-harvest`) with no library root (`lib.rs`). Consequently, external consumers, integration tests, and internal CLI binaries must import subcrates via fractured crate names (`diffuse_core::*`, `diffuse_render::*`, `diffuse_tui::*`, `diffuse_syntax::*`) rather than an idiomatic unified namespace.

This plan establishes a unified, ergonomic `diffuse::*` namespace facade:
1. Exposes `diffuse::core`, `diffuse::render`, `diffuse::syntax`, and `diffuse::tui` from the primary `diffuse` crate root (`src/lib.rs`).
2. Provides convenient top-level prelude re-exports (`run_pipeline`, `PipelineOutput`, `default_theme`, `run_interactive_tui`).
3. Equips downstream crates (`diffuse-render`, `diffuse-tui`) with consistent inter-crate facades (`diffuse_render::core`, `diffuse_tui::core`, etc.).
4. Migrates all CLI binaries (`main.rs`, `harvest.rs`) and test suites (`golden.rs`, `harvest_benchmarks.rs`, `cli.rs`) to the unified namespace.

### Architectural Invariants
- **Acyclic Dependency Graph**: The DAG remains strictly ordered: `diffuse-syntax` $\to$ `diffuse-core` $\to$ `diffuse-render` $\to$ `diffuse-tui` $\to$ `diffuse` (facade + binaries). No circular dependencies are introduced.
- **Zero Runtime Overhead**: All facade modules and preludes are pure zero-cost `pub use` aliases resolved at compile time.
- **Strict Verification & Latency Budget**: All tests, wire contracts, and rendering suites remain 100% byte-identical and satisfy the sub-3s `./scripts/check.sh` execution budget.

---

## Table of Contents

1. [Phase 1 — Top-Level Library Facade & Workspace Crate Manifest](#1-phase-1--top-level-library-facade--workspace-crate-manifest)
2. [Phase 2 — Downstream Inter-Crate Facades & Re-Exports](#2-phase-2--downstream-inter-crate-facades--re-exports)
3. [Phase 3 — Binary & CLI Entrypoint Migration](#3-phase-3--binary--cli-entrypoint-migration)
4. [Phase 4 — Integration Test Suites & Benchmarks Migration](#4-phase-4--integration-test-suites--benchmarks-migration)
5. [Phase 5 — Documentation, Code Style Alignment & Final Verification](#5-phase-5--documentation-code-style-alignment--final-verification)

---

## 1. Phase 1 — Top-Level Library Facade & Workspace Crate Manifest

**Goal:** Transform the `diffuse` package in `crates/diffuse-cli` into a hybrid library/binary package that exposes the unified `diffuse` namespace.

### Affected Files
- `crates/diffuse-cli/Cargo.toml`:
  - Explicitly define `[lib]` with `path = "src/lib.rs"`.
  - Maintain `[[bin]]` entries for `diffuse` (`src/main.rs`) and `diffuse-harvest` (`src/bin/harvest.rs`).
- `crates/diffuse-cli/src/lib.rs` (new file):
  - Expose subcrate modules:
    ```rust
    pub use diffuse_core as core;
    pub use diffuse_render as render;
    pub use diffuse_syntax as syntax;
    pub use diffuse_tui as tui;
    ```
  - Expose a top-level `prelude` module with high-frequency types and functions:
    ```rust
    pub mod prelude {
        pub use diffuse_core::PipelineOptions;
        pub use diffuse_core::pipeline::{run_pipeline, run_pipeline_full};
        pub use diffuse_core::wire::{OutputFile, OutputRow, PipelineOutput};
        pub use diffuse_render::{default_theme, render_pipeline_output};
    }
    ```
  - Add unit tests in `crates/diffuse-cli/src/lib.rs` validating facade access.

### Verification Gate
- `cargo check -p diffuse --lib`
- `cargo test -p diffuse --lib`

---

## 2. Phase 2 — Downstream Inter-Crate Facades & Re-Exports

**Goal:** Ensure downstream crates (`diffuse-core`, `diffuse-render`, `diffuse-tui`) provide consistent sub-facades for their direct upstream dependencies.

### Affected Files
- `crates/diffuse-core/src/lib.rs`:
  - Verify and maintain `pub use diffuse_syntax as syntax;`.
- `crates/diffuse-render/src/lib.rs`:
  - Add `pub use diffuse_core as core;` and `pub use diffuse_syntax as syntax;`.
- `crates/diffuse-tui/src/lib.rs`:
  - Add `pub use diffuse_core as core;`, `pub use diffuse_render as render;`, and `pub use diffuse_syntax as syntax;`.

### Verification Gate
- `cargo check --workspace`
- `cargo test -p diffuse-core -p diffuse-render -p diffuse-tui`

---

## 3. Phase 3 — Binary & CLI Entrypoint Migration

**Goal:** Refactor the CLI binary (`crates/diffuse-cli/src/main.rs`) and the harvesting tool (`crates/diffuse-cli/src/bin/harvest.rs`) to consume the unified `diffuse::*` namespace.

### Affected Files
- `crates/diffuse-cli/src/main.rs`:
  - Replace `use diffuse_core::...` with `use diffuse::core::...`.
  - Replace `use diffuse_render::...` with `use diffuse::render::...`.
  - Replace `use diffuse_tui::...` with `use diffuse::tui::...`.
- `crates/diffuse-cli/src/bin/harvest.rs`:
  - Replace `use diffuse_core::...` with `use diffuse::core::...`.

### Verification Gate
- `cargo build --bin diffuse --bin diffuse-harvest`
- Run `target/debug/diffuse --version` and `target/debug/diffuse-harvest --help`

---

## 4. Phase 4 — Integration Test Suites & Benchmarks Migration

**Goal:** Update all integration tests and benchmark harnesses to use the unified `diffuse::*` namespace.

### Affected Files
- `crates/diffuse-cli/tests/golden.rs`:
  - Replace `use diffuse_core::...` with `use diffuse::core::...`.
  - Replace `use diffuse_render::...` with `use diffuse::render::...`.
  - Replace `use diffuse_syntax::...` with `use diffuse::syntax::...`.
- `crates/diffuse-cli/tests/harvest_benchmarks.rs`:
  - Replace `use diffuse_core::...` with `use diffuse::core::...`.
- `crates/diffuse-cli/tests/cli.rs`:
  - Migrate imports to `diffuse::*`.

### Verification Gate
- `cargo test --test golden`
- `cargo test --test harvest_benchmarks`
- `cargo test --test cli`

---

## 5. Phase 5 — Documentation, Code Style Alignment & Final Verification

**Goal:** Update codebase documentation, style guides, and run full test suites.

### Affected Files
- `CODE_STYLE.md`: Add guidelines on the unified `diffuse::*` namespace for external and integration usage vs. internal crate DAG dependencies.
- `AGENTS.md`: Update architectural notes regarding crate layout and facades.
- `TODO.md`: Check off item 8.2 with link to `plan/done/Unified Crate Namespace.md` (upon completion).
- Execute `./scripts/check.sh`.

### Verification Gate
- `./scripts/check.sh` passes cleanly (fmt, clippy with zero warnings, all cargo tests, extension tests).
