# Ratatui Direct Rendering

- [x] Phase 1 — Shared span IR and style conversions
- [x] Phase 2 — Row, prefix, and raw-row span builders plus viewport grid lines
- [x] Phase 3 — Banner, divider, and note span builders
- [x] Phase 4 — Finder, highlighter, dead-code removal, and final signoff

## Executive Summary

The interactive grid viewport renders each row to an ANSI string in
`diffuse-render`, then parses that ANSI back into Ratatui `Line`s in
`diffuse-tui` (`render_diff_viewport_lines` → `optimize_ansi` →
`ansi_to_ratatui_line`). This plan replaces that lossy round-trip with a
single renderer-agnostic span model owned by `diffuse-render`: builders
emit ordered `StyledSpan`s (plain text plus foreground, background, and
modifier flags), the CLI batch path serializes them to ANSI exactly as
today, and the TUI converts them directly to Ratatui `Span`s. The wire
contract and golden CLI renders stay byte-identical.

## 1. [Phase 1 — Shared span IR and style conversions](#1-phase-1--shared-span-ir-and-style-conversions)

New `crates/diffuse-render/src/span.rs` defines `StyledSpan` (text plus
`Option<yansi::Color>` foreground/background and bold, dim, italic,
underline, and strikethrough flags) with a `yansi::Style` projection and
an ANSI serializer. New `crates/diffuse-tui/src/view/span.rs` converts a
`StyledSpan` slice to Ratatui `Span`s (extending the partial `to_style`
helper, which currently drops modifiers) and provides visual-width
slicing, truncation, and padding over span slices. SGR ids 1, 2, 3, 4,
9, and 14 map to modifier sets in one shared function matching
`ansi::sgr_on`. Tests: span serialization of every modifier combination,
SGR mapping parity with `sgr_on`/`sgr_off`, Ratatui conversion of every
flag, and slice/pad width invariants (wide graphemes, tabs).

## 2. [Phase 2 — Row, prefix, and raw-row span builders plus viewport grid lines](#2-phase-2--row-prefix-and-raw-row-span-builders-plus-viewport-grid-lines)

Add `row_segment_spans`, `styled_prefix_spans`, and `raw_row_spans` in
`diffuse-render` mirroring the ANSI builders branch-for-branch over
shared background/foreground resolution helpers extracted from
`row/segment.rs`, `row/gutter.rs`, and `row/raw.rs` (no behavior change
to the ANSI path). `render_diff_viewport_lines` gains a span-native
sibling returning `Vec<Line>`; `ratatui.rs` drops `clamp_or_pad_ansi`,
`optimize_ansi`, and `ansi_to_ratatui_line` from the frame path. Tests:
text parity (`strip_ansi` of the ANSI line equals concatenated span
text), style spot checks (struck del runs, word-fill backgrounds,
cursor/band selection), and updated viewport callers.

## 3. [Phase 3 — Banner, divider, and note span builders](#3-phase-3--banner-divider-and-note-span-builders)

Extend the existing canonical banner model (`FileHeaderBanner`) with a
span projection covering path brightening, count colors, and the rename
underline; add segment-aware (`seg_start`) Ratatui line builders for
file headers, hunk dividers, and bottom-gap dividers. Add span-native
note callout lines (stripe prefixes plus Markdown token spans) and a
span-aware focus highlight replacing ANSI prefix matching in
`style_focused_note_line`. Tests: banner/divider line parity with the
ANSI segments (text plus key styles), wrapped-segment slicing,
callout/underline spot checks.

## 4. [Phase 4 — Finder, highlighter, dead-code removal, and final signoff](#4-phase-4--finder-highlighter-dead-code-removal-and-final-signoff)

Migrate the finder preview (`finder.rs`) and the Markdown highlighter
(`editor/highlight.rs`) to the span builders, delete `ansi_to_ratatui_line`
and any newly dead ANSI-only helpers (`clamp_or_pad_ansi`,
`visible_width` if unreferenced), and run the full gate (`scripts/check.sh`)
plus the golden suite (CLI outputs must be byte-identical). Agent review
of any test-only churn; human visual signoff of the interactive grid
before commit.
