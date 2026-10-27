# diffuse — Agent and contributor guidelines

## Project overview and architecture

*diffuse* is a structured diff analysis engine and interactive terminal review tool implemented in Rust. Given a unified diff and optional pre- and post-image file snapshots, the engine reconstructs the granular structure of code changes:

- Line-level correspondence: asymmetric additions and deletions paired into logical modification rows.
- Relocation detection: moved blocks distinguished from additions and deletions.
- Sub-line token alignment: common, struck, and filled characters with syntactic token classifications.

The engine is renderer-agnostic and performs no direct terminal I/O. Renderers (the interactive TUI reviewer, CLI pretty-printers, Markdown generators, or external extensions such as `extension/pi/`) consume a structured token stream over a frozen JSON wire contract specified in [`crates/diffuse-core/src/wire.rs`](crates/diffuse-core/src/wire.rs).

## Workspace architecture and crate layout

The repository is organized into discrete Cargo workspace member crates under `crates/`, with strictly unidirectional dependencies:

$$\text{diffuse-algo} + \text{diffuse-syntax} \longrightarrow \text{diffuse-core} \longrightarrow \text{diffuse-render} \longrightarrow \text{diffuse-tui} \longrightarrow \text{diffuse (CLI)}$$

| Crate | Directory | Purpose & Key Canonical Modules |
| :--- | :--- | :--- |
| **`diffuse`** | [`crates/diffuse/`](crates/diffuse/) | CLI binary entry point ([`src/main.rs`](crates/diffuse/src/main.rs)) and unified workspace library facade ([`src/lib.rs`](crates/diffuse/src/lib.rs)). |
| **`diffuse-algo`** | [`crates/diffuse-algo/`](crates/diffuse-algo/) | Language-agnostic algorithms: Dyck language container pushdown automata ([`src/dyck.rs`](crates/diffuse-algo/src/dyck.rs)), `SmallStack`, Myers/affine-gap primitives. |
| **`diffuse-syntax`** | [`crates/diffuse-syntax/`](crates/diffuse-syntax/) | Syntactic classifications, Markdown parser ([`src/markdown.rs`](crates/diffuse-syntax/src/markdown.rs)), language profiles & operator tables ([`src/profile.rs`](crates/diffuse-syntax/src/profile.rs), [`src/language/`](crates/diffuse-syntax/src/language/)). |
| **`diffuse-core`** | [`crates/diffuse-core/`](crates/diffuse-core/) | Core diff analysis pipeline: LCS dynamic programming ([`src/lcs.rs`](crates/diffuse-core/src/lcs.rs)), token classification & entropy ([`src/token.rs`](crates/diffuse-core/src/token.rs)), alignment tape ([`src/tape.rs`](crates/diffuse-core/src/tape.rs)), clustering, git access ([`src/repo.rs`](crates/diffuse-core/src/repo.rs)), and the public wire contract ([`src/wire.rs`](crates/diffuse-core/src/wire.rs)). |
| **`diffuse-render`** | [`crates/diffuse-render/`](crates/diffuse-render/) | Terminal styling, color palettes, yansi rendering, Markdown diff emitters. |
| **`diffuse-tui`** | [`crates/diffuse-tui/`](crates/diffuse-tui/) | Interactive terminal reviewer (`Ratatui`, `crossterm`). |
| **`tests/golden`** | [`tests/golden/`](tests/golden/) | 23 frozen golden acceptance corpora guarding regression baselines. |
| **`extension/pi`** | [`extension/pi/`](extension/pi/) | TypeScript / Pi IDE review extension. |

## Canonical reference and specification

- **Rust codebase as ground truth**: The native Rust implementation (`crates/`) is the primary engine and the canonical specification for all algorithmic, rendering, and interaction behaviors.
- **Verification via golden corpus**: Unit tests and the frozen corpora in [`tests/golden/`](tests/golden/) guard acceptance criteria and regression baselines.
- **Wire contract**: The JSON wire format defined in [`crates/diffuse-core/src/wire.rs`](crates/diffuse-core/src/wire.rs) forms the public serialization schema between the engine and consuming clients.

## The golden corpus

The golden corpora in [`tests/golden/`](tests/golden/) are the locked, verified baseline output of the engine across real-world changesets. They encode fine-tuned algorithmic properties: strike/fill minimums, anchor pairings, move-block boundaries, and reverse-stretch behavior.

The acceptance suite in [`crates/diffuse/tests/golden.rs`](crates/diffuse/tests/golden.rs) verifies the pipeline across two complementary execution paths:

1. **Library leg**: Direct in-process invocation of the pipeline over frozen inputs.
2. **Binary leg**: External invocation of the compiled `diffuse` CLI binary, parsing the emitted JSON stream.

Both execution paths must match the recorded golden outputs (`.wire.jsonl`, `.render.dark.txt`, `.render.light.txt`, `.render.raw.txt`, `.markdown.md`) byte-for-byte.

> [!IMPORTANT]
> **Golden change policy**: Golden fixtures change only when engine behavior is intentionally modified and reviewed. A failing golden test signifies an unintended deviation; never regenerate fixtures merely to resolve test failures. Detailed fixture inventory and sign-off criteria are specified in [`tests/AGENTS.md`](tests/AGENTS.md).

## Test-first discipline and quality invariants

Every modification, bug fix, or feature must expand the automated test suite under a strict test-first methodology:

1. **Write a failing test first**: Author a targeted unit test (`#[test]`) capturing the missing capability or reproducing the defect, before modifying implementation code.
2. **Confirm the initial failure**: Execute the test to verify that the failure mode accurately identifies the condition.
3. **Implement the minimal solution**: Author the minimal, architecturally sound changes necessary to satisfy the test.
4. **Exhaustive branch coverage**: Ensure that all new and altered code paths achieve 100% branch coverage, exercising every conditional branch and pattern match arm (see [`GUIDELINES.md`](GUIDELINES.md)).
5. **Enforce performance budgets**:
   - Individual unit tests must execute in `< 0.5s`.
   - Test binaries must complete in `< 1.5s`.
   - The full verification gate (`scripts/check.sh`) must complete in `< 3s` on warm builds.
   - Long-running stress tests must be tagged with `#[ignore = "..."]`.

## Standard development workflow

1. **Failing test**: Write the reproduction or specification test and confirm failure.
2. **Implementation**: Make the minimal changes required; verify existing tests pass.
3. **Verification gate**: Execute `scripts/check.sh` after every change (applies nightly `rustfmt`, runs `clippy --fix`, and executes all cargo tests). All checks must pass with zero warnings. Do not run `cargo fmt` or `cargo clippy` separately — `scripts/check.sh` already applies both to the maximum extent.
4. **Golden review**: If golden outputs change, review diffs per [`tests/AGENTS.md`](tests/AGENTS.md) to confirm that changes are strictly improvements or equivalent.
5. **Installation**: `scripts/check.sh` installs the development binary (`~/.cargo/bin/diffuse`) and synchronizes the dev extension automatically. Release builds are compiled via `scripts/install.sh`.
6. **Pre-commit review, cleanup & sign-off**: At the end of each task, perform a code review and cleanup pass of the changes, and perform any refactorings motivated by the changes so code quality does not deteriorate. Present the result together with the change summary, always pausing before committing to allow for user testing. Do not commit changes without explicit instruction.

## Conventions and standards

- **Rust toolchain**: Pinned in [`rust-toolchain.toml`](rust-toolchain.toml) (Edition 2024).
- **Code style and formatting**: Nightly `cargo fmt` and `cargo clippy -- -D warnings`, enforced by `scripts/check.sh`. Detailed component layout and import conventions are specified in [`CODE_STYLE.md`](CODE_STYLE.md).
- **Architectural guidelines**: Decoupling, fail-fast invariants, and algorithmic rules are specified in [`GUIDELINES.md`](GUIDELINES.md).
- **Documentation and tone standards**: All official project documentation (including [`README.md`](README.md), user guides, API docstrings, and architectural specifications) must maintain a clean, neutral, and academic tone. Avoid colloquialisms, promotional phrasing, and informal conversational commentary. Use American spelling per the GOV.UK spelling exception in [`GUIDELINES.md`](GUIDELINES.md).
- **Engineering plans**: Active design proposals follow [`plan/Plan Guidelines.md`](plan/Plan%20Guidelines.md). Completed plans are archived under `plan/done/`.
