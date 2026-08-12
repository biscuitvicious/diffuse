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

## The porting phase — the one-for-one rewrite

The engine did not start here. It grew organically inside another
project: a terminal review tool whose diff machinery accumulated hundreds
of hand-tuned edge-case rules. Those rules are the project's real
specification, captured two ways:

1. **The reference implementation** — the original engine, written in
   TypeScript. It lives at `~/lab/pi-review/review.ts` (the functions
   `parseDiff`, `markMoves`, `collapseHunkRows`, the merge machinery, the
   LCS, the similarity scoring). You do not need to know TypeScript or
   the host tool to work here — you need to be able to *read* that file
   as the spec, line by line.
2. **The golden corpus** — `tests/golden/mirage-2025-08.{json,golden.txt}`:
   a real captured diff and the locked *ideal* output of the engine over
   it, exercising dozens of the edge cases. The golden is the ground
   truth.

**Phase 1 (the current phase) is a behavior-identical port.** While it is
open, the TypeScript file is the spec, and the port discipline is:

- Same function names (snake_case), same control flow, same thresholds,
  same fallbacks, same comments. The comments carry the *why* of every
  odd rule — port them.
- **No "improvements"**. No Rust idioms that change behavior: no
  iterator chains that alter the evaluation order of a tie-break ladder,
  no enum-shaped rewrites of the pairing logic, no early returns that
  skip a branch. Break only for syntax (no `undefined`, no dynamic
  typing) and naming (`camelCase` → `snake_case`).
- The port map in PLAN.md §3 is the checklist. A task is done when every
  row is checked and `cargo build` is clean.
- **If a behavior looks wrong, do not "fix" it.** Check the reference
  implementation first. If the reference does the same thing, the
  behavior is intended — the golden encodes it. If the Rust port deviates
  from the reference, the port is wrong; fix the port.

A later phase (PLAN.md Phase 4) will redesign the engine in idiomatic
Rust. That phase is explicitly deferred: first the port must prove itself
identical to the reference, then the redesign may change internals — but
never the golden without explicit human sign-off.

## The golden — the ground truth

`tests/golden/mirage-2025-08.golden.txt` is the locked ideal output of
the engine over the frozen corpus. It encodes the hand-tuned preferences:
the exact strike/fill minimums, which anchor pairings win, how moved
blocks read, the reverse-stretch shape.

The acceptance test (`tests/golden.rs`) walks it twice:

1. **lib leg** — embed the engine, run the full pipeline, compare.
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

- The spec (the original engine): `~/lab/pi-review/review.ts`.
- The reference's own tests: `~/lab/pi-review/test/review.test.ts` (unit
  blocks — Phase 2 ports each block to a `#[test]` with the same inputs
  and expectations).
- The reference's golden walk: `~/lab/pi-review/test/golden.test.ts` —
  the acceptance walk is its port.
- The corpus here is a frozen copy of the reference's fixture + golden.
  Regenerate from the reference only when the corpus is deliberately
  changed (rare, and only with human sign-off).

## Conventions

- Rust edition 2024; the toolchain is pinned in `rust-toolchain.toml`
  (same pin as the broader workspace it was born from).
- `cargo clippy -- -D warnings` and `cargo fmt --all -- --check` must be
  clean after every change.
- Commit policy: do not commit without explicit instruction — present a
  summary and ask.
- When a phase ends, update PLAN.md (check off the phase, note what
  changed) and this file if the conventions changed.
