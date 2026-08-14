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
the pi review extension (TypeScript renderer + TUI), moved in at PLAN.md
Phase 2 so the engine and its consumer change together. The extension runs
the **rust backend** — it spawns the diffuse binary BUNDLED with it
(`bin/diffuse`, resolved next to review.ts) and renders its token-stream
JSON; **Phase 4's equivalence gate passed** (the rust backend renders the
byte-identical golden). The TS engine and the reference repo are removed.

## Engine Development & Specification

The engine port from TypeScript to Rust is complete (Phases 1–4 passed with byte-identical equivalence). **The Rust implementation is now the primary engine and canonical specification.**

In this active development phase:

- **Rust is the Canonical Reference**: The Rust codebase (`src/`) is the source of truth for engine behavior.
- **Verification via Test Corpus**: Engine behavior and edge cases are specified and guarded by unit tests (`#[test]` in Rust, `review.test.ts` in TS).
- **Behavior Improvements & Visual Tuning**: Subword boundaries, reflow pairing, move detection, syntax highlighting, and rendering aesthetics are actively refined to deliver the best visual diff experience.
- **Golden Corpus**: `tests/golden/mirage-2025-08.{json,golden.txt}` serves as the visual acceptance baseline. As rendering and subword improvements land, golden fixtures are regenerated following human review.

## The golden — the ground truth

`tests/golden/mirage-2025-08.golden.txt` is the locked ideal output of
the engine over the frozen corpus. It encodes the hand-tuned preferences:
the exact strike/fill minimums, which anchor pairings win, how moved
blocks read, the reverse-stretch shape.

The golden is a **rendered** output — the TUI's colored scrolldown — so
it lives where the renderer lives: the extension's
`test/golden.test.ts` (at `extension/pi/test/`). Its rust leg (Phase 4's
equivalence proof) feeds the diffuse backend's JSON into the renderer and
must produce the byte-identical golden — it does, and it ALSO walks every
wire run asserting the engine's per-file classification equals the TS
tokenizer's (the renderer re-derives
colors with its own profile, so the gate proves the structure AND the
classification separately).

The Rust acceptance (`tests/golden.rs`) proves the engine + the wire
contract over the same corpus, twice:

1. **lib leg** — embed the engine, run the full pipeline, walk the output.
2. **bin leg** — spawn the built binary, feed it the same input, decode
   its JSON, run the same walk, compare.

Both must match byte-for-byte. The bin leg exists because the wire
contract (PLAN.md §2) is itself part of the spec — a change to the JSON
schema breaks the bin leg even if the engine's semantics are unchanged.

**The golden changes only when a rendering preference is deliberately
changed** — by a human, after review. Never regenerate it to make a test
pass. A failing golden means the port deviates from the reference; find
the deviation.

## Working with the reference

- The spec (the original engine): `extension/pi/review.ts` (pristine
  upstream: `~/lab/pi-review/review.ts` — read-only).
- The reference's own tests: `extension/pi/test/review.test.ts` (unit
  blocks — Phase 3 ports each ENGINE-side block to a `#[test]` with the
  same inputs and expectations; the renderer-side blocks stay in TS and
  run with the extension).
- The reference's golden walk: `extension/pi/test/golden.test.ts` — the
  rendered acceptance walk; its rust leg is Phase 4's equivalence gate.
- The corpus: `tests/golden/` — the single canonical copy, shared by the
  Rust acceptance and the extension's walk. Regenerate only when the
  corpus is deliberately changed (rare, and only with human sign-off).

## Test-first bugfix & feature discipline

Every bug fix, behavior adjustment, or new feature must expand the test corpus with a targeted test covering the exact scenario:

1. **Write the test first**: Before editing implementation code, write a unit or component test (in Rust `#[test]` or TS `review.test.ts`) specifying the desired behavior or reproducing the bug.
2. **Confirm initial failure**: Run the test to verify that it fails in the expected way, proving that the test suite actually catches the bug or missing capability.
3. **Implement the change**: Write the minimal code needed to fulfill the requirement.
4. **Verify passing run**: Ensure `cargo test` and `npm test` pass cleanly, permanently expanding the test suite to guard against regressions.

## Conventions

- Rust edition 2024; the toolchain is pinned in `rust-toolchain.toml`
  (same pin as the broader workspace it was born from).
- `cargo clippy -- -D warnings` and `cargo +nightly fmt --all -- --check`
  must be clean after every change (see [`CODE_STYLE.md`](file:///home/daniel/lab/diffuse/CODE_STYLE.md)).
- `target/`, `extension/*/node_modules/`, and `extension/pi/bin/` are
  gitignored (build artifacts).
- The extension's suite runs with `npm test` in `extension/pi/` (rust-only:
  the unit suite + the golden walk over both corpora). `scripts/build-bin.sh`
  bundles the diffuse binary; `scripts/install.sh` syncs review.ts + the
  binary into `~/.pi/agent/extensions/` for pi (`/reload` to pick it up).
- Commit policy: do not commit without explicit instruction — present a
  summary and ask.
- When a phase ends, update PLAN.md (check off the phase, note what
  changed) and this file if the conventions changed.
