# diffuse — agent guidelines

## What this is

The diff-engine token-stream backend, ported one-for-one from the pi
review extension's `review.ts` (the ~3500-line engine + renderer in
`~/lab/pi-review`). diffuse is the ENGINE ONLY — parse → moves → collapse
→ merged runs — renderer-agnostic. The pi extension (and later a Rust TUI)
consumes it over the wire contract in PLAN.md §2.

## The porting discipline (Phase 1 — the one-for-one port)

The TypeScript in `~/lab/pi-review/review.ts` is the SPEC. While Phase 1
is open:

- Same function names (snake_case), same control flow, same thresholds,
  same fallbacks, same comments. The comments carry the *why* of every
  odd rule — port them.
- **No Rust idioms that change behavior**: no iterator chains that alter
  the evaluation order of the tie-break ladders, no enum-shaped rewrites
  of the pairing, no early returns that skip a branch. Break only for
  syntax and naming.
- The port map in PLAN.md §3 is the checklist — a task is done when every
  row is checked and `cargo build` is clean.
- If a behavior looks wrong in the Rust port, DO NOT "fix" it to match
  what you think it should do — check the TS first. The golden file is
  the ground truth: any deviation from it is a port bug (Phase 2 gate).

## The golden

`tests/golden/mirage-2025-08.golden.txt` is the locked "ideal" render of
the frozen corpus (`tests/golden/mirage-2025-08.json`), generated from
the same mirage snapshot as the pi-review golden. It encodes the user's
hand-tuned preferences (the strike/fill minimums, the anchor pairings,
the reverse-stretch shape, the trace-wobble fix).

The e2e acceptance test (`tests/golden.rs`) has TWO legs — the lib leg
(embed the engine, walk the output) and the bin leg (spawn the binary,
decode the token-stream JSON, walk that) — both must match the golden
byte-for-byte. The bin leg pins the wire contract to the golden.

**Golden updates are deliberate, not automatic.** The golden only changes
when the USER changes a rendering preference — never to make a test pass.
A failing golden means the port deviates from the TS; find the deviation.

## Working with the reference

- The TS engine: `~/lab/pi-review/review.ts` (functions listed in the
  port map, PLAN.md §3), its unit tests in `test/review.test.ts`, the
  golden walk in `test/golden.test.ts`.
- The fixture + golden: `~/lab/pi-review/test/fixtures/mirage-2025-08.json`
  and `test/golden/mirage-2025-08.golden.txt` (the copies here are frozen
  snapshots — regenerate from there only when the corpus intentionally
  changes).

## Conventions

- Rust edition 2024, toolchain pinned in `rust-toolchain.toml` (matches
  the mirage workspace).
- `cargo clippy -- -D warnings` and `cargo fmt --all -- --check` must be
  clean after every change.
- Commit policy: do not commit without explicit instruction — present a
  summary and ask.
