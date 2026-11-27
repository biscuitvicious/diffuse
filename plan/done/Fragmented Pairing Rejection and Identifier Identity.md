# Fragmented Pairing Rejection and Identifier Identity

## Roadmap & Status

- [x] **Step 1 (Action)** — [Fix Indentation-Inflated Content Accounting & Multiset Bag Capping](#step-1-action--fix-indentation-inflated-content-accounting--multiset-bag-capping)
- [x] **Step 2 (Experiment)** — [Whole-Identifier Identity for Pairing Admissibility](#step-2-experiment--whole-identifier-identity-for-pairing-admissibility) *(dropped — falsified; subword alignment is valuable when post-tape mass holds)*
- [x] **Step 3 (Experiment)** — [Post-Tape Salient Coverage & Fragmentation / Chaff-Anchor Gate](#step-3-experiment--post-tape-salient-coverage--fragmentation--chaff-anchor-gate) *(promoted → Step 6)*
- [x] **Step 4 (Experiment)** — [Declaration Identity & `head_agrees` / `decl_compat` Tightening](#step-4-experiment--declaration-identity--head_agrees--decl_compat-tightening) *(merged & promoted → Step 6)*
- [x] **Step 5 (Experiment)** — [Intra-Line Fragment Dissolution by Run Merging (Before Pair Dissolution)](#step-5-experiment--intra-line-fragment-dissolution-by-run-merging-before-pair-dissolution) *(dropped — spun out to [Intra-Line Fragment Dissolution by Run Merging.md](Intra-Line%20Fragment%20Dissolution%20by%20Run%20Merging.md) for colored `less -R` review)*
- [x] **Step 6 (Action)** — [Post-Tape Salient Floor, Multi-Island Fragmentation Gate, and `decl_compat` Removal](#step-6-action--post-tape-salient-floor-multi-island-fragmentation-gate-and-decl_compat-removal)

---

## Executive Summary

When two lines share minimal scaffolding and a single short identifier or coincidental subword inside unrelated compound names — for example:

```rust
- self.scroll_to_visual(self.scroll.saturating_sub(n));
+ self.on_mouse_scroll(true, n, row, col);
```

the pair was previously admissible (`Ctx` runs: `self.`, `(`, `n`, `);`), yet rendered as visual garble because the edits overwhelmingly overpowered the surviving anchors.

**Step 1** fixed the two underlying accounting bugs in [`evidence.rs`](../crates/diffuse-core/src/collapse/evidence.rs) (leading-indentation inflation and uncapped set-membership counting), immediately dissolving `scrollbar-grab-drag:46` so it resurfaced as a true move into `fn on_mouse_scroll`. Comparative experiments (**Steps 2–4**) across all 23 golden corpora then falsified strict pre-tape whole-identifier gating (which broke ~90+ valid subword renames) and identified a strong post-tape invariant (`3E`). Before promoting pair dissolution into production, **Step 5** explores a two-tier strategy: first dissolve weak interior `Common` fragments by merging flanking edit runs inside [`AlignmentTape`](../crates/diffuse-core/src/tape/) — which often turns a noisy `Mod` into one or two clean, readable replacement spans — and only fall back to dissolving the pairing into `Del` + `Add` (**Step 6**) when the merged runs still lack meaningful surviving anchors.

Per the diffing domain prior in [`Plan Guidelines.md`](Plan%20Guidelines.md), admitting a bad pairing is roughly 3× worse than dissolving a desired one.

---

## Living Truth Table

| Expected | Old Line | New Line | Rationale |
| :--- | :--- | :--- | :--- |
| **SPLIT** | `self.scroll_to_visual(self.scroll.saturating_sub(n));` | `self.on_mouse_scroll(true, n, row, col);` | Different methods, different args; only `self.`, punctuation, and 1-char `n` survive (fixed in Step 1; resurfaced as `mov` → L432). |
| **SPLIT** | `self.scroll_to_visual(self.scroll + n);` | `self.on_mouse_scroll(false, n, row, col);` | Same pattern (`app/dispatch.rs:54`); Step 5 dissolves it and resurfaces `mov` → L434. |
| **SPLIT** | `if !seg.is_empty() {` | `if is_ws_char(next_c) \|\| is_delim_char(next_c) \|\| is_op_char(next_c) {` | Only `if`, clipped subword `is_`, and `{` survive. |
| **SPLIT** | `let top = stack.pop();` | `let mut out = vec![0u8; items.len()];` | Unrelated `let` statements sharing only keyword + punctuation. |
| **SPLIT** | `pub fn note_anchors_cleanly(note: &ReviewNote, line_content: &str) -> bool {` | `pub fn label(self) -> String {` | Unrelated function declarations sharing only `pub fn` + `{`. |
| **SPLIT** | `fn wholesale_pairs_render_as_baseline_rows() {` | `fn subword_diff_token_spans() {` | Unrelated zero-arg functions previously kept by `\|\| decl_compat`. |
| **SPLIT** | `let thumb_row = header_offset + vh / 2;` | `let (thumb_off, _) = AppState::scrollbar_thumb_for(track_h, vh, total, scrolled, header_vis)` | Multi-island confetti anchored only by `let`, `thumb_`, and `vh`. |
| **PAIR** | `self.scroll_to_visual(target_vis);` | `self.on_mouse_scroll(target_vis);` | Shared whole argument `target_vis`, single clean edit island. |
| **PAIR** | `location: NoteLocation::line(side_val, start_line, end_line),` | `location: provisional_location,` | Shared field key (`key_agrees`), single RHS replacement. |
| **PAIR** | `if scrollbar_seek {` | `if is_on_scrollbar {` | Short control-flow swap with 1 clean edit island. |
| **PAIR** | `line_hash: n.line_hash,` | `content_hash: n.content_hash,` | Legitimate compound-identifier rename where LCS aligns strong subword mass (`_hash: n.` + `_hash`). |
| **PAIR** | `impl Run {` | `impl<'a> Run<'a> {` | Short 3-char identifier (`Run`) constitutes 100% of `min_salient` and 50% of line. |
| **PAIR** | `while share < dch.len() && share < ach.len() && dch[share] == ach[share] {` | `while share < d_bytes.len()` | Genuine 1-to-N expansion head (`nws_ctx / smaller` is 67%). |

---

## Step 1 (Action) — Fix Indentation-Inflated Content Accounting & Multiset Bag Capping

- **Module**: [`crates/diffuse-core/src/collapse/evidence.rs`](../crates/diffuse-core/src/collapse/evidence.rs)
- **Trimmed-coordinate `matched_chars` and `ctx_chars`**:
  - Strip leading horizontal whitespace from the first `Ctx` run before counting matched characters in `matched_chars(&runs)` and in `admissible_from_runs_prechecked`'s `is_decl_head` 50% guard, restoring `matched <= smaller <= larger` on trimmed coordinates.
  - Ensure `total_chars` in `is_decl_head` likewise measures trimmed lengths.
- **Multiset intersection in pre-tape salient bag**:
  - Replace `v.all_tokens: FxHashSet<&str>` membership with multiplicity-capped counting (`FxHashMap<&str, usize>`): each distinct token `t` contributes `min(count_u(t), count_v(t)) * char_len(t)` to `matched_salient`, restoring `u`/`v` symmetry and preventing double-counting.

### Check Result

- **Committed**: `ce4da0c` (`tests/golden`), `189c344` (`diffuse`). 625/625 `diffuse-core` unit tests and 47/47 dual-leg golden tests pass (`./scripts/check.sh` green).
- **Side-effect caught & resolved**: Stripping leading indent revealed that equal-indent brace block heads (`Row::Del(x) => {`, `.map(|(i, s)| {`) and code-only non-delimited chain heads (`toks.iter()`, `self.layers`) previously relied on indentation bonus to beat deeper-indented body lines. Added `block_head_prefix_len` and code-only `is_chain_head` recognition in `evidence.rs`.
- **Golden impact (12 of 23 corpora; low-anchor non-group `Mod` rows 146 → 127)**:
  - `scrollbar-grab-drag:46` dissolved and **resurfaced as a true move (`mov` ol:46 → nl:432 inside `fn on_mouse_scroll`)**.
  - Dissolved garbled pairings across `ansi-optimizer:181`, `diffcoord-anchoring`, `diffuse-2026-08:225`, `diffuse-history-mid:49`, `opt-1:277`, `rust-lifetime-char:176`, and 8 ANSI format strings in `theme-light-toggle`; upgraded 8 method-chain expansion heads in `rust-lifetime-char`.
  - **Surprises folded into Steps 3–4**: `scrollbar-grab-drag:54` survived by one space (`" n"` → `raw_content = +7`), and unrelated `let`/`pub fn` pairs already failed `admissible_from_runs_prechecked` but were overruled by `|| decl_compat`.

---

## Step 2 (Experiment) — Whole-Identifier Identity for Pairing Admissibility

- **Hypothesis**: Evaluating pairing-eligibility salient overlap on whole compound identifiers (`[A-Za-z0-9_]` runs) rather than subword tokens eliminates cross-identifier false matches while preserving subword diffing inside the intra-line LCS renderer once paired.
- **Falsifier**: Legitimate single-identifier renames on short lines might break apart if whole-identifier matching lacks a clean-rename escape hatch.
- **Competing Variants**: `2A` (strict whole-identifier pre-tape bag), `2B` (whole-identifier primary + 1-island subword fallback), `2C` (`2A` + intact-only post-tape salient check).
- **Probe**: Ran uncommitted probes across all 23 golden corpora.

### Result

- **Verdict**: **Drop** *(falsified)*.
- **Delta counts**: `2A` `-122 / +18`, `2B` `-98 / +18`, `2C` `-128 / +18`.
- **Decisive casualties**: Even with the 1-island escape hatch (`2B`), strict pre-tape whole-identifier gating dissolved ~90+ legitimate, readable renames where the LCS tape actually aligned substantial subword anchors:
  - `line_hash: n.line_hash,` → `content_hash: n.content_hash,` (2 rename islands)
  - `"mdBold" => Some(self.add_md_bold),` → `TokenColor::Bold => Some(self.add_bold),`
  - `MD_SGR_DIM, MD_SGR_ITALIC, MD_SGR_STRIKE, MD_SGR_UNDERLINE,` → `SGR_BOLD_UNDERLINE, SGR_DIM, ...`
  - `p.syntax_token_color("syntaxKeyword", RowKind::Add),` → `p.token_color(TokenColor::Keyword, RowKind::Add),`
  - `h.rows[di].old_line_num().unwrap_or(LineNum::MIN),` → `h.rows[di].old_line().expect(...),`
- **Invariant learned**: Subword matches are valuable *when the alignment tape actually aligns them with real mass*. The defect on `scroll_to_visual` vs. `on_mouse_scroll` was that the LCS tape *rejected* `"scroll"` (leaving only 1 salient char `"n"`), yet post-tape never re-enforced the salient floor.

---

## Step 3 (Experiment) — Post-Tape Salient Coverage & Fragmentation / Chaff-Anchor Gate

- **Hypothesis**: Judging the actual rendered `Vec<Run>` from the alignment tape — re-enforcing the salient floor on surviving `Ctx` runs and gating multi-island edits (`islands >= 2`) whose anchors are overpowered by edits — directly separates clean edits from confetti.
- **Falsifier**: Short-identifier edits (`impl Run {` → `impl<'a> Run<'a> {`) and genuine 1-to-N expansion heads (`while share < d_bytes.len()`, `match &self.location {`) must not be over-split.
- **Competing Variants**:
  - `3A`: Post-tape salient floor only (`ctx_sal >= 4 && ctx_sal / min_salient >= 0.30`).
  - `3B`: `3A` + multi-island gate (`islands >= 2`, `!key_agrees`, reject if `ctx_sal < 4 || nws_ctx / larger < 0.25`).
  - `3C`: `3B` + intact-only (non-clipped) salient tokens on `islands >= 2`.
  - `3D`: `3B` refined — clamp salient floor to `4.min(min_salient)` (protects `impl Run {`) and require `nws_ctx / smaller < 0.45` alongside `nws_ctx / larger < 0.25` (protects 1-to-N expansion heads).
  - `3E`: `3D` + `decl_compat` override removed (from Step 4A).
- **Probe**: Ran all 5 variants head-to-head across all 23 golden corpora.

### Result

- **Verdict**: **Promote `3E` → Step 6 (Action)** *(subject to Step 5 run-merging interaction)*.
- **Delta counts**: `3A` `-20 / +1`, `3B` `-84 / +17`, `3C` `-162 / +16`, `3D` `-70 / +18`, `3E` `-71 / +18`.
- **Best wins (`3D`/`3E`)**:
  - `scrollbar-grab-drag:54` (`self.scroll_to_visual(self.scroll + n);` vs. `self.on_mouse_scroll(false, n, row, col);`) dissolved and **resurfaced as a true move (`mov` ol:54 → nl:434 inside `fn on_mouse_scroll`)** — both scroll handlers now show clean extraction moves!
  - Dissolved garbled multi-island `let` / `pub fn` / `if` / `for` pairings across `scrollbar-grab-drag:2151/2152`, `container-compatibility:319`, `diffcoord-anchoring:28/197/519/527/581/385`, `theme-light-toggle:255`, `opt-4:302`, and 19 rows in `diffuse-history-mid`.
- **Casualties avoided by `3D`/`3E` vs. `3B`/`3C`**:
  - Preserved `impl Run {` → `impl<'a> Run<'a> {`, `if hit("▲") {` → `if wrapped_glyph_hit(...)`, `while share < d_bytes.len()`, `match &self.location {`, `if let Some(...) =`, and all valid subword renames (`line_hash: n.line_hash`, `MD_SGR_*`, `add_md_bold`, `syntax_token_color`).
  - `3C` was rejected for over-splitting valid 2-island subword renames (`-162`).
- **One surprise to verify in Step 6**: `diffuse-history-mid` newly paired `ol:57` (`if m.start() > last {`) with `nl:187` (`if is_ws_char(c) {`) after dissolving `64->187`; investigate why it passed during Step 6 test-first implementation before finalizing.

---

## Step 4 (Experiment) — Declaration Identity & `head_agrees` / `decl_compat` Tightening

- **Hypothesis**: Removing the `|| decl_compat` override at `evidence.rs:314` lets `admissible_from_runs_prechecked`'s trimmed 50% declaration guard reject unrelated declaration swaps without needing a separate declared-name parser.
- **Competing Variants**: `4A` (remove `|| decl_compat` alone) vs. `3E` (`3D` + remove `|| decl_compat`).
- **Probe**: Ran `4A` and `3E` across all 23 golden corpora.

### Result

- **Verdict**: **Merged & Promoted → Step 6 (Action)** (`4A` alone dissolved 4 unrelated `const`/`fn`/`pub fn` pairs; layered onto `3D` as `3E`, it dissolved 1 additional 1-island garbled function swap — `fn wholesale_pairs_render_as_baseline_rows() {` vs. `fn subword_diff_token_spans() {` — with zero casualties, rendering a separate `4B` declared-name parser unnecessary).

---

## Step 5 (Experiment) — Intra-Line Fragment Dissolution by Run Merging (Before Pair Dissolution)

- **Hypothesis**: Dissolving tiny, low-information interior `Common` bridges (chaff punctuation, whitespace-only separators, or short 1–2 char tokens trapped between substantial `Strike`/`Fill` edits) by merging them into a single contiguous replacement run inside [`AlignmentTape`](../crates/diffuse-core/src/tape/) achieves two wins at once:
  1. Legitimate modifications that only looked noisy because of stray interior bridges (e.g., `, `, `(`, `)`, or a coincidental 1-char variable inside a rewritten argument list) become crisp, readable `Mod` rows instead of being broken into `Del` + `Add`.
  2. Truly unrelated replacements (like `self.scroll_to_visual(self.scroll + n);` vs. `self.on_mouse_scroll(false, n, row, col);`) naturally collapse down to bare scaffolding (`self.` + `);`) with zero surviving salient anchors, letting **Step 6**'s post-tape salient floor dissolve them cleanly as a unified Tier 2 fallback.
- **Falsifier**:
  - Over-merging legitimate short structural separators (such as ` = ` in `let a = foo();` → `let b = bar();`, `, ` between distinct changed arguments in `foo(a, b)` → `foo(c, d)`, or `::` / `.` between changed path segments) could turn crisp, surgical sub-line edits into blurry whole-expression blobs.
  - Must respect balanced delimiter boundaries (never dissolve an opening `(` without its matching `)` when doing so would visually unbalance a kept outer call).
- **Competing Variants**:
  - **5A (Conservative chaff dissolution)**: Dissolve an interior `Common` chunk trapped between two edit spans only when it contains zero salient identifiers (pure whitespace/punctuation) AND is overwhelmed by its flanking edits (e.g., `bridge_chars <= 2` while both flanking edit spans exceed a minimum size, excluding top-level ` = ` / ` => ` / `: ` clause separators).
  - **5B (Weak-anchor dissolution)**: Same as 5A, plus dissolve tiny 1–2 char interior alphanumeric bridges (like lone `n` inside `(self.scroll + n)` → `(false, n, row, col)`) when flanked on both sides by large replacements whose combined edit mass dwarfs the bridge.
  - **5C (5B + Tier 2 post-tape gate `3E` layered on top)**: Measure how many of `3E`'s 71 dissolved pairs are *rescued* as crisp merged `Mod` rows vs. still cleanly dissolved into `Del` + `Add` when Tier 1 runs before Tier 2.
- **Probe**: Run uncommitted probes of `5A`, `5B`, and `5C` across all 23 golden corpora and present **visual before/after rendered diffs** (showing inline run boundaries clearly) across Best Wins, Rescued `Mod`s, Worst Casualties, and Borderline Cases for active human visual review before rewriting Step 6.

### Result

- **Verdict**: **Dropped from this plan** — spun out to [Intra-Line Fragment Dissolution by Run Merging.md](Intra-Line%20Fragment%20Dissolution%20by%20Run%20Merging.md) with an exact ANSI visual review pack at [intra-line-fragment-dissolution.ansi](reviews/intra-line-fragment-dissolution.ansi) (`less -R plan/reviews/intra-line-fragment-dissolution.ansi`) for dedicated colored review and discussion. Proceeding directly with **Step 6 (Action)** on this plan.

---

## Step 6 (Action) — Post-Tape Salient Floor, Multi-Island Fragmentation Gate, and `decl_compat` Removal

- **Module**: [`crates/diffuse-core/src/collapse/evidence.rs`](../crates/diffuse-core/src/collapse/evidence.rs)
- **Changes**:
  1. **Remove `|| decl_compat` override** in `pair_evidence_prepared` so `admissible_from_runs_prechecked`'s trimmed 50% declaration guard cannot be bypassed by unrelated same-role declarations (`fn foo()` vs. `fn bar()`, `let a = ...` vs. `let b = ...`). Remove dead `decl_role` / `are_roles_compatible` plumbing if unused elsewhere.
  2. **Post-tape salient coverage floor**: Compute `ctx_salient_chars(&runs, profile)` from surviving `Ctx` runs (same tokenization & salience rules as pre-tape). On non-`head_link`, non-`affix`, non-block-head, non-chain-head pairs with `min_salient > 0`, require `ctx_sal >= MIN_MATCHED_SALIENT_CHARS.min(min_salient)` and `ctx_sal / min_salient >= MIN_MATCHED_SALIENT_RATIO`.
  3. **Multi-island fragmentation gate**: Count contiguous non-`Ctx` edit islands (`count_edit_islands(&runs)`). When `!key_agrees && !affix && block_head_stem.is_none() && !is_chain_head && islands >= 2`, reject when `ctx_sal < MIN_MATCHED_SALIENT_CHARS.min(min_salient)` OR (`nws_ctx / larger < 0.25 && nws_ctx / smaller < 0.45`).
  4. **Investigate the `diffuse-history-mid (57->187)` surprise**: Verify why `if m.start() > last {` vs. `if is_ws_char(c) {` survived the `3E` probe (e.g., whether `count_edit_islands` or `is_token_salient_in_context` behaved unexpectedly) and cover it with a reproducing unit test before finalizing golden fixtures.

### Check Result

- **Tests & verification**: 627/627 `diffuse-core` unit tests and 47/47 dual-leg golden tests pass (`./scripts/check.sh` green; `big_hunk_perf` within budget). Removed unused `are_roles_compatible` from [`residual.rs`](../crates/diffuse-core/src/collapse/residual.rs) and `decl_role` from `PreparedRow`.
- **`diffuse-history-mid (57->187)` diagnosis**: `if m.start() > last {` vs. `if is_ws_char(c) {` collapsed `()` inside `AlignmentTape` (Condition B: empty vs. non-empty parens across distinct callers), leaving 1 edit island (`if [...] {`) with `ctx_sal == 0` and `raw_content == -11 <= 0`, which `head_agrees` (`if` == `if`) had still subsidized with `+320` in bonuses. Adding `!key_agrees && ctx_sal == 0 && raw_content <= 0` cleanly rejected it (while keeping `if scrollbar_seek {` → `if is_on_scrollbar {` and `if a {` → `if b {`), recovering `counts.insert(t.clone(), n - 1);` → `counts.insert(t, n - 1);` and `!used_adds.contains(&pos) && {` → `if !used_adds.contains(&pos) {` in `diffuse-history-mid`.
- **Golden corpus impact (`-76 dissolved, +19 created`; low-anchor non-group `Mod` rows 146 → 127 → 94)**:
  - `scrollbar-grab-drag:54` (`self.scroll_to_visual(self.scroll + n);` vs. `self.on_mouse_scroll(false, n, row, col);`) dissolved and **resurfaced as a true move (`mov` ol:54 → nl:434 inside `fn on_mouse_scroll`)** — both `MouseScrollUp` and `MouseScrollDown` now render as clean extraction moves.
  - Dissolved garbled pairings across `scrollbar-grab-drag:2151/2152`, `container-compatibility:319`, `diffcoord-anchoring` (unrelated `pub fn`, `if`, `let`, `source_lines`), `theme-light-toggle:255`, `opt-4:302`, and `diffuse-history-mid` (`fn wholesale_pairs_...`, unrelated `let`/`const`/`for`/`if`).
  - Preserved `impl Run {` → `impl<'a> Run<'a> {`, `while share < d_bytes.len()`, `match &self.location {`, `location: ...`, `line_hash: n.line_hash,`, and `MD_SGR_*` renames.
