# Side-by-Side Diff — Engineering Plan

- [x] Phase 1 — Split Pane Geometry and Gutter Formatting (`diffuse-render`)
- [x] Phase 2 — Side-by-Side Row Segment and Span Pipeline (`diffuse-render`)
- [ ] Phase 3 — Dual-Pane Visual Line Height and Geometry Caching (`diffuse-tui`)
- [ ] Phase 4 — Viewport Grid Integration and Command Dispatch (`diffuse-tui`)
- [ ] Phase 5 — CLI Options, Status Bar, Help Modal and Pager Support (`diffuse`)
- [ ] Phase 6 — Integration Testing, Golden Invariants and Sign-Off

---

## 1. Executive Summary

`diffuse` currently renders code changes in a single unified flow: modification rows (`RowKind::Mod`) reconcile additions and deletions into a single inline visual line, with context expansion (`ExpandMod`) optionally splitting them vertically into stacked pre-image (`┌-`) and post-image (`└+`) rows. While compact, unified representations require visual decoding when comparing large structural edits, multi-token alterations, or parallel refactorings.

This plan introduces a **collapsible side-by-side (dual-pane) diff layout** across the rendering engine and interactive TUI reviewer. The diff content area is partitioned horizontally into two synchronized panes separated by a vertical border:
1. **Left Pane (Old / Pre-Image)**: Displays the pre-image state (`old_line`, `old_text()`), highlighting deleted and struck tokens with `del_bg`. Pure additions are represented as empty / padded lines.
2. **Center Divider**: A single-column vertical border (`│`) rendered in the theme border/canvas color.
3. **Right Pane (New / Post-Image)**: Displays the post-image state (`new_line`, `new_text()`), highlighting added and filled tokens with `add_bg`. Pure deletions are represented as empty / padded lines.

```mermaid
flowchart TD
    subgraph Unified ["Unified Inline Diff (Default)"]
        U1["[Gutter: old new ~] fn calculate(x: i32, y: i32) -> Result<i32, Error>"]
    end

    subgraph SideBySide ["Side-by-Side Diff (Split Dual-Pane)"]
        direction LR
        subgraph LeftPane ["Left Pane (Pre-Image)"]
            L1["[42 ~] fn calculate(x: i32) -> i32"]
        end
        Divider["│"]
        subgraph RightPane ["Right Pane (Post-Image)"]
            R1["[42 ~] fn calculate(x: i32, y: i32) -> Result<i32, Error>"]
        end
        LeftPane --- Divider --- RightPane
    end

    Unified -->|Toggle Split: \\ or \| | SideBySide
    SideBySide -->|Toggle Split: \\ or \| | Unified
```

### Architectural Principles and Mathematical Invariants

1. **Deterministic Line-Level Correspondence**: The pipeline in `diffuse-core` already pairs modified lines into `OutputRow`s. Side-by-side mode consumes the identical `OutputRow` stream:
   - For `RowKind::Ctx`: Identical line numbers and text on both panes.
   - For `RowKind::Mod`: Left pane displays `r.old_text()` with deletion runs; right pane displays `r.new_text()` with addition runs.
   - For `RowKind::Del`: Left pane displays `r.old_text()`; right pane is blank.
   - For `RowKind::Add`: Left pane is blank; right pane displays `r.new_text()`.
   - For `RowKind::Mov`: Relocation source lines appear on the left; relocation target lines appear on the right.
2. **Visual Line Height Synchronization**: When text in either pane wraps across multiple visual lines, both panes allocate matching visual rows:
   $$S_{\text{row}} = \max\left(S_{\text{left}}, S_{\text{right}}, 1\right)$$
   where $S_{\text{left}} = \lceil \text{width}(text_{\text{left}}) / \text{cols}_{\text{left}} \rceil$ and $S_{\text{right}} = \lceil \text{width}(text_{\text{right}}) / \text{cols}_{\text{right}} \rceil$. This guarantees that opposing lines in a modification row remain horizontally aligned across wrapped continuation rows.
3. **Full-Width Structural Headers and Callouts**: File headers, hunk dividers, and note callouts span the full terminal content width $cw$, preserving path visibility, count badges, and Markdown table legibility without horizontal compression.
4. **Wire Contract and Unified Baseline Immutability**: The public JSON wire contract ([`crates/diffuse-core/src/wire.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/wire.rs)) and unified golden acceptance baselines ([`tests/golden/`](file:///home/daniel/lab/diffuse/tests/golden/)) remain 100% byte-identical.

---

## 2. Table of Contents

1. [Phase 1 — Split Pane Geometry and Gutter Formatting (`diffuse-render`)](#1-phase-1--split-pane-geometry-and-gutter-formatting-diffuse-render)
2. [Phase 2 — Side-by-Side Row Segment and Span Pipeline (`diffuse-render`)](#2-phase-2--side-by-side-row-segment-and-span-pipeline-diffuse-render)
3. [Phase 3 — Dual-Pane Visual Line Height and Geometry Caching (`diffuse-tui`)](#3-phase-3--dual-pane-visual-line-height-and-geometry-caching-diffuse-tui)
4. [Phase 4 — Viewport Grid Integration and Command Dispatch (`diffuse-tui`)](#4-phase-4--viewport-grid-integration-and-command-dispatch-diffuse-tui)
5. [Phase 5 — CLI Options, Status Bar, Help Modal and Pager Support (`diffuse`)](#5-phase-5--cli-options-status-bar-help-modal-and-pager-support-diffuse)
6. [Phase 6 — Integration Testing, Golden Invariants and Sign-Off](#6-phase-6--integration-testing-golden-invariants-and-sign-off)

---

## 1. Phase 1 — Split Pane Geometry and Gutter Formatting (`diffuse-render`)

### Objective
Define horizontal pane partitioning primitives and dedicated left/right gutter prefix builders with zero heap allocation.

### Files
- [`crates/diffuse-render/src/row/gutter.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/row/gutter.rs)
- [`crates/diffuse-render/src/row/mod.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/row/mod.rs)
- [`crates/diffuse-render/src/row/tests.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/row/tests.rs)

### Implementation Details

1. **Pane Width Partitioning**:
   Define `split_pane_widths(content_width: usize) -> (usize, usize)`:
   - Given available content width $W$, reserve 1 column for the vertical divider:
     $$W_{\text{avail}} = W.saturating\_sub(1)$$
   - Left pane width: $W_{\text{left}} = W_{\text{avail}} / 2$.
   - Right pane width: $W_{\text{right}} = W_{\text{avail}} - W_{\text{left}}$.
   - Asserts $W_{\text{left}} > 0$ and $W_{\text{right}} > 0$ when $W \ge 3$.

2. **Gutter Widths**:
   - Left pane gutter width: `left_gutter_w = EXPAND_GUTTER_WIDTH (1) + old_line_w + 2`.
   - Right pane gutter width: `right_gutter_w = new_line_w + 2`.
   - Content column budgets:
     $$\text{cols}_{\text{left}} = W_{\text{left}}.saturating\_sub(\text{left\_gutter\_w}).\max(1)$$
     $$\text{cols}_{\text{right}} = W_{\text{right}}.saturating\_sub(\text{right\_gutter\_w}).\max(1)$$

3. **Left Pane Prefix Spans**:
   Define `styled_side_by_side_left_prefix_spans(...) -> Vec<StyledSpan>`:
   - For `RowKind::Ctx`: renders expand icon + `{old_pad}  `.
   - For `RowKind::Mod`: renders expand icon + `{old_pad} ~`.
   - For `RowKind::Del` / `MovSide::Src`: renders expand icon + `{old_pad} -`.
   - For `RowKind::Add` / `MovSide::Tgt`: renders blank spaces `static_spaces(left_gutter_w)`.
   - Continuation segments ($s > 0$): renders expand icon stem (`│` or space) + `static_spaces(old_line_w + 2)`.

4. **Right Pane Prefix Spans**:
   Define `styled_side_by_side_right_prefix_spans(...) -> Vec<StyledSpan>`:
   - For `RowKind::Ctx`: renders `{new_pad}  `.
   - For `RowKind::Mod`: renders `{new_pad} ~`.
   - For `RowKind::Add` / `MovSide::Tgt`: renders `{new_pad} +`.
   - For `RowKind::Del` / `MovSide::Src`: renders blank spaces `static_spaces(right_gutter_w)`.
   - Continuation segments ($s > 0$): renders `static_spaces(new_line_w + 2)`.

### Verification Gate
- Unit tests in `crates/diffuse-render/src/row/tests.rs` verifying:
  - Exact column width assertions for left and right prefixes across `Ctx`, `Mod`, `Add`, and `Del` rows.
  - Zero-allocation static space slices for blank opposing panes.
  - Total width parity: `prefix_len(left) + cols_left + 1 + prefix_len(right) + cols_right == content_width`.

---

## 2. Phase 2 — Side-by-Side Row Segment and Span Pipeline (`diffuse-render`)

### Objective
Implement the row segment builder that composes the left pane, vertical divider, and right pane into a unified `RowSegmentLine`.

### Files
- [`crates/diffuse-render/src/row/segment.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/row/segment.rs)
- [`crates/diffuse-render/src/row/mod.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/row/mod.rs)
- [`crates/diffuse-render/src/row/tests.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/row/tests.rs)

### Implementation Details

1. **Pane Content Builder**:
   Define helper `build_pane_segment_spans(...)`:
   - Arguments: `text: &str`, `kind: RowKind`, `mod_side: Option<ModSide>`, `runs: &[OutputRun]`, `seg_idx: usize`, `cols: usize`, `prefix: Vec<StyledSpan>`, `selected: bool`, `band: bool`, `theme: &DiffuseTheme`.
   - Slices text at `[seg_idx * cols .. (seg_idx + 1) * cols]`.
   - Styles syntax tokens and diff backgrounds using `row_segment_spans`.
   - Right-pads with spaces to guarantee exact pane column width.

2. **Side-by-Side Composite Builder**:
   Define `side_by_side_row_segment_spans(...) -> RowSegmentLine`:
   - Computes left pane spans using `ModSide::Del` for `RowKind::Mod`, or empty fill for `RowKind::Add`.
   - Emits 1-column vertical border: `StyledSpan::styled("│", Some(divider_color), line_bg)`.
   - Computes right pane spans using `ModSide::Add` for `RowKind::Mod`, or empty fill for `RowKind::Del`.
   - Concatenates:
     $$\text{spans} = \text{spans}_{\text{left}} \mathbin{\Vert} [\text{divider}] \mathbin{\Vert} \text{spans}_{\text{right}}$$
   - Returns composite `RowSegmentLine` with resolved background.

3. **ANSI CLI Serializer**:
   Define `write_side_by_side_row_segment<W: Write>(...) -> fmt::Result`:
   - Serializes composite styled spans to terminal ANSI escapes for non-interactive pager output (`diffuse -p --side-by-side`).

### Verification Gate
- Unit tests in `crates/diffuse-render/src/row/tests.rs` validating:
  - Struck text rendered only on the left pane of a `Mod` row; filled text rendered only on the right pane.
  - Context lines rendered with identical text on both panes.
  - Blank pane padding matching column budgets without drift.
  - Total line display width exactly equals terminal content width.

---

## 3. Phase 3 — Dual-Pane Visual Line Height and Geometry Caching (`diffuse-tui`)

### Objective
Update visual row height calculations and the geometry cache to support side-by-side wrapping without layout anomalies.

### Files
- [`crates/diffuse-tui/src/app/geometry/vis.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/app/geometry/vis.rs)
- [`crates/diffuse-tui/src/app/geometry/tests.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/app/geometry/tests.rs)

### Implementation Details

1. **CachedGeometry Key Extension**:
   - Add `pub(crate) side_by_side: bool` to `CachedGeometry`.
   - In `AppState::cached_geometry`, check `g.side_by_side == self.side_by_side`.
   - In `AppState::bump_content()`, invalidate cached geometry when toggling view mode.

2. **Dual-Pane Segment Height Calculation**:
   Define `row_seg_count_side_by_side(&self, idx: usize, content_width: usize, left_cols: usize, right_cols: usize) -> usize`:
   - For `FlatRowKind::FileHeader` and `FlatRowKind::HunkDivider`: calculates full-width wrapped segment count against `content_width`.
   - For `FlatRowKind::DiffRow`:
     - If `RowKind::Ctx`:
       $$\text{segs} = \max\left(\lceil \text{display\_width}(text) / \text{left\_cols} \rceil, \lceil \text{display\_width}(text) / \text{right\_cols} \rceil, 1\right)$$
     - If `RowKind::Mod`:
       $$\text{segs}_{\text{left}} = \lceil \text{display\_width}(old\_text) / \text{left\_cols} \rceil$$
       $$\text{segs}_{\text{right}} = \lceil \text{display\_width}(new\_text) / \text{right\_cols} \rceil$$
       $$\text{segs} = \max\left(\text{segs}_{\text{left}}, \text{segs}_{\text{right}}, 1\right)$$
     - If `RowKind::Del`: $\text{segs} = \lceil \text{display\_width}(old\_text) / \text{left\_cols} \rceil$.
     - If `RowKind::Add`: $\text{segs} = \lceil \text{display\_width}(new\_text) / \text{right\_cols} \rceil$.
     - If `RowKind::Mov`: evaluates active side.

3. **Layout Pipeline Update**:
   - In `AppState::compute_geometry`:
     - If `self.side_by_side`: precomputes `left_cols` and `right_cols` once before the loop and invokes `row_seg_count_side_by_side`.
     - Else: invokes `row_seg_count_with_gutter`.

### Verification Gate
- Unit tests in `crates/diffuse-tui/src/app/geometry/tests.rs` asserting:
  - Asymmetric wrapping: left text 120 chars in 40 cols (3 segs) + right text 20 chars in 40 cols (1 seg) produces `segs = 3`.
  - Prefix sum monotone continuity: `starts[idx + 1] == starts[idx] + counts[idx]`.
  - Binary search viewport navigation lands on identical logical row across layout modes.

---

## 4. Phase 4 — Viewport Grid Integration and Command Dispatch (`diffuse-tui`)

### Objective
Integrate dual-pane rendering into the TUI frame renderer, bind keyboard shortcuts (`\` and `|`), and ensure cursor navigation and mouse clicks map accurately.

### Files
- [`crates/diffuse-tui/src/app/mod.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/app/mod.rs)
- [`crates/diffuse-tui/src/app/init.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/app/init.rs)
- [`crates/diffuse-tui/src/command.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/command.rs)
- [`crates/diffuse-tui/src/app/dispatch/command.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/app/dispatch/command.rs)
- [`crates/diffuse-tui/src/event.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/event.rs)
- [`crates/diffuse-tui/src/app/render/viewport.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/app/render/viewport.rs)

### Implementation Details

1. **State Tracking**:
   - Add `pub side_by_side: bool` to `AppState` (default: `false`).

2. **Command & Event Dispatch**:
   - Add `TuiCommand::ToggleSideBySide` to `command.rs`.
   - In `dispatch/command.rs`:
     ```rust
     TuiCommand::ToggleSideBySide => {
         self.side_by_side = !self.side_by_side;
         self.bump_content();
     }
     ```
   - In `event.rs`:
     - Bind `(_, KeyCode::Char('\\'))` and `(_, KeyCode::Char('|'))` to `TuiCommand::ToggleSideBySide`.

3. **Viewport Frame Rendering**:
   - In `render_diff_viewport_lines`:
     - If `self.side_by_side`:
       - Precomputes `(left_w, right_w) = split_pane_widths(content_width)`.
       - For each diff row segment: calls `side_by_side_row_segment_spans(...)`.
       - Renders file headers, hunk dividers, and note callouts full-width across `content_width`.
     - Else: retains standard unified rendering path.

4. **Mouse Coordinate Mapping**:
   - In `viewport_row_to_visual` and mouse click handling:
     - Clicking in `col < left_w` targets the left (pre-image) pane.
     - Clicking in `col > left_w` targets the right (post-image) pane.
     - For note creation (`m`), resolves `DiffCoord` to the clicked side.

### Verification Gate
- TUI integration tests driving:
  - Toggling `\` switches buffer between unified and side-by-side layouts.
  - Verifying presence of vertical divider `│` at center column.
  - Mouse click on left pane vs right pane associates note with respective line number.

---

## 5. Phase 5 — CLI Options, Status Bar, Help Modal and Pager Support (`diffuse`)

### Objective
Expose `--side-by-side` across the CLI binary, add status bar indicators and help dialog entries, and update shell completion scripts.

### Files
- [`crates/diffuse/src/cli/args.rs`](file:///home/daniel/lab/diffuse/crates/diffuse/src/cli/args.rs)
- [`crates/diffuse/src/cli/dispatch.rs`](file:///home/daniel/lab/diffuse/crates/diffuse/src/cli/dispatch.rs)
- [`crates/diffuse-tui/src/view/status.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/view/status.rs)
- [`crates/diffuse-tui/src/view/modals/help.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/view/modals/help.rs)

### Implementation Details

1. **CLI Flag**:
   - In `crates/diffuse/src/cli/args.rs`:
     ```rust
     /// Display diff in side-by-side (split dual-pane) layout.
     #[arg(
         long = "side-by-side",
         alias = "split",
         action = clap::ArgAction::SetTrue,
         overrides_with = "unified",
         global = true,
         help_heading = "Display Options",
         help = "Display diff in side-by-side (split dual-pane) layout"
     )]
     pub side_by_side: bool,

     /// Display diff in unified inline layout (default).
     #[arg(
         long = "unified",
         action = clap::ArgAction::SetTrue,
         overrides_with = "side_by_side",
         global = true,
         help_heading = "Display Options",
         help = "Display diff in unified inline layout (default)"
     )]
     pub unified: bool,
     ```
   - In `cli::dispatch::dispatch`: initializes `state.side_by_side = args.side_by_side`.

2. **Status Bar Indicators**:
   - Secondary status bar: displays `[split]` badge when `side_by_side` is active.
   - Bottom key hint bar: includes `[|]split`.

3. **Help Modal Reference**:
   - In `crates/diffuse-tui/src/view/modals/help.rs`:
     - Add `(r"\ · |", "toggle side-by-side diff")` to `HELP_ENTRIES`.

4. **Pager Mode Serialization**:
   - When running `diffuse -p --side-by-side`: formats terminal diff output with dual-pane layout using `write_side_by_side_row_segment`.

### Verification Gate
- CLI argument tests in `crates/diffuse/tests/cli.rs`:
  - `diffuse --side-by-side` initializes TUI with `side_by_side = true`.
  - `diffuse -p --side-by-side` emits dual-pane diff to stdout.
- Help modal unit test asserting `HELP_ENTRIES` contains `\ · |`.

---

## 6. Phase 6 — Integration Testing, Golden Invariants and Sign-Off

### Objective
Enforce strict regression prevention across existing unified outputs, author end-to-end integration tests, and obtain human visual sign-off.

### Files
- [`crates/diffuse-tui/tests/app_integration.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/tests/app_integration.rs)
- [`crates/diffuse/tests/golden.rs`](file:///home/daniel/lab/diffuse/crates/diffuse/tests/golden.rs)
- [`TODO.md`](file:///home/daniel/lab/diffuse/TODO.md)

### Implementation Details

1. **End-to-End Integration Tests**:
   - Author `test_side_by_side_diff_layout_and_toggle` in `app_integration.rs`:
     - Loads a multi-file golden changeset.
     - Renders at `120x36` in default unified mode and verifies baseline.
     - Executes `TuiCommand::ToggleSideBySide` and renders buffer.
     - Asserts center column divider `│` across code rows.
     - Asserts left pane contains pre-image line numbers and right pane contains post-image numbers.
     - Verifies wrapped row segment heights maintain opposing line alignment.
     - Toggles back to unified mode and asserts exact byte-equality with baseline buffer.

2. **Regression Testing & Golden Immutability**:
   - Verify that all 23 frozen golden corpora (`.wire.jsonl`, `.pager.render.*.txt`, `.tui.*.render.*.txt`, `.markdown.md`) remain **100% byte-for-byte unchanged** when running without `--side-by-side`.

3. **Verification Gate**:
   - Run `./scripts/check.sh` ensuring nightly `cargo fmt`, `clippy --fix` (0 warnings), and all workspace tests pass cleanly within the performance budget (< 3s warm).
   - Update `TODO.md` to check off `- [x] Collapsible side-by-side diff`.

4. **Review & Human Sign-Off**:
   - Present working tree diff and change summary.
   - Pause for human review and interactive visual sign-off in Pi / terminal before committing.
