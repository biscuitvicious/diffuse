# Note Anchoring Context Hunks & Unmodified Review Files

- [ ] Global TODO
  - [x] **Phase 1 — Note Inactivity & Scoping for Standalone Diffs**
    - [x] In standalone/piped diff mode (`diffuse` without repo target), disable notes loading, saving, and synthesis unless `--notes-file` is explicitly specified
    - [x] When `--notes-file <path>` is provided in standalone/piped mode, load notes from `<path>`, bypass git commit scoping (`repo@<commit>`), and match notes by file path
    - [x] In git repo mode, retain commit-scoped note lookup (`repo@<commit>`) with fallback to `$XDG_STATE_HOME/diffuse/notes.json`
    - [x] Add unit tests verifying note inactivity for piped diffs without `--notes-file` and path-scoped activation when `--notes-file` is passed
  - [·] **Phase 2 — Synthetic Note Hunk Generation & Context Merging**
    - [ ] In `diffuse-core` hunk normalization, identify notes whose target span `[start..=end]` is uncovered by existing diff hunks
    - [ ] Cluster uncovered notes within $2N$ lines of each other (where $N = \text{visual\_context}$)
    - [ ] For each uncovered note cluster, compute deterministic pre- and post-image line numbers using gap line offset formulas
    - [ ] If an uncovered note cluster is within $2N$ lines of an existing diff hunk, extend context and merge with the existing hunk
    - [ ] If an uncovered note cluster is isolated in a gap or outside all hunks, synthesize a new `Hunk` containing `Row::Ctx` lines spanning $[\max(1, \text{start} - N) \dots \text{end} + N]$
    - [ ] Pull line text from `file.new_content` and `file.old_content`
    - [ ] Allow `shift::normalize_context` to merge touching or overlapping hunks
    - [ ] Add unit tests in `diffuse-core` covering isolated note hunks, boundary-adjacent merging, multi-note clustering, and missing snapshot fallback
  - [ ] **Phase 3 — Unmodified Note-Carrying Files (`FileStatus::Unmodified`)**
    - [ ] Add `FileStatus::Unmodified` (wire code `"unmodified"`) to `crates/diffuse-core/src/parse.rs` and `crates/diffuse-core/src/wire.rs`
    - [ ] In git repo mode, scan the active `NotesDB` for files in the repository that have notes but zero changes in `git diff`
    - [ ] For each unmodified file with notes, load content from the repository and synthesize a `DiffFile` with `FileStatus::Unmodified`
    - [ ] Apply Phase 2 note hunk synthesis to generate context hunks around all notes in unmodified files
    - [ ] Ensure `display_status()` and `file_status_tag()` keep `"unmodified"` unadorned in banners, allowing the zero change cells and speech-bubble badge (`💬K`) to indicate note-only presence
    - [ ] Add unit tests for `FileStatus::Unmodified` parsing, serialization, and header formatting
  - [ ] **Phase 4 — TUI & Renderer Verification**
    - [ ] Verify TUI `note_callouts` anchors cleanly inline on the synthesized rows instead of falling back to the file banner
    - [ ] Verify TUI gap navigation and context expansion (`▲`, `▼`, `↕↕`, Enter) operate seamlessly around synthetic note hunks
    - [ ] Verify Markdown (`-m`) renders synthetic code context around note quotes
    - [ ] Verify CLI pagers (`-p`, `--raw`) render synthetic note context
    - [ ] Add integration tests in `crates/diffuse-tui` and `crates/diffuse-render`
  - [ ] **Phase 5 — Full Verification & Golden Invariance**
    - [ ] Execute `scripts/check.sh` with zero warnings (nightly fmt, clippy, cargo test, dev binary install)
    - [ ] Verify golden test suite invariance (no unintended deviations across existing golden fixtures)

---

## Executive Summary

When reviewing code, review notes often target lines outside the narrow default diff context window ($N=4$), or target files that were untouched by the commit. In diffuse currently, notes outside existing diff hunks cannot anchor to a diff row and fall back to the file header banner (`target_row = banner_idx`), dislocating review discussions from the source code. Furthermore, untouched files with review notes are omitted entirely from diff reviews.

This engineering plan introduces a principled, renderer-agnostic solution:

1. **Synthetic Note Context Hunks**: Notes falling outside existing diff hunks are treated as anchor points that synthesize context hunks spanning $[\max(1, \text{start} - N) \dots \text{end} + N]$. If near an existing hunk ($\le 2N$), context expands and merges seamlessly. If isolated, a standalone context hunk is formed with standard gap dividers around it.
2. **Explicit Piped Notes Discipline**: When diffs are piped or read standalone without a git repository target, commit-scoped anchoring against local `HEAD` is disabled. Standalone diffs only load/save notes when `--notes-file` is explicitly supplied, matching notes across the file by path rather than commit hash.
3. **`FileStatus::Unmodified`**: Files in a repository that have notes but zero diff modifications are synthesized as `FileStatus::Unmodified` (wire code `"unmodified"`). Their banners remain clean and unadorned, naturally signaled by the absence of modification counts and the presence of the speech-bubble badge (`💬K`).

---

## Table of Contents

1. [Phase 1 — Note Inactivity & Scoping for Standalone Diffs](#1-phase-1--note-inactivity--scoping-for-standalone-diffs)
2. [Phase 2 — Synthetic Note Hunk Generation & Context Merging](#2-phase-2--synthetic-note-hunk-generation--context-merging)
3. [Phase 3 — Unmodified Note-Carrying Files (`FileStatus::Unmodified`)](#3-phase-3--unmodified-note-carrying-files-filestatusunmodified)
4. [Phase 4 — TUI & Renderer Verification](#4-phase-4--tui--renderer-verification)
5. [Phase 5 — Full Verification & Golden Invariance](#5-phase-5--full-verification--golden-invariance)

---

## 1. Phase 1 — Note Inactivity & Scoping for Standalone Diffs

### Problem & Motivation
When diffuse receives a piped patch (`git diff | diffuse`) or `--diff patch.diff` outside a git worktree, the current logic still attempts to read `$XDG_STATE_HOME/diffuse/notes.json` and query commit hashes. Piped patches may come from stashes, other branches, or foreign repos where local `HEAD` has no semantic relationship to the patch.

### Concrete Changes
- **Module**: [`crates/diffuse/src/main.rs`](file:///home/daniel/lab/diffuse/crates/diffuse/src/main.rs) and [`crates/diffuse-core/src/pipeline.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/pipeline.rs)
- **Rules**:
  1. If diffuse is running in standalone/piped mode (no repo target or worktree):
     - If `--notes-file` (or `input.notes_file`) is `None`: set `opts.notes_db = None`. Notes actions are disabled, and no notes are loaded, saved, or synthesized.
     - If `--notes-file` (or `input.notes_file`) is `Some(path)`: load the DB from `path`. Bypass `repo@<commit>` scoping; match notes directly by `note.file == file.path`.
  2. If diffuse is running in git repo mode (inside a work tree or reviewing a git target):
     - Retain `ReviewTarget::notes_key()` commit-hash scoping (`repo@<commit>`), defaulting to `$XDG_STATE_HOME/diffuse/notes.json` if `--notes-file` is omitted.
- **Verification**: Unit tests verifying that piped runs without `--notes-file` produce zero notes, while `--notes-file` loads path-scoped notes without requiring a commit hash.

---

## 2. Phase 2 — Synthetic Note Hunk Generation & Context Merging

### Problem & Motivation
If a note targets a line in a modified file that sits in a gap between hunks (e.g. line 80, when hunks exist at lines 10–20 and 150–160), line 80 is not in `OutputFile.rows`. The note cannot anchor inline and is forced onto the file banner header.

### Mathematical Invariants & Gap Line Mapping
Between hunk $H_k$ and $H_{k+1}$, all lines in the source file are unmodified context.
The cumulative net line delta $\Delta_k$ caused by all hunks up to $H_k$ is:
$$\Delta_k = (H_k.\text{new\_start} + H_k.\text{new\_count}) - (H_k.\text{old\_start} + H_k.\text{old\_count})$$

For any line in the gap:
- Given $L_{\text{new}}$: $L_{\text{old}} = L_{\text{new}} - \Delta_k$
- Given $L_{\text{old}}$: $L_{\text{new}} = L_{\text{old}} + \Delta_k$
- Text is taken directly from `file.new_content[L_{\text{new}} - 1]` or `file.old_content[L_{\text{old}} - 1]`.

### Placement & Merging Algorithm
Given context budget $N = \text{opts.visual\_context}$ (default 4):
1. **Span Check**: For each note on `f.path`, check whether its target span `[start..=end]` on `note.side` is already fully contained within an existing hunk's line range.
2. **Clustering**: Group all uncovered notes on `f.path` whose distance is $\le 2N$ into clusters.
3. **Merge vs Synthesize**:
   - For a cluster with span $[C_{\text{start}}..=C_{\text{end}}]$:
     - Check distance to preceding hunk $H_{\text{prev}}$ and succeeding hunk $H_{\text{next}}$.
     - If distance to $H_{\text{prev}} \le 2N$, expand $H_{\text{prev}}$'s trailing context to include the cluster and its $N$ trailing context lines.
     - If distance to $H_{\text{next}} \le 2N$, expand $H_{\text{next}}$'s leading context to include the cluster and its $N$ leading context lines.
     - If isolated (distance $> 2N$ on both sides), synthesize a standalone `Hunk` spanning $[\max(1, C_{\text{start}} - N) \dots C_{\text{end}} + N]$ consisting of `Row::Ctx(CtxLine)`.
4. **Context Normalization**: Run [`shift::normalize_context`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/shift.rs#L84), which automatically merges any touching/overlapping hunks via [`merge_adjacent_hunks`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/shift.rs#L148).
5. **Fallback**: If `file.new_content` and `file.old_content` are missing, skip synthesis gracefully without error.

---

## 3. Phase 3 — Unmodified Note-Carrying Files (`FileStatus::Unmodified`)

### Problem & Motivation
In code reviews, reviewers often leave feedback on unmodified files in the repository. Currently, `git diff` ignores unmodified files, so they never appear in diffuse even if notes exist for them in the database.

### Concrete Changes
- **Data Model**:
  - Add `FileStatus::Unmodified` to [`crates/diffuse-core/src/parse.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/parse.rs).
  - Add wire representation `"unmodified"` in [`crates/diffuse-core/src/wire.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-core/src/wire.rs).
- **Discovery in Git Repo Mode**:
  - When reviewing a git repo target (in [`crates/diffuse/src/main.rs`](file:///home/daniel/lab/diffuse/crates/diffuse/src/main.rs) and streaming/loader):
  - After loading changed files from `git diff`, check `notes_db` for any file paths in the repo that are not in the diff file list.
  - For each such file that exists in the repo tree, load its content from the worktree/blob.
  - Create a `DiffFile` with:
    - `path`: file path
    - `status`: `FileStatus::Unmodified`
    - `new_content` / `old_content`: identical copies of the file text
    - `hunks`: empty initially, then populated by Phase 2 note hunk synthesis
- **Formatting & Banners**:
  - Counts: `adds = 0, dels = 0, mods = 0, movs = 0, notes = K`.
  - In [`diffuse-render/src/diff.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-render/src/diff.rs) and [`diffuse-tui/src/view/header.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/view/header.rs):
    `status_suffix` for `"unmodified"` remains empty (unadorned), just like `"modified"`.
  - The banner renders as:
    ```
    F3/3 src/utils.rs 💬2
    ```
    The absence of change counts and the presence of `💬2` clearly indicates an untouched file carrying notes.

---

## 4. Phase 4 — TUI & Renderer Verification

### Integration Verification
1. **TUI Inline Anchoring**:
   In [`crates/diffuse-tui/src/app/notes.rs`](file:///home/daniel/lab/diffuse/crates/diffuse-tui/src/app/notes.rs), verify `note_callouts` locates the synthesized `Row::Ctx` matching the note's target line, verifying that `target_row` is the inline row index and NOT `banner_idx`.
2. **TUI Gap Navigation**:
   Verify that gap dividers (`··· K lines ···`) between normal hunks and synthetic note hunks expand correctly when pressing Enter, clicking `▲`, `▼`, or `↕↕`.
3. **Markdown Generation**:
   Verify `diffuse -m` renders the synthesized context hunk around the note, followed by the blockquote callout.
4. **CLI Pretty-Printer**:
   Verify `diffuse -p` and `diffuse --raw` display the synthesized context rows with syntax highlighting and line numbers.

---

## 5. Phase 5 — Full Verification & Golden Invariance

### Verification Gates
- Execute `scripts/check.sh` ensuring zero warnings across:
  - Nightly `cargo fmt`
  - `cargo clippy --fix -- -D warnings`
  - Workspace test suite (`cargo test --all-targets`)
  - Dev binary and extension sync
- Verify that standard golden test outputs in `tests/golden/` are 100% byte-identical (since golden test runs use piped diffs without `--notes-file`, triggering Phase 1 note inactivity).
