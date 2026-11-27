# Plan Guidelines — Authoring Engineering Plans

This document defines the canonical structure, formatting, and discipline for engineering and architectural plan documents in the `diffuse` repository.

---

## Plan Structure & Requirements

Every plan document in `plan/` follows this structure:

### 1. Roadmap & Status at the Top
- Place a **Roadmap & Status** section directly after the title. It serves as both the live status dashboard and the clickable Table of Contents — each **Step** is an **Action** or an **Experiment**, listed in execution order with a status checkbox and a clickable Markdown link to its section anchor (e.g., `- [ ] **Step 1 (Action)** — [Tighten screws, check stability](#step-1-action--tighten-screws-check-stability)`).
- **Do not** list separate "Check" or "Evaluation" steps — checks are implied by every Action, and evaluations are implied by every Experiment. Record only their outcomes.

#### Status markers

Step checkboxes use three states:

- `- [ ]` — not started.
- `- [·]` — **in progress** (center dot, U+00B7; copy it from here — it is not typeable on most keyboards). Applied when work has begun but has not yet completed review.
- `- [x]` — **complete**.
  - For an **Action**: work is done *and reviewed* — never merely "code written".
  - For an **Experiment**: probe and evaluation are complete and folded back into the plan. Append a compact inline verdict tag (e.g., `*(promoted → Step 5)*`, `*(pivoted → Step 3)*`, or `*(dropped)*`) so the roadmap stays scannable.

#### Checkoff & signoff discipline (Actions)

- An Action step is marked `[x]` **only after review, just before committing** (per the repository's commit policy). Normally that review is the human's visual review of the change, including any golden/wire output diffs (human visual inspection catches anomalies that automated checks cannot).
- If the user has instructed the agent to commit automatically, the agent may mark the Action `[x]` after performing the **agent review** of the wire/golden output described in [`tests/AGENTS.md`](../tests/AGENTS.md) — asserting the changes are strictly better or at least equivalent to the previous representation.
- Even when explicitly asked to auto-commit, the agent must **highlight anything suspicious and pause for review** rather than commit a questionable golden change. Questionable changes always require human review and signoff.
- While work is underway (implemented but not yet reviewed/committed), the step keeps the `- [·]` marker.

### 2. Executive Summary
- A concise summary explaining:
  - What the plan achieves.
  - The core problem or limitation being addressed.
  - The mathematical invariants and architectural principles governing the solution.

### 3. Living Truth Table (Recommended for Perceptual / Heuristic Plans)
- For plans governing diff pairing, reflow, or rendering quality, include a compact table of concrete input pairs with expected outcomes (e.g., `PAIR` vs. `SPLIT`) directly after the Executive Summary.
- Every Experiment and Action evaluates against this table and appends newly discovered borderline cases from corpus audits.

### 4. Steps: Actions vs. Experiments

Each step has a dedicated heading matching its anchor in **Roadmap & Status**. Plans interleave two step types reflecting confidence level:

#### Actions (High Confidence — Committed Deliverables)
- Concrete engineering tasks whose design and expected outcome are already established.
- Specify the affected modules/files, the exact data structures, algorithms, or invariants to implement, and any non-obvious constraints.
- **Implied checks**: Every Action implicitly carries full repository verification — test-first unit and integration tests, 100% branch coverage on altered paths, golden corpus audit, and `scripts/check.sh`. Do not spell out boilerplate check steps in the plan; record only the check outcome (e.g., affected golden corpora, unexpected findings) once executed.
- Actions may motivate plan updates if a check reveals a surprise, though this is not expected.

#### Experiments (Varying Confidence — Empirical Probes)
- Exploratory probes for ideas, thresholds, or structural hypotheses that need empirical validation before earning production code.
- **Strip implementation chaff**: Do *not* specify helper names, struct layouts, or line-by-line blueprints inside an Experiment, and do *not* run full verification ceremony (`scripts/check.sh`, exhaustive unit tests, formatting) during probes. Keep Experiments lean with four fields:
  1. **Hypothesis**: 1–2 sentences stating the mechanism and expected effect.
  2. **Falsifier**: The concrete failure mode or regression that would disprove or reshape the idea.
  3. **Competing Variants**: 2–3 contrasting mechanisms to pit head-to-head on the same corpus (avoid testing a single pre-baked heuristic in isolation).
  4. **Probe**: How to compare variants cheaply via uncommitted prototypes against the 23 golden corpora.
- **Clean working tree before probing (advisory)**: Before launching an Experiment, prefer a clean working tree (commit finished work, stage plan edits out of the way, or stash as appropriate — use judgment). A clean baseline makes rollback trivial (`git checkout -- ...`) and lets probes regenerate golden fixtures directly in place for fast `git diff` inspection.
- **Implied evaluation & plan rewrite**: Every Experiment implicitly ends with a corpus-wide evaluation reporting **Best Wins / Worst Casualties / Surprises** and folding findings back into the plan. Record a compact **Result** block (verdict: `Promote → Step N (Action)`, `Pivot`, or `Drop`; 3–5 concrete corpus examples; and the invariant learned — never prose diaries).
- **Full license to rewrite, mutate, or spawn sub-plans**: A plan is a living document. Experiments are expected to rewrite downstream sections — and may do so arbitrarily at the agent's discretion: promoting winners into Actions, merging or re-rooting remaining Experiments, mutating the plan's scope or direction completely, dissolving the plan if the premise proves unnecessary, or spawning separate sub-plans in `plan/` to capture out-of-scope discoveries surfaced along the way. Whenever a rewrite changes direction meaningfully, highlight what shifted and why for the reviewer. The plan converges when every Experiment is resolved and replaced by high-confidence Action(s) (or cleanly retired).

### 5. Historical Plan Archive
- When a plan is fully executed, move it to `plan/done/<Plan Name>.md` to maintain a permanent historical log of architectural decisions and milestones.

---

## Domain Classifier — Diffing & Alignment

Decisions in the `diffuse` diffing, pairing, and reflow pipeline operate under an **asymmetric cost prior**:

> **It is roughly 3× worse to admit a bad pairing than to dissolve a desired pairing. False positives far outweigh false negatives.**

Operational consequences for Actions and Experiments:

- **Visual garble is the primary failure mode**: An incorrect `Mod` pairing or spurious cross-line relocation produces fragmented "confetti" highlights that actively mislead the reviewer, whereas dissolving an ambiguous pairing falls back to clean, honest `Del` + `Add` rows that remain immediately readable.
- **Weighing corpus deltas**: When evaluating an Experiment's **Best Wins** against its **Worst Casualties** across the 23 golden corpora, treat each introduced bad pairing (false positive) as ~3× heavier than each dissolved valid pairing (false negative). A heuristic that cleans up 10 garbled rows at the cost of breaking 2 legitimate ones is a clear net win; one that rescues 2 borderline pairings at the cost of introducing 1 garbled row is a net regression.
- **Tie-breaking under uncertainty**: Whenever a pairing decision is genuinely ambiguous, prefer dissolving it (`Del` + `Add`) over guessing a weak correspondence.
