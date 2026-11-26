# Two-Sided Gap Partitioning and Single-Tape Evidence

- [x] **Phase 1: Two-Sided Inter-`Diag` Gap Queues and Multi-`Diag` Enclosing Frames**
- [ ] **Phase 2: Single-Tape Evidence Scoring Optimization**

---

## Executive Summary

[`Unified Reflow Lattice`](done/Unified%20Reflow%20Lattice.md) replaced macro cluster factorization with a single monotone path solver ([`lattice.rs`](../crates/diffuse-core/src/collapse/lattice.rs)) and typed-Dyck path segmentation ([`reflow.rs`](../crates/diffuse-core/src/collapse/reflow.rs)). Two bounded follow-ups remain from that rollout:

1. **Structural framing across mixed skip gaps**: `PathSegmenter` currently slices contiguous ranges of the 1D `ClusterPath` and limits `try_enclosing_frame` to a single interior `Diag` step. When an `old`-only or `new`-only outer frame (for example `Some(` … `)` in `scrollbar-grab-drag` `geometry.rs`, `old 231..=235` → `new 243..=244`) wraps multiple `Diag` steps with unrelated opposite-side skips interleaved in the gap, the outer frame cannot attach without either pulling those unrelated skips into a hull or splitting across separate windows.
2. **Evidence scoring allocation overhead**: `pair_evidence` in [`evidence.rs`](../crates/diffuse-core/src/collapse/evidence.rs) and downstream slot typing rebuild sub-line alignment tapes across overlapping checks. Unifying tape construction (while auditing the two golden corpora where span alignment shifts) yields an estimated ~3× speedup per scored pair on large clusters.

---

## Table of Contents

1. [Phase 1 — Two-Sided Inter-`Diag` Gap Queues and Multi-`Diag` Enclosing Frames](#1-phase-1--two-sided-inter-diag-gap-queues-and-multi-diag-enclosing-frames)
2. [Phase 2 — Single-Tape Evidence Scoring Optimization](#2-phase-2--single-tape-evidence-scoring-optimization)

---

## 1. Phase 1 — Two-Sided Inter-`Diag` Gap Queues and Multi-`Diag` Enclosing Frames

**Goal:** Recover hull-free enclosing-frame relocations (such as `Some(` / `)` and cross-row `as usize`) across intervening pure additions or deletions without reintroducing hulls.

### 1.1 Two-Sided Gap Partitioning (`crates/diffuse-core/src/collapse/reflow.rs`)

- Model each mixed inter-`Diag` gap as two independent ordered queues (`old_skips`, `new_skips`) rather than a single flat slice of 1D `ClusterPath` steps.
- Allow `PathSegmenter` to attach skips from either side to an adjacent correspondence while emitting the opposite side's unclaimed skips outside the window (topologically valid because neither side crosses a `Diag`).
- Remove sensitivity to `lattice.rs`'s 1D tie-break ordering on mixed `DropOld`/`DropNew` gaps.

### 1.2 Multi-`Diag` Enclosing Frame Detection (`crates/diffuse-core/src/collapse/reflow.rs` + `peer.rs`)

- Extend `PathSegmenter::try_enclosing_frame` to detect an `old`-only or `new`-only balanced outer delimiter pair spanning multiple interior `Diag` steps.
- Emit the resulting window with unclaimed opposite-side skips ordered ahead/after the formation so `relocate_cross_slot_tokens` sees the enclosing frame and interior rows in one closed-economy window.

### 1.3 Test-First Criteria

- Unit test in `reflow.rs` reproducing `geometry.rs` `old 231..=235` × `new 238..=244`: `new 238..=242` stay plain `Add` rows while `old 231..=235` × `new 243..=244` form one hull-free `5×2` window.
- Full invariant verification (`assert_peer_group_invariants`, `apply_oracle`) and golden review per [`tests/AGENTS.md`](../tests/AGENTS.md).

---

## 2. Phase 2 — Single-Tape Evidence Scoring Optimization

**Goal:** Eliminate redundant sub-line alignment tape builds per candidate pair while preserving or improving rendered spans.

### 2.1 Tape Reuse (`crates/diffuse-core/src/collapse/evidence.rs`, `slot.rs`)

- Audit remaining duplicate calls to `word_spans_and_runs_with_profile` / `word_spans` / `merged_runs_from_texts` across `evidence.rs` and `slot.rs`.
- Consolidate onto one profiled tape per scored pair and inspect the two corpora identified during Unified Reflow Lattice §3.8 where rendered spans shift.

### 2.2 Test-First Criteria

- Benchmark on large-hunk perf tests (`crates/diffuse-core/tests/big_hunk_perf.rs`) confirming measurable latency reduction.
- Golden review and `./scripts/check.sh` clean with zero warnings.
