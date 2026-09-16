# diffuse — Agent & Contributor Guidelines

## Project Overview & Architecture

*diffuse* is a structured diff analysis engine and interactive terminal review tool implemented in Rust. Given a unified diff and optional pre- and post-image file snapshots, the engine reconstructs the granular structure of code changes:
- Line-level correspondence (asymmetric additions and deletions paired into logical modification rows).
- Relocation detection (moved blocks distinguished from additions and deletions).
- Sub-line token alignment (common, struck, and filled characters with syntactic token classifications).

The engine is renderer-agnostic and performs no direct terminal I/O. Renderers (the interactive TUI reviewer, CLI pretty-printers, Markdown generators, or external extensions such as `extension/pi/`) consume a structured token stream over a frozen JSON wire contract specified in [`crates/diffuse-core/src/wire.rs`](crates/diffuse-core/src/wire.rs).

## Canonical Reference & Specification

- **Rust Codebase as Ground Truth**: The native Rust implementation (`crates/`) is the primary engine and canonical specification for all algorithmic, rendering, and interaction behaviors.
- **Verification via Golden Corpus**: Acceptance criteria and regression baselines are guarded by unit tests and the frozen corpora in [`tests/golden/`](tests/golden/).
- **Wire Contract**: The JSON wire format defined in [`crates/diffuse-core/src/wire.rs`](crates/diffuse-core/src/wire.rs) forms the public serialization schema between the engine and consuming clients.

## The Golden Corpus

The golden corpora in [`tests/golden/`](tests/golden/) represent the locked, verified baseline output of the engine across real-world changesets. They encode fine-tuned algorithmic properties: strike/fill minimums, anchor pairings, move-block boundaries, and reverse-stretch behavior.

The acceptance suite in [`tests/golden.rs`](tests/golden.rs) verifies the pipeline across two complementary execution paths:
1. **Library Leg**: Direct in-process invocation of the pipeline over frozen inputs.
2. **Binary Leg**: External invocation of the compiled `diffuse` CLI binary, parsing the emitted JSON stream.

Both execution paths must match the recorded golden outputs (`.wire.jsonl`, `.render.txt`, `.markdown.md`) byte-for-byte.

> [!IMPORTANT]
> **Golden Change Policy**: Golden fixtures change only when engine behavior is intentionally modified and reviewed. A failing golden test signifies an unintended deviation; never regenerate fixtures merely to resolve test failures. Detailed fixture inventory and sign-off criteria are specified in [`tests/AGENTS.md`](tests/AGENTS.md).

## Test-First Discipline & Quality Invariants

Every modification, bug fix, or feature must expand the automated test suite under a strict test-first methodology:

1. **Write Failing Test First**: Author a targeted unit test (`#[test]`) capturing the missing capability or reproducing the defect prior to modifying implementation code.
2. **Confirm Initial Failure**: Execute the test to verify that the failure mode accurately identifies the condition.
3. **Implement Minimal Solution**: Author the minimal, architecturally sound changes necessary to satisfy the test.
4. **Exhaustive Branch Coverage**: Ensure that all new and altered code paths achieve 100% branch coverage, exercising every conditional branch and pattern match arm (see [`GUIDELINES.md`](GUIDELINES.md)).
5. **Enforce Performance Budgets**:
   - Individual unit tests must execute in `< 0.5s`.
   - Test binaries must complete in `< 1.5s`.
   - The full verification gate (`scripts/check.sh`) must complete in `< 3s` on warm builds.
   - Long-running stress tests must be tagged with `#[ignore = "..."]`.

## Standard Development Workflow

1. **Failing Test**: Write the reproduction or specification test and confirm failure.
2. **Implementation**: Make the minimal changes required; verify existing tests pass.
3. **Verification Gate**: Execute `scripts/check.sh` (applies nightly `rustfmt`, runs `clippy --fix`, and executes all cargo tests). All checks must pass with zero warnings.
4. **Golden Review**: If golden outputs change, review diffs per [`tests/AGENTS.md`](tests/AGENTS.md) to confirm that changes are strictly improvements or equivalent.
5. **Installation**: `scripts/check.sh` installs the development binary (`~/.cargo/bin/diffuse`) and synchronizes the dev extension automatically. Release builds are compiled via `scripts/install.sh`.
6. **Review & Sign-Off**: Present changes and diff summaries for review. Do not commit changes without explicit instruction.

## Conventions & Standards

- **Rust Toolchain**: Pinned in [`rust-toolchain.toml`](rust-toolchain.toml) (Edition 2024).
- **Code Style & Formatting**: Nightly `cargo fmt` and `cargo clippy -- -D warnings` enforced by `scripts/check.sh`. Detailed component layout and import conventions are specified in [`CODE_STYLE.md`](CODE_STYLE.md).
- **Architectural Guidelines**: Decoupling, fail-fast invariants, and algorithmic rules are specified in [`GUIDELINES.md`](GUIDELINES.md).
- **Documentation & Tone Standards**: All official project documentation (including [`README.md`](README.md), user guides, API docstrings, and architectural specifications) must maintain a clean, neutral, and academic tone. Avoid colloquialisms, promotional phrasing, or informal conversational commentary.
- **Engineering Plans**: Active design proposals follow [`plan/Plan Guidelines.md`](plan/Plan%20Guidelines.md). Completed plans are archived under `plan/done/`.
