# Prose Reflow Window Folding and Single-Word Relocation Hygiene

## Roadmap & Status

- [x] **Step 1 (Experiment)** — [Cross-`Diag` Boundary-Wrap Window Folding for Prose & Comments](#step-1-experiment--cross-diag-boundary-wrap-window-folding-for-prose--comments) — **Pivot → Promote (`1B` + `prose_folded` + Stage 2 wholesale bypass)**
- [x] **Step 2 (Experiment)** — [Step 4 Skip-Evidence & Single-Word Prose Relocation Hygiene](#step-2-experiment--step-4-skip-evidence--single-word-prose-relocation-hygiene) — **Pivot → Promote (Comment/Code Parity + Stage 1/2 Slot-Pair Prose Hygiene)**
- [x] **Step 3 (Action)** — [Implement Comment/Code Parity & Prose Cross-Slot Relocation Hygiene](#step-3-action--implement-commentcode-parity--prose-cross-slot-relocation-hygiene)
- [ ] **Step 4 (Action)** — [Implement Cross-`Diag` Prose Boundary-Wrap Folding, Comment-Prefix Preservation & Stage 2 Substring Fallback](#step-4-action--implement-cross-diag-prose-boundary-wrap-folding-comment-prefix-preservation--stage-2-substring-fallback)

---

## Executive Summary

In `scrollbar-grab-drag` (`crates/diffuse-tui/src/app/geometry.rs:290–292` → `359–360`), a rewritten `///` doc-comment paragraph produces a spurious single-word relocation from a deleted middle line even though a stronger, contiguous wrap match sits immediately below:

```rust
- 290: /// yields `max_scroll`; everything between is proportional with no
- 291: /// flats, so the [<thumb<] moves on every pointer row and never sticks or
- 292: /// [-snaps. The thumb -]follows through [`Self::scrollbar_thumb_for`],
+ 359: /// yields `max_scroll` (bottom of thumb at `track_h - 1`). The [>thumb>]
+ 360: /// follows smoothly through [`Self::scrollbar_thumb_for`] with zero
```

Experiments across all 23 golden corpora revealed **four** interacting root causes rather than two:

1. **Isolated segmentation windows & skip-free `emit_segments` drop**: [`reflow.rs`](../crates/diffuse-core/src/collapse/reflow.rs) never folds consecutive prose/comment `Diag`s on wrapped phrases, AND `emit_segments` drops any skip-free multi-`Diag` window (`!win.has_skips()`), discarding `2×2` / `3×3` wrapped paragraphs back into plain `Mod` pairs.
2. **Comment/code cross-contamination & weak single-word / punctuation moves**: [`reflow.rs`](../crates/diffuse-core/src/collapse/reflow.rs) Step 4 had no comment/code parity guard (allowing `review-notes:307→311` to steal `"hash"` out of a deleted Rust `let` statement into a `//` comment), while Stage 1 ([`project.rs`](../crates/diffuse-core/src/collapse/project.rs)) and Stage 2 ([`relocate.rs`](../crates/diffuse-core/src/collapse/relocate.rs)) allowed zero-salient punctuation (`'.'`, `').'`, `'and'`, `'│'`) and lone mid-sentence nouns (`'thumb'`, `'golden'`, subword prefix `'byte-identical'`) to move across prose lines.
3. **Stage 1 stealing before Stage 2 across the `///` barrier**: On `scrollbar-grab-drag:290–292`, `old[292]`'s `///` matched `new[360]`'s `///` as same-slot `Ctx`, making the upward unwrap `". The thumb"` (`292 → 359`) non-monotone (Stage 2's job). Meanwhile, Stage 1 greedily matched `old[291]`'s lone `"thumb"` monotonically to `new[359]`'s `"). The [>thumb>]"`, destroying `". The thumb"` before Stage 2 ever ran.
4. **Stage 2 candidate blindness under wholesale rejection**: [`relocate.rs:80`](../crates/diffuse-core/src/collapse/relocate.rs#L80) extracts cross-slot candidates via `merged_runs_for_texts_with_profile`, whose line-level wholesale threshold (`ratio < ~0.30`) suppresses all `Ctx` runs whenever a short wrapped phrase (`". The thumb"`, 11 chars) sits inside a much longer added run (`" (bottom of thumb at \`track_h - 1\`). The thumb"`, 46 chars).

Addressing all four cleanly turns `scrollbar-grab-drag:290–292` into the exact desired relocation (`". The thumb"` moving `292 → 359`, `291` pure `Del`), rescues **23** genuine prose/comment wraps across the 23 golden corpora with **0** false-positive new groups, and dissolves **7** pure-noise legacy prose groups.

---

## Living Truth Table

| Expected | Context | Rationale |
| :--- | :--- | :--- |
| **MOVE `". The thumb"` (`292 → 359`), KEEP `291` pure `Del`** | `scrollbar-grab-drag` `geometry.rs:290–292` → `359–360` | `". The thumb"` wraps directly from line 292 (before `"follows"`) onto the end of line 359; `"thumb"` on deleted line 291 is an unrelated mid-sentence noun. |
| **NO GROUP / NO MOVE** | `diffuse-2026-08:83→106` (`'.'`), `rust-lifetime-char:442→412` (`').'`), `theme-light-toggle:267→273` (`').'`), `diffuse-history-mid:10→8` (`'and'`) | Zero-salient punctuation and stopword relocations in comment/prose groups are visual noise. |
| **NO GROUP / NO MOVE** | `review-notes:307→311` (`'hash'`) | Comment line must never absorb an adjacent deleted code line (`let last_line_hash = hash_line_content(...)`) in Step 4. |
| **NO GROUP / NO MOVE** | `diffuse-2026-08:91→97` (`'golden'`), `diffuse-2026-08:375→442` (`'byte-identical'`) | Lone mid-sentence noun and false upward subword prefix (`byte-identical` → `byte-identically.`) are spurious matches. |
| **KEEP / UPGRADE MOVES** | `diffcoord-anchoring:110–113` (`` `is_current_line` is `` + `` background (`Fixed(237)`). ``), `diffcoord-anchoring:727–730` (4-line chain), `diffuse-2026-08:108–110` (`deliberate: it`, `redesign be`), `diffcoord-anchoring:1405→1629` (`them!)`), `scrollbar-grab-drag:281–282` (`exact`) | Genuine multi-word and exact line-boundary prose wraps across adjacent comment/Markdown lines. |
| **UNCHANGED (NO CODE REGRESSION)** | `diffuse-history-mid:52→68` (`fn process_file(...) { ... }` inside fenced Markdown), `linenum-u32` `4×4` `Row::Mod {` → `Row::mod_row(` blocks | Fenced code blocks inside Markdown and skip-free code delimiter folds must not be altered by prose rules. |

---

## Step 1 (Experiment) — Cross-`Diag` Boundary-Wrap Window Folding for Prose & Comments

- **Hypothesis**: Extending [`reflow.rs`](../crates/diffuse-core/src/collapse/reflow.rs) to fold consecutive `Diag` pairs within a contiguous comment/prose paragraph when unmatched trailing text on `Diag p` wraps onto the start of `Diag p+1` (or vice versa) places wrapped paragraphs — including `scrollbar-grab-drag:290–292` — into one window so alignment captures the true boundary wrap.
- **Falsifier**:
  - Folding too eagerly could collapse rewritten docblocks into noisy `N×M` groups or introduce false cross-line moves (3× false-positive prior).
  - Must not degrade skip-free code windows or strand trailing skips belonging to `last_diag`.
- **Competing Variants**:
  - **1A (Strict multi-word boundary-phrase folding)** — fold `Diag q → q+1` (matching indent & comment marker, ≤ 2 intervening prose gap rows, no blank lines) when an unmatched ≥ 2-word (≥ 6-char, ≥ 1 salient) tail of one side matches the unmatched head (offset `0..=1`) of the opposite side on `q+1`.
  - **1B (1A + single-word boundary handoff)** — also fold when a single salient boundary word (≥ 4 core chars) wraps strictly from exact end of `q` to exact start (`off == 0`) of `q+1`.
  - **1C (Early prototype without `emit_segments` / `prose_folded` / Stage 2 substring fallback)** — baseline comparison showing why folding alone fails without pipeline coordination.

### Result

- **Verdict**: **Pivot → Promote (`1B` scoped by `WindowSpan.prose_folded`, paired with Step 2 hygiene + Stage 2 wholesale-bypass substring extraction)**.
- **Quantitative delta across all 23 golden corpora**:
  - Initial `1A`/`1B`/`1C` (before fixing `emit_segments` and trailing Step 4): `-4/-5/-6 rem, +2 new` — almost every skip-free `2×2`/`3×3` prose fold was silently discarded by `emit_segments`'s `if win.has_skips()`, and `scrollbar-grab-drag:290–292` regressed to `moved=['with', 'thumb', 'thumb']` because Stage 1 stole `old[291]`'s `"thumb"` first.
  - Refined `1a_fix` / `1b_fix` (`emit_segments` emits multi-`Diag` windows, Step 4 runs on `last_diag`): `-4 rem, +24 new` (`1a_fix`) and `-5 rem, +35 new` (`1b_fix`) — rescued 14 (`1a_fix`) / 25 (`1b_fix`) genuine prose wraps, while exposing 6 accidental `linenum-u32` `4×4` code groups (`moved=[')']`) caused by emitting non-prose skip-free `fold_multi_pair` windows.
  - Combined **`v3_clean`** (`1B` gated by `WindowSpan.prose_folded` + Step 2 hygiene + Stage 2 common-token fallback): **`-13 rem, +23 new, ~8 chg`**, with **0 false-positive new groups**, **0 `linenum-u32` regressions**, and **`scrollbar-grab-drag:290–292` moving `['. The thumb']`**!
- **Best Wins**:
  - `scrollbar-grab-drag` `geometry.rs:290–292` (`3×2`): moves `'. The thumb'` cleanly from `292 → 359`; `del:291` is 100% pure `Del` (and `geometry.rs:281–282` cleanly wraps `'exact'`).
  - `diffcoord-anchoring` `notes.rs:727–730` (`4×3` chain): moves all three wraps (`their target line display`, `range. Notes that do not`, `banner.`).
  - `diffcoord-anchoring` `notes.rs:110–113` (`2×3`): moves both `` `is_current_line` is `` and `` background (`Fixed(237)`). ``.
  - `diffuse-2026-08` `README.md:108–110` (`3×3`) & `AGENTS.md:28–30` (`3×3`): moves `deliberate: it` + `redesign be` and `the host tool` + `as the spec,`.
- **Surprises & Invariants Learned**:
  1. `emit_segments` MUST gate skip-free multi-`Diag` emission on an explicit `win.prose_folded` flag (never `win.last_diag > win.first_diag`), or skip-free code `fold_multi_pair` windows (`linenum-u32`) leak spurious bare-`)` moves.
  2. In `///` / `//` / `*` comment `Mod` pairs, a downward wrap (`old[k]` tail → `new[k+1]` head) crosses `old[k+1]`'s leading comment marker; Stage 1 Gotoh currently sacrifices `///` (`[-///-][+/// +]`) to make the match monotone (visible on `diffcoord-anchoring:111/112` and `728/734`). Preserving identical leading comment markers as same-slot `Ctx` on `Mod` slots lets Stage 2 relocate the wrapped phrase cleanly without `[-///-][+/// +]` flashes.
  3. Stage 2 (`relocate.rs:80`) was blind to `". The thumb"` because `merged_runs_for_texts_with_profile` applies line-level wholesale rejection (`11 / 46 = 24%`), wiping out all `Ctx` candidates when a short wrapped phrase lands inside a long `Add`/`Del` run. Falling back to longest common contiguous token spans when `merged_runs` yields no `Ctx` unlocks `". The thumb"` immediately.

---

## Step 2 (Experiment) — Step 4 Skip-Evidence & Single-Word Prose Relocation Hygiene

- **Hypothesis**: Filtering spurious zero-salient and isolated non-boundary single-word prose relocations — and tightening prose skip admission — eliminates all false-positive comment/prose noise (`'.'`, `').'`, `'and'`, `'hash'`, `'thumb'`, `'golden'`) without harming legitimate wraps or code groups.
- **Falsifier**:
  - Over-tightening Step 4 (`shares_unmatched_salient`) could dissolve genuine multi-word prose groups whose matched words include stopwords (`the schema IS the`, `The extension`, `` `tests/golden/` is ``).
  - Treating fenced code inside `.md` files as prose could degrade code-block expansions (`diffuse-history-mid:52→68`).
- **Competing Variants**:
  - **2A (Post-alignment group dissolution in `peer.rs`)**: Dissolve all-prose groups with zero salient moved words or a single non-boundary moved word.
  - **2B (Tighten `reflow.rs` Step 4 for prose skips)**: Require ≥ 2 salient words or exact boundary match in `shares_unmatched_salient`.
  - **2D / `v3_clean` (Comment/code parity in Step 4 + Stage 1/2 slot-pair prose hygiene)**:
    1. In `reflow.rs` Step 4 (`shares_unmatched_salient`): forbid mixing comment heads with non-comment code skips (and vice versa).
    2. In `project.rs` (Stage 1) and `relocate.rs` (Stage 2): on prose/comment windows (excluding fenced/code-like Markdown blocks), suppress bare-punctuation candidates and allow a cross-slot move only when it carries ≥ 1 salient word AND either (a) ≥ 2 alphabetic words in the phrase, (b) ≥ 2 salient words relocated across the same `(src, tgt)` slot pair, or (c) an exact whole-word line-boundary wrap (`o_at_end → n_at_start/end` downward, or `o_at_start → n_at_end` upward).

### Result

- **Verdict**: **Drop `2B`** (too destructive on multi-word phrases containing stopwords); **Pivot `2A` → Promote `2D` / `v3_clean`** (Stage 1/2 slot-pair hygiene + Step 4 comment/code parity).
- **Quantitative delta across all 23 golden corpora**:
  - `2A` (post-alignment only): `-5 rem, +0 new` — killed `'.'`, `').'` (×2), `'thumb'`, but missed `review-notes:307→311` (`'hash'`, mixed comment+code), missed `'golden'`/`'and'`/`'byte-identical'`, and accidentally dissolved Markdown fenced code block `diffuse-history-mid:52→68` (`fn process_file { ... }`).
  - `2B` (Step 4 salient-count ≥ 2): `-13 rem, +3 new` — killed real noise (`'golden'`, `'and'`, `'thumb'`, `'byte-identical'`) and unlocked two huge wins (`refactoring structure is a rewrite of the`, `LCS, the similarity scoring)`), **but** suffered 6 unacceptable casualties (`the schema IS the`, `The extension`, `` `tests/golden/` is ``, `/// SGR attribute`, `// 1. Note`, `component`) because stopwords were not counted as multi-word phrase length.
  - **`2D` / `v3_clean` (Step 4 comment/code parity + Stage 1/2 slot-pair prose hygiene)**:
    - **7/7 target false positives eliminated**: `'.'`, `').'` (×2), `'and'`, `'hash'` (comment↔code), `'thumb'`, `'golden'`, `'byte-identical'` (subword prefix).
    - **8 existing groups strictly cleaned up** (`CHG`): stripped stray `)`, `│`, `.`, `/// `, `redesign`, `golden`, `;` while preserving every real moved phrase (`(PLAN.md Phase`, `port must prove itself`, `the extension`, `SGR attribute`, `component`, `struck`/`filled`).
    - **0 fenced code casualties**: `diffuse-history-mid:52→68` (`fn process_file`) untouched.
    - **2 borderline Markdown casualties**: `diffuse-2026-08` `PLAN.md:113` (`['test', '§5)']`) and `PLAN.md:178` (`[', walk', 'output']`) — both heavily rewritten 2×1 bullets moving two disjoint scraps (`§5)` is non-alphabetic, `walk`/`output` split across rewritten clause); acceptable under the 3× false-positive prior.
- **Invariant Learned**:
  - Filtering weak prose relocations **inside Stage 1 (`project.rs`) before Stage 2 (`relocate.rs`)** is strictly superior to post-alignment group dissolution (`2A`): demoting a noisy single-word match (`old[291]`'s `'thumb'`) early frees the target tokens (`"). The thumb"`) so Stage 2 can discover the longer contiguous phrase (`". The thumb"`) instead of being blocked by Stage 1's theft.
  - Comment/code parity in `reflow.rs` Step 4 is a standalone structural invariant (symmetric with `scan_valence_forward`'s existing comment exclusion).

---

## Step 3 (Action) — Implement Comment/Code Parity & Prose Cross-Slot Relocation Hygiene

Clean, self-contained implementation of the Step 2 findings before enabling cross-`Diag` prose folding:

1. **`crates/diffuse-core/src/collapse/reflow.rs`**:
   - Enforce comment/code parity in `PathSegmenter::shares_unmatched_salient`: when `profile` is present and both head rows agree on `is_comment_line`, reject skips on the opposite side of that boundary (kills `review-notes:307→311` `'hash'`).
2. **Shared prose/comment window classification helper** (used cleanly by both `project.rs` and `relocate.rs`, keyed off `LangProfile` — `profile.is_comment_line` for code languages, `profile.markdown` excluding fenced/code-syntax lines for Markdown; no catch-all cross-language heuristics):
   - Extract body span stripping active comment marker / Markdown quote marker (`>`).
3. **`crates/diffuse-core/src/collapse/project.rs`**:
   - Add `filter_weak_prose_cross_slot_phrases` after `bridge_enclosed_cross_slot_spans`: on prose/comment windows, drop cross-slot moved phrases unless they carry ≥ 1 salient word AND satisfy at least one of: (a) ≥ 2 alphabetic words, (b) ≥ 2 salient words across the `(so, sn)` slot pair, or (c) whole-word exact line-boundary wrap.
4. **`crates/diffuse-core/src/collapse/relocate.rs`**:
   - Skip bare-punctuation candidate pass on prose/comment windows and gate salient prose relocations with the same rule as Stage 1.
5. **Tests & golden review**:
   - Golden-driven integration tests verifying `review-notes:307→311`, `diffuse-2026-08:83→106`, `rust-lifetime-char:442→412`, `theme-light-toggle:267→273`, and fenced Markdown preservation (`diffuse-history-mid:52→68`), plus unit tests for edge branches.
   - Regenerate affected goldens, inspect wire & render diffs visually, and pass `scripts/check.sh`.

### Check Result

- **Implemented**: `LangProfile::leading_comment_prefix` (`crates/diffuse-syntax/src/profile/lang.rs`), Step 4 comment/code parity (`crates/diffuse-core/src/collapse/reflow.rs`), shared prose/comment relocation hygiene (`crates/diffuse-core/src/collapse/prose.rs`) wired into Stage 1 (`project.rs`) and Stage 2 (`relocate.rs`).
- **Golden delta**: 7 of 23 corpora updated (17 wire hunks total): all 7 target false-positive groups dissolved (`review-notes:307/311` `'hash'`, `scrollbar-grab-drag:290/359` `'thumb'`, `diffuse-2026-08` `'.'`/`'golden'`/`'byte-identical'`, `diffuse-history-mid` `'and'`, `rust-lifetime-char` & `theme-light-toggle` `').'`), 8 existing prose groups stripped of stray punctuation/gutter scraps, plus a bonus cleanup on `diffcoord-anchoring:554/692` (comment/code parity kept `{` as `Ctx`). Zero code or fenced-Markdown regressions.

---

## Step 4 (Action) — Implement Cross-`Diag` Prose Boundary-Wrap Folding, Comment-Prefix Preservation & Stage 2 Substring Fallback

Builds cleanly on Step 3's hygiene layer to solve `scrollbar-grab-drag:290–292` and rescue wrapped prose paragraphs corpus-wide:

1. **`crates/diffuse-core/src/collapse/reflow.rs`**:
   - Add `prose_folded: bool` to `WindowSpan`; update `emit_segments` to emit `Segment::Window` when `win.has_skips() || win.prose_folded` (preserving skip-free code `fold_multi_pair` behavior untouched).
   - Add Step 3.75 (`fold_prose_boundary_wraps`): when `!delim_extended` and valence did not fire, chain consecutive prose/comment `Diag`s (`q → q+1`, ≤ 2 prose gap rows, matching indent & comment prefix, no blank lines) when an unmatched tail wraps onto the opposite head (≥ 2-word ≥ 6-char salient phrase at `off ∈ 0..=1`, or exact single ≥ 4-char salient word at `off == 0`).
   - Run Step 4's `salient_following_skip` against `last_diag` (not `p`) so trailing skips stay attached to the end of a folded chain (`diffcoord-anchoring:110–113`, `727–730`).
2. **`crates/diffuse-core/src/collapse/relocate.rs`**:
   - When `merged_runs_for_texts_with_profile(del_text, add_text, profile)` returns no non-whitespace `Ctx` (due to line-level wholesale rejection on asymmetric run lengths), extract common contiguous multi-token spans (≥ 3 tokens, ≥ 6 chars, carrying a salient token) directly so wrapped phrases like `". The thumb"` inside longer `Add`/`Del` runs always enter the candidate pool.
3. **Comment-prefix `Ctx` preservation on `Mod` slots (`crates/diffuse-core/src/lcs/multiline.rs` / `project.rs`)**:
   - Prevent Stage 1 Gotoh from sacrificing an identical leading comment marker (`///`, `//`, `*`) on a paired `Mod` slot just to force a downward cross-slot match — eliminating `[-/// -][+///+]` flashes on `diffcoord-anchoring:111/112` & `728–729` while letting Stage 2 relocate the wrapped phrase cleanly.
4. **Tests & golden review**:
   - Golden-driven integration test asserting `scrollbar-grab-drag` `geometry.rs:290–292` emits `". The thumb"` moved from `292 → 359` with `291` pure `Del`, plus `diffcoord-anchoring:110–113` & `727–730` multi-line wraps without `[-/// -][+///+]` flashes, and unit tests for all boundary branches.
   - Regenerate affected goldens, review visual diffs carefully (including `less -R` / ANSI inspection if helpful), and pass `scripts/check.sh`.
