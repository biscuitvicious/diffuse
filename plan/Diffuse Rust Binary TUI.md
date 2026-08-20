# Diffuse Rust Binary TUI — CLI Line Rendering & Native Interactive Diff Reviewer

- [ ] **Phase 1 — Native Colored CLI Line Rendering (`diffuse` CLI & Wire Pre-rendering)**
  - [ ] Step 1.1: Terminal Theme & ANSI Stylizer module in Rust (`src/render/theme.rs`)
  - [ ] Step 1.2: Row & Segment Renderer with subword styling and column slicing (`src/render/row.rs`)
  - [ ] Step 1.3: Non-interactive CLI Pretty-Printer mode (`diffuse [git-args...]` without TUI)
  - [ ] Step 1.4: Pre-rendered Visual Rows on Wire Contract & Extension Consumption
- [ ] **Phase 2 — Native Interactive TUI Engine (`src/tui/`)**
  - [ ] Step 2.1: Terminal Raw Mode, Event Loop, Crossterm/Terminal Abstraction (`src/tui/terminal.rs`)
  - [ ] Step 2.2: TUI Layout, Visual Frame Geometry & Note Markers (`src/tui/frame.rs`)
  - [ ] Step 2.3: Keyboard & Mouse Navigation (`src/tui/input.rs`)
  - [ ] Step 2.4: Selection, Note Editing & Inline Prompts (`src/tui/notes.rs`)
  - [ ] Step 2.5: Interactive Git Actions (`stage`, `unstage`, `discard`, `open in editor`) (`src/tui/actions.rs`)
  - [ ] Step 2.6: Extension Thin-Client Migration & Native `/review` Invocation Gate

---

## Executive Summary

Currently, terminal rendering and interactive navigation are split across two layers:
1. **The Rust engine (`src/`)**: Parses diffs, resolves movement/anchors, solves 2D DP clusters, generates word-level token streams (`OutputRun`), and persists notes.
2. **The Pi extension (`extension/pi/src/`)**: Consumes token streams over JSON, recalculates terminal column wrapping/slicing, executes ANSI/SGR color styling via `THEME_MAGIC`, formats file headers/footers, and runs the interactive TUI event loop (`ReviewComponent`).

This plan moves rendering and interactivity down into the canonical Rust binary:
- **Phase 1**: Implement pure Rust terminal line rendering and ANSI stylization. When `diffuse` is run non-interactively (e.g. piped or invoked directly with diff arguments), it produces a styled visual diff output (equivalent to a syntax-highlighted, word-diff-aware `git diff`). Additionally, visual row lines can be pre-rendered or streamed directly to renderers.
- **Phase 2**: Implement the complete, zero-dependency interactive TUI inside `diffuse` (`crossterm` / raw terminal events), replicating all Pi extension reviewer capabilities: `j/k` navigation, `v` visual selection, `m` note management, `o` external editor jumps, `s`/`x` hunk staging/discarding, and mouse scroll/drag support.

---

## Table of Contents

1. [Phase 1 — Native Colored CLI Line Rendering](#1-phase-1--native-colored-cli-line-rendering)
   - [Step 1.1: Terminal Theme & ANSI Stylizer Module](#step-11-terminal-theme--ansi-stylizer-module)
   - [Step 1.2: Row & Segment Renderer (`src/render/row.rs`)](#step-12-row--segment-renderer-srcrenderrowrs)
   - [Step 1.3: Non-interactive CLI Pretty-Printer Mode](#step-13-non-interactive-cli-pretty-printer-mode)
   - [Step 1.4: Pre-rendered Visual Rows on Wire Contract & Extension Consumption](#step-14-pre-rendered-visual-rows-on-wire-contract--extension-consumption)
2. [Phase 2 — Native Interactive TUI Engine](#2-phase-2--native-interactive-tui-engine)
   - [Step 2.1: Terminal Raw Mode, Event Loop & Backend (`src/tui/terminal.rs`)](#step-21-terminal-raw-mode-event-loop--backend-srctuiterminalrs)
   - [Step 2.2: TUI Layout, Visual Frame Geometry & Note Markers (`src/tui/frame.rs`)](#step-22-tui-layout-visual-frame-geometry--note-markers-srctuiframers)
   - [Step 2.3: Keyboard & Mouse Navigation (`src/tui/input.rs`)](#step-23-keyboard--mouse-navigation-srctuiinputrs)
   - [Step 2.4: Selection, Note Editing & Inline Prompts (`src/tui/notes.rs`)](#step-24-selection-note-editing--inline-prompts-srctuinotesrs)
   - [Step 2.5: Interactive Git Actions (`src/tui/actions.rs`)](#step-25-interactive-git-actions-srctuiactionsrs)
   - [Step 2.6: Extension Thin-Client Migration & Acceptance Walk](#step-26-extension-thin-client-migration--acceptance-walk)

---

## 1. Phase 1 — Native Colored CLI Line Rendering

### Step 1.1: Terminal Theme & ANSI Stylizer Module
- **Affected Files**: `src/render/mod.rs`, `src/render/theme.rs`
- **Specification**:
  - Port `ROW_STYLES` (`add` (tint 22, cursor 28, fill 28, fillCursor 34), `del` (tint 52, cursor 88), `mod` (tint 236, cursor 237), `mov` (tint 236, cursor 237)).
  - Port the `THEME_MAGIC` 256-color palette (`syntaxKeyword` = 147/183, `syntaxType`/`syntaxNumber` = 221/222, `syntaxFunction` = 123, `syntaxOperator` = 117, `syntaxComment` = 150/243, `syntaxString` = 121, `accent`/`mdDelim` = 51, `mdBold` = 172, `mdItalic`/`muted` = 229).
  - Port ANSI SGR code application and SGR off reset mappings (`bold` = 1/22, `italic` = 3/23, `underline` = 4/24, `strike` = 9/29).
- **Verification**:
  - Unit tests in `src/render/theme/tests.rs` asserting exact ANSI escape sequence outputs for each token category and row kind.

### Step 1.2: Row & Segment Renderer (`src/render/row.rs`)
- **Affected Files**: `src/render/row.rs`
- **Specification**:
  - Implement `display_text(row, col_offset, word_diff)` and `render_row_segment(row, seg_start, seg_end, prefix, selected, col_offset, word_diff, theme)`.
  - Handle prefix formatting: `+` (added), `-` (deleted), `~` (modified), `≈` (moved), ` ` (context), with coordinate padding (`pad4`).
  - Correctly blend background tints, word-diff fill backgrounds, and strikethroughs.
  - Implement visual width measurement (`unicode-width` crate or ASCII fast path).
- **Verification**:
  - Unit tests validating rendered ANSI lines match golden TS component output across addition, deletion, modification, and moved rows.

### Step 1.3: Non-interactive CLI Pretty-Printer Mode
- **Affected Files**: `src/main.rs`, `src/cli.rs`
- **Specification**:
  - When invoked from the terminal as `diffuse` without JSON piping (or when passed git diff arguments / `--no-tui` / non-TTY stdout), read git diff for the repository, run the pipeline, render styled diff lines to stdout.
  - Render file summary bars: `path (status) · +adds -dels ~mods ≈movs`.
  - Output colored hunks with coordinate gutters, subword highlights, and syntax colors.
- **Verification**:
  - Direct CLI invocation tests against sample diffs and git repositories.

### Step 1.4: Pre-rendered Visual Rows on Wire Contract & Extension Consumption
- **Affected Files**: `src/bin_pipeline.rs`, `extension/pi/src/backend.ts`, `extension/pi/src/component_render.ts`
- **Specification**:
  - Provide an optional flag / field in pipeline output for pre-rendered visual ANSI rows (`rendered_line`).
  - Extension updates to render pre-formatted lines directly when available rather than re-tokenizing.
- **Verification**:
  - `npm test` and `tests/golden.rs` validation.

---

## 2. Phase 2 — Native Interactive TUI Engine

### Step 2.1: Terminal Raw Mode, Event Loop & Backend (`src/tui/terminal.rs`)
- **Affected Files**: `Cargo.toml`, `src/tui/mod.rs`, `src/tui/terminal.rs`
- **Specification**:
  - Add `crossterm` dependency (for cross-platform raw mode, alternate screen buffer, mouse capture, and key event stream).
  - Implement `TerminalSession` RAII guard ensuring alternate screen and mouse reporting are restored on exit/panic.
  - Implement event polling loop with frame rate throttling and resize handling.
- **Verification**:
  - Unit tests and headless terminal mocking.

### Step 2.2: TUI Layout, Visual Frame Geometry & Note Markers (`src/tui/frame.rs`)
- **Affected Files**: `src/tui/frame.rs`
- **Specification**:
  - Port `renderComponentFrame` layout calculations:
    - Top header bar: `Review: <repo> · <path> (<status>) · +A -D ~M ≈Mov · N notes`.
    - Secondary status line: file index, hunk index, base ref, branch, context depth, column offset.
    - Viewport scrolling with sticky cursor visibility constraint (`maxRows = terminal_height - 5`).
    - Note callout overlays: `💬 [start-end] <rendered markdown runs>`.
    - Bottom command ribbon: `[j/k]move [v]select [m]note [o]edit [s]stage [x]discard [q]quit [?]help`.
- **Verification**:
  - Snapshot test verifying frame render buffers match exact column and row dimensions.

### Step 2.3: Keyboard & Mouse Navigation (`src/tui/input.rs`)
- **Affected Files**: `src/tui/input.rs`
- **Specification**:
  - Implement key bindings:
    - `j` / `k` / `Down` / `Up`: line navigation with wrapped line awareness (`vis_line`).
    - `J` / `K` / `}` / `{`: next/previous hunk jumps.
    - `n` / `p` / `]` / `[`: next/previous file jumps.
    - `g` / `G`: jump to top/bottom, numeric line goto buffer (`123g`).
    - `h` / `l` / `Left` / `Right`: column scrolling offset (`colOffset`).
    - `w`: toggle word diff; `y`: toggle syntax highlighting; `?`: toggle help modal.
    - `q` / `Esc` / `Ctrl+C`: quit.
  - Implement mouse events: wheel scroll, click-to-position, click-and-drag line range selection.
- **Verification**:
  - Input state machine unit tests simulating key and mouse sequences.

### Step 2.4: Selection, Note Editing & Inline Prompts (`src/tui/notes.rs`)
- **Affected Files**: `src/tui/notes.rs`, `src/notes/mod.rs`
- **Specification**:
  - Visual mode selection (`v`): anchor-based selection band across rows within the active file.
  - Note creation prompt (`m`): inline multi-line draft buffer at cursor position with `Ctrl+Enter` save and `Esc` cancel.
  - Note persistence: save/load via `NotesDB` (`~/.pi/review/notes.json` / custom path).
  - Note deletion (`d` / `M`): delete note on current line or active selection.
- **Verification**:
  - Integration tests verifying note lifecycle (add, query, render, delete) in interactive state.

### Step 2.5: Interactive Git Actions (`src/tui/actions.rs`)
- **Affected Files**: `src/tui/actions.rs`, `src/git/mod.rs`
- **Specification**:
  - `s` / `S`: Stage current hunk / whole file (`git apply --cached` / `git add`).
  - `u` / `U`: Unstage current hunk / whole file (`git apply --cached -R` / `git reset`).
  - `x` / `X`: Discard current hunk / whole file with confirmation prompt and safety stash checkpoint (`git stash create`).
  - `o`: Open file in external editor (`$VISUAL` / `$EDITOR` / `code` / `cursor` / `vim`) suspended from alternate screen, then reload file content upon return.
- **Verification**:
  - Automated integration tests running staging, unstaging, discard, and refresh on temporary git test repos.

### Step 2.6: Extension Thin-Client Migration & Acceptance Walk
- **Affected Files**: `extension/pi/src/backend.ts`, `extension/pi/review.ts`, `tests/golden.rs`
- **Specification**:
  - When running `/review` in Pi, the extension can directly spawn the interactive `diffuse` binary inside the terminal session or consume pre-rendered visual frames.
  - Ensure the Rust binary standalone experience and Pi extension experience are 100% feature-identical and share the exact same styling, keybindings, and note database.
- **Verification**:
  - `./scripts/check.sh`, `npm test`, golden test suite.
