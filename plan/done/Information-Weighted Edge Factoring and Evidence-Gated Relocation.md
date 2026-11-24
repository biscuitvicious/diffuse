# Information-Weighted Edge Factoring and Evidence-Gated Relocation — Engineering Plan

- [x] Phase 0 — Built-in Type & Operator Keyword Recognition in LangProfile (`crates/diffuse-syntax/`, `crates/diffuse-core/`)
- [x] Phase 1 — Information-Theoretic Edge Weighting & Fragile Edge Pruning in Cluster Factoring (`crates/diffuse-core/src/cluster/factor.rs`)
- [x] Phase 2 — Competing Destination Affinity in Token Relocation (`crates/diffuse-core/src/collapse/slot.rs`)
- [x] Phase 3 — Evidence-Gated Cross-Slot Relocation & Anti-Scavenging Floor on Add Rows (`crates/diffuse-core/src/collapse/slot.rs`)
- [x] Phase 4 — Multi-Line Expression Collapse & Expansion in Micro-DP Reflow (`crates/diffuse-core/src/collapse/slot.rs`)
- [x] Phase 5 — Golden Corpus Calibration, Asymmetric Drift Audit & Final Sign-Off (`tests/golden/`)
- [x] Phase 6 — Leftover-LCS Frame Completion with Dyck Container Disambiguation (`crates/diffuse-core/src/collapse/slot.rs`, `crates/diffuse-algo/src/dyck/`)

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
4. [Phase 4 — Multi-Line Expression Collapse & Expansion in Micro-DP Reflow](#4-phase-4--multi-line-expression-collapse--expansion-in-micro-dp-reflow)
5. [Phase 5 — Golden Corpus Calibration, Asymmetric Drift Audit & Final Sign-Off](#5-phase-5--golden-corpus-calibration-asymmetric-drift-audit--final-sign-off)
6. [Phase 6 — Leftover-LCS Frame Completion with Dyck Container Disambiguation](#6-phase-6--leftover-lcs-frame-completion-with-dyck-container-disambiguation)

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

> [!NOTE]
> Reflow atomicity across multi-line expression heads (e.g. `let matched =` severed from RHS expressions during compound factoring) is temporarily guarded by `is_open_continuation_line`. A complete language-agnostic replacement combining universal operator valence, indentation geometry, and asymmetric reflow gating is specified in [`Structural Continuation and Punctuation Valence in Cluster Factoring.md`](Structural%20Continuation%20and%20Punctuation%20Valence%20in%20Cluster%20Factoring.md).

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

### 4. Phase 4 — Multi-Line Expression Collapse & Expansion in Micro-DP Reflow

#### Problem
In `crates/diffuse-core/src/collapse/slot.rs`:
- The micro-DP solver (`align_slots_micro_dp`) operates strictly on 1-to-1 line slot alignments (diag/up/left transitions only).
- When an old 2-line expression (`u1: ((vh as f64...` + `u2: .round()`) collapses into a single line (`v2: let prop_thumb = ((vh as f64...).round() as usize;`), the 1-to-1 solver matches `u1 ↔ v2` and strands `u2` as an orphan deletion.
- This orphaned deletion is what triggers downstream cross-slot relocation to hunt for a home.
- Symmetrically, when a single old line expands across two new lines (e.g. a long call reflowed onto a continuation line), the solver pairs one side and strands the other as an orphan addition, which then acts as scavenging bait.

#### Design Notes (from Phase 4 analysis + implementation)
- **Whole-group n-to-1 / 1-to-m already exists**: `align_collapse_n_to_1` and `align_expansion_1_to_m` handle groups with exactly `m == 1` / `n == 1`. The gap — and this phase's scope — is strictly **inside multi-row groups** (`n > 1 && m > 1`), where only the micro-DP runs. Do not rebuild the whole-group paths; reuse their `\n`-joined run-splitting logic (`distribute_collapse_runs` / `distribute_expansion_runs`) so struck/filled runs attribute to the correct constituent line.
- **Post-DP absorption, not in-DP transitions (pivot during implementation)**: letting a joined pair compete in the micro-DP argmax was tried first and abandoned. It perturbs the global path: the joined candidate absorbs a line whose rightful 1-to-1 partner then strands elsewhere, tripping DBC invariants and 15+ lib tests. Repairing the already-chosen alignment instead — only ever converting (Mod + honest orphan) into (Mod + grouped continuation) — cannot steal partners: slot structure is invariant, only run typing changes. The DP, canonicalization, and relocation stages run untouched.
- **Gating (all required for a fold)**:
  1. *Adjacency + line consecutiveness*: the orphan must immediately follow its Mod with consecutive old/new line numbers.
  2. *Substance*: orphan trim length $\ge 3$ **and** contains an ASCII letter. Single-punctuation orphans (`,`, `;`, `)]}`) always "cover" trivially and their shared glyph double-attributes (struck in the Mod *and* moved into it).
  3. *Continuation evidence (necessary, not sufficient)*: head open (`delim > 0`, `is_open_continuation_line`, trailing `,`/`&&`/`||`) or tail starting with `.`/`&&`/`||`.
  4. *Unilateral coverage $\ge 0.85$* of the joined side plus full `is_admissible_slot_pair` (keeps Phase 0/1 salient-context and wholesale-bound protections).
  5. *Bilateral coverage $\ge 0.60$ when the head evidence is weak* (balanced line ending in `{`/`,`/`&&`/`||`): a trailing comma may merely separate siblings (next struct field, next arg), so the other side must be substantially explained too. Strong heads (unclosed `(`/`[`, open endings) are exempt.
- **Projection guard (abstain on garbled runs)**: the distribute+combine machinery assumes a clean split at the join boundary; cross-boundary shared text (a `len` tail living on both the old line and the joined new lines) can scramble the Mod's run order. Each fold verifies its runs project back to the slot texts (mirroring the group DBC `slots_project_to_text`) and abstains otherwise — a garbled fold would trip the group check and dissolve the whole group into raw rows (observed once: a `highlight_path_matches` expansion group lost its Mod until the guard was added; with the guard the group keeps its relocation-built grouping). Join-newline artifacts are stripped from distributed runs first.
- **No anti-scavenging floor change**: an `is_pure_add` exemption for folded slots was tried and removed as a no-op — the floor only reverts runs listed in relocation `exchanges`, and precomputed fold runs are never exchanged.
- **Interactions verified**: closed-economy conservation holds by construction (`MovedSrc`/`MovedTgt` emitted pairwise; `assert_token_conservation` in every Phase-4 test); relocation gathering skips `MovedSrc`/`MovedTgt`, so folded slots neither donate nor attract scavenging beyond residual glue runs.
- **Golden impact (agent review, pending human sign-off)**: 6 failures across 3 fixtures (`row-groups-contrast`, `theme-light-toggle`, `theme-scrollbar-palette`), all run-typing-only (same row kinds, line numbers, group metadata — only `Del`/`Add` vs `MovedSrc`/`MovedTgt` run typing differs). Categorized: strictly better (preserved arg lists like `(del_line.num, del_line.text.clone())` now marked moved instead of struck+refilled; `_themed` suffix struck while the surviving path prefix is marked moved) vs equivalent noise (surviving `)`/keyword fragments in comments and scaffolding marked moved). No dissolved pairings, no new orphans, no row-count changes.

#### Implementation Steps
1. **Multi-Line Expression Collapse ($2 \to 1$) as post-DP fold** (`fold_collapse_window`):
   - After canonicalization, scan adjacent `(Mod(u, v), Del(u'))` slot pairs; when `u'` directly continues `u` (consecutive lines + evidence gates above) and the joined text covers `v`, recompute the Mod runs from the joined alignment (`distribute_collapse_runs` + `combine_mod_runs_collapse`) and mark the orphan Del grouped (`MovedSrc`, never struck bait).
2. **Single-Line Expression Expansion ($1 \to 2$) as post-DP fold** (`fold_expansion_window`, symmetric opposite of step 1):
   - Scan adjacent `(Mod(u, v), Add(v'))` pairs; when `v'` continues `v` and the joined new text covers `u`, recompute via `distribute_expansion_runs` + `combine_mod_runs_expansion` so `Ctx`/`MovedTgt` runs land on the correct new line.
3. **Tests**:
   - Unit tests in `crates/diffuse-core/src/collapse/tests.rs`:
     - Multi-line chained call collapsing into a single line pairs naturally as a unified reflow row (the `prop_thumb` shape: no orphan Del, token conservation holds, runs attributed per old line).
     - Single-line call expanding onto a continuation line pairs as a unified reflow row (no orphan Add bait).
     - Negative control: two independent statements do not collapse; `canonicalize_slot_order` keeps Del-Mod-Add order around a 2-to-1 slot.
4. **Golden Drift Validation Check**:
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

---

### 6. Phase 6 — Leftover-LCS Frame Completion with Dyck Container Disambiguation

#### Problem
In `scrollbar-grab-drag` group 6 (`n=5`, `m=4`, slots 0–6) the expression frame survives but is not recognized:
- Slot 0 `Del` old231 `Some(` is stranded; slot 5 `Mod` old234↔new244 has an unassigned `Add` prefix `Some(`. Verbatim candidate gathering misses it because source→target distance (0→5) exceeds `CROSS_SLOT_WINDOW = 4` for a weak token (`Some` is generic per `is_generic_subtoken`).
- Slot 5 `Mod` carries an unassigned `Del` suffix ` as usize,` while slot 3 `Mod` old232↔new243 carries an unassigned `Add` suffix ` as usize;`. Verbatim containment gathering (`del_text.contains(a_trim)` / `find_matching_pair` on whole-run substrings) never proposes the common subsequence because the terminators differ (`,` vs `;`). Affinity routing therefore never sees the `as`/`usize` correspondence.
- Slot 6 `Del` old235 `)` is the outer `Some(` closer; new244 ends in `))` (inner `clamp(` closer + outer `Some(` closer). Token identity alone is ambiguous — there are two `)` recipients — so pure information matching must be disambiguated structurally.

Phase 3 machinery is correct for these same tokens in their ambiguous roles (`.round()` 1→2, stray `)` reuse, pure-`Add` rows 241/242 must stay pure `+`) and must not be weakened: no wider window, no weaker `is_generic_subtoken` gate, no terminator normalization (`as usize,` ≉ `as usize;` by fiat — `,` ≠ `;` must stay unmatched `Del`/`Add`).

#### Design: leftover LCS, not language tactics
No separator trimming, no opener/closer vocabulary, no head/tail special-casing. After `relocate_cross_slot_tokens` + `fold_tail_orphans` in `align_and_type_peer_group_with_content` (`crates/diffuse-core/src/collapse/slot.rs`), run one group-local pass over *stranded* runs only:
1. **Leftover pools**: collect unassigned `Del` subtoken sequences (source pool) and unassigned `Add` subtoken sequences (target pool) in group slot order. Tokenize with the existing subtokenizer (same word/punct/whitespace splitting used for `word_spans`), preserving order. Pure-information input: exact token identity, no normalization.
2. **Group-local LCS** over the two leftover sequences proposes common subsequences in their correct places. `as`, ` `, `usize` align as a contiguous block; `,` vs `;` naturally stay `Del`/`Add` — the correct output, with no language-specific rule.
3. **Dyck disambiguation for delimiter-only matches**: when the proposed block is delimiter-only (`(`/`)`/`[`/`]`/`{`/`}`), compute per-side delimiter partners and nesting depths with `crates/diffuse-algo/src/dyck/` (`compute_delimiter_partners`, `track_nesting_depths_*`, `UniversalDelimiterClassifier` over the group-old / group-new concatenated token stream). Require same-depth, partner-consistent correspondence: old235 `)` (partner = old231 `Some(` at depth 0) matches the new244 final `)` (partner = new244 `Some(` at depth 0), not the inner `clamp(` closer. If depths/partners are ambiguous or unbalanced, abstain.
4. **Pure-information gates (all required; Phase-3 protection is structural)**:
   - *Rarity in the leftover pool*: kept token occurs frequency-1 (or below a low-frequency floor) on both sides. `Some(`, `as`, `usize` pass; `)`, `(`, `.` fail unless Dyck-disambiguated as above.
   - *Mod-anchored recipient*: target slot is an already-paired `Mod`, never a pure `Add`. Rows 241/242 are unreachable by construction.
   - *Anchor adjacency*: the kept block sits adjacent to an already-anchored correspondence (same paired `Mod` or a `linked` pair from relocation). Unanchored scraps in pure-`Add` rows cannot match.
   - *Order-consistent contiguous block*: kept matches form a contiguous run in both pools (the `as␣usize` block), not scattered glyph reuse.
   - *Single eligible recipient*: exactly one target slot holds the normalized-identity block at the required depth; two or more → abstain and let affinity own it.
   - *Closed economy + projection*: retype pairwise `Del→MovedSrc` / `Add→MovedTgt`, verify `slots_project_to_text` and conservation (`MovedSrc` multiset == `MovedTgt` multiset); call `repair_unmatched_moves` semantics on failure (abstain rather than emit lone marks).
5. **Ordering**: runs strictly last (after relocation, anti-scavenging floor, and folds). It only consumes leftovers the earlier passes stranded; it never re-routes `.round()` (already placed by Phase-2 affinity) and never donates to pure Adds. `CROSS_SLOT_WINDOW` is not widened — group-order LCS replaces distance for unique anchored frames.

Expected effect on group 6: slot 0 `Some(` → `MovedSrc` paired with slot 5 `Some(` → `MovedTgt`; slot 5 ` as usize` (`as`, ` `, `usize`) → `MovedSrc` block paired with slot 3 ` as usize` → `MovedTgt` block while `,`/`;` stay `Del`/`Add`; slot 6 `)` → `MovedSrc` paired with the Dyck-outermost `)` of new244 → `MovedTgt`. Row kinds, line numbers, and slots 1–2 (pure `+`) are unchanged.

#### Implementation Notes (as built; golden signed off and committed 2026-11-24)

`relocate_leftover_frames(slot_runs, is_moved_slot, is_pure_add, profile)` in `crates/diffuse-core/src/collapse/slot.rs`, called immediately after `relocate_cross_slot_tokens` in `align_and_type_peer_group_with_content`. It rebuilds leftover token pools from the surviving `Del`/`Add` runs each round and accepts the longest admissible block, then repeats (bounded to 16 rounds) so crossed orders settle one block at a time. Deviations from the sketch above, all forced by the real corpus:

- **Terminator normalization was not needed and was not implemented.** Matching on exact token identity already leaves `,` vs `;` outside the common block (` as usize` matches, the separator does not). No separator stripping exists anywhere in the pass.
- **Contiguity is slot-level, not run-level.** The word diff splits ` as usize;` into ` ``/`as `/`usize`/`;` runs, so a frame block legitimately spans runs; retyping is applied per run with byte offsets verified before any mutation.
- **Uniqueness is block-level.** Per-token frequency would reject ` as usize` because `as` also occurs in ` as f64`; the joined block occurs exactly once per side and exactly one slot per side holds it.
- **Jurisdiction gate (not in the sketch).** A bracketed block within `CROSS_SLOT_WINDOW` is relocation's call: verbatim whole-run matching already proposed it, and declining it was deliberate. The pass only completes what verbatim matching could not see — glue with a differing terminator, or a frame beyond the window.
- **Distance exemption requires bracket *external* evidence.** Beyond the window, a block must be bracket-unbalanced on at least one side (an opener whose closer lives elsewhere), so a balanced far fragment is not treated as a frame.
- **Structural frame gate.** On each side the block holds a bracket, or its nearest non-whitespace neighbour in the slot projection does (` as usize` follows `)`). Separators never count, which is what keeps `,`/`;` glued to their own side.
- **Donor exhaustion replaces the symmetric test.** The stranded `Del` fragment must carry the frame alone; the recipient is exempt because its remainder is genuine addition content (`Some(` out of the glued `Some(prop_thumb`). Requiring recipient exhaustion refused every glued addition.
- **Stranded-donor rule.** A donor whose old side already donated (`MovedSrc`) was adjudicated by relocation; its leftovers belong to that exchange. Receiving (`MovedTgt`) does not disqualify a donor — group 6's slot 5 hosts the moved `)` and still owns ` as usize`.
- **Dyck partner matching was not required.** In the live pipeline the stray `)` of old235/new244 is already relocated by Phase 3 before this pass runs, so no delimiter-only block ever reached it. `UniversalDelimiterClassifier` is used for the bracket test, the externals test, and the neighbour scan; a partner/depth pass would be dead code until a corpus exercises it.

Tests added in `crates/diffuse-core/src/collapse/slot.rs`: `test_leftover_frame_completes_scrollbar_group_6` (the group-6 runs transcribed from the golden wire, so it also runs when the golden submodule is absent), `test_leftover_frame_two_candidate_weak_token_abstains`, and `test_leftover_frame_bracketed_within_window_abstains`.

Gate state at sign-off: `./scripts/check.sh` FORMAT/CLIPPY/BUILD all clear; after regenerating the three drifted rows, all 45 golden legs pass and the full gate exits 0.

Golden impact (signed off line-by-line; regenerated and committed): `scrollbar-grab-drag` only, three rows, run typing and spans only — no row-kind, line-number, group, or slot changes.
- old231 slot 0: `        Some(` becomes `Del(\"        \")` + `MovedSrc(\"Some(\")`.
- new243 slot 3: ` as usize` becomes `MovedTgt` (the moved `()` run absorbs the space), `;` stays `Add`.
- new244 slot 5: `Some(` becomes `MovedTgt`; ` as usize` becomes `MovedSrc`; `,` stays `Del`.
- Pure additions (new241/242), the relocated `.round()` (slot 4) and `)` (slot 6), and every other corpus are unchanged.

#### Implementation Steps
1. **Leftover-pool LCS + gating** (`crates/diffuse-core/src/collapse/slot.rs`):
   - Add `relocate_leftover_frames(slot_runs, is_moved_slot, is_pure_add, linked, profile)` called after `relocate_cross_slot_tokens` (and after `fold_tail_orphans` output flows through), operating only on `RunKind::Del`/`RunKind::Add` runs that survived relocation.
   - Build ordered leftover token streams with slot/run/offset provenance; compute LCS on exact token identity; filter through the six gates above; expand to run spans with the existing `expand_match` word-boundary discipline; retype via `replace_span_in_run`; record `exchanges`/`linked` so the existing pure-`Add` coverage floor and `repair_unmatched_moves` continue to hold.
2. **Dyck partner/depth check** (`crates/diffuse-algo/src/dyck/partners.rs`, `containers.rs`; call from `slot.rs`):
   - For delimiter-only candidate blocks, compute `compute_delimiter_partners` + nesting depths over the group-old and group-new token streams with the language-agnostic ASCII/universal classifier already in the tree. Match outermost-to-outermost (depth-0 `Some(` pair) for old235↔new244-final-`)`; abstain on depth mismatch, multiple same-depth candidates, or unbalanced input.
3. **Tests (test-first, all in `crates/diffuse-core/src/collapse/tests.rs` unless noted)**:
   - Opener travels despite window: stranded `Some(` orphan → paired-`Mod` recipient becomes `MovedSrc`/`MovedTgt`, conservation holds.
   - Terminator block: ` as usize,` vs ` as usize;` yields `Moved` block for `as`/` `/`usize` while `,`/`;` remain `Del`/`Add`.
   - Dyck disambiguation: two-`)` recipient resolves to the outermost partner; same-depth duplicate closers abstain.
   - Negative controls (Phase-3 preservation): ambiguous `)` does not travel; pure-`Add` row never receives; two-candidate weak token abstains; coverage floor still reverts scavenged pure Adds.
   - Confirm initial failure (stranded `Del`/`Add` before the fix), then minimal implementation; combined primary + edge tests must cover 100% of new branches; unit tests `< 0.5s`.
4. **Verification gate**:
   - `./scripts/check.sh` green (nightly `fmt`, `clippy -- -D warnings`, workspace tests).
   - `cargo test -p diffuse --test golden`: expect surgical drift confined to `scrollbar-grab-drag` group 6 run-typing (`Del`/`Add` → `MovedSrc`/`MovedTgt` on the frame blocks above; `,`/`;` untouched; row kinds/line numbers unchanged) and zero drift elsewhere. Apply the asymmetric rule (+3 per eliminated spurious match, −1 per dissolved acceptable pairing); any Validated Golden regression needs line-by-line justification and human sign-off before committing.
