# Expand Mod Line Rendering Mode — Engineering Plan

- [x] Phase 1 — Data Model & Mod Side Representation
- [x] Phase 2 — Gutter Icons (`┌-` / `└+`) & Segment Rendering Pipeline
- [x] Phase 3 — Flat List Construction & State Tracking (`expanded_mods`)
- [x] Phase 4 — Enter Key Toggle & Navigation Pivot Stability
- [x] Phase 5 — Global Keyboard Shortcut (`e`) & Secondary Status Bar Indicator
- [x] Phase 6 — Golden Acceptance Corpora & Test Verification

---

## 1. Executive Summary

This plan introduces an **"expand mod line"** rendering mode in `diffuse`. A modification row (`RowKind::Mod`) normally reconciles asymmetric additions and deletions into a single logical visual line containing both struck (`Del`) and filled (`Add`) spans. When expanded, the mod row decomposes into two sequential visual rows:
1. One **`del` row** displaying the pre-image line (`old_line`, `old_text()`) with deletion spans highlighted. Gutter icon is `┌-`.
2. One **`add` row** displaying the post-image line (`new_line`, `new_text()`) with addition spans highlighted. Gutter icon is `└+`.

This behavior mirrors `MovSrc` / `MovTgt` indentation change rendering, extended across all change spans on the mod row.

Expansion can be toggled per-row using `Enter`, or toggled globally across all files using `e` with two-state cycle semantics:
- If zero or some mod rows are expanded: expand all mod rows.
- If all mod rows are expanded: collapse all mod rows.

```mermaid
flowchart TD
    subgraph Collapsed ["Collapsed Mod Row (1 line)"]
        C["RowKind::Mod: ~ 42 old_text -> new_text (struck & filled)"]
    end

    subgraph Expanded ["Expanded Mod Rows (2 lines)"]
        E1["ModSide::Del: ┌- 42 old_text (del spans highlighted)"]
        E2["ModSide::Add: └+ 43 new_text (add spans highlighted)"]
    end

    C -->|Enter (cursor lands on del)| E1
    E1 -->|Enter (pivots to mod)| C
    E2 -->|Enter (pivots to mod)| C
```

---

## 2. Table of Contents

1. [Phase 1 — Data Model & Mod Side Representation](#phase-1--data-model--mod-side-representation)
2. [Phase 2 — Gutter Icons (`┌-` / `└+`) & Segment Rendering Pipeline](#phase-2--gutter-icons-----and---segment-rendering-pipeline)
3. [Phase 3 — Flat List Construction & State Tracking (`expanded_mods`)](#phase-3--flat-list-construction--state-tracking-expanded_mods)
4. [Phase 4 — Enter Key Toggle & Navigation Pivot Stability](#phase-4--enter-key-toggle--navigation-pivot-stability)
5. [Phase 5 — Global Keyboard Shortcut (`e`) & Secondary Status Bar Indicator](#phase-5--global-keyboard-shortcut-e--secondary-status-bar-indicator)
6. [Phase 6 — Golden Acceptance Corpora & Test Verification](#phase-6--golden-acceptance-corpora--test-verification)

---

## Phase 1 — Data Model & Mod Side Representation

### Objectives
Define the core data structures representing the expanded half of a mod row.

- In `diffuse-core` or `diffuse-tui`:
  - Define `ModSide`:
    ```rust
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub enum ModSide {
        Del,
        Add,
    }
    ```
- Extend `FlatRowKind` in `crates/diffuse-tui/src/types.rs`:
  ```rust
  pub enum FlatRowKind {
      FileHeader,
      HunkDivider { hunk_idx: usize },
      DiffRow { hunk_idx: usize, row_idx: usize },
      ExpandedMod { hunk_idx: usize, row_idx: usize, side: ModSide },
      BottomGapDivider { hunk_idx: usize },
  }
  ```
- Add helper methods on `FlatRow`:
  - `FlatRow::expanded_mod(file_idx: usize, hunk_idx: usize, row_idx: usize, side: ModSide) -> Self`
  - `FlatRow::is_mod(&self) -> bool`
  - `FlatRow::mod_side(&self) -> Option<ModSide>`

---

## Phase 2 — Gutter Icons (`┌-` / `└+`) & Segment Rendering Pipeline

### Objectives
Render `ExpandedMod` rows cleanly through the terminal renderer, matching `MovSrc` / `MovTgt` styling for change spans and using `┌-` / `└+` gutter icons.

- **`crates/diffuse-render/src/row/gutter.rs`**:
  - In `styled_prefix_spans`:
    - For `ModSide::Del`: render `old_line` with `┌-` icons in the gutter (`┌` styled with mod-icon orange `r#mod.icon.color`, `-` styled with `del.icon`).
    - For `ModSide::Add`: render `new_line` with `└+` icons in the gutter (`└` styled with mod-icon orange `r#mod.icon.color`, `+` styled with `add.icon`).
- **`crates/diffuse-render/src/row/segment.rs`**:
  - Update `row_segment_spans` to accept `Option<ModSide>`.
  - For `ModSide::Del`:
    - Filter runs to exclude `Add`-like runs (retaining `Ctx` and `Del`).
    - Use `old_text()` as display text.
    - Style `Del` spans with deletion background (`del_bg`), and row tint/cursor with `s_del`.
  - For `ModSide::Add`:
    - Filter runs to exclude `Del`-like runs (retaining `Ctx` and `Add`).
    - Use `new_text()` as display text.
    - Style `Add` spans with addition background (`word_bg`), and row tint/cursor with `s_add`.
- **`crates/diffuse-tui/src/app/render/viewport.rs`**:
  - Match on `FlatRowKind::ExpandedMod { hunk_idx, row_idx, side }` to construct prefix spans and content lines.

---

## Phase 3 — Flat List Construction & State Tracking (`expanded_mods`)

### Objectives
Track expanded mod rows in `AppState` and construct the flattened visual row sequence accordingly.

- In `AppState`:
  - Add `expanded_mods: HashSet<(usize, usize)>` representing `(file_idx, row_idx)` in `OutputFile::rows`.
  - Add helper methods:
    - `is_mod_expanded(file_idx: usize, row_idx: usize) -> bool`
    - `total_mod_rows(&self) -> usize`
- In `crates/diffuse-tui/src/app/flat.rs`:
  - In `build_flat`, when encountering a `RowKind::Mod` at `(fi, ri)`:
    - Check if expanded:
      - If collapsed (default): emit `FlatRow::diff_row(fi, hi, ri)`.
      - If expanded: emit `FlatRow::expanded_mod(fi, hi, ri, ModSide::Del)` followed by `FlatRow::expanded_mod(fi, hi, ri, ModSide::Add)`.
  - Ensure `HunkStart` and line source mappings correctly map both halves back to their logical hunk row.

---

## Phase 4 — Enter Key Toggle & Navigation Pivot Stability

### Objectives
Implement Enter-key toggle and cursor/viewport stability during expansion and collapse.

- In `crates/diffuse-tui/src/app/dispatch/command.rs`:
  - When Enter (`ToggleCollapseFile(None)`) is triggered:
    - If on `FlatRowKind::DiffRow` with `RowKind::Mod`: expand this mod row.
    - If on `FlatRowKind::ExpandedMod { .. }`: collapse this mod row.
- **Pivoting & Cursor Positioning**:
  - **Expanding (1 -> 2 rows)**:
    - Cursor lands on the **`del` row** (the first/upper row).
    - Viewport scroll adjustment keeps the `del` row pinned at the intended screen line.
  - **Collapsing (2 -> 1 row)**:
    - Whether cursor was on the `del` line or the `add` line, it collapses to the single `mod` row.
    - Viewport preserves the visual Y position of the mod row.

---

## Phase 5 — Global Keyboard Shortcut (`e`) & Secondary Status Bar Indicator

### Objectives
Provide a global shortcut to cycle mod line expansion across all files and secondary status bar feedback.

- Add command `TuiCommand::ToggleExpandAllMods`.
- Behavior:
  - If `expanded_mods.len() < total_mod_rows`: expand ALL mod rows in all files.
  - If `expanded_mods.len() == total_mod_rows`: collapse ALL mod rows in all files.
- Bind `e` in `event.rs`.
- In secondary status bar (`status_layout.rs`):
  - Add an indicator segment right after `raw diff`:
    - Logically mutually exclusive with `raw diff`: when `raw_diff` is active, mods are already fully expanded in the unified patch, so the `expanded mods` badge is suppressed.
    - When not in raw mode and any mod rows are expanded:
      - When all mod rows are expanded: `"expanded mods"`
      - When a subset of mod rows are expanded: e.g. `"N expanded mods"` (or `"1 expanded mod"`)

---

## Phase 6 — Golden Acceptance Corpora & Test Verification

### Objectives
Ensure 100% test coverage and verify no regressions in golden outputs.

- Target unit tests:
  - Enter toggling single mod row expanded -> collapsed -> expanded.
  - Cursor landing on `add` row on expansion; collapsing from both `del` and `add` rows.
  - Global `e` cycle toggle: 0 expanded -> all expanded -> 0 expanded.
  - Partial state: 1 expanded -> press `e` -> all expanded.
  - Opposing move jumps and hunk navigation skip or correctly traverse expanded rows.
  - Viewport scroll preservation during expansion and collapse.
- Verification gate: `./scripts/check.sh` passes with zero warnings.
