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
  - [·] Step 2.4: Selection, Note Editing & Inline Prompts (`src/tui/`) — draft input + note persistence (XDG-state `notes.json`), callouts, delete, visual selection (`v`/`V`), selection band done; hunk structure/highlight + raw unified-diff view mode pending (TODO)
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
- **Status**: in progress. Draft input + **note persistence done** (`m` draft, Enter/Ctrl+Enter commits via `commit_note_draft`, `d`/`D` deletes via `delete_notes`, 💬 callouts rendered from `AppState.notes`). **Visual selection done** (`v`/`V` anchor/confirm, Esc cancels anchor before quit, `compute_selection` coordinate snapping mirroring `component.ts`, `band_rows` render for the anchor range and the confirmed selection). Pending sub-steps:
  1. ~~**Note persistence**~~ done: `AppState` owns the `NotesDB` + path; commit `add_note` + `save_notes`; delete `remove_notes` + save; callouts render `💬 [start-end] <markdown runs>`. **Storage decision (design review):** state lives in an **extension-agnostic** location — `$XDG_STATE_HOME/diffuse/` (default `~/.local/state/diffuse/`), `notes.json` + `state.json` — never under a consumer's dir (`~/.pi/review/`). The pi thin client passes `--notes-file` for its own continuity; diffuse's default stays its own (one-way dependency: pi → diffuse).
  2. ~~**Visual selection** (`v`/`V`)~~ done: `anchor`/`confirmed_sel` wired; selection band render (prefix + line lift); selection feeds note commit (`m` on a range), delete (`d`), and editor (`o`). Note: `m`/`d` still act on the single current row; range-feed for `m`/`d`/`o` is part of Step 2.4's remaining work below.
  3. **TODO — Hunk structure & current-hunk highlight**: coordinate-based hunk attribution (fixes the broken all-zero `hunk_idx` on the `f.rows` path, which also breaks `next_hunk`/`prev_hunk`): `hunk_starts` (first flat row of each hunk per file) + `current_hunk(file, coord)` derived from hunk start-lines (mirror `component.ts:507` — no engine/wire change, golden-safe). Visuals: (a) **hunk divider header lines** — render-only interleaved rows (the proven callout/extra-line pattern, not part of `flat`, so movement/scroll math is untouched) `──────── H2 · −118,8 +123,5 · ctx 6▴ 4▾ ──────`, bright for the current hunk, dim for the rest; ctx counters recomputed via `allocate_context` in `visible.rs` (public; the wire carries no `above_ctx`/`below_ctx`); (b) **current-hunk extent** — a soft one-shade lift on the current hunk's *context* rows only (reuse the `band_rows` machinery re-targeted at hunk membership; content tints untouched; the selection band wins where they overlap).
  4. **TODO — Raw unified-diff view mode**: a toggle (key TBD — `r` is reserved for reset-ctx in the `⋯ hidden` row) switches the *content representation only*: the same hunks render as standard unified diff — `@@ -a,b +c,d @@` hunk headers, ` ` ctx / `-` del / `+` add lines, plain green/red line text highlight (no word-level spans/fills). The UI shell is identical: same flat-row navigation, selection, notes/callouts, status, footer; the divider header from (3) becomes the `@@` line in this mode. Mod rows render un-collapsed (`del_text` as a red `-` line, `text` as a green `+` line); moved rows lose the `≈` and read as plain `-`/`+`. `col_offset`/`word_diff` are inert in raw mode. `raw_diff: bool` on `AppState`; a raw path in `render_row_segment` (or a sibling simple renderer); status indicator shows the mode.
    5. **TODO — Cursor anchoring across view switches**: toggling visual ↔ raw must never move the reader — anchor at the cursor's exact position and resolve the *equivalent line* in the other representation (content lengths differ: a visual `~` mod row un-collapses into two raw rows (`-` del_text + `+` text), and raw mode adds `@@` header rows). On toggle, capture `(file_idx, coord, side)` from the current row, rebuild the other view, then locate the flat index whose `row_coord == coord` (prefer the same side: `new_line` for the `+`/visual-new side, `old_line` for the `-` side; a visual mod row resolves to its new side). If the coordinate no longer exists, fall back to the file's first coordinate row. This is the same coordinate-resume mechanism the reference already uses for `state.json` restore (`component.ts:155-169` — restore only when line content matches, else first row) — reuse it as `goto_flat(file, coord, side)`.
- **Verification**: `test_note_commit_persists_and_deletes`, `notes::tests` state-dir resolution tests; note lifecycle (add → saved → reloaded → delete); `test_selection_anchor_span_snaps_to_coord_rows`, `test_selection_v_confirm_clears_anchor_and_band_renders`, `test_selection_requires_coord_rows`, `test_v_starts_and_confirms_selection`, `test_esc_cancels_anchor_before_quit`; new raw-mode render tests (red/green ANSI, `@@` header, un-collapsed mod rows) and hunk-attribution tests; view-toggle tests proving the cursor lands on the equivalent coordinate in both directions (visual→raw→visual round-trips to the same line).

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