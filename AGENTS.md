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

## The porting phase — the one-for-one rewrite

The engine did not start here. It grew organically inside another
project: a terminal review tool whose diff machinery accumulated hundreds
of hand-tuned edge-case rules. Those rules are the project's real
specification, captured two ways:

1. **The reference implementation** — the original engine, written in
   TypeScript. It lives at `extension/pi/review.ts` in this workspace (the
   functions `parseDiff`, `markMoves`, `collapseHunkRows`, the merge
   machinery, the LCS, the similarity scoring). The pristine upstream
   copy still exists at `~/lab/pi-review` — do not edit it; the workspace
   copy is the spec. You do not need to know TypeScript or the host tool
   to work here — you need to be able to *read* that file as the spec,
   line by line.
2. **The golden corpus** — `tests/golden/mirage-2025-08.{json,golden.txt}`:
   a real captured diff and the locked *ideal* output of the engine over
   it, exercising dozens of the edge cases. The golden is the ground
   truth. It is the SINGLE canonical copy — the extension's golden walk
   reads it from here too; never fork a second fixture/golden pair.

**Phase 1 (done) was a behavior-identical port.** While a port is open,
the TypeScript file is the spec, and the port discipline is:

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

The Rust-y redesign (PLAN.md Phase 5) is explicitly deferred: first the
port must prove itself identical to the reference (Phase 3's acceptance +
Phase 4's byte-identical golden), then the redesign may change internals —
but never the golden without explicit human sign-off.

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

## Conventions

- Rust edition 2024; the toolchain is pinned in `rust-toolchain.toml`
  (same pin as the broader workspace it was born from).
- `cargo clippy -- -D warnings` and `cargo fmt --all -- --check` must be
  clean after every change.
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
