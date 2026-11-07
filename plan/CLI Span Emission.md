# CLI Span Emission

- [x] Phase 1 — Span IR retains exact SGR ids; shared token emitter
- [·] Phase 2 — `theme.write_token` family renders from spans
- [ ] Phase 2 — `theme.write_token` family renders from spans
- [x] Phase 3 — Row segment and gutter prefix render from spans
- [x] Phase 4 — Raw rows render from spans
- [x] Phase 5 — File banner and hunk divider render from spans
- [x] Phase 6 — Note callouts, note segments, and Markdown lines render from spans
- [x] Phase 7 — Verification gate and signoff (`check.sh` exit 0; 278 render + 364 TUI + 42 golden tests pass; zero files changed under `tests/golden/`) (goldens byte-identical)

## Executive Summary

The interactive grid now renders from the renderer-agnostic `StyledSpan`
IR while the one-shot CLI path (`render_pipeline_output`,
`render_raw_pipeline_output`, Markdown emitters) still hand-assembles ANSI
escapes. This plan reimplements the CLI ANSI builders on top of the span
builders, so style *decisions* live once (in `diffuse-render` span code)
while each CLI builder keeps its exact emission skeleton. The acceptance
criterion is strict: `tests/golden/` outputs must remain byte-identical —
post-optimization, which is what the goldens compare
(`render_file_diff_with_width` and `render_raw_file_diff_with_width` apply
`optimize_ansi` before returning).

The optimizer is a pure deletion machine (drops only provably no-op
sequences), so the design rule is: the new emission may drop redundant
sequences, but must never add, remove, or reorder real state transitions.
Pre-optimization unit tests that pin provably-redundant quirk bytes
(mid-prefix `\x1b[23m`, redundant `\x1b[1m`, trailing `\x1b[49m`/`\x1b[22m`)
move to post-optimization comparison; the optimizer's deletion rule is the
certificate that terminal effect is unchanged.

## 1. [Phase 1 — Span IR retains exact SGR ids; shared token emitter](#1-phase-1--span-ir-retains-exact-sgr-ids-shared-token-emitter)

- `crates/diffuse-render/src/span.rs`: `StyledSpan` gains
  `sgrs: Option<Box<[u32]>>` preserving the exact source SGR id list
  (`None` = flag-derived). `with_sgr(u32)` records a single id;
  `with_sgrs(&[u32])` records a stacked list. Flags are still set from the
  ids so the Ratatui converter is unaffected.
- `crates/diffuse-render/src/theme.rs`: `token_span_stacked(color, text,
  kind, tok_kind, sgrs)` twin of `token_span` for the stacked path.
- New `write_span_token(out, span)` in `span.rs`: byte-pattern twin of
  `write_token` sourced from the span — SGR on via `sgr_on_str` (single),
  combined `\x1b[a;b;…m` (stacked), or raw `\x1b[{s}m` fallback; then fg,
  text, `FG_RESET` iff fg emitted, then off codes in reverse (`sgr_off` per
  id, or flag-derived `22/23/24/29` — strike uses plain `\x1b[29m`, never
  the id-9 dim+strike spelling). No bg: callers emit span bg around it, as
  today.
- Tests: `span/tests.rs` (or `span.rs` unit tests) pin on/off spellings for
  ids 1/2/3/4/9/14/unknown, stacked combos, flag-derived spans, and
  fg-absent spans (no `FG_RESET`).

## 2. [Phase 2 — `theme.write_token` family renders from spans](#2-phase-2--themewrite_token-family-renders-from-spans)

- `crates/diffuse-render/src/theme.rs`: `write_token` builds
  `token_span(kind, tok_kind, sgr)` and calls `write_span_token`;
  `write_token_stacked` builds `token_span_stacked` the same way. Same
  helpers, same order — pre-optimization byte-exact by construction.
- `render_token` unchanged (delegates). Existing `theme/tests.rs`
  expectations must pass unmodified.

## 3. [Phase 3 — Row segment and gutter prefix render from spans](#3-phase-3--row-segment-and-gutter-prefix-render-from-spans)

- `crates/diffuse-render/src/row/gutter.rs`: new
  `write_gutter_prefix(out, spans, mid_italic_off: bool)` emitting
  `{fg}text` chains with the trailing `{FG_RESET}\x1b[23m` and the
  positional mid `\x1b[23m` after the pad span for deletion-kind prefixes
  (provable no-ops post-optimization; positions mirror the old
  `format!` strings).
- `crates/diffuse-render/src/row/segment.rs`: `write_row_segment` takes
  `prefix: &[StyledSpan]`, builds `row_segment_spans`, and emits with the
  old skeleton: `lead_bg`, gutter prefix, per body span
  `{bg} + write_span_token + {bg}` (del-arm spans carry
  `sgrs: None` + strikethrough so the token reads `\x1b[9m…\x1b[39m\x1b[29m`
  exactly), then `trail_bg`. Uniform trailing bg differs pre-opt where the
  old code omitted it (provable no-op; optimizer drops it).
- `crates/diffuse-render/src/diff/row.rs` (`render_row_line`,
  `render_source_rows`): build `styled_prefix_spans` /
  `continuation_prefix_spans` instead of ANSI strings; padding logic
  unchanged (still operates on the serialized string via `strip_ansi`).
- `render_row_segment` wraps the new writer. `row/tests.rs` expectations
  that pin quirk bytes move to post-optimization comparison; all others
  must pass unmodified.

## 4. [Phase 4 — Raw rows render from spans](#4-phase-4--raw-rows-render-from-spans)

- `crates/diffuse-render/src/row/raw.rs`: `write_raw_row` takes prefix
  spans, builds `raw_row_spans`, and keeps its skeleton (`{bg}`, prefix,
  `{fg}`, body, bespoke trailing fill with `\x1b[29m` close and no
  `FG_RESET` on deletion rows, `{bg-or-49}`, `{39}{22}` framing).
- `diff/render.rs` raw path builds `styled_prefix_spans`. `row/tests.rs`
  raw expectations move to post-optimization where quirk bytes were
  pinned.

## 5. [Phase 5 — File banner and hunk divider render from spans](#5-phase-5--file-banner-and-hunk-divider-render-from-spans)

- `crates/diffuse-render/src/diff/header.rs`:
  `write_file_header_bar_with_width` iterates `banner_spans(banner, None)`
  with the old role-equivalent state machine: bold+base-fg spans emit bare
  text; other spans emit `{fg}text` with the old closes
  (`\x1b[39m\x1b[1m{base}` for path/count, `\x1b[24m{base}` for the rename
  underline — CLI never passes a rename range, so underline spans cannot
  occur, but the emitter preserves them if present). Redundant `\x1b[1m`
  on count spans is kept so output stays pre-opt byte-exact.
- `crates/diffuse-render/src/diff/hunk.rs`: new `hunk_divider_spans`
  (single bold span with `bg_base` fallback for `Primary`, mirroring the
  pager's explicit-background rule — CLI and TUI differ here by design);
  `render_hunk_divider` serializes it with the ruler fill and trailing
  `\x1b[0m`, pre-opt byte-exact.
- `diff/tests.rs` banner/divider expectations must pass unmodified.

## 6. [Phase 6 — Note callouts, note segments, and Markdown lines render from spans](#6-phase-6--note-callouts-note-segments-and-markdown-lines-render-from-spans)

- `crates/diffuse-render/src/notes.rs`: `render_note_callout_lines`
  serializes `render_note_callout_spans` per line (prefix pieces chained
  `{bg}{fg}` with single trailing `{FG_RESET}`, markdown runs via the
  migrated `write_markdown_line`); `render_source_rows` already migrated
  via Phase 3; last-line padding logic unchanged.
- `crates/diffuse-render/src/row/note.rs`: new `note_segment_spans`
  (prefix spans + sliced `token_span` runs); `write_note_segment` keeps
  its tab-expansion pre-step and skeleton with `write_span_token` per run.
- `crates/diffuse-render/src/markdown/terminal/mod.rs`:
  `write_markdown_line` routes runs through `token_span` /
  `token_span_stacked` + `write_span_token`, preserving the stacked-codes
  array logic exactly.
- `write_markdown_line_highlighted`, `markdown/diff.rs`, and the notes CLI
  inherit the migration via `write_token`. Markdown/notes unit tests move
  to post-optimization only where quirk bytes were pinned.

## 7. [Phase 7 — Verification gate and signoff](#7-phase-7--verification-gate-and-signoff)

- `./scripts/check.sh` fully green (applies `rustfmt`, `clippy --fix`,
  all tests).
- Golden suite byte-identical: no file under `tests/golden/` may change.
  Any golden diff is a defect in the new emission (or newly-exposed span
  color drift — fix the spans, never the fixtures, per `tests/AGENTS.md`).
- TUI behavior re-verified (its tests exercise `render_frame`; no TUI
  source changes expected, but span changes from drift fixes affect it).
- Present the change summary with the quirk-decision log; pause before
  committing (no commit without explicit instruction).
