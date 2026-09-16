# Widen Golden Corpus — Multi-Language Diversity & Realistic Diff Acceptance

- [ ] Phase 1 — Multi-Language Harvester & Dataset Generator
- [ ] Phase 2 — Multi-Language Corpus Curation (50–100+ Diffs)
- [ ] Phase 3 — High-Performance Parallel Acceptance Test Runner
- [ ] Phase 4 — Golden Baseline Generation, Oracle Verification & Review Signoff
- [ ] Phase 5 — Documentation, Benchmark Integration & CI Gates

---

## Executive Summary

Diffuse's core strength lies in its hand-tuned, language-aware diff alignment engine, semantic token streams, and reverse-stretch reconstruction. Today, the golden test corpus consists of 8 primary corpora, heavily centered on Rust and TypeScript. To establish diffuse as the gold standard across all modern software stacks, the golden suite must be expanded to **50–100+ realistic, production-grade diff fixtures** spanning at least 12 programming languages and data formats: **Rust, C/C++, Go, Zig, TypeScript, JavaScript, Python, JSON, YAML, Markdown, SQL, and Shell**.

This plan systematically builds out the wide fixture dataset, enhances `diffuse-harvest` with automated language classification and sanitization, curates realistic change topologies (such as multi-line function signature refactors, indentation re-nesting, docstring/comment edits, moved logic, and schema modifications), and upgrades the golden test harness to execute 100+ golden comparisons in parallel within strict sub-second performance budgets.

### Core Mathematical & Architectural Invariants
1. **Oracle Correctness**: Every harvested fixture includes `old_contents` and `new_contents` such that the mechanical apply oracle strictly satisfies $\text{apply}(\text{diff}, \text{old}) \equiv \text{new}$.
2. **Reversibility & Bijection**: All generated wire representations satisfy $\text{wire.reverse().reverse()} \equiv \text{wire}$ and reconstruct clean forward and backward text diffs.
3. **Bounded Storage Footprint**: All fixture assets (`*.json.gz`, `*.wire.json.gz`, `*.render.txt.gz`) use gzip compression, keeping repository size additions under $\sim 5\,\text{MB}$ total.
4. **Sub-Second Test Performance**: The wide golden test suite executes using multi-threaded batching to ensure test suites maintain the `< 1.5\text{s}` latency ceiling mandated by `AGENTS.md`.

---

## Table of Contents

1. [Phase 1 — Multi-Language Harvester & Dataset Generator](#1-phase-1--multi-language-harvester--dataset-generator)
2. [Phase 2 — Multi-Language Corpus Curation (50–100+ Diffs)](#2-phase-2--multi-language-corpus-curation-50100-diffs)
3. [Phase 3 — High-Performance Parallel Acceptance Test Runner](#3-phase-3--high-performance-parallel-acceptance-test-runner)
4. [Phase 4 — Golden Baseline Generation, Oracle Verification & Review Signoff](#4-phase-4--golden-baseline-generation-oracle-verification--review-signoff)
5. [Phase 5 — Documentation, Benchmark Integration & CI Gates](#5-phase-5--documentation-benchmark-integration--ci-gates)

---

## 1. Phase 1 — Multi-Language Harvester & Dataset Generator

**Goal:** Enhance `diffuse-harvest` and build synthetic/harvesting utilities to effortlessly generate compliant, sanitized `.json.gz` golden fixtures across diverse repositories and synthetic test benches.

### Key Deliverables & Affected Files
- `crates/diffuse-cli/src/bin/harvest.rs`:
  - Add `--gzip` / `-z` direct output compression support.
  - Add `--strip-binary` and `--max-file-size` guards to filter out large binary blobs and massive lockfiles automatically.
  - Add `--name <CORPUS_NAME>` and `--out-dir <DIR>` to output standardized fixture naming (`<category>-<lang>-<desc>.json.gz`).
- `crates/diffuse-cli/src/bin/generate_corpus.rs` (or integrated harvesting subcommand):
  - Batch harvesting script to process curated commits from real repositories (`curl`, `flask`, `ripgrep`, `kubernetes/go`, `zig-lang/zig`, `facebook/react`, `rust-lang/rust`, `postgres/postgres`, etc.).
  - Synthetic diff generator producing deterministic, edge-case diff topologies for languages without local git clones.

### Verification Gate
- `cargo test -p diffuse-cli --bin harvest`
- Verify `diffuse-harvest` runs against sample commits and emits valid gzipped fixture payloads containing `diff`, `new_content`, `old_content`, and `revision`.

---

## 2. Phase 2 — Multi-Language Corpus Curation (50–100+ Diffs)

**Goal:** Curate and harvest 50–100+ high-value, realistic diff fixtures covering the matrix of languages and diff patterns.

### Language & Domain Breakdown (Target: 60–80 Focused Fixtures)
1. **Rust (6–8 fixtures)**: Lifetime annotations, async/await transforms, trait impls, macro expansions, pattern matching arms.
2. **C / C++ (6–8 fixtures)**: Struct initialization, pointer arithmetic, header vs source updates, macro guards, preprocessor branches (from `curl`, `linux`, `redis`).
3. **Go (6–8 fixtures)**: Multi-value error returns, goroutines/channels, interface definitions, struct tags, defer statements (from `go`, `kubernetes`).
4. **Zig (4–6 fixtures)**: Comptime constructs, error unions, optional unwrapping, arena allocators.
5. **TypeScript / TSX (6–8 fixtures)**: Generic type constraints, React JSX/TSX component refactors, interface vs type aliases.
6. **JavaScript (4–6 fixtures)**: Prototype/ES6 class updates, destructuring assignments, CommonJS to ESM conversions.
7. **Python (6–8 fixtures)**: Decorators, list/dict comprehensions, type hints (PEP 484), dataclasses, pytest fixtures (from `flask`, `django`).
8. **JSON / JSONC (4–6 fixtures)**: Deeply nested configs, package.json dependency bumps, array reorderings.
9. **YAML (4–6 fixtures)**: Kubernetes manifests, CI/CD pipeline workflows, indentation shifts, multi-document streams.
10. **Markdown / Docs (4–6 fixtures)**: GFM tables, list renumbering, fenced code blocks, link updates.
11. **SQL (4–6 fixtures)**: DDL migrations, complex SELECT JOINs, window functions, index alterations.
12. **Shell / Bash (4–6 fixtures)**: Pipeline conditionals, heredocs, variable expansion `${var:-default}`, option parsing.

### Diff Topology Matrix
Each language group must capture representative topological challenges:
- Reflow / re-wrapping of long parameter lists.
- Indentation blocks (e.g. wrapping inside `if`, `try/catch`, `with`, `for`).
- Intra-line token changes with keyword retention.
- Block moves with minor internal edits.
- Additions and deletions across comment blocks vs executable code.

---

## 3. Phase 3 — High-Performance Parallel Acceptance Test Runner

**Goal:** Upgrade `crates/diffuse-cli/tests/golden.rs` so that 100+ fixtures execute concurrently and cleanly without slowing down the test runner.

### Technical Approach & Invariants
- **Dynamic Corpus Discovery & Parameterized Harness**:
  - Automatically discover all `tests/golden/*.json` and `tests/golden/*.json.gz` files.
  - Separate fixtures into two tiers:
    - **Core Tier (Fast Smoke, ~10 fixtures)**: Runs on every single `scripts/check.sh` invocation (lib leg + bin leg + render check + oracle) within `< 1.0\text{s}`.
    - **Wide Tier (Extended Acceptance, 50–100+ fixtures)**: Runs fully in parallel with in-memory Rayon thread pool (`run_lib_leg` + `assert_wire_json_matches` + `apply_oracle`), with fast batch bin-leg checks.
- **Fixture Cache & Zero-Copy Decompression**:
  - In-memory thread-safe `FIXTURE_CACHE` preventing redundant decompression.
- **Memory & Latency Control**:
  - Streaming verification to ensure whole-suite runs complete in `< 2.5\text{s}` on standard developer hardware.

### Affected Files
- `crates/diffuse-cli/tests/golden.rs`
- `crates/diffuse-cli/tests/wide_golden.rs` (or unified `golden.rs` dynamic test matrix)

---

## 4. Phase 4 — Golden Baseline Generation, Oracle Verification & Review Signoff

**Goal:** Generate the canonical `.wire.json.gz` and `.render.txt.gz` golden baselines for all new fixtures under strict review discipline.

### Execution Steps
1. Run `UPDATE_WIRE=1 UPDATE_RENDER=1 cargo test --test golden` to generate the baseline wire and render targets.
2. Execute mechanical `apply_oracle` across 100% of fixtures to prove zero regression or corruption in diff representations.
3. Perform agent-level and human visual inspection of sample rendered representations to guarantee optimal tokenization and highlight aesthetics.

---

## 5. Phase 5 — Documentation, Benchmark Integration & CI Gates

**Goal:** Finalize documentation and update repository guidelines and tracking.

### Deliverables
- Update [`tests/AGENTS.md`](../tests/AGENTS.md) with the comprehensive corpus table and language coverage statistics.
- Update [`TODO.md`](../TODO.md) marking items 5.2 and 5.3 complete.
- Verify `scripts/check.sh` runs fully green and sub-3s on warm runs.
