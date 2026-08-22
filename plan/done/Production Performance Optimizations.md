# Production Performance Optimizations

## Global TODO
- [x] [Phase 1 — Manual Zero-Allocation Line Tokenizer](#1-phase-1--manual-zero-allocation-line-tokenizer)
  - [x] Implement fast byte/char classification table for whitespace, operators, and delimiters
  - [x] Replace `regex::Regex::find_iter` in `diffuse_core::lcs::tokenize_line` with manual scanner
  - [x] Add parity unit tests asserting 100% token sequence and span equivalence with regex baseline
- [·] [Phase 2 — Index-Based `FlatRow` View Model](#2-phase-2--index-based-flatrow-view-model)
  - [ ] Refactor `FlatRow` to store coordinate indices `(file_idx: u32, hunk_idx: u16, row_idx: u16)`
  - [ ] Replace deep-cloning in `AppState::build_visual_flat` with zero-copy row indexing
  - [ ] Verify instant file folding and streaming UI updates in `diffuse-tui`
- [ ] [Phase 3 — Precomputed Tokens & DP Scratch Arena](#3-phase-3--precomputed-tokens--dp-scratch-arena)
  - [ ] Pre-tokenize lines once per cluster and pass token references to `line_similarity_parts`
  - [ ] Introduce reusable thread-local scratch arena for `solve_cluster_with` 2D DP matrix
  - [ ] Verify cluster projection invariant on all golden corpora
- [ ] [Phase 4 — ASCII Fast-Path UTF-8 Slicing](#4-phase-4--ascii-fast-path-utf-8-slicing)
  - [ ] Add $O(1)$ ASCII fast-path in `char_slice` and `char_len`
  - [ ] Optimize Unicode non-ASCII fallback to single linear pass
  - [ ] Verify subword diff spans, whole-line mods, and markdown emphasis across all test fixtures
- [ ] [Phase 5 — Zero-Allocation Line Gutter & ANSI Segment Buffers](#5-phase-5--zero-allocation-line-gutter--ansi-segment-buffers)
  - [ ] Replace `pad4` heap `String` allocation with fixed stack array `[u8; 4]`
  - [ ] Refactor `display_text` and `render_row_segment` to write directly into target string buffer
  - [ ] Verify clean ANSI reset invariants and render goldens

---

## Executive Summary

A comprehensive performance audit across production crates revealed five high-impact optimization opportunities in the hot execution paths of diff computation, terminal styling, and TUI navigation. 

Currently, diff processing incurs non-trivial allocation overhead: `tokenize_line` invokes dynamic regular expressions on every line of every hunk, `line_similarity_parts` repeatedly re-tokenizes lines across $O(M \times N)$ cluster pairings, `solve_cluster_with` creates 11 heap vectors per cluster, `char_slice` performs three linear UTF-8 iterations per slice, and `AppState::build_visual_flat` deep-clones entire `OutputRow` trees on every UI fold or streaming chunk.

This plan systematically eliminates regex overhead, memory allocations, and redundant string cloning across `diffuse-core`, `diffuse-tui`, and `diffuse-render`.

---

## Table of Contents
1. [Phase 1 — Manual Zero-Allocation Line Tokenizer](#1-phase-1--manual-zero-allocation-line-tokenizer)
2. [Phase 2 — Index-Based FlatRow View Model](#2-phase-2--index-based-flatrow-view-model)
3. [Phase 3 — Precomputed Tokens & DP Scratch Arena](#3-phase-3--precomputed-tokens--dp-scratch-arena)
4. [Phase 4 — ASCII Fast-Path UTF-8 Slicing](#4-phase-4--ascii-fast-path-utf-8-slicing)
5. [Phase 5 — Zero-Allocation Line Gutter & ANSI Segment Buffers](#5-phase-5--zero-allocation-line-gutter--ansi-segment-buffers)
6. [Verification Gates & Invariants](#6-verification-gates--invariants)

---

## 1. Phase 1 — Manual Zero-Allocation Line Tokenizer

### Problem
[`diffuse-core::lcs::tokenize_line`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/lcs/mod.rs#L50-L78) uses `regex::Regex::find_iter` with the pattern:
```text
([\p{White_Space}\u200b]+|[<>!=+\-*\/\\|&^%~?:._]+|[()\[\]{};,@#`"])
```
This regex NFA is executed on every single added and deleted line across all diffs. In Phase 4 of `diffuse-syntax`, replacing a similar regex with a hand-rolled scanner yielded a **50× speedup**.

### Implementation Steps
1. In `crates/diffuse-core/src/lcs/mod.rs`, implement `tokenize_line_fast(&str) -> Vec<&str>` using direct character inspection:
   - Whitespace run: `c.is_whitespace() || c == '\u{200b}'`
   - Operator run: `<>!=+\-*/\\|&^%~?:._`
   - Single-character delimiter: `()[]{};,@#"`
   - Word token: everything else (identifiers, literals).
2. Ensure the manual scanner returns exact sub-slices `&text[start..end]` identical to the regex output.
3. Add a unit test verifying that `tokenize_line_fast` matches `tokenize_line_regex` byte-for-byte on thousands of random and realistic code lines.
4. Replace `tokenize_line` body with the zero-allocation scanner.

---

## 2. Phase 2 — Index-Based `FlatRow` View Model

### Problem
In [`diffuse-tui::app::AppState::build_visual_flat`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/app.rs#L200-L228), every row in every file is cloned (`row: r.clone()`) into `Vec<FlatRow>`. Each `OutputRow` carries heap allocations (`text: String`, `runs: Vec<OutputRun>`, `spans: Spans`). In a 1,000-file changeset, every expand/collapse keypress or streaming arrival clones tens of thousands of heap objects.

### Implementation Steps
1. In `crates/diffuse-tui/src/app.rs`, redefine `FlatRow` as an index pointer:
   ```rust
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   pub struct FlatRow {
       pub file_idx: u32,
       pub hunk_idx: u16,
       pub row_idx: u16,
       pub is_file_header: bool,
   }
   ```
2. Add helper method `AppState::get_row(&self, flat_row: &FlatRow) -> Option<&OutputRow>` that indexes directly into `self.output.files[flat_row.file_idx]`.
3. Update `build_visual_flat` to emit lightweight index structs without cloning `OutputRow`s.
4. Update TUI view rendering and selection handlers to borrow rows via `get_row`.

---

## 3. Phase 3 — Precomputed Tokens & DP Scratch Arena

### Problem
1. [`similarity::line_similarity_parts`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/similarity/mod.rs#L31-L64) tokenizes lines inside nested loops ($O(M \times N)$).
2. [`cluster::dp::solve_cluster_with`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/cluster/dp.rs#L340-L485) allocates 11 independent vectors per cluster (`old`, `new`, `old_comment`, `new_comment`, `old_kind`, `new_kind`, `old_close_dist`, `new_close_dist`, `old_open_dist`, `new_open_dist`, and `dp: Vec<Cell>`).

### Implementation Steps
1. In `crates/diffuse-core/src/similarity/mod.rs`, accept precomputed token slices:
   ```rust
   pub fn line_similarity_precomputed(ot: &[&str], nt: &[&str], a_raw: &str, b_raw: &str) -> SimilarityParts
   ```
2. In `crates/diffuse-core/src/cluster/dp.rs`, introduce `DpScratchArena`:
   - Maintain pre-allocated buffers for `dp: Vec<Cell>` and distance tables.
   - Use thread-local scratch or pass `&mut DpScratchArena` into `solve_cluster_with`.
   - Clear buffers without deallocating capacity across consecutive clusters.

---

## 4. Phase 4 — ASCII Fast-Path UTF-8 Slicing

### Problem
[`diffuse-core::lcs::char_slice`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/lcs/mod.rs#L25-L40) executes 3 full linear scans over strings:
1. `s.chars().count()`
2. `s.char_indices().nth(start)`
3. `s.char_indices().nth(end - 1)`

### Implementation Steps
1. Add an ASCII fast-path:
   ```rust
   if s.is_ascii() {
       let len = s.len();
       let start = start.min(len);
       let end = end.min(len).max(start);
       return &s[start..end];
   }
   ```
2. For non-ASCII UTF-8 strings, replace the 3 sequential scans with a single pass that records both `start_byte` and `end_byte` simultaneously.
3. In `char_len`, check `if s.is_ascii() { s.len() } else { s.chars().count() }`.

---

## 5. Phase 5 — Zero-Allocation Line Gutter & ANSI Segment Buffers

### Problem
[`diffuse-render::row::pad4`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/row.rs#L36-L41) allocates a heap `String` via `format!("{n:>4}")` on every line gutter. [`display_text`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/row.rs#L120-L145) concatenates run text into a new `String` on every viewport row.

### Implementation Steps
1. Replace `pad4` with a zero-allocation stack formatter:
   ```rust
   pub fn pad4_bytes(num: Option<u32>, out: &mut [u8; 4]) -> &str
   ```
2. In `render_row_segment`, write directly into an output `&mut String` buffer instead of creating intermediate formatted strings.
3. Add tests verifying exact byte parity with the existing ANSI render outputs.

---

## 6. Verification Gates & Invariants

1. **Test Latency Invariant**: Every test must run in `< 0.5s`, every test binary in `< 1.5s`, and `./scripts/check.sh` in `< 3s` (warm cache).
2. **Wire Contract Invariant**: Wire JSON outputs across all golden corpora in `tests/golden/` must remain byte-identical.
3. **Render Golden Invariant**: All ANSI terminal renders (`.render.txt`) must remain 100% identical.
4. **Mechanical Check Gate**: Nightly format, clippy with zero warnings, all cargo tests, and TypeScript smoke suite must pass.
