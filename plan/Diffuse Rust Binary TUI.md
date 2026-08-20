# Diffuse Rust Binary TUI — CLI Line Rendering & Native Interactive Diff Reviewer

- [x] **Phase 1 — Native Colored CLI Line Rendering (`diffuse` CLI & Wire Pre-rendering)**
  - [x] Step 1.1: Terminal Theme & ANSI Stylizer module in Rust (`src/render/theme.rs`)
  - [x] Step 1.2: Row & Segment Renderer with subword styling and column slicing (`src/render/row.rs`)
  - [x] Step 1.3: Non-interactive CLI Pretty-Printer mode (`diffuse [git-args...]` without TUI)
  - [x] Step 1.4: Render acceptance via static render golden walk (`tests/golden.rs` `terminal_render_golden_walk`); wire pre-rendered rows superseded by the native TUI (Step 2.6)
- [·] **Phase 2 — Native Interactive TUI Engine (`src/tui/`)**
  - [x] Step 2.1: Terminal Raw Mode, Event Loop, Crossterm/Terminal Abstraction (`src/tui/terminal.rs`)
  - [x] Step 2.2: TUI Layout & Visual Frame Geometry (`src/tui/app.rs` — header, status, viewport, footer ribbon, help modal)
  - [·] Step 2.3: Keyboard & Mouse Navigation (`src/tui/event.rs`) — core keys done; mouse partial (wheel only), `NG` goto, Tab file switch, `^`/`$`, sep expansion pending
  - [·] Step 2.4: Selection, Note Editing & Inline Prompts (`src/tui/`) — draft input done; note persistence (NotesDB), visual selection (`v`/`V`), note delete (`d`/`D`), callout rendering pending
  - [ ] Step 2.5: Interactive Git Actions (`s`/`S` stage, `u`/`U` unstage, `x`/`X` discard, `o` editor) (`src/tui/`, `src/git.rs`)
  - [ ] Step 2.6: Extension Thin-Client Migration & Native `/review` Invocation Gate

---

## Executive Summary

Terminal rendering and interactive navigation have moved **down into the canonical Rust binary**. The Rust engine (`src/`) owns: diff parsing, cluster DP, token streams, and now the terminal renderer (`src/render/`) and the interactive TUI (`src/tui/`).

The end-state architecture (the goal of Phase 2):

- `diffuse [git-args...]` run in a terminal opens the **native crossterm TUI**: raw mode, alternate screen, mouse capture, full navigation, notes, and git actions — the single canonical reviewer.
- The pi extension becomes a **thin client**: `/review` resolves the git repo + args, **hands the terminal to the diffuse binary** (`ctx.tui.stop()` → spawn `bin/diffuse` with `stdio: "inherit"`, cwd=repoRoot → on exit `ctx.tui.start()`), and reports the summary. This is exactly the proven `openEditorAction` flow (stop TUI → run subprocess → restart TUI). All rendering, keybindings, note persistence, and git actions live in the binary — there is exactly **one** renderer, one event loop, one note database, no TS re-implementation to keep in sync.
- Acceptance gate (Step 2.6): the binary standalone experience and the pi `/review` experience are the same process; `./scripts/check.sh` + `npm test` prove the wire and render contracts; the TS renderer/component/golden-walk are deleted.

Phase 1 shipped the renderer (row styles via the single `S_ADD/S_DEL/S_MOD/S_MOV` table, grapheme-cluster slicing via `unicode-segmentation`, byte-identical render goldens). Phase 2 builds the TUI to feature parity with the old TS `ReviewComponent`, then migrates the extension.

---

## Table of Contents

1. [Phase 1 — Native Colored CLI Line Rendering](#1-phase-1--native-colored-cli-line-rendering)
   - [Step 1.1: Terminal Theme & ANSI Stylizer Module](#step-11-terminal-theme--ansi-stylizer-module)
   - [Step 1.2: Row & Segment Renderer (`src/render/row.rs`)](#step-12-row--segment-renderer-srcrenderrowrs)
   - [Step 1.3: Non-interactive CLI Pretty-Printer Mode](#step-13-non-interactive-cli-pretty-printer-mode)
   - [Step 1.4: Render Acceptance Walk](#step-14-render-acceptance-walk)
2. [Phase 2 — Native Interactive TUI Engine](#2-phase-2--native-interactive-tui-engine)
   - [Step 2.1: Terminal Raw Mode, Event Loop & Backend (`src/tui/terminal.rs`)](#step-21-terminal-raw-mode-event-loop--backend-srctuiterminalrs)
   - [Step 2.2: TUI Layout & Visual Frame Geometry (`src/tui/app.rs`)](#step-22-tui-layout--visual-frame-geometry-srctuiapprs)
   - [Step 2.3: Keyboard & Mouse Navigation (`src/tui/event.rs`)](#step-23-keyboard--mouse-navigation-srctuieventrs)
   - [Step 2.4: Selection, Note Editing & Inline Prompts](#step-24-selection-note-editing--inline-prompts)
   - [Step 2.5: Interactive Git Actions (`src/tui/`, `src/git.rs`)](#step-25-interactive-git-actions-srctui-srcgitrs)
   - [Step 2.6: Extension Thin-Client Migration & Native `/review` Invocation Gate](#step-26-extension-thin-client-migration--native-review-invocation-gate)

---

## 1. Phase 1 — Native Colored CLI Line Rendering

### Step 1.1: Terminal Theme & ANSI Stylizer Module
- **Affected Files**: `src/render/mod.rs`, `src/render/theme.rs`
- **Status**: done. `DiffuseTheme` (`MAGIC` 256-color palette), `decode_sgr`/`encode_sgr` escape-code encoding, `render_token`, the `S_ADD/S_DEL/S_MOD/S_MOV` row-style table (tint/cursor/fill/fillCursor/icon), and the `fallback_named_color` resolver.
- **Verification**: unit tests (exact ANSI sequences), `test_encode_decode_sgr`.

### Step 1.2: Row & Segment Renderer (`src/render/row.rs`)
- **Affected Files**: `src/render/row.rs`
- **Status**: done. `display_text`, `render_row_segment` (prefix gutter with pad4 + icons, background tint/blend via the row-style table, word-fill, strikethrough, col_offset slicing, syntax_on threading, grapheme-cluster slicing — clusters are never split).
- **Verification**: unit tests incl. `test_col_offset_applies_to_token_runs`, `test_syntax_on_toggle_disables_token_colors`, `test_grapheme_clusters_never_split`; render goldens stay byte-identical.

### Step 1.3: Non-interactive CLI Pretty-Printer Mode
- **Affected Files**: `src/main.rs`, `src/git.rs`
- **Status**: done. `diffuse [git-args...]` in a non-TTY (or `--no-tui`/`--json`) emits the styled diff (or the token-stream JSON). `load_git_repo` errors on non-repo dirs (exit 2) instead of blocking on stdin.
- **Verification**: `test_load_git_repo_non_repo_errors`, manual CLI runs.

### Step 1.4: Render Acceptance Walk
- **Affected Files**: `tests/golden.rs`
- **Status**: done. `terminal_render_golden_walk` asserts the renderer byte-for-byte over the 7 frozen corpora (`.render.txt`). Regeneration is surgical via `UPDATE_RENDER=1` per `tests/AGENTS.md`. With the Step 2.6 migration the rendered golden becomes the **only** render gate (the TS walk is deleted).

---

## 2. Phase 2 — Native Interactive TUI Engine

### Step 2.1: Terminal Raw Mode, Event Loop & Backend (`src/tui/terminal.rs`)
- **Affected Files**: `Cargo.toml` (crossterm 0.29), `src/tui/mod.rs`, `src/tui/terminal.rs`
- **Status**: done. `TerminalGuard` RAII (alternate screen, raw mode, mouse capture, cursor hide; restored on drop/panic), `suspend`/`resume` (ready for the Step 2.5 editor flow), `poll_event` (50 ms throttle), `write_frame`.
- **Verification**: full TUI runs; `run_interactive_tui` loop in `src/tui/mod.rs`.

### Step 2.2: TUI Layout & Visual Frame Geometry (`src/tui/app.rs`)
- **Affected Files**: `src/tui/app.rs`
- **Status**: done (callouts pending under 2.4). Header bar (`Review: <repo> · <path> (<status>) · +A −D ~M ≈Mov · N notes`), secondary status line (file/hunk/go/col/word/syntax), flat-row viewport with per-row styles + selection lift, footer rule + key ribbon, `?` help modal overlay (Esc closes), note-draft inline editor line.
- **Verification**: `test_app_state_scroll_through_all_rows`, `test_help_modal_toggles_and_renders`.

### Step 2.3: Keyboard & Mouse Navigation (`src/tui/event.rs`)
- **Affected Files**: `src/tui/event.rs`
- **Status**: in progress. Done: `j/k/↓/↑` move, `J/K/}/{` hunk, `n/p/]/[` file, `g/G/Home/End` top/bottom, page up/down (PgDn/PgUp/Ctrl+d/f/u/b/Space), `h/l/←/→` col scroll ±4, `w`/`y` toggles, `?` help, `m` note draft, `q`/Ctrl+c/Esc quit, mouse wheel ±3.
- **Pending**: `NG` numeric goto (num_buffer exists), Tab/Shift+Tab file switch, `^`/`$` col-scroll ends, `Enter` expands a `sep` (hidden-gap) row, mouse click-to-position + click-drag.
- **Verification**: `#[test]` key-sequence state tests (`tui::event`).

### Step 2.4: Selection, Note Editing & Inline Prompts
- **Affected Files**: `src/tui/app.rs`, `src/tui/event.rs`, `src/notes/mod.rs`, `src/main.rs`
- **Status**: in progress. Draft input done (`m`, Enter/Ctrl+Enter commit, Esc cancel). Pending sub-steps:
  1. **Note persistence**: `AppState` gains the `NotesDB` + notes path + repo; commit actually `add_note` + `save_notes`; delete (`d`/`D`) `remove_notes` + save; `run_interactive_tui` passes the notes path. Notes render as `💬 [start-end] <markdown runs>` callouts (engine `note_runs`/`render_note_markdown`).
  2. **Visual selection** (`v`/`V`): `anchor`/`confirmed_sel` wired; selection band render (prefix + line lift); selection feeds note commit (`m` on a range), delete (`d`), and editor (`o`).
- **Verification**: note lifecycle tests (add → saved → reloaded → delete), selection state-machine tests, render tests for callouts and the selection band.

### Step 2.5: Interactive Git Actions (`src/tui/`, `src/git.rs`)
- **Affected Files**: `src/tui/event.rs`, `src/tui/app.rs`, `src/git.rs`, `src/main.rs`
- **Status**: not started. Sub-steps:
  1. **Reload**: after any git op, re-run `load_git_repo` + pipeline and swap `AppState.output`/`flat` (keep cursor anchored by line number).
  2. **Stage/unstage** (`s`/`S`/`u`/`U`): hunk patch (`git diff -M --no-color -U<ctx> <base> -- <path>`, sliced at `@@`), whole file (`git add`/`git reset`); guards mirror the TS (stage needs default worktree review; unstage needs `--cached`).
  3. **Discard** (`x`/`X`): safety stash (`git stash create review-checkpoint`) then `git checkout --` / reverse apply; confirmation prompt before discard.
  4. **Open editor** (`o`): `TerminalGuard::suspend` → spawn `$VISUAL`/`$EDITOR`/pattern table at `file:line` → `resume` → reload.
- **Verification**: integration tests on a scratch git repo (stage → status changes → reload → unstage → discard with stash ref returned).

### Step 2.6: Extension Thin-Client Migration & Native `/review` Invocation Gate
- **Affected Files**: `extension/pi/review.ts`, `extension/pi/src/*` (delete renderer), `tests/AGENTS.md`, `extension/pi/AGENTS.md`, root `AGENTS.md`, `tests/golden.rs`
- **Status**: not started. Sub-steps:
  1. **Thin wrapper**: `/review` = TUI guard → `git rev-parse` root/prefix → `parseReviewArgs` → `ctx.tui.stop()` → spawn `bin/diffuse` (`stdio: "inherit"`, `cwd: repoRoot`, argv = base/paths + `--repo <name>` [+ `--notes-file` if custom]) → await exit → `ctx.tui.start()` → summary notify (count notes in `~/.pi/review/notes.json`). Mouse-tracking escapes and the `ctx.ui.custom` overlay are gone — the binary owns the terminal, exactly like the editor flow.
  2. **Delete the TS renderer**: `component.ts`, `component_render.ts`, `model.ts`, `theme.ts`, `persistence.ts`, `component_actions.ts`, `editor.ts`; `backend.ts` shrinks to `diffuseBinPath` + argv building. `reviewInternals` and `review.test.ts`/`golden.test.ts` are removed — the Rust `tests/golden.rs` walk (wire + render) is the single acceptance gate; `npm test` reduces to a thin smoke test (spawn the binary, assert the review opens/JSON gate) or is removed from `check.sh`.
  3. **Docs**: `tests/AGENTS.md`, `extension/pi/AGENTS.md`, root `AGENTS.md` updated (single canonical renderer; the TS renderer is gone).
- **Verification**: `./scripts/check.sh` (fmt, clippy, unit, golden walks) + a manual `/review` in pi proving the binary TUI runs inside the terminal session and pi resumes afterwards.