# Unified Reflow Lattice — Single-Path Cluster Alignment and Derived Formations

- [x] **Phase 1: Cluster Path DP**
- [x] **Phase 2: Path Segmentation and Row Emission**
- [x] **Phase 3: Formation Typing and Group Attribution**
- [x] **Phase 4: Pipeline Integration and Invariant Enforcement**
- [x] **Phase 5: Legacy Retirement**
- [x] **Phase 6: Specification Tests and Reference Corpus Verification**
- [x] **Phase 7: Golden Recalibration and Reference-Corpus Review**
- [x] **Phase 8: Plan Closure and Archive** (final calibration recorded, specification tests completed, archived in `plan/done/`)

---

## Executive Summary

The current engine factors a change cluster into rectangles before it aligns anything.
[`factor_cluster_with_content`](../../crates/diffuse-core/src/cluster/factor.rs) builds a sparse
weighted edge graph, walks a monotonic cut with a closure loop, and emits `FactoredGroup`
ranges; [`peer_reflow_pipeline`](../../crates/diffuse-core/src/collapse/peer.rs) then runs the
micro-DP inside each range. Two consequences follow from that order:

1. A rectangle is the axis-aligned hull of the edges it closed over, so it contains rows that
   took part in no edge (pure additions in the middle of a rewritten paragraph) and excludes
   rows past its last edge (the continuation lines the paragraph's deletions actually became).
2. The cut is drawn with one evidence scale (jaccard, `calculate_edge_explanation_ratio`,
   τ 0.25–0.65) while the alignment inside it runs on a stricter or looser one
   (`match_score`: shared salient `Ctx` run, four matched salient characters, 30% of the
   smaller side). A boundary the graph cannot see is invisible to the DP forever, because the
   DP never sees past the cut.

This plan removes the factorization stage. Each cluster is solved once, as a single monotone
alignment path, and formations are **derived from that path** instead of imposed around it.

### Why a joint DP has no separate group state

Option 3 was sketched as a DP with an explicit `group_open` state. That state is redundant,
and proving it is part of the work:

- A slot is a pair `(Option<old row>, Option<new row>)`. Slot order is old-then-new under a
  monotone path, and the path is monotone in both sequences, so a path fixes the slot order
  completely.
- Given the path, every row is either paired, or skipped on one side between two pairs, or
  skipped outside all pairs. The path therefore fixes the *windows*: maximal intervals that
  contain at least one pair and at least one skip.
- Membership inside a window is not decided by position. In `scrollbar-grab-drag`, three
  windows contain skips at their edges that the wire keeps as plain rows
  (`geometry/tests.rs` adds 421–422) and skips at their edges that the wire makes members
  (`command.rs` adds 75–78). The split is decided by relocation: a skip is a member when it
  is typed `MovedSrc`/`MovedTgt`, and membership is the span between the outermost such
  participants, else the pairs themselves.
- Therefore the free decisions are the correspondence and the token typing. A `group_open`
  transition would encode a third choice that does not exist, but the layout is a projection
  of *(path, typing)*, not of the path alone. This is why `recalibrate_peer_groups` is
  load-bearing rather than a gate (Phase 3).

The joint objective lives entirely in the pair weight and the gap penalty. The optimum path
under that objective is the maximizer of explained content per unit of displacement, which is
the same quantity the earlier `group_open` sketch was meant to optimize.

### Why not a correctly implemented graph partition

The factorizer resembles a graph algorithm without being one: edges are thresholded into
booleans, weights are discarded, and the partition is a greedy hull grown by a closure loop.
The natural repair is to keep the graph and compute a real partition — minimum cut,
normalized cut, or correlation clustering over the affinity edges. That repair does not
reach a better model, for four independent reasons.

**A cut measures the boundary, and membership is an interior property.** Cut cost sums the
weight of edges crossing the partition. A row with no incident edge has degree zero and
contributes nothing to any cut, so no cut objective can prefer excluding it from a group it
happens to sit inside. The 4×4 hull is therefore not a defect of the greedy walk that a
better optimizer would remove; it is what the cut objective means. Recovering the needed
judgment requires a unary term per row, which is no longer a cut: it is an energy over
labels, and the labels are given by the pairing that the alignment already computes.

**With unary terms it becomes inference, and the tractable instance is again a DP.** A unary
plus pairwise energy over ordered labels is a Markov random field. Exact MAP inference on a
chain is Viterbi, which is the transition system in §1.1; exact MAP on a general graph is
intractable, and the approximations that replace it lose the global optimum that motivated
the change. Correlation clustering with must-link and cannot-link constraints is likewise
NP-hard in general and becomes polynomial on restricted classes such as interval and
permutation graphs.

Those algorithms are not this one, and the difference is worth stating because it bounds what
this plan claims. Grouping under a non-crossing constraint is a partition of the slots, and
the lattice of such partitions is the non-crossing partition lattice, sized by the Catalan
numbers; a DP over it carries a partial partition or a segment as state, and its objective is
additive over *element pairs*, not over path steps. The solver here carries a prefix pair
`(i, j)` as state over the product of two chains, and its objective is additive over steps —
a shortest path in a DAG of `(n+1)(m+1)` nodes. The two share only the reason they are
tractable: monotone correspondence converts the crossing constraint from a global condition
into a chain condition.

Admissibility does more than restrict the output, it collapses the state space. Requiring the
partition to consist of interval pairs emitted in increasing order, with each pair's members
contiguous on the path, rules out every non-crossing partition that is not an interval
partition with a monotone pairing. What remains is the chain lattice, so the cheaper state is
not an approximation of the clustering DP; it is the whole admissible set. The extra power of
the clustering formulation lies in partitions this writer cannot emit.

The design consequence is that a clustering objective cannot be adopted for free. Its natural
cost is pairwise across all slot pairs, which is not step-additive and therefore has no
`V(i, j)` recurrence. Making it additive means scoring a segment as a unit, which is the
block score of the legacy DP (§1.5) and reintroduces exactly the uncalibrated weights this
plan retires.

**The output is an ordered stream, not a labeling.** The wire contract emits rows in
increasing old and new line order, so any partition must decompose into interval pairs
`(old_range, new_range)` that are pairwise non-crossing. Those constraints are not
submodular pairwise terms, so a cut does not respect them and its output must be projected
back onto the admissible set afterwards, discarding both optimality and determinism. The
admissible set is the monotone path.

**A partition buys reordering, which the writer cannot express.** Clustering is the right
formulation for unordered data, and its extra power over a monotone path is exactly the
ability to group rows that appear in a different order on each side. A unified diff cannot
show that: the emission order is fixed. Reordering survives only as token typing
(`MovedSrc`/`MovedTgt`) inside a formation, which runs after correspondence and does not
need a partition.

The decisive practical argument is identifiability rather than expressiveness. This model has
one evidence function and one constant `γ`. The legacy block DP carried nineteen `Weights`
fields whose documentation records them as historical hand tuning, reviewed only against the
frozen corpora; a cut formulation adds edge weights, unaries and a regularizer on top. Every
added degree of freedom is a knob that 23 corpora cannot validate, and an objective that
cannot be stated cannot be checked.

**Known limitation of the path.** A block transition can score a `k`-to-`l` correspondence as
a unit, whereas the path scores it as the sum of its steps, so a block whose joint evidence
exceeds its pairwise evidence cannot influence correspondence. That expressiveness is
retained in typing (`joined_alignment`, frame folding) but only as a post-path fold, which
cannot revise the path. This is accepted: the fold already handles the flattened-statement
cases it was written for, and the alternative is the block state whose calibration is the
problem being removed.

### Governing Invariants

1. **Correspondence before formation.** No row becomes a member of a formation unless the
   path paired it or placed it between two pairs. No rectangle, no hull, no filler.
2. **Distance is a cost, not a wall.** A far pair pays `2γ` per skipped row. No diagonal
   threshold, no `explained_deletion` gate, no τ curve. A jump survives when its evidence
   outbids the local alternative.
3. **One path per cluster.** The cluster is solved once at full extent. There is no interior
   boundary at which correspondence becomes invisible.
4. **Evidence conservation.** Token typing still runs over a formation's closed economy:
   `Σ MovedSrc ≡ Σ MovedTgt` per group id, and every formation either carries relocated text
   or dissolves.
5. **Determinism.** Equal-scoring paths resolve by the fixed transition order
   `Diag > DropOld > DropNew` and then by lexicographic `(i, j)`, so the wire stays
   byte-reproducible.

### Expected Effect on the Motivating Hunks

| Change set | Current output | Target output |
| :--- | :--- | :--- |
| `theme-scrollbar-palette`, `AGENTS.md` paragraph | 4×4 group: three adds, one mod, three dels | tight `2×1` formation (`18..=19 ↔ 21`, carrying relocated `"the pi review extension"`), adds `18–20` & `22–25` and dels `20–23` ungrouped — **no 4×4 hull (see §2.4)** |
| `theme-scrollbar-palette`, `scripts/install.sh` | two ungrouped mods, no formation | unchanged |
| `scrollbar-grab-drag` | reference corpus; see Phase 6 | no regression; every deviation reviewed in Phase 7 |

---

## Table of Contents

1. [Phase 1 — Cluster Path DP](#1-phase-1--cluster-path-dp)
2. [Phase 2 — Path Segmentation and Row Emission](#2-phase-2--path-segmentation-and-row-emission)
3. [Phase 3 — Formation Typing and Group Attribution](#3-phase-3--formation-typing-and-group-attribution)
4. [Phase 4 — Pipeline Integration and Invariant Enforcement](#4-phase-4--pipeline-integration-and-invariant-enforcement)
5. [Phase 5 — Legacy Retirement](#5-phase-5--legacy-retirement)
6. [Phase 6 — Specification Tests and Reference Corpus Verification](#6-phase-6--specification-tests-and-reference-corpus-verification)
7. [Phase 7 — Golden Recalibration and Reference-Corpus Review](#7-phase-7--golden-recalibration-and-reference-corpus-review)
8. [Phase 8 — Plan Closure and Archive](#8-phase-8--plan-closure-and-archive)

---

## 1. Phase 1 — Cluster Path DP

**Goal:** One monotone path per cluster, with the pair weight and gap cost that decide
correspondence. The factorizer's edge graph, closure loop, and 16-row caps disappear.

### 1.1 New Solver (`crates/diffuse-core/src/collapse/lattice.rs`)

- Define `PathStep { Diag, DropOld, DropNew }` and `ClusterPath { steps, score }`.
- `solve_cluster_path(old_rows, new_rows, old_content, new_content, profile) -> ClusterPath`,
  with `V(0,0) = 0` and:

  ```text
  V(i, j) = max(
      V(i-1, j-1) + P(i-1, j-1),   // Diag
      V(i-1, j)   - γ,             // DropOld
      V(i,   j-1) - γ,             // DropNew
  )
  ```

- `P(u, v)` is defined once, in `collapse/evidence.rs`, and is the **union** of the two
  evidence functions that exist today: the micro-DP's `match_score`
  ([`slot.rs`](../../crates/diffuse-core/src/collapse/slot.rs)) and the graph's
  `calculate_edge_explanation_ratio` ([`factor.rs`](../../crates/diffuse-core/src/cluster/factor.rs)).
  The union is required: each carries branches the other lacks, and §1.5 records which is
  ported and which is dropped.
- `match_score` contributes: exact match bonus, `is_affix_pair` bonus, wholesale replacement
  rejection, `is_admissible_slot_pair` (declaration-role agreement, verbatim-rival veto),
  the compound gate (shared salient `Ctx` run, or statement head at offset ≤ 1; ≥ 4 matched
  salient characters; ≥ 30% of the smaller side), indentation cost, and the
  statement-head/key/prefix bonuses. Three of its terms must be corrected before it can be
  the single scale; §6.3 derives each from a formation in `scrollbar-grab-drag`:
  the affix fast path must compute `cached_runs` or the compound gate cannot see its shared
  prefix, the affix score must pay the unmatched-character penalty and scale its constant by
  coverage, and the head/terminator bonuses must survive because they decide formation 3.
- Delete from `P` every positional gate: `diag_dist`, `LONG_DIAGONAL`,
  `explained_deletion`, `BRIDGED_EXPLANATION`. Displacement is priced by `γ` only.
- `γ` is a single constant in `crates/diffuse-core/src/cluster/dp/config.rs`. **Units:**
  `P` scores matched salient characters at 10 points each, so "a pair worth `k` characters
  bridges at most `k` rows" means `γ = 10`, five times today's `GAP_PENALTY = 2`. Deleting
  `LONG_DIAGONAL` while keeping `γ = 2` would make displacement almost free and is not the
  intended change.

### 1.2 Budget and Band

- Clusters at or below `FULL_GRID_CELLS` (20,000) are solved on the whole grid; larger
  clusters are solved inside a corridor built from exact-match anchors. Each anchor-to-anchor
  segment contributes its monotone rectangle while that rectangle is small (at most 4,096
  cells) and a `±2` diagonal band through it otherwise, so an inserted or deleted block stays
  inside the corridor without paying for its area. Every corpus cluster stays below the
  threshold, which keeps the golden review a review of whole-grid paths.
- A corridor candidate is scored only when it shares a salient token, a statement head, an
  assignment target, or a field key; short, affix, identical, and blank pairs are always
  scored. This admission test is a cost policy for large hunks, not a semantic rule, because
  evidence scoring builds an alignment tape (~1.2 ms per pair on long lines).
- Keep `cluster_budget()` as the only fallback: above budget, emit all deletions then all
  additions. This is a resource ceiling, not a compatibility path.
- Retain the scratch arena ([`arena.rs`](../../crates/diffuse-core/src/cluster/dp/arena.rs))
  for row metadata and token interning.

### 1.3 Locked Rows

- Rows already typed `MovSrc`/`MovTgt` before the collapse stage stay outside the path: each
  contiguous run is emitted as its own pure group, as the factorizer does today.
- Blank rows take no special path transition; a blank old row paired with a blank new row is
  an ordinary `Diag` with a constant weight.

### 1.4 Test-First Criteria

- Unit tests in `lattice.rs`: empty cluster, all-add, all-del, 1×1, uniform shift, offset
  jump, negative-evidence pair rejected, budget fallback, tie-break determinism (same path
  from repeated runs and from transposed input order where applicable).
- Property test: every path is monotone and consumes each row at most once.
- `cargo test -p diffuse-core lattice` passes before Phase 2 starts.

### 1.5 Retirement Ledger

The factorizer is not removable as a unit. Five of its symbols are imported by code that
stays, and several of its evidence branches have no counterpart in `match_score`. The order
must be move, port, delete.

**Moves (no behavior change).** Relocate before touching the partition logic:

| Symbol | Callers that stay | Destination |
| :--- | :--- | :--- |
| `FactoredGroup` | six typing helpers in `slot.rs` take `&FactoredGroup` | `collapse/formation.rs`, as `Formation` |
| `robust_delim_delta` | `is_expression_head_open`, `is_strong_expression_head`, micro-DP `precompute` | `collapse/frame.rs` |
| `is_open_continuation_line` | the same two frame predicates | `collapse/frame.rs` |
| `is_salient_word_match` | six cross-slot relocation call sites | `collapse/tokens.rs` |
| `dp::solver::is_admissible_mod_pair` | `is_admissible_slot_pair`, the surviving pair gate | `collapse/evidence.rs` |

**Ports (evidence branches with no `match_score` equivalent).** `calculate_edge_explanation_ratio`
is deleted only after each branch is re-expressed in `pair_evidence` or consciously dropped
with a spec test pinning the new verdict:

| Branch | Graph weight | In `match_score` today | Decision |
| :--- | :--- | :--- | :--- |
| mid-string containment, ≥ 8 chars | ≥ 0.40 | none (`is_affix_pair` covers prefix/suffix only) | replaced in final calibration by `HEAD_LINE_BONUS` (prefix/opener heads at equal indent) rather than an uncalibrated mid-string containment bonus |
| `key:` match with no space in the key | 0.80 | none | port, or drop and measure on `scrollbar-grab-drag` |
| graded Jaccard with `full containment` / `bilateral 0.40` / `dominant 0.65` modes | Jaccard | replaced by `≥ 4 matched salient chars ∧ ≥ 30% of the smaller side` | keep the gate, not the score; record why |
| comment/non-comment veto | hard `None` | not visible in `match_score` or either admissibility predicate | verify, then port as a hard veto |

**Measured scale note.** The graph's τ curve (0.25 near the diagonal to 0.65 far) applied to
symmetric Jaccard corresponds to 40%–79% bilateral coverage, while `match_score` admits at
30% of the smaller side. So the graph is *stricter* on coverage in most of its range and
compensates with structural rescue branches, not looser. The motivating 4×4 hull comes from
the closure, not from the τ curve; the union above is therefore justified on branch merit and
not by "the graph was too permissive".

**Drops (graph-only partition machinery).** `raw_edges`, distance pruning τ_min, the
`max_u_weight`/`max_v_weight` competing-edge suppression, `is_exact_landmark`, the
`u_target`/`v_target` seed walk, the closure loop, the 16-row caps, and the delimiter-delta
expansion. Each is replaced by a property of the single path, except for the three forcing
rules in §1.6.

Two of the drops deserve a named replacement because they encode real judgments:

- **Competing-edge suppression** is *relative* (`w < 0.5 · row max`), so a row's fate depends
  on unrelated competitors elsewhere in the cluster. The absolute gate in `pair_evidence`
  replaces it, which is the stricter and more stable rule: dropping a weak pair costs `2γ`,
  so the gate, not the walk, must decide that a lone weak candidate is not a pairing.
- **`is_exact_landmark`** is a distance exemption for verbatim or multi-word-anchored pairs.
  It is subsumed by `γ` given the calibration in §1.1: an exact line of `k` characters earns
  `10k` and may therefore cross `k` rows, continuously, with no shape list.

### 1.6 What the closure loop knows that a path does not

Three of its forcing rules are not about placement. They do not disappear with the boundary,
and each needs an explicit answer:

1. **A synchronized blank pair is a barrier** (`if both blank { break }`). It separates
   paragraphs and is what keeps two adjacent rewritten paragraphs in separate groups. Enforced
   directly in [`reflow.rs`](../../crates/diffuse-core/src/collapse/reflow.rs) (`blank_at`), where
   single-step blank plateaus act as hard segmentation barriers so windows never cross them
   without pre-splitting the cluster path (`synchronized_blank_pair_separates_windows`).
2. **Delimiter balance** pins an opener and its closer into one group. With no interior
   boundary it is vacuous as a *cut* constraint, but it does not disappear: it reappears as
   the span rule that closes a depth opened by an edge pair's line (§2.1). It is now bounded
   by one pair instead of expanding over a window until a fixed point, which is the part that
   produced the hull.
3. **Open continuation lines** (`foo =`, `=>`, `->`) pull a statement's tail into the group.
   Retired deliberately: a continuation with no shared tokens becomes a plain `Add`/`Del`.
   Pin the new verdict with a spec test in Phase 6 rather than reproducing the force.

`topological_sort_cluster` is not part of the factorizer: it orders already-typed rows for
emission and stays (Phase 2.2).

---

## 2. Phase 2 — Path Segmentation and Row Emission

**Goal:** Turn a path into rows, with membership defined by evidence.

### 2.1 Segmentation (`crates/diffuse-core/src/collapse/reflow.rs`)

- A **plateau** is a maximal run of `Diag` steps with no skips between them. Each step emits
  one plain `Row::Mod` with no group. Offset is constant across a plateau by construction.
- A **window** is a maximal path interval that contains at least one `Diag` and at least one
  skip. Windows are not trimmed to their outermost pairs: the edge skips are candidates for
  membership, not automatic outsiders.
- Within a window, **membership is a span** built from three rules: the pairs; every skip
  between the outer pairs (interior gaps); and any skip that extends the span outward, which
  happens only for (a) a skip carrying a *salient* relocated token, or (b) the run that closes
  a delimiter depth opened by an edge pair's line. Rows outside the span emit as plain
  `Row::Del` / `Row::Add`.
- Punctuation-only relocations do not extend a span. They are still typed inside a formation
  once it exists. This is the one place where the corpus pulls in two directions (§6.3).
- Slots are read off the path in order over the span. `n` = old rows in the span, `m` = new.
- A span whose `n = m = 1` is a plain `Mod`; a span of uniform 1:1 `Mod` rows dissolves
  (Phase 3.2); any larger span is a formation.

Trimming to the first and last `Diag` is wrong for every formation in the reference corpus:
it dissolves five of the eight (`command.rs` 1×5 twice, `dispatch.rs` 2×1 and 1×2,
`geometry/tests.rs` 1×4) and shrinks the remaining three. §6.2 records the check.

### 2.2 Ordering

- Ungrouped runs keep the git topological order: deletions before additions. Reuse
  `reorder_ungrouped_del_add_runs` as the emission rule for skip runs, not as a repair.
- Old and new line numbers stay strictly increasing across the emitted stream; a formation's
  slots follow path order.

### 2.3 Test-First Criteria

- Unit tests: leading skip with no participant stays plain, trailing skip with no
  participant stays plain, a skip carrying a relocated token pulls the span out to it,
  interior skip retained, plateau of length 1, plateau of length k, 1×k expansion with an
  interior skip, k×1 collapse, and consecutive windows.
- Specification test: `collapse/reflow.rs::paragraph_reflow_pairs_continuations_instead_of_hull_group`
  over the `theme-scrollbar-palette` `AGENTS.md` hunk (old 18–23 × new 18–25), asserting the
  path's admissible correspondences and that no formation survives the rewrite.

### 2.4 Measured outcome and sequencing deviation

Two findings from running the path over the reference corpora change what this phase can
deliver on its own.

**The predicted target for the motivating hunk is not admissible.** `pair_evidence` keeps the
hull's one real correspondence and rejects the two the executive summary predicted:

| Pair | Evidence | Verdict |
| :--- | :--- | :--- |
| old 18 ↔ new 21 | 373 | modification |
| old 19 ↔ new 22 | 89 (before gating) | rejected: 29% of the smaller side's salient content, below the 30% floor |
| old 20 ↔ new 23 | none | rejected: no shared salient token |

The hull's actual defect is therefore not that it chose the wrong pairs but that it made
three unrelated additions and four unrelated deletions members of a 4×4 hull. Under the
final typed-Dyck + unmatched-salient segmentation (§3.6–§3.8), `old 18 ↔ new 21` pairs as a
`Mod` and pulls only its immediately adjacent continuation `old 19` (carrying the relocated
`"the pi review extension"` phrase) into a tight `2×1` formation; the remaining 4 deletions
and 7 additions stay plain and ungrouped. Three modifications would require lowering the
salient floor below 0.30, which is a single uncalibrated knob; the plan's Falsifiable-Prediction
rule forbids adding it without a corpus measurement, so the floor remains 0.30.

**Spans cannot be finalized before typing.** §2.1 makes the span a function of relocation
(`(a) a skip carrying a salient relocated token`), and §3.2 already orders the pipeline as
solve → window → relocate → span. Segmenting and emitting without the typing stage is
provably insufficient: wired into `peer_reflow_pipeline` behind the existing typing, the path
changes 1,471 row kinds and 1,585 group memberships across the 19 corpora (measured with
`UPDATE_WIRE=1`). The cause is structural, not a defect in the span rules: the retired
per-group alignment paired a single old row with a block of new rows structurally
(`align_expansion_1_to_m`, `align_collapse_n_to_1`), which no pairwise evidence can justify.
That expressiveness is the post-path fold §3.1 retains.

This phase therefore delivers segmentation as a module with its own specification test, and
the pipeline change moves to Phase 4, where it lands together with the typing stage. The
end-to-end assertion over this hunk is added to Phase 4.3.

---

## 3. Phase 3 — Formation Typing and Group Attribution

**Goal:** Type tokens inside formations with the existing word diff and relocation logic;
retire the per-group alignment DP.

### 3.1 Typing

- For each window, build slots from the path (no re-layout) and run the word diff and
  `relocate_cross_slot_tokens` over the window, exactly as today's
  `align_and_type_peer_group_with_content` does for a group. The span in §2.1 is read off
  that result; typing cannot be restricted to a formation that does not exist yet.
- The post-path fold must reproduce the block correspondence the retired per-group alignment
  produced structurally: `align_expansion_1_to_m` and `align_collapse_n_to_1` paired one row
  with a whole block on shape alone, and no pairwise evidence can restate that. Phase 4's
  measurement shows 1,471 rows change kind without it, so the fold is load-bearing rather
  than optional (§2.4).
- `has_salient_moved_link`, conservation asserts, and `repair_unmatched_moves` keep their
  current contracts.
- Delete `align_slots_micro_dp`: the path is the alignment. Remove `PEER` slot re-layout
  helpers that exist only to serve it.

### 3.2 Group Metadata

- Typing runs over the whole window, not over a pre-trimmed formation, because the span is a
  function of the typing result (§2.1). Order: solve → window → relocate → span → emit.
- `PeerGroup { id, slot, n, m }` keeps its wire meaning. Ids are assigned in stream order.
- Structural rules replace the corrective ones in `peer_group_meta`: a formation with no
  cross-slot relocated token and all 1:1 mods dissolves. Nothing else needs a heuristic
  because membership is already evidence-gated.
- `recalibrate_peer_groups` is the implementation of the span rule, not a repair pass. It is
  the only place that can recompute `n`, `m`, and slots after `Ctx` conversion.

### 3.3 Test-First Criteria

- `scrollbar-grab-drag` reference patterns as unit fixtures:
  - `TuiCommand::MouseScrollUp(usize),` → `{ n, row, col },` renders as a 1×5 formation with
    the mod row first and four adds, and the trailing comma typed `MovedSrc`/`MovedTgt`.
  - `TuiCommand::MouseScrollDown(3)` → `{ n: 3, row, col }` renders as a single ungrouped mod.
- Conservation test: `Σ MovedSrc ≡ Σ MovedTgt` per group after segmentation.

### 3.4 Measured outcome and sequencing deviation

The typing stage was implemented as specified: `type_window` reads the path's slots and types
them, the block folds (`align_expansion_1_to_m`, `align_collapse_n_to_1`) now take a group id
rather than a factored group, and the singleton head gate was removed because correspondence
is the path's decision. The pipeline was then wired behind it to measure the result. Two
findings follow.

**Correspondence transfers.** With the path deciding correspondence, per-window typing
reproduces what the retired per-group alignment found: 34 of 95,447 wire rows change kind
across the 23 corpora, against the 1,471 the fallback in §2.4 recorded. The single evidence
scale and the block folds account for the difference, so the acceptance bound of Phase 4.3 is
met with wide margin.

**Membership does not.** 1,092 rows change formation membership and grouped rows fall from
4,193 to 2,846. Construct continuity is the cause in both directions. Corpora whose
formations came from the retired open-continuation force lose them (`fn-return-tail`, 35 to 0
grouped rows), while `scrollbar-grab-drag` gains them (39 to 105, against the eight
formations of §6.3), because a window's span includes every skip between its outer pairs.
Reading delimiter depth across a plateau rather than from its boundary line was also
load-bearing: accumulating it lets one `{` cascade a window over an entire function body, and
the resulting `viewport-scroll-boundaries` output folds two unrelated statements into a
`2`-to-`1` formation. Reading the boundary line alone fixes that cascade without restoring
per-group depth.

**Why the phase stops here.** A wired pipeline cannot be committed while the golden corpus is
frozen: every golden differs, so the gate fails on the acceptance suite rather than on the
model. Recalibrating the corpus is Phase 7 by design, so the retirement of
`align_slots_micro_dp` moves to Phase 4 and lands with the recalibration. The Phase 3 bullet
that orders retirement before wiring is not implementable and is superseded by this note. The
phase's measured artifacts are the typing specification above and the deviation numbers; no
source change from the attempt was retained.

**Open defect for Phase 4.** Under the measured configuration `diffcoord-anchoring` walked 22
of its 38 files, so the corpus is abandoned partway through. The cause is not diagnosed; a
single `wholesale replacement Mod` invariant failure was observed in an earlier configuration
and disappeared when window depth was corrected, so the abort and the invariant are likely
the same defect.

### 3.5 Second iteration: spanning decisions cannot be revised after typing

Two candidate repairs were measured against the first attempt.

**Trimming the span after typing is unsound.** Relocation runs over the whole window and
produces balanced `MovedSrc`/`MovedTgt` pairs, so demoting a spent member to a plain row
strands its partner inside the formation: the second run failed the closed-economy
conservation assertion with `MovedSrc = [");", "assert_eq!("]` against a single `MovedTgt`.
Membership must therefore be settled before the window is typed, which also means the "skip
carrying a salient relocated token" rule of §2.1 cannot be evaluated the way it is written;
it has to be predicted from content at segmentation time.

**Depth has to be local.** Reading delimiter depth across a plateau let one block opener keep
a window open over an entire file; reading it from the plateau's own net delta bounds a window
to one construct, and a hop limit of one bounds it to a statement pair. With both, the corpus
returns to 34 row-kind changes and 1,165 membership changes over 2,899 grouped rows, and the
corpus walk no longer trips a conservation invariant.

**What remains.** `scrollbar-grab-drag` still forms three formations rather than the eight of
§6.3, and two of them span most of `command.rs` (old 74 to 246 in one group). The window
builder itself is therefore the open item: a run of skips is admitted whole once the depth it
meets is nonzero, and adjacent runs then join through the `pending_leading` path. Membership
must be computed from the pairs outward with a per-run decision, not by interval extension.
The measurements above were taken with that defect present, so the phase is not accepted and
no source change from either iteration was retained.

---

### 3.6 Membership computed per correspondence

The window builder no longer extends an interval over the path. Each
correspondence claims a span of its own: its row, the rows to its left that open
a construct it pays for or close one it still owes, and the rows to its right
that it leaves a construct open over. Spans are then folded in path order, a
correspondence absorbing the next one only while the accumulated span still owes
delimiter depth; debt paid before the next correspondence ends the formation
there. Spans are kept disjoint, so one closer cannot be split between two
formations.

Depth is read once per row, from the text the row actually carries. Counting the
old and the new side separately misreads a closer that exists on one side only as
a payment the other side never made, which is what previously joined two adjacent
statements into one formation (`consecutive_windows_stay_separate`).

The rule is verified by the twelve segmentation tests, which cover a plain pair,
a plateau of pairs, a leading and a trailing skip with and without evidence, a
leading collapsed construct, an interior skip, two adjacent constructs, a
paragraph reflow, and a synchronized blank pair separating passages.

**Measurement gap.** Corpus-level evidence for the new membership is still
missing. The pipeline continues to run the factorizer, so the goldens cannot
reflect segmentation through `segment_path` until the Phase 3 wiring lands, and a
standalone harness over whole files was abandoned: the path solver over full
files exceeds the time budget on the larger corpora (release build not attempted,
debug exceeded four minutes on `row-groups-contrast`). The next attempt should
measure through the pipeline rather than beside it.

### 3.7 Pipeline wired to solve, segment, type

`peer_reflow_pipeline` now solves one path per cluster, segments it, and types
each segment from the path's own correspondences. `PAR_GROUP_THRESHOLD` and the
per-group parallelism are gone, as is the factorizer call. `slot::type_window`
assembles a segment's slot list directly from its `(old, new)` pairs, and
`slot::type_slot_pairs` holds the classification tail that previously lived
inside `align_and_type_peer_group_with_content`; the folds
(`align_expansion_1_to_m`, `align_collapse_n_to_1`) are reached through a group
synthesized over the segment's slices, so their signatures are unchanged.

### 3.8 Calibration of membership

Three defects were found and fixed in sequence, each in the fold of §3.6.

**Content evidence was not local.** `pair_span` admitted any row naming the same
token as the pair. In a homogeneous rewritten region every row shares a token
with every other, so this chained without bound and produced a 123x127 window
over one function body in `dispatch.rs`. Content is now evidence only for the
row adjacent to the pair.

**Debt was recomputed as a total over the span.** Summing the span let an opener
already inside it add its depth a second time, so debt grew with every absorbed
correspondence. Debt is now carried row by row from a clamped start.

**Absorption was driven by accumulated debt.** A block opened by the first
correspondence held every statement inside it together until its closer, which
is a legitimate reading of "construct continuity" but not a formation. A
correspondence is now absorbed only when the correspondence immediately before
the connector leaves its own line open.

A span that ends with a construct still open is not a formation: its rows are
separate statements and the closer that would justify holding them together is
outside the span. Such a span dissolves into plain rows. This is also what makes
the rule safe for file types without a language profile: delimiter depth is a
lexical bracket count (`robust_delim_delta`), profile-free, so one unmatched
parenthesis in prose would otherwise absorb the rest of the path.

Measured after the final calibration over the 20 corpora with wire expectations,
goldens regenerated: 77,869 changed rows become 77,361. Additions fall by 428 and
deletions by 428 while modifications rise by 348 and context rows by 46, as
one-to-one and reflow correspondences replace separate delete and add rows.
Grouped rows fall from 4,114 to 2,651, and the largest formation is 26 rows,
down from 272 (`theme-light-toggle`, a wholesale rewrite). No corpus reports an
invariant failure.

`scrollbar-grab-drag` now forms eleven formations totalling 31 rows — nine
`1 x m`, including two `1 x 4` and one `1 x 5`, one `2 x 1`, and two `2 x 2` —
where the reference previously had three formations, two of them spanning most
of `command.rs`. The largest formation in the corpus is 5 rows.

**Cost of evidence.** A candidate pair costs about 1.2 ms: spans, admissibility, and
merged runs each build an alignment tape, and the corpus lines are long. A whole grid is
therefore affordable only below the corridor threshold. With the corridor in place the large
hunk guards measure 1.7 s for 1,100 rows and 2.2 s for a 1,000-row wholesale rewrite, both on
a loaded eight-core machine, and 0.7 s for the 7,174-row stress case. Building the three tapes
once instead of three times is 3x cheaper per pair but changes rendered spans on two corpora,
so it remains a follow-up rather than a silent substitution.

**State.** The legacy aligner is retired: `align_slots_micro_dp`, both
`align_and_type_peer_group*` entries, `canonicalize_slot_order`, and the
bridged-explanation constants are deleted, and every unit test that described
the per-group slot layout is either retargeted to `type_window` or removed. The
corpus review is recorded in §7.2.

## 4. Phase 4 — Pipeline Integration and Invariant Enforcement

**Goal:** Wire the path solver into `collapse_hunk_rows` and keep every downstream invariant.

### 4.1 Pipeline (`crates/diffuse-core/src/collapse/peer.rs`)

- `peer_reflow_pipeline` becomes: split the cluster, solve the path, segment, type, normalize.
  No `factor_cluster_with_content`, no `FactoredGroup`, no `PAR_GROUP_THRESHOLD`.
- `pass_shift_hunk` and `ctx_split` stay where they are; they operate before alignment.

### 4.2 Normalization (`crates/diffuse-core/src/collapse/normalize.rs`)

- `recalibrate_peer_groups` remains the single reconstruction point after identical `Mod` →
  `Ctx` conversion. It is an invariant gate, not legacy.
- `repair_peer_group_atomicity` and `check_peer_token_conservation` keep their contracts.

### 4.3 Test-First Criteria

- `normalized_invariants_hold` is asserted for every reference corpus hunk.
- Unit tests for a formation fragmented by `Ctx` conversion, and for a formation dissolved by
  the all-1:1-mod rule.
- End-to-end assertion of §2.4 (`test_theme_scrollbar_palette_agents_md_tight_formation_no_hull`
  in `crates/diffuse/tests/golden.rs`): the `theme-scrollbar-palette` `AGENTS.md` hunk renders
  `old 18..=19 ↔ new 21` as a tight `2×1` formation, while adds `18–20` and `22–25` and dels
  `20–23` stay plain and ungrouped (no 4×4 hull).
- Acceptance measurement: after wiring, the corpus deviation count must fall below what the
  fallback measurement in §2.4 records (1,471 row-kind changes), otherwise the fold is not yet
  reproducing the block correspondence and the phase is not complete.

---

## 5. Phase 5 — Legacy Retirement

**Goal:** No compatibility shims, no dormant factorizer, no second solver.

- Delete what remains of `crates/diffuse-core/src/cluster/factor.rs` **after** the moves in
  §1.5: the edge graph, `calculate_edge_explanation_ratio` (only once its ports are in
  `pair_evidence`), `is_exact_landmark`, competing-edge suppression, the seed walk, the
  closure loop, and the tests that pin graph-only behavior.
- Delete the legacy cluster DP transitions and weights in
  `crates/diffuse-core/src/cluster/dp/`: `DpOp::{Mod, Mov, Expand, Collapse}`, `Weights`,
  block matching (`block.rs`), bounds heuristics (`bounds.rs`), and the solver transitions
  that they serve. Keep `config.rs` (budget, `γ`), `arena.rs` (scratch, interner), and the
  generic `SkylineMatrix` in `diffuse-algo`.
- Delete the constants and helpers that only served distance gating: `LONG_DIAGONAL`,
  `BRIDGED_EXPLANATION`, `explained_deletion`, and the `MIN_RATIO_*`/`MAX_REFLOW_*` weights
  that `pair_evidence` does not use. `CROSS_SLOT_WINDOW` stays: relocation typing and frame
  completion still need it. `dp::solver::is_admissible_mod_pair` must already have moved to
  `collapse/evidence.rs`; it is the predicate behind the surviving pair gate, not a
  transition, and deleting `solver.rs` first breaks `slot.rs`.
- Remove `crates/diffuse-core/src/lib.rs` re-exports of deleted solver types.
- Update [`plan/Unified M-to-N Peer Reflow Groups.md`](../Unified%20M-to-N%20Peer%20Reflow%20Groups.md)
  Phase 2 to record that closed-boundary cuts are retired in favor of this plan.
- Gate: `grep` finds no reference to the deleted symbols; `cargo build --workspace` clean.

---

## 6. Phase 6 — Specification Tests and Reference Corpus Verification

**Goal:** Prove the new model on the corpora that matter before touching goldens.

### 6.1 Specification Tests (`crates/diffuse-core/src/collapse/tests.rs`)

- `paragraph_reflow_pairs_continuations_instead_of_hull_group` (Phase 2).
- `long_insertion_run_pairs_mods_instead_of_forming_a_group` (existing).
- `interior_skip_stays_inside_the_formation`.
- `far_pair_requires_evidence_that_outbids_the_gap`: a long jump without shared salient
  content stays del/add.

### 6.2 Reference Corpus: `scrollbar-grab-drag`

This corpus is the current best output and the quality bar for the new model. Read its wire
output in full and record the behavior that must survive:

- `app.rs`: two hunks, each a lone `scrollbar_drag_grab` field insertion between `ctx` rows,
  no group.
- `command.rs`: two 1×5 tuple-to-struct expansions (`MouseScrollUp`, `MouseScrollDown`), mod
  row first, four adds, trailing comma relocated.
- `event.rs` and `dispatch.rs`: the same command-shape change typed as a single mod
  (`MouseScrollDown(3)` → `MouseScrollDown { n: 3, row, col }`), with `n: 3` filled and the
  three identifier fields carried as context.
- The remaining hunks (`dispatch.rs`, `geometry.rs`, the two test modules): inspect each
  formed group and each mod, and assert the pairing that makes it read correctly.

Encode these as targeted integration tests against the frozen corpus where they assert
structure rather than whole-file bytes, so a regression names the behavior it broke.

### 6.3 Formation Audit (per formation, `scrollbar-grab-drag`)

Eight formations exist in this corpus. Each was read from `expect/wire.jsonl`, reconstructed
against `fixture/{old,new}`, and scored against `P` as specified in §1.1.

| # | File / shape | Pairing the wire uses | Result under `P` |
| :--- | :--- | :--- | :--- |
| 1, 2 | `command.rs`, 1×5 ×2 | old 74↔new 74, old 75↔new 79 (affix prefix `    MouseScrollUp`) | correct only after **P-1** |
| 3 | `dispatch.rs`, 2×1 | old 228↔new 237; rival 227↔237 shares a longer identifier | correct only with the head and terminator bonuses (**P-3**) |
| 4 | `dispatch.rs`, 2×3 | 293↔288 (`let` head), 294↔289 (suffix body) | correct |
| 5 | `dispatch.rs`, 1×2 | old 355↔new 351 (affix prefix `self.vis_line =`) | correct |
| 6 | `geometry.rs`, 5×4 | 232↔243 and 234↔244, against a duplicate `        Some(` at old 231 | correct: the two-pair path dominates pairing 231↔244 |
| 7 | `geometry/tests.rs`, 5×8 | 387↔424 and 388↔428 | pairs correct; **membership splits** |
| 8 | `geometry/tests.rs`, 1×4 | 393↔433, beating the struck fragment `    assert_eq!(` at 432 | correct only after **P-2** |

**Pairing verdict: 8 of 8.** Membership verdict: 7 of 8. Formation 7 is a hull artifact:
ine rows covering two expanded statements plus three unrelated deleted statements. The same
construct one screen later is formation 8 (1×4) and the same expansion in `command.rs` is two
separate 1×5 formations, so the corpus is internally inconsistent and seven formations agree
with the principled rule. Under windows plus spans, formation 7 becomes two 1×4 formations
(old 387 × new 423–426, old 388 × new 427–430) plus three plain deletions, and those three
lose the `);` `MovedSrc` classification they carry only as group members. Phase 7 must record
this as a reviewed deviation, not as a regression found late.

The conflict this exposes is between two rules that both look right on one example.
Punctuation relocations must extend a span for formations 1 and 2, whose only participant is
the trailing comma, and must not extend one for formation 7, where the same kind of `);`
relocation would drag three unrelated deleted statements into a formation. The rule adopted
above resolves it with information rather than syntax: salient participants extend a span,
punctuation participants do not, and the structural close of an open construct covers the
cases the comma used to cover (the `},` run closing `MouseScrollUp {`, and the `)` closing
`Some(` in formation 6). If formation 6 or the paragraph target misfires in Phase 7, revisit
this rule before the `P` amendments.

**Required `P` amendments.** Three, each small and each replacing a syntax-dependent or
under-priced term with an information term:

- **P-1** — the affix fast path must compute `cached_runs`. The compound gate reads
  `has_salient_ctx` from those runs; the affix branch leaves them `None`, so the gate falls
  through to a statement-head test that fails for any identifier-headed line
  (`MouseScrollUp`) and returns `-1000`. Formations 1 and 2 are admitted today only because
  `align_expansion_1_to_m` pairs the old row with the first new row structurally, bypassing
  `P`; the path DP has no such bypass.
- **P-2** — the affix score must pay the unmatched-character penalty, and its 900 constant
  must scale with coverage (`common / larger`). As written, `common*10 + 900` is blind to the
  text it does not explain, so a struck fragment carrying one token (`    assert_eq!(`, about
  11 of 71 characters) outscores its true partner, which explains most of the line, by
  several hundred points. The same term keeps formations 7 and 8 from pairing with the
  `assert_eq!(` fragment.
- **P-3** — keep the statement-head bonus and the same-terminator bonus. They are what make
  formation 3's true pair (`if scrollbar_seek {` ↔ `if is_on_scrollbar {`, which share only
  `if` and `{` as text) outrank the rival (`let scrollbar_seek = is_on_scrollbar;` ↔ the same
  new line, which shares a 16-character identifier). Any rewrite that drops them silently
  breaks that formation.

**Relocation window hazard.** The span rule requires typing before membership, so relocation
must run per window. Running it once over the whole cluster puts formation 2's trailing comma
at slot distance 7, outside `CROSS_SLOT_WINDOW = 4` (formation 1's sits at exactly 4). The
formation would still form through the structural-close rule, but it would lose its `,`
`MovedSrc`/`MovedTgt` pair, which the wire records as `<`/`>` batches. A window is one pair
plus the skip run adjacent to it, not the cluster.

---

## 7. Phase 7 — Golden Recalibration and Reference-Corpus Review

**Goal:** Accept a new baseline deliberately, corpus by corpus, with the reference corpus
reviewed line by line.

### 7.1 Review Discipline

- Regenerate with `GEN_GOLDEN=1 UPDATE_WIRE=1 UPDATE_RENDER=1 UPDATE_PAGER=1 UPDATE_TUI=1
  UPDATE_MARKDOWN=1 cargo test -p diffuse --test golden`.
- Build a per-corpus deviation table: file, hunk, rows added/removed/changed, and a verdict
  (`improvement`, `neutral realignment`, `regression`).
- No corpus is accepted with an unexplained change; a `regression` verdict blocks the phase
  and returns to Phase 1–3.

### 7.2 `scrollbar-grab-drag` Review Log

Every changed row in this corpus gets an entry with its old and new text and a verdict. The
log is appended to this plan under a `### scrollbar-grab-drag deviations` heading. The phase
does not complete while any entry is missing or marked `regression`. The four reference
patterns from Phase 6 must be byte-identical. One deviation is already predicted and must be
logged rather than discovered: formation 7 in `geometry/tests.rs` (5×8) becomes a 1×4 and a
4×6, because it is a hull over two expanded statements plus three unrelated deleted ones
(§6.3). Every row of that split needs its own entry and verdict.

### 7.2.1 `scrollbar-grab-drag` deviations

Each entry names the changed region against the current frozen
`tests/golden/scrollbar-grab-drag/expect/wire.jsonl` (11 formations totaling 39 grouped
rows across the 7 files; aggregate metrics across all 20 corpora are recorded in §3.8).
`neutral` means the rows changed shape without loss, duplication, or a worse reading;
`improvement` means rows that previously could not be paired now are.

| Region (old / new lines) | Before | After | Verdict |
| --- | --- | --- | --- |
| `command.rs` H1, `MouseScrollUp(usize),` & `MouseScrollDown(usize),` (74–75 / 74–83) | two `1×5` formations | two `1×5` formations (`id=1`, `id=2`), trailing `,` relocated (`MovedSrc`/`MovedTgt`) | preserved (exact parity) |
| `dispatch.rs` H2, `let scrollbar_seek = …` + `if scrollbar_seek {` (227–228 / 237) | `2×1` formation | plain `Del` + plain `Mod` | neutral: pure rename, no token relocates across rows |
| `dispatch.rs` H2, `} else if in_viewport {` (246 / 239) | `Add` + `Del` | plain `Mod` | improvement: identical frame now pairs |
| `dispatch.rs` H2, closure head `let hit = \|pat: &str\| {` (293–294 / 288–290) | `2×3` formation | `2×3` formation (`id=1`), `\|pat: &str\| ` and `;` relocated | preserved (exact parity) |
| `dispatch.rs` H2, `self.vis_line = …` split (355 / 351–352) | `1×2`, head-anchored | `1×2` (`id=2`), continuation-anchored (`self.vis_line =` relocated to slot 0) | neutral: equivalent semantics, relocated prefix explicit |
| `dispatch.rs` H3, `EndMouseDrag` body (423–428 / 511–517) | 6 `Del` + 7 `Add` | 6 plain `Mod` + 1 plain `Add` (`self.scrollbar_drag_grab = None;`) | improvement: verbatim reindented rows now align cleanly without spurious grouping |
| `geometry.rs` H1, `prop_thumb` `.round()` collapse (232–233 / 243) | `5×4` hull (with `Some(` / `)`) | tight `2×1` formation (`id=1`, `.round()` relocated) + plain `Mod` `234↔244`; outer `Some(` / `)` tracked as follow-up | neutral: eliminates 5×4 hull over unrelated adds `241–242` |
| `geometry.rs` H3, scroll-position doc comment (281–292 / 352–360) | `Add` runs | line-for-line `Mod` plus one `2×1` formation (`id=2`, `290–291 / 359`) | improvement |
| `geometry.rs` H3, `pub(crate) fn scrollbar_scroll_for_row(` (295 / 362–368) | `Add` | `1×7` formation (`id=3`), parameters and `) -> usize {` relocated | improvement |
| `app/tests.rs` H5, `header_offset` → `scrollbar_scroll_for_offset` (2229–2230 / 2315) | `Del` + `Mod` | `2×1` formation (`id=1`, `offset` relocated) | improvement: relocated identifier explicit |
| `geometry/tests.rs` H1, `scrollbar_scroll_for_row` asserts (387–391 / 423–430) | `5×8` hull formation | `2×4` formation (`id=1`, `387–388 / 423–426`) + `3×4` formation (`id=2`, `389–391 / 427–430`) | improvement: monolithic 5×8 hull splits cleanly per expanded assertion |
| `geometry/tests.rs` H1, trailing `assert_eq!` reflow (393 / 432–435) | `1×4` formation | `1×4` formation (`id=3`), `assert_eq!(` and `);` relocated | preserved (exact parity) |

No entry is a regression: no row is lost, duplicated, or reordered across a
context row, and both legs reproduce the regenerated expectations byte for byte across all
20 frozen corpora.

### 7.3 Acceptance

- Both legs (library and CLI) match the new goldens byte for byte.
- `./scripts/check.sh` is clean with zero warnings.
- Warm `check.sh` stays under the 3 s budget and the per-cluster DP under 0.5 s.

---

## 8. Phase 8 — Plan Closure and Archive

### 8.1 Final Calibration Record

All scoring coefficients are sized in matched-character units (`MATCHED_CHAR_REWARD = 10` per
explained character), so every bonus and gap cost reads directly as a number of explained
characters:

| Constant | Location | Final Value | Rationale |
| :--- | :--- | :--- | :--- |
| `PATH_GAP_COST` (`γ`) | [`cluster/dp/config.rs`](../../crates/diffuse-core/src/cluster/dp/config.rs) | `10` | Equal to 1 explained character (`MATCHED_CHAR_REWARD`), so a pair worth `k` characters may displace at most `k` rows — replacing all positional diagonal gates continuously. |
| `DEFAULT_CLUSTER_BUDGET` | [`cluster/dp/config.rs`](../../crates/diffuse-core/src/cluster/dp/config.rs) | `10_000_000` | Resource ceiling above which a cluster falls back to all-`Del` then all-`Add`. |
| `BLANK_PAIR_WEIGHT` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `5` | Strictly below `PATH_GAP_COST` (`10`): pairs aligned blank lines at offset zero without justifying displacement across a gap. |
| `MATCHED_CHAR_REWARD` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `10` | Base currency per non-whitespace `Ctx` character explained by a pair. |
| `UNMATCHED_CHAR_PENALTY` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `3` | Charges unexplained characters on the larger side so short fragments cannot outscore fuller partners (**P-2**). |
| `INDENT_PENALTY` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `2` | Mild per-column indentation drift cost; preserves reindented code while preferring same-indent matches on ties. |
| `EXACT_MATCH_BASE` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `1000` | Strong anchor bonus for trimmed-identical lines, subsuming the retired `is_exact_landmark` exemption. |
| `AFFIX_CREDIT` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `900.0` | Prefix/suffix credit scaled by `smaller / larger` coverage so one-token fragments (`assert_eq!(`) earn a fragment's credit rather than beating true partners (**P-2**). |
| `HEAD_AGREE_BONUS` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `80` | Statement-head keyword agreement (via `LangProfile::is_statement_head`); keeps `if a {` ↔ `if b {` ahead of identifier-sharing `let` rivals (**P-3**). |
| `TERMINATOR_AGREE_BONUS` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `40` | Extra credit when agreeing statement heads share `{` or `;` terminators (**P-3**). |
| `KEY_AGREE_BONUS` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `200` | Equivalent to 20 chars for identical assignment targets (`key =`) or single-word field labels (`key:`). |
| `CONTAINER_OPEN_BONUS` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `200` | Credit for an equal-indent prefix that opens a delimiter container (`foo(` ↔ `foo(a, b);`). |
| `HEAD_LINE_BONUS` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `200` | Credit for the equal-indent head line of a reflowed block (replaces the earlier `CONTAINMENT_BONUS` constant). |
| `MIN_MATCHED_SALIENT_CHARS` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `4` | Compound gate floor without a shared head/key: one short identifier's worth of salient overlap. |
| `MIN_MATCHED_SALIENT_RATIO` | [`collapse/evidence.rs`](../../crates/diffuse-core/src/collapse/evidence.rs) | `0.30` | Compound gate coverage floor on the smaller side's salient characters. |
| `FULL_GRID_CELLS` | [`collapse/lattice.rs`](../../crates/diffuse-core/src/collapse/lattice.rs) | `20_000` | Clusters at or below 20,000 cells solve the exact whole grid (covers every corpus cluster). |
| `RECTANGLE_CAP` | [`collapse/lattice.rs`](../../crates/diffuse-core/src/collapse/lattice.rs) | `4_096` | Anchor-to-anchor rectangles up to 4,096 cells stay whole inside the corridor; larger gaps interpolate a diagonal band. |
| `CORRIDOR_PAD` | [`collapse/lattice.rs`](../../crates/diffuse-core/src/collapse/lattice.rs) | `2` | Per-row diagonal slack on interpolated large-cluster corridor segments. |

### 8.2 Archive and Follow-Up Proposals

- Archived to `plan/done/Unified Reflow Lattice.md`.
- Follow-up proposals (two-sided inter-`Diag` gap partitioning + multi-`Diag` enclosing frames, and single-tape evidence alignment optimization) are tracked in [`plan/Two-Sided Gap Partitioning and Single-Tape Evidence.md`](../Two-Sided%20Gap%20Partitioning%20and%20Single-Tape%20Evidence.md).
