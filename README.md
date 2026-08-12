# diffuse

A **diff engine** — the part of a code-review tool that reads a `git diff`
and turns it into a structured, word-level description of *what changed*.

A raw diff says "these lines were removed, those were added". A diff
engine says more: *which removed line became which added line*, *which
lines just moved elsewhere*, *what exactly changed inside a line that was
rewritten*. That richer description is what a review UI displays — with
struck-through deleted words, filled-in added words, muted moved lines,
and method-level anchors.

## The pipeline

```
git diff + file contents
        │
        ▼
  1. parse        split the diff into files, hunks, and add/del rows
  2. moves        mark exact-content del/add pairs as *moved* (relocated,
                  not changed) — with the block-move rule (a deleted block
                  whose first line did not move is a deletion, not a move)
  3. collapse     pair each remaining del with its best-matching add into
                  one "mod" row — with the stretch (one del over several
                  adds), the reverse-stretch (several dels into one add),
                  the reflow rules (formatting merges), and the
                  brace-anchor (a tail before a closing brace pairs with
                  its counterpart in the same function)
  4. merge        produce the *token stream* per mod row: which characters
                  are common, which are struck (deleted), which are filled
                  (added) — computed by a word-level LCS with a long list
                  of hand-tuned rules (see PLAN.md §6)
```

The output is **renderer-agnostic**: the token stream describes the
semantics of the change, not how to paint it. A renderer decides colors,
layout, and interactivity. diffuse ships as a library (embed the
pipeline) and a command-line binary (read a diff, emit the token streams
as JSON).

## The two sides

```
diffuse (the engine, this crate)
   ↓ token-stream JSON (the wire contract)
renderer (a TUI — any TUI)
```

The engine never touches a terminal. It takes a diff and contents, and
produces structured rows. The wire contract (input/output schema) is
frozen in PLAN.md §2 — renderers are written against the contract, not
the internals, so a new renderer (or a redesigned engine) doesn't break
the other side.

## Layout

```
src/              the engine (lib + bin)
  lib.rs        the engine API (module tree)
  main.rs       the binary: a CLI that reads a diff and emits the token
                streams as JSON
  parse.rs      diff → files, hunks, rows
  lcs.rs        the word-level diff: tokenizing, the LCS, the spans
  similarity.rs line-similarity scoring (the pairing's metric)
  moves.rs      move detection + the block-move rule
  tail.rs       the brace-anchor helpers
  collapse.rs   the pairing: stretch, reverse-stretch, reflow, ordering
  merge.rs      the token-stream merge per row
  counts.rs     the add/del/mod/moved counts
  syntax.rs     the semantic token classification (keyword, string, …)
tests/
  golden.rs     the acceptance test (two legs)
  golden/       the SHARED frozen diff corpus + its locked expected output
                (the extension's golden walk reads it from here too)
extension/        the extensions home — one subdir per agent
  pi/             the pi review extension — the engine's first renderer (TS)
    review.ts     the whole extension (single file, pi loads it as-is)
    bin/diffuse   the BUNDLED diffuse binary (scripts/build-bin.sh)
    test/         the unit + golden suites (npm test; npm run test:rust)
```

## Build & test

```bash
cargo build                      # the lib + the binary
cargo test                       # unit tests + the golden acceptance test
cargo clippy -- -D warnings
cargo fmt --all -- --check
```

### The golden acceptance test

`tests/golden/mirage-2025-08.golden.txt` is a locked snapshot of the
engine's *ideal* output over a real captured diff
(`tests/golden/mirage-2025-08.json`). It encodes dozens of hand-tuned
rendering behaviors — the exact strike/fill minimums, which anchor
pairings win, how moved blocks read. The corpus is the SINGLE canonical
copy: the extension's golden walk reads the same files.

The rendered golden is a TUI output, so the byte-exact walk against it
lives with the renderer (the extension's `test/golden.test.ts`). The
Rust acceptance test (`tests/golden.rs`) proves the engine + the wire
contract over the corpus in two ways:

1. **lib leg** — embed the engine, run the pipeline, walk the output.
2. **bin leg** — spawn the binary, decode its JSON output, walk it the
   same way.

Both must match byte-for-byte. The golden changes only when a rendering
preference is deliberately changed — never to make a test pass.

## The story so far

diffuse is a faithful **port** of an existing, battle-tested diff engine
that grew organically inside a terminal review tool. That engine has
hundreds of edge-case rules, each tuned against the golden corpus. This
crate is being rebuilt from it in phases (PLAN.md): a one-for-one port
(behavior-identical, so the golden proves correctness), the extension
moved into the workspace, the tests ported to Rust, the original tool
switched over to this backend (the extension spawns the bundled diffuse
binary; the TS engine and the reference repo are removed), and finally a
Rust-native redesign. The one-for-one phase is
deliberate: it separates "did the port break anything" from "would a
redesign be better".
