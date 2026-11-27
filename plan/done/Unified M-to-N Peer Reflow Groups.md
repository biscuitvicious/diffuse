# Unified M-to-N Peer Reflow Groups

- [x] **Phase 1: Symmetric Peer Group Data Model & Wire Contract Extension**
- [x] **Phase 2: Macro Cluster Factorization & Closed-Boundary Cuts**
- [x] **Phase 3: Micro Slot-Alignment DP & Closed-Economy Token Typing**
- [x] **Phase 4: Pipeline Integration & Shadow Engine Verification**
- [x] **Phase 5: Golden Acceptance, Fallout Triaging, Dual-Leg Verification & Visual Sign-off**
- [x] **Phase 6: Legacy Assembly Retirement & Structural Simplification**

---

## Executive Summary

Historically, *diffuse* modeled code modifications primarily as $1:1$ line correspondences (`Row::Mod`). Multi-line reflows (expansions, collapses, and compound modifications) were accommodated by designating one line as the primary "anchor" and attaching secondary lines as asymmetric "auxiliary" rows (`RowGroup::Anchor`, `RowGroup::Aux`, `Row::AuxAdd`, `Row::AuxDel`). 

While this procedural hierarchy successfully captured many real-world patterns, it introduced substantial structural strain in [`crates/diffuse-core/src/collapse/assemble.rs`](../crates/diffuse-core/src/collapse/assemble.rs) (over 2,600 lines of entangled lookahead, multi-line slicing, and manual character budgets). Furthermore, it enforced an artificial asymmetry on edits where lines are intrinsically peers (e.g. multi-line closure wrapping, parameter relocations, and expression reflows).

This plan establishes a **unified $N \to M$ peer reflow architecture**. Every change between context boundaries is modeled uniformly as an $(N \to M)$ transition matrix where $N \ge 0$ old lines and $M \ge 0$ new lines participate as equal peers:
- $(1 \to 0)$: Pure deletion
- $(0 \to 1)$: Pure addition
- $(1 \to 1)$: Single-line modification
- $(1 \to M)$: Single-to-multi expansion
- $(N \to 1)$: Multi-to-single collapse
- $(N \to M)$: Compound peer reflow group

### Governing Invariants & Mathematical Duality

1. **Spatial Duality of Information Conservation**:
   Conserved textual content is invariant. Its semantic role is determined entirely by vertical displacement ($\Delta y$) across the horizontal slot assignment:
   $$\Delta y = 0 \implies \text{Ctx} \quad (\text{Horizontal context: identical, calm})$$
   $$\Delta y \ne 0 \implies \text{Mov}(\text{Src} \to \text{Tgt}) \quad (\text{Cross-line move: struck on left, moved-in on right})$$

2. **Visual Acuity Optimization**:
   Within an $(N \to M)$ group, slot sequencing optimizes for maximum visual acuity by minimizing displacement cost:
   $$\min \sum_{t \in \text{Tokens}} \text{mass}(t) \cdot |\Delta y(t)|$$

3. **Closed-Economy Conservation**:
   Once macro boundaries are sealed, every peer group forms a closed thermodynamic system. Weak intra-group tokens (e.g. delimiters, trailing punctuation) require no heuristic gating; any token deleted in $U$ and added in $V$ is conserved by construction:
   $$\sum_{u \in U_{\text{group}}} \text{MovSrc}(u) \equiv \sum_{v \in V_{\text{group}}} \text{MovTgt}(v)$$

---

## Table of Contents

1. [Phase 1 — Symmetric Peer Group Data Model & Wire Contract Extension](#1-phase-1--symmetric-peer-group-data-model--wire-contract-extension)
2. [Phase 2 — Macro Cluster Factorization & Closed-Boundary Cuts](#2-phase-2--macro-cluster-factorization--closed-boundary-cuts)
3. [Phase 3 — Micro Slot-Alignment DP & Closed-Economy Token Typing](#3-phase-3--micro-slot-alignment-dp--closed-economy-token-typing)
4. [Phase 4 — Pipeline Integration & Shadow Engine Verification](#4-phase-4--pipeline-integration--shadow-engine-verification)
5. [Phase 5 — Golden Acceptance, Fallout Triaging, Dual-Leg Verification & Visual Sign-off](#5-phase-5--golden-acceptance-fallout-triaging-dual-leg-verification--visual-sign-off)
6. [Phase 6 — Legacy Assembly Retirement & Structural Simplification](#6-phase-6--legacy-assembly-retirement--structural-simplification)

---

## 1. Phase 1 — Symmetric Peer Group Data Model & Wire Contract Extension

### 1.1 Core Data Structures (`crates/diffuse-core/src/wire/row.rs`)

Introduce a symmetric `PeerGroup` definition replacing the asymmetric `Anchor` / `Aux` tagging:

```rust
/// Unique identifier for an irreducible (N -> M) peer reflow group within a hunk.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub u32);

/// Symmetric group metadata describing an N -> M transition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerGroupMeta {
    pub id: GroupId,
    pub old_count: usize, // N
    pub new_count: usize, // M
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowGroup {
    /// A row belonging to an (N -> M) peer reflow group.
    Peer {
        id: GroupId,
        slot_index: usize, // index within the unified slot layout 0..max(N, M)
    },
    /// Backward-compatibility fallback for legacy renderers during transition.
    LegacyAnchor,
    LegacyAux { anchor: u32 },
}
```

### 1.2 Wire Protocol Schema (`crates/diffuse-core/src/wire.rs`)

Extend the public wire format to serialize peer group participation without breaking existing consumers:
```json
["mod", 293, 288, ["..."], {"group": {"role": "peer", "id": 1, "slot": 0, "n": 2, "m": 3}}]
["mod", 294, 289, ["..."], {"group": {"role": "peer", "id": 1, "slot": 1, "n": 2, "m": 3}}]
["add", null, 290, ["..."], {"group": {"role": "peer", "id": 1, "slot": 2, "n": 2, "m": 3}}]
```
- Consumers ignoring the extended attributes gracefully fall back to row-kind rendering.
- Renderers aware of peer groups gain direct access to draw seamless surrounding borders and bracket glyphs.

### 1.3 Test-First Verification Criteria
- Unit tests in `wire::tests` validating serialization and deserialization roundtrips.
- Wire invariant tests guarding that for every `group.id`, the number of pre-image slots equals $N$ and post-image slots equals $M$.

---

## 2. Phase 2 — Macro Cluster Factorization & Closed-Boundary Cuts

> [!NOTE]
> **Retired**: Macro cluster factorization and closed-boundary cuts (`crates/diffuse-core/src/cluster/factor.rs`) were retired in favor of single-path cluster alignment and derived formations in [`plan/done/Unified Reflow Lattice.md`](done/Unified%20Reflow%20Lattice.md).

### 2.1 Bipartite Graph Construction (`crates/diffuse-core/src/cluster/factor.rs`)

Given a raw cluster $(U \to V)$ of $N_{\text{total}}$ pre-image rows and $M_{\text{total}}$ post-image rows between stable context lines:
1. Construct bipartite graph $G = (U, V, E)$.
2. Compute edge capacities based on:
   - Myers subline LCS mass (entropy-weighted character matches).
   - Significant token relocation (tokens of length $\ge 6$ or multi-token phrases).
   - Container matching (Dyck pushdown automata enclosing paired brackets/parentheses).

### 2.2 Monotonic Factorization Cut Algorithm
1. Find separable boundary cuts $0 = i_0 < i_1 < \dots < i_K = N_{\text{total}}$ and $0 = j_0 < j_1 < \dots < j_K = M_{\text{total}}$ such that the cross-boundary edge weight is below the severance threshold.
2. If Dyck container depth across the boundary is non-zero, the boundary is blocked until container closure.
3. Emit a sequence of strictly independent groups:
   $$\mathcal{G} = [ (U_1 \to V_1), (U_2 \to V_2), \dots, (U_K \to V_K) ]$$

### 2.3 Reserve Capacity / Slack for Unexpected Cluster Topologies
- **Deep nesting & long closures**: Reserve fallback cuts at indentation decreases if container depth tracking encounters unbalanced syntax (e.g. macro fragments).
- **Prose vs Code**: Markdown and plaintext fall back to purely horizontal line matching when container syntax is absent.

---

## 3. Phase 3 — Micro Slot-Alignment DP & Closed-Economy Token Typing

### 3.1 Micro Slot-Alignment DP (`crates/diffuse-core/src/collapse/slot.rs`)

For each isolated peer group $(U_k \to V_k)$ with $N = |U_k|$ and $M = |V_k|$:
1. Compute the $N \times M$ score matrix $S(i, j)$:
   $$S(i, j) = \text{LCS\_common\_chars}(U_k[i], V_k[j]) - \text{indent\_penalty}(i, j)$$
2. Run micro-DP finding the monotonic slot assignment of length $L = \max(N, M)$ maximizing horizontal context.
3. Every slot $l \in [0, L)$ receives:
   - `old_line`: $U_k[i]$ or `None` (blank filler)
   - `new_line`: $V_k[j]$ or `None` (blank filler)

### 3.2 Closed-Economy Token Typing
Within the sealed container of the peer group:
1. All tokens in $\bigcup_{i} U_k[i]$ deleted and appearing in $\bigcup_{j} V_k[j]$ added are catalogued into the group's closed token pool.
2. For each token $t$:
   - If slot of origin $l_{\text{src}}$ equals slot of destination $l_{\text{tgt}}$ ($\Delta y = 0$): Token is emitted as **`RunKind::Ctx`**.
   - If $l_{\text{src}} \ne l_{\text{tgt}}$ ($\Delta y \ne 0$): Token is emitted as **`RunKind::MovedSrc`** on slot $l_{\text{src}}$ and **`RunKind::MovedTgt`** on slot $l_{\text{tgt}}$.
3. All remaining unmatched tokens become **`RunKind::Del`** (left) or **`RunKind::Add`** (right).
4. Strict DBC assertion: $\sum \text{MovedSrc} \equiv \sum \text{MovedTgt}$ holds universally by construction without manual HashMap budget tracking.

---

## 4. Phase 4 — Pipeline Integration & Shadow Engine Verification

### 4.1 Shadow Engine Execution (`crates/diffuse-core/src/pipeline/mod.rs`)

To ensure stability during migration:
1. Introduce a feature-gated or shadow comparison mode:
   - Execute both legacy `pass_assemble_mods` and the new `peer_reflow_pipeline`.
   - Log any topological deviations across large test corpuses.
2. Verify that for all $1:1$ mods, pure adds, and pure dels, the output is bit-for-bit identical to legacy output.

### 4.2 Renderer Compatibility (`crates/diffuse-render/src/row/segment.rs`)
1. Side-by-side renderer consumes `RowGroup::Peer` naturally:
   - Slots with `old_line: Some` render on the left pane (with `MovedSrc` actively struck).
   - Slots with `new_line: Some` render on the right pane.
2. Gutter glyphs render clean peer brackets for multi-line peer groups:
   - First row: `┌`
   - Intermediate rows: `│`
   - Final row: `└`

---

## 5. Phase 5 — Golden Acceptance, Fallout Triaging, Dual-Leg Verification & Visual Sign-off

### 5.1 Anticipated Golden Movements & Triage Discipline

A transition to an overarching $N \to M$ model will inevitably produce output shifts across the 45 acceptance corpora in [`tests/golden/`](../tests/golden/). Per [`tests/AGENTS.md`](../tests/AGENTS.md), golden deviations must be actively categorized:

#### Category A: Genuine Improvements (Accept & Document)
- Cases where previously fragmented or orphaned additions/deletions are now cleanly united into cohesive reflow blocks (e.g. multi-line method chaining, nested closures, parameter rewrites).
- Reduction in visual noise: fewer stray red/green lines, more calm horizontal context.

#### Category B: Unintended Regressions / Fallout (Fix in Algorithm)
- Accidental coalescence of two distinct logical edits separated by whitespace.
- Over-eager grouping across semantic declaration breaks (functions, struct definitions).
- *Mitigation*: Adjust macro graph cut thresholds and ensure blank lines or declaration breaks act as hard cut barriers.

#### Category C: Equivalent Neutral Realignment (Review & Sign-off)
- Symmetrical shifts in tie-breaking where two slots had identical cost.

### 5.2 Verification Gate
- Dual-leg execution: both in-process library leg and compiled CLI binary leg match updated goldens byte-for-byte.
- `./scripts/check.sh` passes with zero clippy warnings and all performance budgets ($< 3\text{s}$ warm build).

---

## 6. Phase 6 — Legacy Assembly Retirement & Structural Simplification

### 6.1 Pruning Legacy Mechanics in `assemble.rs`
Once the peer reflow pipeline is verified:
1. Remove redundant special cases:
   - `expand_blocks` pre-slicing
   - `collapse_blocks` multi-line del slicing
   - `compound_anchors` and `compound_aux_of` ad-hoc pre-pass
   - Imperative delimiter depth lookaheads in `assemble.rs`
2. Reduce code footprint in [`crates/diffuse-core/src/collapse/assemble.rs`](../crates/diffuse-core/src/collapse/assemble.rs) by an estimated 1,500+ lines.
3. Archive this plan to `plan/done/Unified M-to-N Peer Reflow Groups.md`.
