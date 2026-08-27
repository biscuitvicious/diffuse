# diffuse — agent guidelines

## What this project is

A **diff engine** — it reads a `git diff` plus the file contents on both
sides, and produces a structured, word-level description of what changed:
which removed line became which added line, which lines moved, what
changed inside rewritten lines. The output is a **token stream** per
changed row: which characters are common, struck (deleted), or filled
(added), plus the semantic token type (keyword, string, comment, …).

The engine is renderer-agnostic — it never touches a terminal. Renderers
(a review TUI, a CLI pretty-printer, anything) consume the token stream
over a frozen JSON contract (PLAN.md §2). The crate ships as a library
(embed the pipeline) and a binary (a CLI that reads a diff and emits the
token-stream JSON).

The workspace also holds the engine's first consumer: **`extension/pi/`** —
the pi review extension. The extension is a **thin client**: `/review`
resolves the git repository, stops the pi TUI, and spawns the bundled
`diffuse` binary (`bin/diffuse`) in interactive TUI mode with `stdio: "inherit"`.
All diff parsing, rendering, TUI interaction, note persistence, and git actions
are owned entirely by the native Rust implementation in `crates/`.

## Engine Development & Specification

The engine, terminal renderer, and interactive reviewer TUI are all natively implemented in Rust. **The Rust implementation is the primary engine and canonical specification.**

In this active development phase:

- **Rust is the Canonical Reference**: The Rust codebase (`crates/`) is the source of truth for all engine, render, and TUI behavior.
- **Verification via Test Corpus**: Engine behavior and edge cases are specified and guarded by unit tests (`#[test]` in Rust) and the smoke suite in `extension/pi/`.
- **Behavior Improvements & Visual Tuning**: Subword boundaries, reflow pairing, move detection, syntax highlighting, and rendering aesthetics are actively refined to deliver the best visual diff experience.
- **Golden Corpus**: `tests/golden/` serves as the acceptance baseline
  (7 frozen corpora). As rendering and subword improvements land, fixtures
  change only after review — see [`tests/AGENTS.md`](tests/AGENTS.md).

## The golden — the ground truth

The golden corpora are the locked, reviewed ideal output of the engine.
They encode the hand-tuned preferences: the exact strike/fill minimums,
which anchor pairings win, how moved blocks read, the reverse-stretch
shape.

The Rust acceptance (`tests/golden.rs`) proves the engine + the wire
contract over the frozen corpora in two legs — the **lib leg** (embed the
engine, run the full pipeline) and the **bin leg** (spawn the built binary,
feed it the same input, decode its JSON, run the same walk). Both must
match byte-for-byte; the bin leg exists because the wire contract
(PLAN.md §2) is itself part of the spec. The rendered golden (the TUI's
colored scrolldown) is walked by the extension's
`extension/pi/test/golden.test.ts`, which needs the TS renderer.

**Fixture types, corpus inventory, regeneration commands, and the review
signoff gate for any golden change live in
[`tests/AGENTS.md`](file:///home/daniel/lab/diffuse/tests/AGENTS.md).**
The essentials: the golden changes only when behavior is deliberately
changed, after review — never regenerate it to make a test pass. A
failing golden means the engine deviates from the reviewed ideal; find
the deviation.

## Working with the reference

- The spec (the original engine): `extension/pi/review.ts` (pristine
  upstream: `~/lab/pi-review/review.ts` — read-only).
- The reference's own tests: `extension/pi/test/review.test.ts` (unit
  blocks — Phase 3 ports each ENGINE-side block to a `#[test]` with the
  same inputs and expectations; the renderer-side blocks stay in TS and
  run with the extension).
- The reference's golden walk: `extension/pi/test/golden.test.ts` — the
  rendered acceptance walk over the shared corpora in `tests/golden/`.
- The corpus: `tests/golden/` — the single canonical copy, shared by the
  Rust acceptance and the extension's walk. Fixture types, regeneration
  mechanics, and change policy: [`tests/AGENTS.md`](file:///home/daniel/lab/diffuse/tests/AGENTS.md).

## Test-first bugfix & feature discipline

Every bug fix, behavior adjustment, or new feature must expand the test corpus with a targeted test covering the exact scenario:

1. **Write the test first**: Before editing implementation code, write a unit or component test (in Rust `#[test]` or TS `review.test.ts`) specifying the desired behavior or reproducing the bug.
2. **Confirm initial failure**: Run the test to verify that it fails in the expected way, proving that the test suite actually catches the bug or missing capability.
3. **Implement the change**: Write the minimal code needed to fulfill the requirement.
4. **Verify passing run**: Ensure `cargo test` and `npm test` pass cleanly, permanently expanding the test suite to guard against regressions.
5. **Verify 100% branch coverage**: All new code and code paths must achieve 100% **branch coverage** with comprehensive unit tests exercising every conditional, match arm, and edge case (see [`GUIDELINES.md § Test-First Methodology & 100% Branch Coverage`](file:///home/daniel/lab/diffuse/GUIDELINES.md#test-first-methodology--100-branch-coverage)).

## Test Performance Invariants

To keep development iterations instantaneous and prevent CI / pre-commit stalls, the test suite enforces strict latency budgets:

- **Individual Test Latency**: Any individual `#[test]` must execute in **`< 0.5s`**.
- **Test Binary Latency**: Any test binary (e.g. `diffuse-core`, `diffuse-tui`, `golden.rs`, `harvest_benchmarks.rs`) must complete in **`< 1.5s`**.
- **Full `./scripts/check.sh` Suite**: The entire `./scripts/check.sh` gate (fmt check, fast clippy, all cargo tests, extension smoke suite) must complete in **`< 3s`** (warm cache, less clippy and compile time).
- **Massive Scale & Stress Benchmarks**: Any multi-megabyte whole-evolution history stress tests (e.g. `diffuse-history-mid`) or 10,000-file scale generators must be tagged with `#[ignore = "..."]` so normal developer test sweeps remain sub-second while full stress validation is runnable on-demand via `cargo test -- --ignored`.

## Standard workflow

Every bug fix, behavior change, or feature follows this loop (steps 1–2
are the test-first discipline above):

1. **Failing test** — write the test demonstrating the missing feature,
   change, or fix; confirm it fails in the expected way.
2. **Implement/fix** — make the minimal change; fix any tests broken by
   the new behavior (an old expectation breaking is a spec change — review
   it consciously, don't just update it).
3. **`./scripts/check.sh`** — nightly fmt + clippy `--fix` (both
   auto-apply), the full cargo test suite, and the extension's `npm test`.
   Everything must pass cleanly.
4. **Agent review of golden changes** — walk any wire/golden diffs per
   [`tests/AGENTS.md`](file:///home/daniel/lab/diffuse/tests/AGENTS.md):
   assert strictly-better-or-equivalent, flag and pause on anything
   suspicious.
5. **Install the extension** — `extension/pi/scripts/install.sh`
   (bundles the release binary + review.ts into
   `~/.pi/agent/extensions/`; `/reload` in pi, then `/review`).
6. **HUMAN REVIEW** — pause for the human's visual & code review of the
   rendered result. The human may instruct auto-commit — in that case
   just install the extension (step 5, if not already done) and commit.

## Conventions

- Rust edition 2024; the toolchain is pinned in `rust-toolchain.toml`
  (same pin as the broader workspace it was born from).
- `cargo clippy -- -D warnings` and `cargo +nightly fmt --all -- --check`
  must be clean after every change. Code style, import conventions, and
  top-down file component layout (public/high-level abstraction at the top
  down to internal detail and tests at the bottom) are specified in
  [`CODE_STYLE.md`](file:///home/daniel/lab/diffuse/CODE_STYLE.md). Architecture,
  language profile decoupling, and algorithm separation rules are specified in
  [`GUIDELINES.md`](file:///home/daniel/lab/diffuse/GUIDELINES.md).
- `target/`, `extension/*/node_modules/`, and `extension/pi/bin/` are
  gitignored (build artifacts).
- The extension's suite runs with `npm test` in `extension/pi/` (rust-only:
  the unit suite + the golden walk over both corpora). `scripts/build-bin.sh`
  bundles the diffuse binary; `scripts/install.sh` syncs review.ts + the
  binary into `~/.pi/agent/extensions/` for pi (`/reload` to pick it up).
- Commit policy: do not commit without explicit instruction — present a
  summary and ask.
- Engineering Plans: Follow [`plan/Plan Guidelines.md`](file:///home/daniel/lab/diffuse/plan/Plan%20Guidelines.md)
  when authoring or updating architectural plans in `plan/`. When a major plan completes,
  move it to `plan/done/<Plan Name>.md`.
- When a phase ends, update the active plan in `plan/` (see the phase
  status conventions in [`plan/Plan Guidelines.md`](file:///home/daniel/lab/diffuse/plan/Plan%20Guidelines.md)),
  and this file if the conventions changed. Golden-output changes that land
  with a phase follow the review gate in
  [`tests/AGENTS.md`](file:///home/daniel/lab/diffuse/tests/AGENTS.md).
