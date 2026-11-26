# 2D Coordinate-Aware Multi-Line LCS and Post-Segmentation Projection

- [x] **Phase 1: 2D Token Stream and Slot-Aware Gotoh LCS**
- [ ] **Phase 2: Unified `n × m` Post-Segmentation Slot Projector**
- [ ] **Phase 3: Hybrid Integration with Non-Monotone Residual Relocation**
- [ ] **Phase 4: Legacy `join("\n")` Retirement and Golden Corpus Calibration**

---

## Executive Summary

After [`Unified Reflow Lattice`](done/Unified%20Reflow%20Lattice.md) and [`Two-Sided Gap Partitioning and Single-Tape Evidence`](done/Two-Sided%20Gap%20Partitioning%20and%20Single-Tape%20Evidence.md), cluster correspondence (`lattice.rs`) and window membership (`reflow.rs`) are cleanly separated from sub-line token typing (`slot.rs`). However, window typing in [`slot.rs`](../crates/diffuse-core/src/collapse/slot.rs) still splits across four overlapping mechanisms:

1. **Slot-0-only `1 × m` expansion** (`align_expansion_1_to_m`) and **`n × 1` collapse** (`align_collapse_n_to_1`), which concatenate companion lines with `join("\n")`, run a 1D `AlignmentTape`, and reconstruct per-line runs via `split('\n')` and `combine_mod_runs_*`. Because both require the single `Mod` at slot `0`, enclosing-frame windows whose `Mod` lands on a body line at slot `k > 0` (such as `assert_eq!(` … `);` in `osc11-query-gate` and `scrollbar-grab-drag`, `stack.push(` in `rust-lifetime-char`, and `#[serde(` in `diffcoord-anchoring`) bypass multi-line alignment altogether.
2. **Post-DP tail-orphan folding** (`fold_tail_orphans`), which duplicates the same `join("\n")` + `split('\n')` + `strip_join_newlines` + `folded_runs_project` machinery for adjacent `2 × 1` and `1 × 2` pairs.
3. **Per-slot 1-to-1 alignment** for general `n × m` windows, which aligns each `Mod(i, j)` in isolation and relies on heuristic substring matching in `relocate_cross_slot_tokens` to recover monotone cross-line token reflows.

This plan introduces a **2D coordinate-aware multi-line LCS** operating directly over `(old_lines: &[&str], new_lines: &[&str])` with explicit per-token `(line, byte_range, is_leading_ws)` coordinates and slot affinity maps `(slot_of_old, slot_of_new)`, paired with a **unified `n × m` slot projector** that emits:
- **`RunKind::Ctx`** when a matched token pair stays within the same `Mod` slot (`slot_of_old[i] == slot_of_new[j]`), and
- **Balanced `RunKind::MovedSrc` / `RunKind::MovedTgt` pairs** when a matched token pair crosses slots (`slot_of_old[i] != slot_of_new[j]`).

**Governing invariants:**
- **Exact text projection by construction**: For every slot `s`, `project_old(slot_runs[s]) == old_text[s]` and `project_new(slot_runs[s]) == new_text[s]`, with zero synthetic `\n` artifacts and zero order-scrambling heuristics.
- **Closed-economy conservation by construction**: Every cross-slot 2D match emits identical text once as `MovedSrc` and once as `MovedTgt` (`Σ MovedSrc ≡ Σ MovedTgt`).
- **Monotone + non-monotone composition**: The 2D LCS handles all monotone `1 × m`, `n × 1`, and `n × m` reflows globally with same-slot affinity; the existing residual pass (`relocate_cross_slot_tokens` + `relocate_leftover_frames`) runs afterward strictly over leftover `Del`/`Add` runs to capture non-monotone transpositions (for example, `Some(` crossing `((vh ...)` in `scrollbar-grab-drag` `geometry.rs` `5 × 2`).

---

## Table of Contents

1. [Phase 1 — 2D Token Stream and Slot-Aware Gotoh LCS](#1-phase-1--2d-token-stream-and-slot-aware-gotoh-lcs)
2. [Phase 2 — Unified `n × m` Post-Segmentation Slot Projector](#2-phase-2--unified-n--m-post-segmentation-slot-projector)
3. [Phase 3 — Hybrid Integration with Non-Monotone Residual Relocation](#3-phase-3--hybrid-integration-with-non-monotone-residual-relocation)
4. [Phase 4 — Legacy `join("\n")` Retirement and Golden Corpus Calibration](#4-phase-4--legacy-joinn-retirement-and-golden-corpus-calibration)

---

## 1. Phase 1 — 2D Token Stream and Slot-Aware Gotoh LCS

**Goal:** Build a 2D coordinate-aware token stream and slot-aware Gotoh LCS over `&[&str]` line slices without concatenating strings with `\n`.

### 1.1 2D Token Representation (`crates/diffuse-core/src/lcs/multiline.rs`)

Define a compact borrowed token record tracking exact line and byte coordinates:

```rust
/// One token in a multi-line slice with exact 2D provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token2D<'a> {
    pub line: usize,
    pub byte_start: usize,
    pub byte_end: usize,
    pub text: &'a str,
    pub is_leading_ws: bool,
}

/// Slot ownership for every old and new line in a segmented window.
pub struct WindowSlotMap<'a> {
    pub slot_of_old: &'a [usize],
    pub slot_of_new: &'a [usize],
}
```

- `tokenize_lines_2d<'a>(lines: &[&'a str]) -> Vec<Token2D<'a>>`: iterates `tokenize_line(line)` per row, recording `line`, exact byte bounds via slice pointer offset, and `is_leading_ws = true` for tokens preceding the first non-whitespace token on that line.
- Line boundaries are structural (`a[k].line != a[k + 1].line`), never encoded as synthetic `"\n"` tokens. Subword contiguity (`are_contiguous_subwords`) is strictly gated on `tok_a.line == tok_b.line`, preventing identifiers at the end of line `i` and start of line `i + 1` from fusing.

### 1.2 Slot-Aware Gotoh LCS Scoring (`crates/diffuse-core/src/lcs/raw.rs`, `pairs.rs`, `multiline.rs`)

Extend the Gotoh 3-state affine gap solver (reusing `LcsScratchArena` and `unique_token_anchors`) with an optional `WindowSlotMap`:

- **Cross-slot whitespace prohibition**: Two tokens with `slot_of_old[ot.line] != slot_of_new[nt.line]` must not match when either is whitespace (`token_is_ws`). Leading indentation on `old[i]` can never bleed into `new[j]` of a different slot, while paired `Mod(i, j)` lines (`slot_of_old[i] == slot_of_new[j]`) retain full whitespace alignment.
- **Same-slot affinity bonus**: When `slot_of_old[ot.line] == slot_of_new[nt.line]`, add a small affinity reward (sized below an identifier match so genuine cross-slot content moves still win, but above ties so repeated delimiters or common tokens like `,`, `)`, and `as f64` stay anchored to their own `Mod` partner instead of drifting across slots).
- **Cross-slot distance window**: Disallow cross-slot matches whose slot distance exceeds `CROSS_SLOT_WINDOW`, preventing stray matches across distant lines in wide formations.
- **Defense-in-depth preservation**: Run the existing container-compatibility and subword-cohesion checks ([`tape/builder.rs`](../crates/diffuse-core/src/tape/builder.rs#L170-L201)) over the 2D token streams (with container tracking spanning the multi-line construct and subword adjacency restricted to same-line neighbors).
- **Token-product cap**: Return `None` immediately when `old_tokens.len() * new_tokens.len() > token_product_cap()` so the caller falls back cleanly to per-slot alignment.

### 1.3 Test-First Criteria

- Unit tests in `lcs/multiline.rs`:
  - `tokenize_lines_2d` preserves exact byte slices across ASCII, multibyte UTF-8, blank lines, and CRLF/LF inputs, and marks `is_leading_ws` independently per line.
  - Same-slot affinity keeps repeated delimiters/keywords attached to their own `Mod(i, j)` slot rather than drifting to adjacent slots.
  - Cross-slot whitespace tokens never match across distinct slots.
  - Subword adjacency never bridges across a line boundary (`old[i]` end to `old[i + 1]` start).
  - Token-product cap triggers clean `None` fallback.

---

## 2. Phase 2 — Unified `n × m` Post-Segmentation Slot Projector

**Goal:** Project matched 2D token pairs directly into per-slot run vectors with guaranteed exact text projection and closed-economy conservation.

### 2.1 Lockstep Slot Projection (`crates/diffuse-core/src/collapse/slot.rs`)

Implement `align_window_2d_tape(aligned_pairs: &[SlotPair<'_>], profile: Option<&LangProfile>) -> Option<Vec<Runs>>`:

1. **Build line tables and slot maps**:
   - Collect non-moved old lines (`old_lines`, `slot_of_old`) and non-moved new lines (`new_lines`, `slot_of_new`) from `aligned_pairs`. (Slots carrying pre-moved `Row::MovSrc` / `Row::MovTgt` rows keep their cached runs untouched.)
   - Run Phase 1's 2D LCS over `(old_lines, new_lines, WindowSlotMap, profile)` to obtain the monotone matched token pairs `pairs: Vec<(usize, usize)>`.

2. **Classify each token onto its line's per-side run stream**:
   - For every old token `oi` on old line `ol`:
     - Unmatched → `Del` on `old_runs[ol]`.
     - Matched with `ni` on new line `nl`:
       - `slot_of_old[ol] == slot_of_new[nl]` → `Ctx` (with shared anchor id `k`) on `old_runs[ol]` and `new_runs[nl]`.
       - `slot_of_old[ol] != slot_of_new[nl]` → `MovedSrc` on `old_runs[ol]` and `MovedTgt` on `new_runs[nl]`.
   - For every unmatched new token `ni` on new line `nl` → `Add` on `new_runs[nl]`.
   - **Cross-slot punctuation discipline**: Demote bare-punctuation cross-slot matches (`!text_has_salient_token`) between slot pairs `(s_old, s_new)` that neither share a salient moved token nor have a bare-punctuation line on one side (matching the existing relocation contract in [`slot.rs`](../crates/diffuse-core/src/collapse/slot.rs#L1650-L1658)), demoting both sides together to `Del` + `Add` so conservation remains exact.

3. **Per-line leading-whitespace alignment & lockstep `Mod` merge**:
   - For each `Del(ol)` slot: coalesce `old_runs[ol]`.
   - For each `Add(nl)` slot: coalesce `new_runs[nl]`.
   - For each `Mod(ol, nl)` slot:
     - Because `pairs` is monotone, the `Ctx` anchor ids on `old_runs[ol]` and `new_runs[nl]` are identical in sequence.
     - Interleave lockstep at each `Ctx` anchor: emit old `Del`/`MovedSrc` runs before the anchor, then new `Add`/`MovedTgt` runs before the anchor, then the `Ctx` anchor (applying the leading-indentation shared-suffix/prefix split from [`tape/spans.rs`](../crates/diffuse-core/src/tape/spans.rs#L47-L126) on the initial leading-whitespace `Del`/`Add` pair of every `Mod` line, not just slot `0`).
   - Verify no `Mod` slot became wholesale (`slot_runs_are_wholesale`); if any `Mod` slot loses all non-whitespace `Ctx`, return `None` to fall back cleanly to per-slot alignment.

### 2.2 Test-First Criteria

- Unit tests in `slot.rs`:
  - `1 × 4` expansion with `Mod` at slot `1` (`assert_eq!(` … `super::resolve_theme(..., false),` … `DiffuseTheme::dark()` … `);` from `osc11-query-gate` and `scrollbar-grab-drag`): single 2D tape aligns `super::resolve_theme(...)` cleanly with `, false` inline and relocates `assert_eq!(`, `DiffuseTheme::dark()`, and `);` without stray punctuation distortion.
  - `5 × 1` collapse with `Mod` at slot `2` (`#[serde(` … `rename_all = "camelCase",` … `)]` from `diffcoord-anchoring`): single 2D tape projects cleanly with exact conservation and projection.
  - General `2 × 3` and `3 × 2` monotone reflows: verify `assert_token_conservation` and `slots_project_to_text`.
  - Wholesale-`Mod` fallback: verify clean `None` return when cross-slot matches would leave a `Mod` slot without non-whitespace `Ctx`.

---

## 3. Phase 3 — Hybrid Integration with Non-Monotone Residual Relocation

**Goal:** Wire `align_window_2d_tape` into `type_slot_pairs` as Stage 1 for all multi-slot windows, composed with Stage 2 non-monotone relocation on remaining `Del`/`Add` runs.

### 3.1 Pipeline Integration (`crates/diffuse-core/src/collapse/slot.rs`)

In `type_slot_pairs`:
1. When `total_slots > 1` (covering `1 × m`, `n × 1`, and general `n × m`), call `align_window_2d_tape(&aligned_pairs, profile)`.
2. If `Some(slot_runs)` is returned, use those runs directly as the initial `slot_runs`.
3. If `None` is returned (token-product cap exceeded, wholesale-`Mod` guard triggered, or single-slot window), fall back to the existing per-slot `owned_runs_for_texts_with_profile` loop.
4. Run Stage 2 (`relocate_cross_slot_tokens` and `relocate_leftover_frames`) strictly over the remaining `Del` and `Add` runs (already skipped for `MovedSrc`/`MovedTgt` runs!). This guarantees transposed/crossed moves — such as `Some(` crossing `((vh ...)` in `scrollbar-grab-drag` `geometry.rs` `5 × 2` — continue to relocate cleanly.

### 3.2 Test-First Criteria

- Integration tests:
  - `scrollbar-grab-drag` `geometry.rs` Hunk 1 (`5 × 2` `Some(` … `)`): Stage 1 aligns `.round()` monotonically and preserves both `Mod` slots; Stage 2 relocates transposed `Some(` and `)`.
  - `scrollbar-grab-drag` `geometry/tests.rs` (`1 × 4` × 3) and `osc11-query-gate` (`1 × 4` × 2): clean inline call-body `Mod`s with balanced `MovedSrc`/`MovedTgt` across all 4 slots.
  - `assert_peer_group_invariants`, `assert_token_conservation`, `slots_project_to_text`, and `apply_oracle` pass across every hunk of all 23 golden corpora.

---

## 4. Phase 4 — Legacy `join("\n")` Retirement and Golden Corpus Calibration

**Goal:** Delete the retired `\n`-joined alignment and tail-fold code paths, calibrate affected golden corpora, and verify performance budgets.

### 4.1 Legacy Retirement (`crates/diffuse-core/src/collapse/slot.rs`)

Once Phase 3 passes all unit, integration, and corpus invariants:
- Delete `align_expansion_1_to_m`, `align_collapse_n_to_1`, `distribute_expansion_runs`, `distribute_collapse_runs`, `combine_mod_runs_expansion`, `combine_mod_runs_collapse`, `strip_join_newlines`, and `folded_runs_project`.
- Delete `fold_tail_orphans`, `fold_window`, `fold_collapse_window`, `fold_expansion_window`, `joined_alignment`, `is_admissible_fold_pair`, `is_expression_head_open`, `is_expression_tail`, and `is_strong_expression_head` (after confirming the 2D tape subsumes all `fold_tail_orphans` unit tests without regression).
- Simplify `SlotPair<'a>` by removing the third `Option<Vec<(RunKind, String)>>` precomputed-fold field.

### 4.2 Golden Review, Performance, and Verification

- Inspect every changed corpus across [`tests/golden/`](../tests/golden/) per [`tests/AGENTS.md`](../tests/AGENTS.md), verifying every diff is strictly cleaner or equivalent (zero regressions, zero `MovedSrc`/`MovedTgt` conservation violations).
- Run `cargo test -p diffuse-core --test big_hunk_perf -- --nocapture --test-threads=1` to confirm no latency regression on large hunks.
- Execute `./scripts/check.sh` cleanly with zero warnings, then archive this plan to `plan/done/`.
