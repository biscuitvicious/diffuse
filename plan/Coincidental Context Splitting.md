# Coincidental Context Splitting — Engineering Plan

- [·] Phase 1 — Failing-first tests (golden integration + unit edge cases)
- [·] Phase 2 — Early trivial-Ctx split pass (`diffuse-core`)
- [·] Phase 3 — Identical-pair normalization back to Ctx (`diffuse-core`)
- [ ] Phase 4 — Golden corpus calibration, acceptance verification & sign-off

---

## Executive Summary

The unified-diff input aligns closing delimiters (`}`, `break;`, blank lines) as context rows by position, even when they are coincidental matches rather than true anchors. Context rows are impenetrable barriers for every downstream stage: the move pre-pass clustering (`moves.rs`) and the pairing segmentation (`partition_hunk_segments`) both cut clusters at every `Row::Ctx`. A single coincidental Ctx row therefore fractures one logical change block into a pure-del cluster on one side and a pure-add cluster on the other, and the DP solver — which only ever sees one cluster at a time — can never pair the two halves.

Concrete instance (`scrollbar-grab-drag`, `app/dispatch.rs`): Ctx row `307/307` (a lone `}`) splits seg17 from seg19. Old lines 355–367 and new lines 351–365 are pure reindent (+4) plus a `vis_line` reflow split and one extra `}`, but they sit as 13 Del + 15 Add across the barrier and render as Del/Add runs. Splitting the incorrect `307/307` pairing frees both clusters to merge so the DP finds the global optimum (new 307 ↔ old 311 as an indent Mod, etc.).

The fix is an early row-level rewrite: inspect each Ctx pair with the salient-evidence half of the pair-eligibility gate (no identity fast-path — identical `}` must still fail), and rewrite failing Ctx rows to Del(old)+Add(new) in place before move detection. Removing the barrier row naturally merges the flanking clusters for all downstream stages. After re-pairing, byte-identical Mod pairs normalize back to Ctx so correctly placed braces stay quiet.

### Architectural Principles and Mathematical Invariants

1. **Barrier Monotonicity**: No pairing (move, DP seed, Tier 2 residual) may cross a `Row::Ctx`. Splitting is therefore the only way to reunite a fractured block — the rewrite must happen before the first stage that clusters on Ctx.
2. **Salient Evidence Invariant**: A Ctx row survives only if both sides carry salient tokens with shared salient context. Punctuation-only lines (`}`, `{`, `break;`, blanks) have near-zero information entropy and must never anchor a block boundary.
3. **Row-Level Merge**: Replacing one Ctx row with Del+Add in hunk order preserves positional monotonicity; flanking clusters merge without explicit cluster surgery. Partial splits leave residual Ctx barriers in place.
4. **Identity Round-Trip**: Splitting is lossless — truly correct identical pairs re-pair in the DP and normalize back to Ctx. The pass can only move pairings, never drop lines (`apply_oracle` must hold).

---

## Table of Contents

1. [Phase 1 — Failing-first tests (golden integration + unit edge cases)](#1-phase-1--failing-first-tests-golden-integration--unit-edge-cases)
2. [Phase 2 — Early trivial-Ctx split pass (`diffuse-core`)](#2-phase-2--early-trivial-ctx-split-pass-diffuse-core)
3. [Phase 3 — Identical-pair normalization back to Ctx (`diffuse-core`)](#3-phase-3--identical-pair-normalization-back-to-ctx-diffuse-core)
4. [Phase 4 — Golden corpus calibration, acceptance verification & sign-off](#4-phase-4--golden-corpus-calibration-acceptance-verification--sign-off)

---

## 1. Phase 1 — Failing-first tests (golden integration + unit edge cases)

### Problem

No test currently pins the fractured-block behavior: Ctx `307/307` survives, old 355–367 / new 351–365 render as Del/Add runs, and nothing fails.

### Implementation Steps

1. **Golden-driven integration test** (`crates/diffuse-core/tests/` or `crates/diffuse-core/src/cluster/tests.rs`):
   - Load the `scrollbar-grab-drag` `dispatch.rs` hunk (old/new snapshots from `tests/golden/scrollbar-grab-drag/files/`), run the full pipeline (`collapse_hunk_rows` or `process_single_diff_file`).
   - Assert old 355–367 and new 351–365 no longer render as pure Del/Add runs (expect Mod pairings for the reindent block; exact row-kind vector pinned).
   - If the golden submodule is uninitialized, exit early and pass silently per `AGENTS.md` (`let Some(out) = load_golden(...) else { return; };`).
   - Confirm the test FAILS before implementation (Del/Add runs observed).
2. **Complementary unit tests** (new module `crates/diffuse-core/src/ctx_split.rs`, `#[cfg(test)]`):
   - Lone `}`/`{`/`break;`/blank Ctx splits; salient Ctx (`let prev_row = …`) survives.
   - Identical salient lines survive; identical braces split.
   - Hunk-edge split attaches to the single adjacent cluster; full-split run merges two clusters; partial split leaves residual Ctx.
   - Runs in `< 0.5s`.
3. **Verification gate**: `cargo test -p diffuse-core` shows the new tests failing for the right reason (barrier intact).

## 2. Phase 2 — Early trivial-Ctx split pass (`diffuse-core`)

### Problem

`partition_hunk_segments` (`crates/diffuse-core/src/cluster.rs`) and the move pre-pass (`crates/diffuse-core/src/moves.rs`) both treat every `Row::Ctx` as a hard cluster boundary, so coincidental brace alignments permanently strand single-sided clusters.

### Implementation Steps

1. **New module** (`crates/diffuse-core/src/ctx_split.rs`):
   - `pub fn should_split_ctx(old_text: &str, new_text: &str, profile: Option<&LangProfile>) -> bool` — split unless both sides carry salient tokens with shared salient context. Reuse `crate::lcs::line_has_salient_token`; deliberately no `del == add → keep` fast-path (contrast `is_admissible_mod_pair` in `crates/diffuse-core/src/cluster/dp/solver.rs:282`). Blank/whitespace-only lines always split (they carry no evidence either way and must not anchor).
   - `pub fn split_trivial_ctx_rows(rows: Vec<Row>, profile: Option<&LangProfile>) -> Vec<Row>` — map each `Row::Ctx` failing the gate to `Row::Del(old_line) + Row::Add(new_line)` in place, preserving order. Pure function over the row vector; no cluster surgery needed.
2. **Wire into the pipeline early** (inside `collapse_hunk_rows`, `crates/diffuse-core/src/collapse/mod.rs`, immediately after `pass_shift_hunk`, before Tier 1 DP):
   - Placement rationale: the move pre-pass (`mark_cross_cluster_moves_with_profile` in `crates/diffuse-core/src/pipeline/file.rs`) clusters on raw-hunk Ctx rows; splitting before it would merge clusters and trip its same-salient-cluster refusal, breaking relocation detection. Splitting after the shift but before any pairing decision preserves move behavior while freeing DP, Tier 2, and reflow row-clusters.
   - Register the module in `crates/diffuse-core/src/lib.rs`.
3. **Invariants**: order-preserving (Del(old_n) immediately followed by Add(new_m)); line numbers untouched; total line coverage unchanged (`apply_oracle` holds trivially — same lines, different row kinds).

## 3. Phase 3 — Identical-pair normalization back to Ctx (`diffuse-core`)

### Problem

After the merge, the DP Mov branch re-pairs identical texts (e.g. old 337 ↔ new 365, both `}`) as Mod/Mov (trimmed equality, large score). The walk in `crates/diffuse-core/src/cluster/pairing/mod.rs:193` accepts them (no triviality check; `is_fragment_pair_rejected` in `crates/diffuse-core/src/cluster/pairing/filter.rs` is fn-scope only), and assembly has no identical-Mod→Ctx normalization — today's quiet Ctx would become Mod-with-no-visible-change or Mov noise.

### Implementation Steps

1. **Post-walk normalization** (`pass_normalize_identical_mods` in `crates/diffuse-core/src/collapse/assemble.rs`, called in `collapse_hunk_rows` after `pass_sort_ordered_walk`, before the DBC gates):
   - Convert a Mod with byte-identical old/new text and no group attachments (`group().is_none()` — anchors and aux hosts untouched) to `Row::Ctx`.
   - Exclude collapse/expand block anchors (news keyed from `p1.collapse_blocks`/`p1.expand_blocks`): a reflow-joined anchor del_text may byte-equal the add text while absorbed member lines still need the anchor to render (opt-2 `win` + `.iter()` anchor; converting it dropped old 372 from the stream).
   - Keep each remaining conversion only when the converted stream still satisfies every collapsed-row invariant (`check_context_old_line_monotonicity`, `check_no_orphaned_continuation_dels`, `check_del_add_topological_order`, `assert_collapsed_reflow_invariants`) — verified against the actual final order with a global repair loop (revert most-recent-first), so crossed pairs stay Mod by construction and the DBC cannot trip.
   - Design note: an in-assembly conversion was tried first and reverted — the walk sorts Ctx rows as anchors, so assembly-time order prediction is unreliable (a converted Ctx tripped the monotonicity DBC on crossed input: theme-light-toggle `event/tests.rs`, leftover Del 918 before identical pair 917↔923).
2. **Unit tests** (`crates/diffuse-core/src/collapse/tests.rs`): faithful identical Mod → Ctx; crossed pair (leftover Del on the wrong side) → stays Mod; anchor Mod untouched.
3. **Walk del-anchoring over reflow Ctx** (`pass_sort_ordered_walk` in `crates/diffuse-core/src/collapse/walk.rs`): stage reflow-synthesized Ctx rows with `out` rows before `anchor_of` computation, so leftover dels anchor to synthesized context exactly as to hunk context. (Without this, dels anchor to earlier rows and sort before their own context — DBC `check_context_old_line_monotonicity` failure on theme-light-toggle `event/tests.rs`, Del 918/919/920 vs reflow Ctx 917↔923.) Tested by `walk::tests::test_reflow_ctx_anchors_leftover_dels`.
4. **Mov proximity parity** (`DpOp::Mov` branch in `crates/diffuse-core/src/cluster/dp/solver.rs`): the 1-to-1 Mod path awards `boundary_proximity_bonus` (500) × close/open proximity, so a 1-char-overlap fuzzy Mod (`};` vs `}`) outbid verbatim Mov identity (`}` vs `}`) once merged clusters let them compete — stranding true identical partners (theme-scrollbar-palette `theme.rs`: old 85/86 stranded, `};` 113/122 stole new 84/85). Mov now earns the same proximity bonuses; verbatim identity outranks fuzzy boundary agreement, per the repo's evidence hierarchy. Tested by `cluster::tests::dp_verbatim_identity_outbids_fuzzy_close_delimiter` (verified failing without the fix).

## 4. Phase 4 — Golden corpus calibration, acceptance verification & sign-off

### Problem

The split pass fires wherever brace-Ctx fractures a block, so widespread golden movement is expected across corpora. Per `tests/AGENTS.md`, fixtures change only on deliberate improvement with dual review.

### Implementation Steps

1. Run `scripts/check.sh` (applies `rustfmt`, `clippy --fix -D warnings`, all tests). All green, `< 3s` on warm builds.
2. Regenerate goldens (`GEN_GOLDEN=1 cargo test --test golden` or scoped `UPDATE_WIRE`/`UPDATE_RENDER`/`UPDATE_MARKDOWN`), then agent-review every changed region per `tests/AGENTS.md`: each hunk attributable to barrier removal, common runs not shrinking, no new struck/fill fragmentation, `apply_oracle` green on all corpora.
3. Present wire/render before-after for human visual sign-off; flag anything questionable and pause — never rationalize a suspicious diff.
4. Move this plan to `plan/done/` once landed; `scripts/check.sh` reinstalls the dev binary and syncs the extension.
