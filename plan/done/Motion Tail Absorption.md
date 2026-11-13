# Motion Tail Absorption

## Global checklist

- [x] Phase 1 — Uniform-Δ definition, tail-seeding rule, blank-line exemption, and a synthetic failing fixture
- [x] Phase 2 — Absorption pass integrated after main-pass moves, with tail-run expansion and roll-back (implemented, golden-reviewed, gate green; committed)
- [x] Phase 3 — Context re-pairing rule, orphan handling, and hunk/context budget behavior after tail absorption
- [x] Phase 4 — Confirm no UI change (absorbed rows reuse existing Mov affordances)
- [x] Phase 5 — Golden corpus review, agent scan, and sign-off

## Executive summary

A unified diff's initial `Ctx` assignment is blind to indentation-displaced but identical code runs. When a block of code is re-indented and moved within a file (or between hunks in the expanded picture), the diff often aligns the block's trailing lines — braces, blanks, and the non-edited continuation after it — as `Ctx`/`Add` leftovers, even though they are actually *the moved block's own tail* that belongs structurally to the relocation.

This plan extends motion detection so that a locked move block can absorb its trailing code tail when that tail is a rigidly translated continuation of the same text — whole block shifted by a uniform indent offset — stopping at the first genuinely changed line. The criterion is **uniform-Δ match after normalizing the new side by the block's indent delta**, not per-line "is this a closer" heuristics or saliency filters.

The motivating fixture is the `hit_indicator` / `break;` block in `scrollbar-grab-drag^!` (`crates/diffuse-tui/src/app/dispatch.rs`), where:

- the locked move block ends at new 331 / old 335 (the move core)
- the immediately following lines (old 336–351 / new 332–347) are a +4-shifted identical continuation that was instead claimed as `Ctx`/`Add` by the pre-diff
- the genuine first edit is at old 355 / new 351–352 (a line split), which is where the run should stop

The absorption is best understood as a post-main-pass step: after moves are locked, walk each move block's tail to see whether the next code (or blank) lines form a uniform-shift match and, if so, absorb them into the move block rather than leaving them stranded as `Ctx`/`Add`. This keeps the existing solver mechanics (expansion, cohesion, anchor marks) but gives it a new seed source and a tail-run boundary rule.

## Relationships with the prior plan

This plan builds on the prior plan's moved-line machinery and its emphasis on:

- locked move blocks as the stable anchor for further decisions
- uniform, deterministic rules over heuristics
- synthesis tests as the primary design artifact before golden corpus changes

It does not modify the prior plan's existing move admission rules; it adds a new tail absorption step that runs *after* those rules, and only proposes extensions where a locked move block's tail exactly matches a rigidly translated continuation.

## Table of contents

1. [Phase 1 — Tail-run partner finding, seeded from move blocks](#1-phase-1--tail-run-partner-finding-seeded-from-move-blocks)
2. [Phase 2 — Absorption pass integration, tail-run expansion, and roll-back](#2-phase-2--absorption-pass-integration-tail-run-expansion-and-roll-back)
3. [Phase 3 — Context re-pairing rule, orphan handling, and hunk/context budget behavior](#3-phase-3--context-re-pairing-rule-orphan-handling-and-hunkcontext-budget-behavior)
4. [Phase 4 — Clicking semantics for moved sections, with a deliberate interpretation](#4-phase-4--clicking-semantics-for-moved-sections-with-a-deliberate-interpretation)
5. [Phase 5 — Golden corpus review and sign-off](#5-phase-5--golden-corpus-review-and-sign-off)

## 1. Phase 1 — Tail-run partner finding, seeded from move blocks

### Step 1.1 — Define the tail candidate lookup

When a move block is locked, we need to look just beyond its current tail on each side:

- new-side candidate set: rows immediately after the move block's new tail
- old-side candidate set: rows immediately after the move block's old tail

The concept here is that the "tail run" is anchored to the move block by continuity: the first tail row adjacent to the move block is the natural place to test whether the following lines are rigidly translated copies of the block's trailing text.

Settled design (Phase 1 review): stepping is by file-order successor among hunk rows, not by hunk-row adjacency and not by naive line-number +1. On the old side, the candidate is the row carrying the smallest old line number greater than the tail's old line (and symmetrically for new lines on the new side). This is robust to sparse hunks, where context outside the hunk means old/new line numbers are not contiguous among rows. If no such row exists on either side, the run stops. Scope stays local: only the immediate file-order successor on each side is ever considered, one step at a time.

### Step 1.2 — Define uniform-Δ tail matching

A genuine move block has a single uniform indent delta Δ across all of its lines: for each matched pair, Δ is the new-side indent minus the old-side indent. Tail absorption extends the block only while that uniform-Δ property continues to hold past the current tail. If Δ drifts, the block has changed shape and absorption stops — the tail beyond the uniform run is a different edit, not part of the block.

A line from the new-side candidate set matches a line from the old-side candidate set if, after subtracting the block's Δ from the new-side indent, the two lines are exactly equal (not merely trimmed-equal). A tail run is therefore a maximal contiguous sequence of new-side rows that can be paired to an equal-length contiguous sequence of old-side rows under that Δ, with:

- exact equality after Δ-normalization (which also catches trailing-whitespace and content differences that trimmed comparison would miss), and
- the run stopping at the first genuine mismatch or Δ change

Single blank lines are exempt from the Δ check: a blank line has no indentation, so a blank new-side row pairs with a blank old-side row simply by existing at the right place in both blocks. Blanks never terminate a run and never establish Δ; the Δ in force is always the last non-blank pair's Δ.

### Step 1.3 — Stop conditions

The tail run stops when:

- the next candidate line does not match its counterpart after Δ-normalization
- the next candidate is not at the same Δ as the block's dominant Δ
- one side runs out of adjacent rows
- or there is no adjacent line to seed the run with

In the motivating case, the run should stop at the line-split edit, because the text on the new side is split across two lines while the old side is a single line, breaking the normalized equality. That is exactly what we want.

### Step 1.4 — Synthetic failing fixture

Before any golden corpus changes, create a minimal synthetic fixture that fails under the current machinery:

- old file snippet with a move core plus a trailing identical run that is shifted uniformly
- the unchanged diff currently mis-aligns the trailing run as `Ctx`/`Add`
- asserting that the tail becomes part of the move block fails

A synthetic fixture is the right first artifact because it reveals the rule in isolation, it is reviewable as a single small example, and it makes the intended stop conditions testable without inventing big corpus churn.

A synthetic fixture should cover:

- the motivating +4 tail case
- a tail that stops at an edit
- a counterexample where the uniform-Δ run does not hold (to confirm we do not over-absorb)
- an empty trailing tail or no adjacent run, to confirm absorption is conditional

One subtlety worth noting explicitly: the tail run's absorption means we must be careful not to create a move block that claims lines which are actually context of a *later* unrelated change. That is the core correctness question, and phase 3 is where we make a deliberate rule about it.

## 2. Phase 2 — Absorption pass integration, tail-run expansion, and roll-back

### Step 2.1 — Integrate the tail-pass after main-pass moves

The tail absorption should run after the places that already lock move pairs. That keeps the rest of the pipeline stable: the existing mover still decides what is a move core and what is a leftover, and the tail pass only extends the tail of an already-locked move when the uniform-Δ criterion holds.

This is also consistent with the guiding idea that move blocks are stable siblings of `Ctx` blocks: once a move block is locked, its tail is a continuation of an unchanged run, and the tail pass extends that run where appropriate.

### Step 2.2 — Tail-run expansion rule

For each locked move block:

- normalize the tail candidates by Δ
- walk forward from the move block tails, pairing new-side and old-side candidates as long as the uniform-Δ normalized equality holds
- absorb those pairs into the move block as `MovSrc`/`MovTgt`

### Step 2.3 — Roll-back semantics for orphaned context lines

If the tail pass absorbs lines that were previously assigned to a different structural role, we must also handle the other side consistently. That is the main correctness tension:

- if the new side's tail is absorbed into the move block, the old side's corresponding tail must be accounted for
- if the old side's tail had been assigned to a far-away `Ctx` pairing, we should not allow it to remain as a stray assignment after absorption

A reasonable rule is that absorption is paired and reversible: if the tail run is absorbed on one side, the corresponding lines on the other side are also re-placed consistently with the move block, and any previous assignment for those rows is undone in the same tail pass. This is why the rule must be local and explicit rather than "let the later solver clean up". The prior plan's emphasis on reversible structure matters here: tail absorption should not leave the representation in a state where old neighbors are still pointed at a now-invalid target.

### As-built (Phase 2, uncommitted)

- `mark_moves_internal` returns the applied `Vec<MovePair>` (`pub type MovePair =
  ((usize, usize), (usize, usize))`); skipped (non-Del/Add) rows are not reported.
- `absorb_mov_tails(f, locked, profile, allow_ctx_break)` runs twice from
  `pipeline/file.rs`: after the pre-pass with `allow_ctx_break=false` (claim
  free `Del`/`Add` leftovers only, so collapse input stays stable and no
  orphans are created for collapse to mis-pair), and after the main pass with
  `true` (also break misassigned `Ctx`, orphans stay as plain leftovers).
  Each call uses its own pass's fresh positions; stale pairs are validated
  out (must still resolve to `MovSrc`/`MovTgt`).
- Absorption returns the absorbed `(old line, new line)` pairs; `file.rs`
  snapshots pre-pass locked lines plus pre-absorbed lines, then re-resolves
  all of them by line number after collapse (`resolve_mov_pairs_by_line`)
  and unions with the main-pass pairs. Without this the main pass skips
  already-moved rows, so pre-pass-locked blocks (e.g. the dispatch core)
  would never be extended — and pre-absorb's own extensions would poison
  whole-run resolution. Found via the new integration test (red first:
  old-336 stayed `Ctx`).
- Integration test `test_dispatch_move_block_absorbs_uniform_tail`
  (`pipeline/tests.rs`): reads the exact `scrollbar-grab-drag^!` golden via
  the live-repo framework path (`diff_repository` + `process_single_diff_file`),
  asserts core (old 315-335 ↔ new 310-331), tail (old 336-354 ↔ new 332-350),
  orphans (new 363-365 leftovers), and the stop at the `vis_line` split
  (old 355, new 351-352). Complemented by `test_resolve_mov_pairs_by_line_drops_stale_pairs`.
- Plan-then-apply per hunk: broken `Ctx` rows expand to `[Mov, orphan]`;
  `Mod`/already-moved/claimed rows stop the run. Cross-hunk locked runs are
  handled by the same per-hunk rebuild (no same-hunk restriction).
- Synthetic fixture: the boundary ctx sits between the runs in row order but
  its line numbers live outside the move (old 5 / new 50), so file-order
  stepping skips it. Un-ignored and green.
- Golden review (this turn): `diffcoord-anchoring` absorbs one `.iter()` pair
  at Δ=0 and stops at the genuine `.any(...)` edit; `diffuse-history-mid`
  absorbs 64 blank/`[[package]]`/`dependencies` pairs at Δ=0 across reordered
  Cargo.lock stanzas (stopping at version bumps) plus one blank each in
  `collapse/tests.rs` and `similarity/mod.rs`; `theme-light-toggle` absorbs 5
  `render_hunk_divider` param pairs at Δ=0, stopping exactly at the
  `seg_start`→`theme` swap. Adds/dels/mods counts unchanged everywhere, no
  `Mod` touched, all other 39 fixtures identical.
- `./scripts/check.sh` green (143s).

### Step 2.4 — Keep the stop-condition tests in sync

The synthetic fixture from phase 1 must continue to pass after integration, and we should add at least one integration-level test that exercises the full tail absorption path from diff input to final moves representation. That second test is where the behavior becomes real rather than merely a rule on paper.

## 3. Phase 3 — Context re-pairing rule, orphan handling, and hunk/context budget behavior

### Step 3.1 — Decide what tail absorption means for context semantics

The plan's conceptual framing is that moved blocks and context blocks are siblings: both represent runs of unchanged text. A tail absorbed into the move block is therefore not "newly changed" — it is unchanged text that was mis-assigned by the initial diff.

That means the tail absorption should be understood as reclassification from `Ctx`/`Add` leftovers into the move block's representation, and it should be auditable as such.

### Step 3.2 — Orphan and re-pairing rule

If the tail run absorbs old-side lines that had previously been paired to some far-away target, the re-pairing rule must say what happens to the far-away partner:

- either re-pair it to whatever the tail run's old-side counterparts were previously matched to (if any), or
- mark it as re-orphaned and leave it for the next stage with an explicit contract

The safer default for this plan is to keep the scope local: only absorb what the uniform-Δ tail run clearly belongs to, and leave any far-away re-pairings for a future, separate pass if needed. That keeps the current change reviewable.

### Step 3.3 — Hunk and context budget behavior

Tail absorption could in principle affect hunk splitting or context budgets if we let it run long enough across structural boundaries. For this plan, the conservative choice is to restrict tail absorption to the region already adjacent to the move block's existing hunk context, and not to start a new hunk-restructuring pass. That keeps the golden review tractable: the change should stay within the same structural neighborhood rather than re-slicing the file into new hunks wholesale.

If that conservative scope proves too weak, future work can extend it, but the present plan should commit to a clearly bounded first version.

### Step 3.4 — Failure-mode tests

Add tests for the failure modes:

- a tail run that is not uniform-Δ, so absorption does not happen
- a tail run whose Δ changes mid-run, so absorption stops early
- a tail run whose end is a real edit, so absorption stops at the edit
- a tail run that overlaps another structural region, so absorption does not aggressively steal it

## 4. Phase 4 — Confirm no UI change

No UI work in this plan. An absorbed tail row is just another `MovSrc`/`MovTgt` row in the same move block, loosely coupled with the rest of the block exactly as any mov line is. The UI keeps its existing affordance (jump to the opposing mov side) with no behavior change. This phase is a confirmation checkpoint only: verify that no renderer, TUI interaction, or selection logic needed modification beyond consuming the pipeline output shape.

## 5. Phase 5 — Golden corpus review and sign-off

### Step 5.1 — Regenerate and review

After the tail absorption is in, regenerate the golden corpus and review all changed fixtures.

The plan's review discipline is:

- each changed fixture should be inspected, not merely regenerated
- the change should be consistent with the uniform-Δ tail rule and should stop at the right edits
- no over-absorption into unrelated regions should appear

### Step 5.2 — Semi-automated review

Use the review tooling to surface candidate tail absorptions and their boundaries, so the human reviewer can inspect the result efficiently and verify that the stop conditions are working.

### Step 5.3 — Agent sign-off

Once the active review is done and the review has signed off, the agent may mark the golden phase complete and commit the changes.

For this final step, the commit policy is that the agent may mark the phase complete after the human has inspected the changes.

### Step 5.4 — Move to done

When the whole plan is complete, move the document to `plan/done/` to archive the architectural decision and milestones.
