# Intra-Line Fragment Dissolution by Run Merging

## Roadmap & Status

- [·] **Step 1 (Experiment)** — [Visual Review of Chaff-Bridge Dissolution (`5b`) & Tier 2 Interactions (`5e` / `5c`)](#step-1-experiment--visual-review-of-chaff-bridge-dissolution-5b--tier-2-interactions-5e--5c)
- [ ] **Step 2 (Action)** — [Synthesized Intra-Line Run Merging Action](#step-2-action--synthesized-intra-line-run-merging-action) *(populated after human visual review)*

---

## Executive Summary

Spun out from [Fragmented Pairing Rejection and Identifier Identity.md](done/Fragmented%20Pairing%20Rejection%20and%20Identifier%20Identity.md) (Step 5) for focused visual review in color.

> [!IMPORTANT]
> **Human visual input required before starting**: Before beginning Step 1 or Step 2 of this plan, prompt the human reviewer to inspect the colored ANSI review pack (`less -R plan/reviews/intra-line-fragment-dissolution.ansi`) and confirm their visual taste on the borderline merges and Tier 2 trade-offs.

Before dissolving a fragmented `Mod` pairing into separate `Del` + `Add` rows, dissolving tiny interior `Common` chaff bridges (stray punctuation, whitespace, or dwarfed 1–2 char tokens trapped between substantial flanking edits) inside [`AlignmentTape`](../crates/diffuse-core/src/tape/builder.rs) often turns a visually noisy `Mod` into one or two crisp, readable replacement spans — and automatically collapses truly unrelated replacements (`scrollbar-grab-drag:54`, `2151`, `diffuse-history-mid:415/416`) to zero surviving salient context so Tier 2 rejects them naturally.

Because the readability call on borderline merges (such as dissolving `oi`/`ni` in `let old_num = LineNum::new(oi).unwrap_or(LineNum::MIN);` → `let old_num = oi.max(1);`) depends heavily on full ANSI color rendering, this sub-plan captures an exact `diffuse -p` ANSI visual review pack so the human reviewer can inspect every affected hunk directly in `less -R` before promoting any rule into production.

---

## ANSI Visual Review Pack

Inspect the exact colored before/after `diffuse -p` renders directly in your terminal:

```bash
less -R plan/reviews/intra-line-fragment-dissolution.ansi
```

File: [intra-line-fragment-dissolution.ansi](reviews/intra-line-fragment-dissolution.ansi) (2,571 lines across three progressive sections):

1. **Section 1 — Probe `5b` vs. baseline (Tier 1 only, zero `evidence.rs` changes)**:
   - Dissolves interior `Common` bridges inside `AlignmentTape::build` when flanked by substantial edits on both sides (whitespace `>= 3+3` chars; non-delimiter punctuation or 1–2 char alphanumeric `>= 4+4` chars with `>= 10:1` edit-to-bridge ratio; protects `()`, `[]`, `{}`, `<>` and clause separators `=`, `=>`, `:`, `->`, `::`, `==`, `!=`, `<=`, `>=`, `&&`, `||`).
   - Shows every single hunk across all 23 golden corpora (run-merged `Mod`s such as `review-notes:538`, `diffcoord-anchoring:317/545`, `diffuse-history-mid:9`, `rust-lifetime-char:235/567`, borderline `linenum-u32:871-872`, plus the 4 rows auto-dissolved by Tier 1 alone including `scrollbar-grab-drag:54` resurfacing as a true `mov` to line 434).
2. **Section 2 — Probe `5e` vs. `5b` (Middle-ground Tier 2 delta)**:
   - Adds post-tape salient coverage floor (`ctx_sal >= 4.min(min_salient)` & `>= 30%`), removes `|| decl_compat`, and rejects non-`key_agrees` multi-island pairs when `ctx_sal < min_sal_floor` (without the `< 25%` coverage ratio cutoff, preserving pairs with real salient anchors such as `r_first` or `wrapping_mul`).
3. **Section 3 — Probe `5c` vs. `5e` (Coverage-cutoff delta)**:
   - Shows every hunk additionally dissolved when adding the `< 25%` larger / `< 45%` smaller multi-island non-whitespace coverage cutoff on top of `5e`.

---

## Step 1 (Experiment) — Visual Review of Chaff-Bridge Dissolution (`5b`) & Tier 2 Interactions (`5e` / `5c`)

- **Hypothesis**: Dissolving dwarfed interior `Common` chaff bridges inside `AlignmentTape` (`5b`) makes kept `Mod` rows cleaner while letting Tier 2 (`5e` or `5c`) fall back cleanly only on genuinely unrelated pairs.
- **Falsifier**: Merging short live variables (`oi` in `linenum-u32:871`) or structural tokens might obscure real edits in full color, or `5b` might require tightening its alphanumeric edit-to-bridge ratio (e.g., `12:1` / `15:1` or punctuation-only `5a`).
- **Competing Variants**: `5a` (punctuation/whitespace chaff only), `5b` (`5a` + dwarfed 1–2 char alphanumeric bridges with `<>` delimiter guard), plus Tier 2 pairings `5e` vs. `5c`.
- **Probe**: Captured exact ANSI renders in [intra-line-fragment-dissolution.ansi](reviews/intra-line-fragment-dissolution.ansi) for human `less -R` review.

### Result

*(Pending human visual review of `less -R plan/reviews/intra-line-fragment-dissolution.ansi`.)*

---

## Step 2 (Action) — Synthesized Intra-Line Run Merging Action

*(To be specified after Step 1 visual review confirms the desired bridge-dissolution rules and thresholds.)*
