# Information-Weighted Edge Factoring and Evidence-Gated Relocation — Engineering Plan

- [x] Phase 0 — Built-in Type & Operator Keyword Recognition in LangProfile (`crates/diffuse-syntax/`, `crates/diffuse-core/`)
- [ ] Phase 1 — Information-Theoretic Edge Weighting & Fragile Edge Pruning in Cluster Factoring (`crates/diffuse-core/src/cluster/factor.rs`)
- [ ] Phase 2 — Competing Destination Affinity in Token Relocation (`crates/diffuse-core/src/collapse/slot.rs`)
- [ ] Phase 3 — Evidence-Gated Cross-Slot Relocation & Anti-Scavenging Floor on Add Rows (`crates/diffuse-core/src/collapse/slot.rs`)
- [ ] Phase 4 — Multi-Line Expression Collapse in Micro-DP Reflow (`crates/diffuse-core/src/collapse/slot.rs`)
- [ ] Phase 5 — Golden Corpus Calibration, Asymmetric Drift Audit & Final Sign-Off (`tests/golden/`)

---

## 1. Executive Summary

In structured diff analysis, the pairing of deletion and addition lines into unified modification rows (`RowKind::Mod`) and the relocation of sub-line tokens (`RunKind::MovedSrc`/`RunKind::MovedTgt`) must reflect genuine code evolution.

A critical defect in the current pipeline is **unweighted binary information edges** and **ungated greedy cross-slot relocation**:
1. **Unrecognized Primitive Types as False Salient Identifiers**: In `crates/diffuse-syntax`, `LangProfile` only recognizes types if they begin with an uppercase letter (`types_by_case: true`). Lowercase primitive types (`f64`, `usize`, `i32`, `bool`, `str`, `int`, etc.) are classified as `TokenColor::Default`. Consequently, the diff engine in `diffuse-core` considers them custom domain identifiers. A type-coercion phrase like `"as f64"` is treated as containing a salient identifier (`f64`), which strips all pair-gating protections and allows it to relocate across unrelated code.
2. **Asymmetric Small-Fragment Trap in Factoring**: In `crates/diffuse-core/src/cluster/factor.rs`, candidate edges are unweighted booleans (`edges.push((u, v))`). If a small deleted line $u$ (e.g. `.round()`, 5 chars) is trivially contained in a large, completely unrelated statement $v$ (e.g. 87 chars), `is_full_containment` fires, creating an edge. This binds completely independent statements into an irreducible reflow group.
3. **Frankenstein Token Scavenging**: In `crates/diffuse-core/src/collapse/slot.rs`, `relocate_cross_slot_tokens` gathers sub-tokens globally across the group. For tokens categorized as `salient`, **no pair gating is applied**.
4. **Greedy First-Come Slot Iteration**: The relocation loop iterates target slots in ascending order (`for tgt in 0..num_slots`). An earlier, completely unrelated addition slot (e.g. new line 241: `let min_thumb = ...`) greedily intercepts tokens from deleted lines before their true, contextually matched destination (e.g. new line 243: `let prop_thumb = ...`) is ever evaluated.

This produces visually absurd diffs where a brand-new addition line is rendered as a modified row (`~+`) stitched together out of micro-scraps stolen from two or three separate deleted lines.

```mermaid
flowchart TD
    subgraph CurrentPipeline ["Current Pipeline (Greedy & Unweighted)"]
        P1["Primitive types treated as salient identifiers (f64, usize)"]
        F1["Factor Cluster (Unweighted binary edges allow small-fragment trap)"]
        DP1["Micro-DP (1-to-1 slot alignment leaves fragments unpaired)"]
        Reloc1["Relocate Tokens (Ungated salient loop; greedy first-slot-wins)"]
        Diff1["Visual Result: Line 241 (~+) steals scraps from old 233 and 234"]
        P1 --> F1 --> DP1 --> Reloc1 --> Diff1
    end

    subgraph ProposedPipeline ["Proposed Information-Weighted Pipeline"]
        P2["Builtin Types & Operator Keywords Demoted from Salience (f64, as)"]
        F2["Information-Theoretic Edge Weighting E(u, v) (Prune fragile < 20% edges)"]
        DP2["Micro-DP with Expression Reflow (Absorb chained expressions naturally)"]
        Reloc2["Evidence-Gated Relocation & Argmax Affinity (Anti-scavenging floor)"]
        Diff2["Visual Result: Line 241 is pure Add (+); Line 243 receives its true .round()"]
        P2 --> F2 --> DP2 --> Reloc2 --> Diff2
    end
```

### Architectural Principles and Mathematical Invariants

1. **Asymmetric Loss Principle ($3\times$ Penalty)**: A false positive pairing or hallucinated token move ("bad" pairing) is at least $3\times$ worse than an honest unpaired deletion and addition. An honest `Del` + `Add` is clear and visually transparent; a spurious pairing or Frankenstein move hallucinates nonsensical struck/filled diffs, destroys visual accounting, and confuses the code reviewer.
2. **Language Profile Agnosticism and Primitive Demotion**:
   Standard built-in types (`f64`, `usize`, `bool`, `int`, etc.) and word operators (`as`, `in`, `is`) are structural language vocabulary, not domain-specific salient tokens. They must be registered in `LangProfile`, highlighted appropriately as `TokenColor::Type`, and demoted from salient identifier status alongside keywords.
3. **Mutual Information Explanation Ratio ($\mathcal{E}(u, v)$)**:
   The explanatory power of an edge between lines $u$ and $v$ is quantified by the ratio of shared information to the total information spanned:
   $$\mathcal{E}(u, v) = \frac{I(M(u, v))}{I(u) + I(v) - I(M(u, v))}$$
   Where $M(u, v)$ represents the aligned salient characters, weighted by token entropy. Edges falling below a critical explanation floor ($\mathcal{E} < 0.20$) represent coincidental vocabulary overlap and must be pruned.
4. **Distance-Decayed Information Transport Invariant**:
   Information edges and token relocations decay with the spatial distance $\Delta$ they must travel:
   - Strong edges (high information explanation ratio, e.g. $\mathcal{E} \ge 0.70$ or large unique blocks) possess high transport capacity and are permitted to travel far ($\Delta \ge 10$).
   - Weak edges (low explanation ratio, e.g. $\mathcal{E} \le 0.35$ or single-token overlap) have low transport capacity and can only bridge immediate, near-zero movements ($\Delta \le 1$–$2$).
   - In cross-slot token relocation, candidate travel radius scales dynamically with token information content: weak sub-tokens ($\le 8$ chars) are restricted to immediate neighbor slots ($|S_{\text{src}} - S_{\text{tgt}}| \le 1$), while high-entropy tokens may traverse the full window.
5. **Argmax Contextual Affinity**: When a token $T$ can match multiple candidate destinations $\{S_{\text{tgt}}\}$, the engine must evaluate global contextual affinity $\mathcal{A}(S_{\text{src}}, S_{\text{tgt}})$ (shared surrounding operands and expression continuity, penalized by distance) and assign $T$ to the maximal affinity target, replacing greedy index-order iteration.
6. **Anti-Scavenging Recipient Coverage Floor**: An unpaired addition row (`RowKind::Add`) represents net-new logic. It cannot accept cross-slot moved tokens unless the relocated tokens explain a substantial fraction (e.g. $\ge 35\%$) of the addition line.
7. **Single-Donor Spatial Cohesion**: A single line must not assemble itself out of disparate micro-fragments scavenged from multiple distinct deletion slots.
8. **Closed-Economy Token Conservation**: All transformations must preserve strict token conservation:
   $$\sum \text{len}(\text{MovedSrc}) = \sum \text{len}(\text{MovedTgt})$$

---

## 2. Table of Contents

0. [Phase 0 — Built-in Type & Operator Keyword Recognition in LangProfile (`diffuse-syntax`, `diffuse-core`)](#0-phase-0--built-in-type--operator-keyword-recognition-in-langprofile-diffuse-syntax-diffuse-core)
1. [Phase 1 — Information-Theoretic Edge Weighting & Fragile Edge Pruning in Cluster Factoring](#1-phase-1--information-theoretic-edge-weighting--fragile-edge-pruning-in-cluster-factoring)
2. [Phase 2 — Competing Destination Affinity in Token Relocation](#2-phase-2--competing-destination-affinity-in-token-relocation)
3. [Phase 3 — Evidence-Gated Cross-Slot Relocation & Anti-Scavenging Floor on Add Rows](#3-phase-3--evidence-gated-cross-slot-relocation--anti-scavenging-floor-on-add-rows)
4. [Phase 4 — Multi-Line Expression Collapse in Micro-DP Reflow](#4-phase-4--multi-line-expression-collapse-in-micro-dp-reflow)
5. [Phase 5 — Golden Corpus Calibration, Asymmetric Drift Audit & Final Sign-Off](#5-phase-5--golden-corpus-calibration-asymmetric-drift-audit--final-sign-off)

---

## 3. Phase Breakdown

### 0. Phase 0 — Built-in Type & Operator Keyword Recognition in LangProfile (`diffuse-syntax`, `diffuse-core`)

#### Problem
1. In `crates/diffuse-syntax`, `LangProfile` does not have a `builtin_types` table. Lowercase primitive types (`f64`, `usize`, `i32`, `bool`, `int`, etc.) are classified as `TokenColor::Default` rather than `TokenColor::Type`.
2. In `crates/diffuse-core`, `is_salient_token` filters out `keywords` and `literals`, but treats primitive types as custom domain identifiers. A phrase like `"as f64"` is marked as containing a salient identifier, bypassing the bare-delimiter/glue protection gate and relocating freely across unrelated lines.

#### Implementation Steps
1. **Extend `LangProfile` in `diffuse-syntax`** (`crates/diffuse-syntax/src/profile/lang.rs`):
   - Add `pub builtin_types: &'static [&'static str]` (sorted slice for $O(\log n)$ lookup).
   - Add `pub fn is_builtin_type(&self, tok: &str) -> bool { self.builtin_types.binary_search(&tok).is_ok() }`.
   - Add `pub operator_keywords: &'static [&'static str]` for word-spelled operators (e.g. `as`, `in`, `is`).
   - Populate across language modules (`crates/diffuse-syntax/src/language/`):
     - **Rust**: `bool`, `char`, `f32`, `f64`, `i8`..`i128`, `isize`, `str`, `u8`..`u128`, `usize`.
     - **TypeScript / JavaScript**: `any`, `bigint`, `boolean`, `never`, `null`, `number`, `object`, `string`, `symbol`, `undefined`, `unknown`, `void`.
     - **Go**: `bool`, `byte`, `rune`, `string`, `int`..`int64`, `uint`..`uintptr`, `float32`, `float64`, etc.
     - **C / C++**: `bool`, `char`, `double`, `float`, `int`, `long`, `short`, `signed`, `size_t`, `unsigned`, `void`, `int32_t`, etc.
     - **Python**: `bool`, `bytes`, `dict`, `float`, `int`, `list`, `object`, `set`, `str`, `tuple`.
2. **Update Token Classification** (`crates/diffuse-syntax/src/classifier/token.rs`):
   - In `for_each_classified_token_dyn`, if `profile.is_builtin_type(tok)`, classify as `TokenColor::Type`.
3. **Demote Built-in Types from Salience in `diffuse-core`**:
   - In `crates/diffuse-core/src/collapse/slot.rs`:
     Update `is_salient_token(t, profile)` to return `false` if `profile.is_some_and(|p| p.is_builtin_type(t))`.
   - In `crates/diffuse-core/src/cluster/factor.rs`:
     Add `"as"` to `COMMON_KEYWORDS`. Filter out `profile.is_builtin_type(w)` in `significant_words`.
4. **Tests**:
   - Unit tests in `crates/diffuse-syntax/src/tests.rs`:
     - Assert `classify_tokens("let x = 1.0 as f64;", Some(rust))` colors `f64` as `TokenColor::Type` and `as` as `TokenColor::Keyword`.
     - Assert `classify_tokens("val: number = 5;", Some(ts))` colors `number` as `TokenColor::Type`.
   - Unit test in `crates/diffuse-core/src/collapse/slot.rs`:
     - Assert `text_has_salient_token("as f64", Some(rust))` evaluates to `false`.
     - Assert `"as f64"` is treated as a bare candidate and cannot relocate without an existing salient link.
5. **Golden Drift Validation Check**:
   - Run acceptance tests across all golden corpora.
   - Categorize drift:
     - *Desirable*: Primitive types now carry standard type syntax coloring (`typ`) in wire JSONL; spurious relocations of `as f64` or `as usize` disappear.
     - *Undesirable*: Legitimate moves broken (none expected; true relocations carry domain identifiers).
   - Assert asymmetric loss condition $\mathcal{L} < \mathcal{L}_{\text{prior}}$.

---

### 1. Phase 1 — Information-Theoretic Edge Weighting & Fragile Edge Pruning in Cluster Factoring

#### Problem
In `crates/diffuse-core/src/cluster/factor.rs`:
- `has_information_edge_with_words` forms unweighted boolean edges.
- When a short deleted line (e.g. `.round()`, 5 chars) is contained in a large new statement (e.g. `let min_thumb = ...`, 87 chars), `is_full_containment` evaluates to `true` because `matched_salient == min_salient`.
- This creates an edge that ropes unrelated statements into the same compound reflow group.

#### Implementation Steps
1. **Define Mutual Information Explanation Ratio** (`crates/diffuse-core/src/cluster/factor.rs`):
   - Implement `calculate_edge_explanation_ratio(del_text, add_text, d_words, a_words) -> f64`.
   - Calculate bilateral coverage:
     $$\text{ratio}_{\text{del}} = \frac{\text{matched\_salient}}{\text{del\_salient}}, \quad \text{ratio}_{\text{add}} = \frac{\text{matched\_salient}}{\text{add\_salient}}$$
   - Eliminate the pure unilateral `is_full_containment` bypass when the counterpart ratio is below a noise floor: if $\text{ratio}_{\text{add}} < 0.20$ (the matched token explains less than 20% of the addition), the edge is rejected unless the matched token itself is a high-entropy compound identifier ($L \ge 16$).
2. **Distance-Penalized Edge Thresholding & Pruning**:
   - Replace raw boolean edge emission in `factor_cluster_with_content` with weighted edges `(usize, usize, f64)`.
   - Scale the required explanation threshold $\tau_{\text{min}}$ with offset distance $\Delta = |u - v|$:
     $$\tau_{\text{min}}(\Delta) = \tau_0 + \beta \cdot \log(1 + \Delta)$$
     Where in-place/adjacent lines ($\Delta \le 1$) require a baseline $\tau_0 = 0.25$, while lines traveling across $\Delta \ge 5$ lines must clear $\tau_{\text{min}} \ge 0.50$ (or contain an exact multi-word landmark).
   - Prune fragile edges falling below $\tau_{\text{min}}(\Delta)$.
   - Where a line has competing edges, suppress weak secondary edges whose weight is less than $0.5 \times$ the dominant edge.
3. **Tests**:
   - Unit tests in `crates/diffuse-core/src/cluster/factor.rs`:
     - Short method call (`.round()`) does not form an information edge with an 87-character unrelated variable assignment (`let min_thumb = ...`).
     - Real reflowed expressions (`((vh as f64...` $\leftrightarrow$ `let prop_thumb = ...`) maintain strong edges ($W \ge 0.60$).
4. **Golden Drift Validation Check**:
   - Run acceptance tests across all 23 golden corpora.
   - Categorize drift:
     - *Desirable*: Groups that previously pooled unrelated statements now cleanly factor into separate, cohesive groups.
     - *Undesirable*: True compound reflow groups erroneously shattered.
   - Assert asymmetric loss condition: $\mathcal{L} = 3 \times N_{\text{bad}} + 1 \times N_{\text{dissolved}} < \mathcal{L}_{\text{prior}}$.

---

### 2. Phase 2 — Competing Destination Affinity in Token Relocation

#### Problem
In `crates/diffuse-core/src/collapse/slot.rs`:
- When candidate token $T$ (e.g. `.round()`) is available in a deletion slot and multiple addition slots contain $T$, `apply_relocation_candidate` iterates `for tgt in 0..num_slots`.
- Target slot 1 (`let min_thumb = ...`) is checked first and steals `.round()`, starving target slot 3 (`let prop_thumb = ...`), which is the true AST recipient.

#### Implementation Steps
1. **Candidate Destination Scoring with Distance Decay** (`crates/diffuse-core/src/collapse/slot.rs`):
   - For a given candidate token $T$ from source slot `src`, identify all eligible target slots $\{ \text{tgt}_k \}$.
   - For each target slot, compute distance-decayed contextual affinity $\mathcal{A}(\text{src}, \text{tgt})$:
     $$\mathcal{A}(\text{src}, \text{tgt}) = \frac{\text{shared\_salient\_chars}(\text{src}, \text{tgt}) + \text{adjacent\_operand\_bonus}}{1 + \lambda \cdot |\text{src} - \text{tgt}|}$$
     Where:
     - `shared_salient_chars` counts shared words between `slot_runs[src]` and `slot_runs[tgt]` outside of $T$.
     - `adjacent_operand_bonus` rewards target slots paired with adjacent rows that share expressions with `src`.
     - $\lambda$ penalizes spatial slot distance.
2. **Argmax Target Selection**:
   - Sort eligible target slots by descending contextual affinity $\mathcal{A}(\text{src}, \text{tgt})$ before applying relocation.
   - Award the token to the highest-affinity recipient.
3. **Tests**:
   - Unit test in `crates/diffuse-core/src/collapse/slot.rs`:
     - In a group with deletion `.round()`, addition `let min_thumb = (...).round()`, and addition `let prop_thumb = ((vh as f64...).round()`, `.round()` relocates to `prop_thumb` due to shared operand affinity, not `min_thumb`.
4. **Golden Drift Validation Check**:
   - Verify that reflowed tokens land in their structurally affiliated destination lines across all corpora.

---

### 3. Phase 3 — Evidence-Gated Cross-Slot Relocation & Anti-Scavenging Floor on Add Rows

#### Problem
In `crates/diffuse-core/src/collapse/slot.rs`:
- Salient tokens currently have `gate: None`. Any deletion and addition slot within `CROSS_SLOT_WINDOW` can exchange them.
- Generic language constructs (`as f64`, `.round()`, `usize`, `None`) qualify as "salient" simply because they contain letters, leading to coincidental scavenging.
- Unpaired addition rows (`RowKind::Add`) scavenge scraps from multiple deletion rows.

#### Implementation Steps
1. **Extend Gate to Weak Salient Tokens & Variable Travel Radius**:
   - Distinguish high-entropy tokens (length $\ge 12$ or non-standard identifier) from generic sub-tokens (primitive types, common single-word methods $\le 8$ chars).
   - Require generic sub-tokens to pass a mutual anchor gate: the source and target slots must already share another salient token, or be part of a paired `Mod` alignment.
   - Enforce distance-bounded travel radius based on token information content:
     - Weak sub-tokens are restricted to immediate neighbor slots: $|\text{src} - \text{tgt}| \le 1$.
     - Strong, compound tokens may travel up to `CROSS_SLOT_WINDOW = 4`.
2. **Anti-Scavenging Floor on Pure Add Rows**:
   - If `slot_runs[tgt]` is a pure `Add` row (`old_line: None`):
     - Calculate recipient coverage:
       $$\text{coverage} = \frac{\sum \text{len}(T_{\text{relocated}})}{\text{len}(\text{slot\_text})}$$
     - Require $\text{coverage} \ge 0.35$ before allowing the addition row to accept relocated tokens.
     - Single-donor rule: prohibit an `Add` row from taking tokens from more than one distinct deletion slot unless the donors form contiguous lines.
3. **Tests**:
   - Integration test in `crates/diffuse-tui/tests/app_integration.rs`:
     - In `scrollbar-grab-drag` `app/geometry.rs`, line 241 renders as pure addition `+`, without any struck tokens from old 233 or 234.
     - Line 242 renders as pure addition `+`, without any struck closing parenthesis from old 235.
4. **Golden Drift Validation Check**:
   - Assert that false "modified addition" lines (`~+`) dissolve back to clean additions (`+`) without stealing unrelated code.

---

### 4. Phase 4 — Multi-Line Expression Collapse in Micro-DP Reflow

#### Problem
In `crates/diffuse-core/src/collapse/slot.rs`:
- The micro-DP solver operates strictly on 1-to-1 line slot alignments.
- When an old 2-line expression (`u1: ((vh as f64...` + `u2: .round()`) collapses into a single line (`v2: let prop_thumb = ((vh as f64...).round() as usize;`), the 1-to-1 solver matches `u1 ↔ v2` and strands `u2` as an orphan deletion.
- This orphaned deletion is what triggers downstream cross-slot relocation to hunt for a home.

#### Implementation Steps
1. **Multi-Line Expression Merging in Micro-DP**:
   - In the DP solver, detect contiguous expression chaining across line breaks (lines starting with `.` or binary operators, or open delimiter continuation).
   - Permit $2 \to 1$ collapse transitions where $u_i + u_{i+1}$ matches $v_j$ with high character coverage.
   - Spliced runs automatically absorb the chained method call directly into the modification row without entering cross-slot relocation.
2. **Tests**:
   - Unit test in `crates/diffuse-core/src/collapse/tests.rs`:
     - Multi-line chained call collapsing into a single line pairs naturally as a unified reflow row.
3. **Golden Drift Validation Check**:
   - Verify that multi-line expression reflows assemble cleanly into single compound rows rather than fracturing into Del + Add runs.

---

### 5. Phase 5 — Golden Corpus Calibration, Asymmetric Drift Audit & Final Sign-Off

#### Implementation Steps
1. **Full Verification Gate**:
   - Run `./scripts/check.sh --no-test` (nightly `cargo fmt`, `clippy` with zero warnings).
   - Run warm workspace test suite (`cargo test --workspace`).
2. **Comprehensive Golden Acceptance Audit**:
   - Run all 45 golden acceptance tests across both library and CLI binary legs:
     `cargo test -p diffuse --test golden`
   - For every diff in `.wire.jsonl`, `.render.txt`, and `.markdown.md`:
     - Apply the **Asymmetric Evaluation Rule**:
       - Score **+3** for every eliminated spurious match, halted Frankenstein theft, or dissolved false `Mod` row.
       - Score **-1** for any dissolved marginal pairing that was visually acceptable.
       - Any regression on a **Validated Golden** (`theme-scrollbar-palette`, `opt-4-manual-scanner-prefilter`, `container-compatibility`, `scrollbar-grab-drag`) requires individual line-by-line justification.
3. **Pre-Commit Review & User Sign-Off**:
   - Prepare structured report of all golden diffs with before/after comparisons.
   - Pause for user inspection and explicit sign-off before committing.
