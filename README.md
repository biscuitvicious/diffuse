# diffuse

The diff-engine, in Rust. A renderer-agnostic port of the pi review
extension's diff machinery: parse a git diff + the per-file contents →
mark moved lines → collapse pairs into mods (with the stretch,
reverse-stretch, reflow, and brace-anchor rules) → emit the merged token
runs that a renderer turns into visual rows.

Consumed today by a thin TypeScript renderer in the pi review extension;
tomorrow by a Rust TUI — both over the same wire contract (PLAN.md §2).

## Layout

```
src/
  lib.rs        the engine API (module tree)
  main.rs       the bin: Clap CLI, reads the diff, emits token-stream JSON
  parse.rs      parseDiff, hunks, rows, the fixture shape
  lcs.rs        tokenizeLine, lcsPairs, spansFrom, wordSpans
  similarity.rs lineSimilarity(Parts)
  moves.rs      markMoves + the block-move rule
  tail.rs       isTailBeforeClosing, fnOf
  collapse.rs   collapseHunkRows: pairing, stretch, reverse-stretch, reflow
  merge.rs      mergedRuns, delPairing, delSpansToShow, wholesalePair
  counts.rs     fileCounts
  syntax.rs     the semantic token colors (langProfile, classifyTokens)
tests/
  golden.rs     the e2e acceptance test (lib leg + bin leg)
  golden/       the frozen corpus + the golden file
```

## Build & test

```bash
cargo build
cargo test          # unit tests + the golden e2e (Phase 2)
cargo clippy -- -D warnings
cargo fmt --all -- --check
```

## Status

Phase 0 (the frame) — the skeleton is up. The engine port is Phase 1; see
PLAN.md for the full roadmap, the port map, and the known shallows.
