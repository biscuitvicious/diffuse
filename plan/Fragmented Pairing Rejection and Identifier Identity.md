# Fragmented Pairing Rejection and Identifier Identity

## Roadmap & Status

- [ ] **Step 1 (Action)** — [Fix Indentation-Inflated Content Accounting & Multiset Bag Capping](#step-1-action--fix-indentation-inflated-content-accounting--multiset-bag-capping)
- [ ] **Step 2 (Experiment)** — [Whole-Identifier Identity for Pairing Admissibility](#step-2-experiment--whole-identifier-identity-for-pairing-admissibility)
- [ ] **Step 3 (Experiment)** — [Post-Tape Salient Coverage & Fragmentation / Chaff-Anchor Gate](#step-3-experiment--post-tape-salient-coverage--fragmentation--chaff-anchor-gate)
- [ ] **Step 4 (Experiment)** — [Declaration Identity & `head_agrees` / `decl_compat` Tightening](#step-4-experiment--declaration-identity--head_agrees--decl_compat-tightening)
- [ ] **Step 5+ (Action)** — [Synthesized Action(s) from Surviving Experiments](#step-5-action--synthesized-actions-from-surviving-experiments) *(populated by experiment results)*

---

## Executive Summary

When two lines share minimal scaffolding and a single short identifier or coincidental subword inside unrelated compound names — for example:

```rust
- self.scroll_to_visual(self.scroll.saturating_sub(n));
+ self.on_mouse_scroll(true, n, row, col);
```

the pair is technically admissible today (`Ctx` runs: `self.`, `(`, `n`, `);`), yet renders as visual garble because the edits overwhelmingly overpower the surviving anchors.

Root-cause analysis of [`evidence.rs`](../crates/diffuse-core/src/collapse/evidence.rs) identified two outright accounting bugs alongside three open design questions of varying confidence:

1. **Confirmed bug (leading-indent inflation)**: `matched_chars` and `is_decl_head`'s `ctx_chars` include leading indentation spaces while `char_count` uses trimmed line length. Sixteen spaces of indentation inflate `matched` from `9` to `25`, flipping `raw_content` on the example above from `-42` (rejected) to `+166` (accepted) and defeating the 50% context guard on deeply indented `let` / `pub fn` replacements.
2. **Confirmed bug (uncapped set-membership bag)**: The pre-tape salient gate checks `v.all_tokens.contains(token)` against a set rather than multiset intersection, allowing repeated subwords (`scroll` in both `scroll_to_visual` and `self.scroll`) to double-count against a single subword on the opposite side.
3. **Open design questions**: Separating whole-identifier pairing identity from subword intra-line diffing, evaluating rendered-run fragmentation and chaff anchors post-tape, and tightening keyword-only declaration pairing (`let`, `pub fn`).

Per the diffing domain prior in [`Plan Guidelines.md`](Plan%20Guidelines.md), admitting a bad pairing is roughly 3× worse than dissolving a desired one. This plan fixes the two confirmed bugs first (**Step 1**), then explores the remaining design clusters through fast, comparative **Experiments** (**Steps 2–4**) that fold back into and rewrite this document until only high-confidence **Actions** remain.

---

## Living Truth Table

| Expected | Old Line | New Line | Rationale |
| :--- | :--- | :--- | :--- |
| **SPLIT** | `self.scroll_to_visual(self.scroll.saturating_sub(n));` | `self.on_mouse_scroll(true, n, row, col);` | Different methods, different args; only `self.`, punctuation, and 1-char `n` survive (`scroll` is cross-identifier subword). |
| **SPLIT** | `self.scroll_to_visual(self.scroll + n);` | `self.on_mouse_scroll(false, n, row, col);` | Same pattern (`app/dispatch.rs:54`). |
| **SPLIT** | `if !seg.is_empty() {` | `if is_ws_char(next_c) \|\| is_delim_char(next_c) \|\| is_op_char(next_c) {` | Only `if`, clipped subword `is_`, and `{` survive. |
| **SPLIT** | `let top = stack.pop();` | `let mut out = vec![0u8; items.len()];` | Unrelated `let` statements sharing only keyword + indent. |
| **SPLIT** | `pub fn note_anchors_cleanly(note: &ReviewNote, line_content: &str) -> bool {` | `pub fn label(self) -> String {` | Unrelated function declarations sharing only `pub fn` + `{`. |
| **PAIR** | `self.scroll_to_visual(target_vis);` | `self.on_mouse_scroll(target_vis);` | Shared whole argument `target_vis`, single clean edit island. |
| **PAIR** | `location: NoteLocation::line(side_val, start_line, end_line),` | `location: provisional_location,` | Shared field key (`key_agrees`), single RHS replacement. |
| **PAIR** | `if scrollbar_seek {` | `if is_on_scrollbar {` | Short control-flow swap with 1 clean edit island. |
| **PAIR** | `let hash = notes[0].line_hash;` | `let hash = notes[0].content_hash;` | Legitimate single-identifier rename; subword diff (`_hash`) remains crisp once paired. |

---

## Step 1 (Action) — Fix Indentation-Inflated Content Accounting & Multiset Bag Capping

- **Module**: [`crates/diffuse-core/src/collapse/evidence.rs`](../crates/diffuse-core/src/collapse/evidence.rs)
- **Trimmed-coordinate `matched_chars` and `ctx_chars`**:
  - Strip leading horizontal whitespace from the first `Ctx` run before counting matched characters in `matched_chars(&runs)` and in `admissible_from_runs_prechecked`'s `is_decl_head` 50% guard, restoring `matched <= smaller <= larger` on trimmed coordinates.
  - Ensure `total_chars` in `is_decl_head` likewise measures trimmed lengths.
- **Multiset intersection in pre-tape salient bag**:
  - Replace `v.all_tokens: FxHashSet<&str>` membership with multiplicity-capped counting (`FxHashMap<&str, usize>`): each distinct token `t` contributes `min(count_u(t), count_v(t)) * char_len(t)` to `matched_salient`, restoring `u`/`v` symmetry and preventing double-counting.

### Check Result

*(Recorded after execution: test outcomes, golden delta summary, and post-Step-1 low-anchor corpus baseline to inform Steps 2–4.)*

---

## Step 2 (Experiment) — Whole-Identifier Identity for Pairing Admissibility

- **Hypothesis**: Evaluating pairing-eligibility salient overlap on whole compound identifiers (`[A-Za-z0-9_]` runs) rather than subword tokens eliminates cross-identifier false matches (`is_empty` vs `is_ws_char`, `thumb_row` vs `thumb_off`, `source_lines` vs `original_source_rows`) while preserving subword diffing inside the intra-line LCS renderer once paired.
- **Falsifier**: Legitimate single-identifier renames on short lines with no other shared identifier (`old_foo_bar()` → `new_foo_bar()`, `pub line_hash: Hash,` → `pub content_hash: Hash,`) might break apart if whole-identifier matching lacks a clean-rename escape hatch.
- **Competing Variants**:
  - **2A (Strict whole-identifier bag)**: Pre-tape and post-tape salient matching require whole-identifier equality.
  - **2B (Whole-identifier primary + single-island subword fallback)**: Full credit for whole identifiers; subword overlap only counts inside a single clean edit island.
- **Probe**: Run uncommitted probes of 2A and 2B across all 23 golden corpora after Step 1; inspect Best Wins / Worst Casualties / Surprises against the 3× false-positive prior.

### Result

*(Pending Step 1.)*

---

## Step 3 (Experiment) — Post-Tape Salient Coverage & Fragmentation / Chaff-Anchor Gate

- **Hypothesis**: Judging the actual rendered `Vec<Run>` from the alignment tape directly captures visual garble: re-enforcing the salient floor on surviving `Ctx` runs catches unordered pre-tape matches that the LCS tape rejected, and gating multi-island edits separated only by punctuation or 1–2 char chaff anchors separates clean 1-island swaps from confetti.
- **Falsifier**: Multi-argument call updates (`foo(a, x, b)` → `foo(c, x, d)`) could over-split, and peer reflow group heads (`1×N` / `N×1` anchor rows sharing only an expression prefix before `reflow.rs` attaches continuations) must not be prematurely rejected.
- **Competing Variants**:
  - **3A (Post-tape salient floor only)**: Re-apply the salient char/ratio floor on surviving `Ctx` runs.
  - **3B (Post-tape salient floor + multi-island chaff gate)**: Reject `>= 2` edit islands whose interior `Ctx` bridges fall below an anchor-quality floor, exempting 1-island edits and `key_agrees`.
  - **3C (Intra-line chaff dissolution vs. pair rejection)**: Compare dissolving trapped 1-char/punctuation `Ctx` crumbs inside `lcs` vs. dissolving the `Mod` pair into `Del` + `Add`.
- **Probe**: Run uncommitted A/B probes across all 23 golden corpora, auditing both standalone `Mod` rows and `g:` peer-reflow anchor rows.

### Result

*(Pending Steps 1–2.)*

---

## Step 4 (Experiment) — Declaration Identity & `head_agrees` / `decl_compat` Tightening

- **Hypothesis**: Two declarations (`let`, `fn`, `pub fn`, `const`, `struct`, `enum`, `type`) sharing only a leading keyword and neither their declared entity name nor meaningful body overlap (`let top = stack.pop();` vs `let mut out = vec![0u8; items.len()];`, `pub fn note_anchors_cleanly(...)` vs `pub fn label(self)`) should split into `Del` + `Add`, while preserving `key_agrees` and short single-edit statements.
- **Falsifier**: Function renames with shared parameter/return signatures (`fn old_name(a: &Foo) -> Bar {` → `fn new_name(a: &Foo) -> Bar {`) and `let` variable renames with shared RHS (`let old = compute(x);` → `let new = compute(x);`) must remain paired.
- **Competing Variants**:
  - **4A (Drop `head_link` salient bypass for declaration keywords)**: Keep `HEAD_AGREE_BONUS` as a score tie-breaker, but require declarations to pass the normal salient gate.
  - **4B (Declared-name agreement as `key_agrees`)**: Grant full bypass when the declared entity name agrees; otherwise require normal salient overlap.
- **Probe**: Run uncommitted probes across all 23 golden corpora, auditing every affected `let` and `fn` pair.

### Result

*(Pending Steps 1–3.)*

---

## Step 5+ (Action) — Synthesized Action(s) from Surviving Experiments

*(Populated once Steps 2–4 converge into high-confidence, minimal invariants.)*
