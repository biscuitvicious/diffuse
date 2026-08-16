# Plan Guidelines — Authoring Engineering Plans

This document defines the canonical structure, formatting, and discipline for engineering and architectural plan documents in the `diffuse` repository.

---

## Plan Structure & Requirements

Every plan document in `plan/` must follow this structure:

### 1. Global TODO at the Top
- Place an actionable, global checkbox list (`- [ ]`) at the top of the plan document directly after the title.
- Covers the core deliverables across the entire plan.
- Gives human reviewers and agents an instant status dashboard of remaining work.

### 2. Executive Summary
- A concise summary explaining:
  - What the plan achieves.
  - The core problem or limitation being addressed.
  - The mathematical invariants and architectural principles governing the solution.

### 3. Numbered Table of Contents
- A numbered table of contents listing every phase and section.
- Every entry in the table of contents must be a clickable Markdown link pointing to its corresponding section anchor (e.g. `1. [Phase 1 — Sliding Hunks & Context Normalization](#1-phase-1--sliding-hunks--context-normalization)`).

### 4. Phase Breakdown into Concrete Steps
- Each phase must have a dedicated numbered heading matching the Table of Contents.
- Each phase is broken down into sequential, actionable steps with:
  - The specific module and file paths affected.
  - The exact data structures, algorithms, or invariants to implement.
  - Test-first criteria and verification gates (`#[test]`, `apply_oracle`, `check.sh`).

### 5. Historical Plan Archive
- When a major plan is fully executed, move it to `plan/done/<Plan Name>.md` to maintain a permanent historical log of architectural decisions and milestones.
