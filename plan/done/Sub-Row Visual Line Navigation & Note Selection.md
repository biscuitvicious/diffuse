# Sub-Row Visual Line Navigation & Note Selection

- [x] Phase 1 — Core Notes DB Targeted Operations (`crates/diffuse-core/`)
- [x] Phase 2 — Sub-Row Viewport Scrolling & Navigation Geometry (`crates/diffuse-tui/`)
- [x] Phase 3 — Visual Target Resolution & Note Focus Rendering (`crates/diffuse-tui/`)
- [x] Phase 4 — Targeted Note Editing (`m`) & Deletion (`d`) Integration (`crates/diffuse-tui/`)
- [x] Phase 5 — Verification, Golden Review & Acceptance Gate

---

## Executive Summary

In `diffuse-tui`, navigation has historically operated on a hybrid model:
- The cursor is anchored to diff spine rows (`row_idx: usize`), while visual line offsets (`vis_line: usize`) were strictly constrained to the diff line's code segments (`0..seg_counts[row_idx]`).
- Viewport scrolling in `render.rs` actively clamped `self.scroll <= min_vis`, forcing the top of the current row to remain on screen whenever the cursor was on it.
- Note callouts were rendered as non-navigable trailing attachments beneath code rows or file banners.

This architecture creates two major usability issues:
1. **Viewport Snapping & Unviewable Content on Small Screens**: On small or narrow terminals (e.g., mobile Termux/SSH, split tmux panes) and with tall wrapped diff lines or multi-line notes (e.g., Markdown tables or blockquotes exceeding 20–50 lines), the user cannot scroll through the interior of the line or note. Navigating past a line immediately snaps the viewport, skipping unviewed content that extends below the screen.
2. **Coarse Note Management**: Pressing `d` deletes *all* notes overlapping a row or selection rather than a specific note. Furthermore, there is no way to edit an existing note in-place (`m` always creates a new empty draft), and file-banner notes cannot be deleted or edited via keyboard navigation.

This plan unifies **sub-row visual line navigation, smooth viewport scrolling without row-top snapping, and targeted note selection**:
- The viewport clamps strictly around the current visual line (`cursor_vis`), allowing the screen to scroll smoothly through any sub-row segment or note line.
- The cursor can step line-by-line through wrapped diff segments and inside note callouts.
- When focused on a note, `d` deletes *only* that specific note, and `m` opens in-place editing pre-populated with the note's text.

### Core Mathematical & Architectural Invariants

1. **Diff Engine Decoupling**: `AppState.flat: Vec<FlatRow>` continues to represent the code diff and file spine. No artificial `FlatRow` variants are introduced. Myers/LCS diff algorithms, hunk context expansion, git staging (`s`/`u`), and coordinate lookups continue operating on code rows without needing to filter out note rows.
2. **Exact Visual Line Geometry**: Total visual lines for a row are given by `counts[i] = seg_counts[i] + hunk_div_segs + callouts[&i].len()`. Sub-row visual line navigation treats `0..counts[i]` as a contiguous, monotonically increasing visual space.
3. **Cursor-Centric Viewport Clamping**: The viewport scroll offset `self.scroll` is bounded strictly by `[cursor_vis - max_rows + 1, cursor_vis]`. The artificial invariant `self.scroll <= min_vis` is eliminated.
4. **Idempotent Note Persistence**: Editing a note preserves its identity (`id`), file path, side, and line coordinates, updating only its text and modified timestamp in the notes database.

---

## Table of Contents

1. [Phase 1 — Core Notes DB Targeted Operations (`crates/diffuse-core/`)](#1-phase-1--core-notes-db-targeted-operations-cratesdiffuse-core)
2. [Phase 2 — Sub-Row Viewport Scrolling & Navigation Geometry (`crates/diffuse-tui/`)](#2-phase-2--sub-row-viewport-scrolling--navigation-geometry-cratesdiffuse-tui)
3. [Phase 3 — Visual Target Resolution & Note Focus Rendering (`crates/diffuse-tui/`)](#3-phase-3--visual-target-resolution--note-focus-rendering-cratesdiffuse-tui)
4. [Phase 4 — Targeted Note Editing (`m`) & Deletion (`d`) Integration (`crates/diffuse-tui/`)](#4-phase-4--targeted-note-editing-m--deletion-d-integration-cratesdiffuse-tui)
5. [Phase 5 — Verification, Golden Review & Acceptance Gate](#5-phase-5--verification-golden-review--acceptance-gate)

---

## 1. Phase 1 — Core Notes DB Targeted Operations (`crates/diffuse-core/`)

### Objective

Equip `diffuse-core::notes` with targeted operations to delete a specific note by ID and update a note's text in-place without disturbing sibling notes or metadata.

### Files

- [`crates/diffuse-core/src/notes.rs`](../crates/diffuse-core/src/notes.rs)
- [`crates/diffuse-core/src/notes/tests.rs`](../crates/diffuse-core/src/notes/tests.rs)

### Steps

1. **Implement `remove_note_by_id`**:
   ```rust
   pub fn remove_note_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str) -> bool {
       let Some(repo_map) = db.get_mut(repo) else {
           return false;
       };
       let Some(list) = repo_map.get_mut(file) else {
           return false;
       };
       let before = list.len();
       list.retain(|n| n.id != note_id);
       let removed = list.len() < before;
       if list.is_empty() {
           repo_map.remove(file);
       }
       removed
   }
   ```

2. **Implement `update_note_text`**:
   ```rust
   pub fn update_note_text(
       db: &mut NotesDB,
       repo: &str,
       file: &str,
       note_id: &str,
       new_text: String,
   ) -> bool {
       let Some(repo_map) = db.get_mut(repo) else {
           return false;
       };
       let Some(list) = repo_map.get_mut(file) else {
           return false;
       };
       if let Some(note) = list.iter_mut().find(|n| n.id == note_id) {
           note.text = new_text;
           true
       } else {
           false
       }
   }
   ```

3. **Test-First Discipline (`notes/tests.rs`)**:
   - `test_remove_note_by_id`: verify removing a specific note leaves sibling notes on the same file/line intact, returns `true` on match, `false` on missing ID, and cleans up empty file entries.
   - `test_update_note_text`: verify updating note text modifies the targeted note in-place while preserving `id`, `start`, `end`, `side`, and `created_at`.

---

## 2. Phase 2 — Sub-Row Viewport Scrolling & Navigation Geometry (`crates/diffuse-tui/`)

### Objective

Enable the cursor to traverse all visual lines of a row (`0..counts[i]`), including multi-segment wrapped diff lines and note callout lines, and eliminate row-top viewport snapping so the viewport scrolls smoothly through tall content.

### Files

- [`crates/diffuse-tui/src/app/geometry.rs`](../crates/diffuse-tui/src/app/geometry.rs)
- [`crates/diffuse-tui/src/app/navigation.rs`](../crates/diffuse-tui/src/app/navigation.rs)
- [`crates/diffuse-tui/src/app/render.rs`](../crates/diffuse-tui/src/app/render.rs)
- [`crates/diffuse-tui/src/app/navigation/tests.rs`](../crates/diffuse-tui/src/app/navigation/tests.rs)
- [`crates/diffuse-tui/src/app/geometry/tests.rs`](../crates/diffuse-tui/src/app/geometry/tests.rs)

### Steps

1. **Row Total Visual Line Query**:
   - Add helper `row_total_vis_lines(&self, row_idx: usize, content_width: usize) -> usize` on `AppState`, leveraging `cached_geometry`.
   - Visual line count includes:
     - Hunk divider segments (if present on row)
     - Diff line code segments (`seg_counts[i]`)
     - Pre-rendered callout lines (`callouts.get(&i).map_or(0, Vec::len)`)

2. **Uncap Visual Navigation in `navigation.rs`**:
   - `move_down`:
     ```rust
     let total = self.row_total_vis_lines(self.row_idx, self.last_content_width);
     if self.vis_line + 1 < total {
         self.vis_line += 1;
     } else if self.row_idx + 1 < self.flat.len() {
         self.row_idx += 1;
         self.vis_line = 0;
     }
     ```
   - `move_up`:
     ```rust
     if self.vis_line > 0 {
         self.vis_line -= 1;
     } else if self.row_idx > 0 {
         self.row_idx -= 1;
         let prev_total = self.row_total_vis_lines(self.row_idx, self.last_content_width);
         self.vis_line = prev_total.saturating_sub(1);
     }
     ```
   - Update `move_down_wrap` and `move_up_wrap` to wrap at `total` rather than `seg_count`.
   - Update `scroll_to_visual`: when mapping a visual target offset back to `(row_idx, vis_line)`, allow `vis_line` to sit inside callout lines without clamping to `seg_counts[i]`.

3. **Eliminate Row-Top Clamping in `render.rs`**:
   - In `render_diff_viewport_lines`:
     ```rust
     // REMOVE:
     // if min_vis < self.scroll {
     //     self.scroll = min_vis;
     // }
     
     // REPLACE WITH STRICT CURSOR-BOUND CLAMPING:
     if cursor_vis < self.scroll {
         self.scroll = cursor_vis;
     }
     if cursor_vis >= self.scroll + max_rows {
         self.scroll = cursor_vis.saturating_sub(max_rows.saturating_sub(1));
     }
     ```
   - This ensures that if a line is 25 rows tall (wrapped or with notes) and the viewport is 15 rows tall, moving to line 10 scrolls the viewport to reveal lines 0..14 or 5..19 smoothly, without snapping back to line 0.

4. **Test-First Discipline**:
   - Test stepping down through a 5-segment wrapped diff row: cursor moves through segments 0..4 without advancing `row_idx`.
   - Test stepping into a 10-line note attached to a row: cursor moves from `seg_count - 1` into note lines `0..9`, then to the next row.
   - Test viewport scroll clamping: verify `self.scroll` can exceed `min_vis` when `vis_line` is scrolled down inside tall content.

---

## 3. Phase 3 — Visual Target Resolution & Note Focus Rendering (`crates/diffuse-tui/`)

### Objective

Map any `(row_idx, vis_line)` coordinate to its exact semantic target (code segment vs. specific note and note line), and render high-contrast cursor focus on the active line.

### Files

- [`crates/diffuse-tui/src/app/notes.rs`](../crates/diffuse-tui/src/app/notes.rs)
- [`crates/diffuse-tui/src/app/render.rs`](../crates/diffuse-tui/src/app/render.rs)
- [`crates/diffuse-tui/src/view/diff_grid.rs`](../crates/diffuse-tui/src/view/diff_grid.rs)

### Steps

1. **Define Semantic Target Representation**:
   ```rust
   pub enum VisualTarget<'a> {
       FileHeader,
       HunkDivider,
       DiffRow {
           seg_idx: usize,
           total_segs: usize,
       },
       Note {
           note: &'a ReviewNote,
           note_idx: usize,
           line_idx: usize,
           total_note_lines: usize,
       },
   }
   ```

2. **Implement `visual_target_at` on `AppState`**:
   - Inspects `self.flat[row_idx]`.
   - If `vis_line < hunk_div_segs`: returns `VisualTarget::HunkDivider`.
   - If `vis_line < hunk_div_segs + seg_counts[row_idx]`: returns `VisualTarget::DiffRow`.
   - If `vis_line >= hunk_div_segs + seg_counts[row_idx]`: computes relative offset into callout lines.
   - Correlates offset with notes anchored to this row in `self.notes` (accounting for multi-line rendered heights per note) to determine the exact `ReviewNote` and `line_idx`.

3. **Visual Focus Rendering**:
   - **Wrapped Diff Rows**: Highlight the cursor indicator `▸` on the active segment `seg_idx`.
   - **Note Callouts**:
     - When `vis_line` lands on a note line, render an active focus gutter block:
       - Default unfocused note prefix: `▌███ │  ` (amber stripe + yellow blocks)
       - Focused note line prefix: `▌███ ▸  ` with an active cyan stripe (`\x1b[38;5;81m`) or bright white cursor indicator.
     - The active note being navigated or targeted is visually unmistakable.

---

## 4. Phase 4 — Targeted Note Editing (`m`) & Deletion (`d`) Integration (`crates/diffuse-tui/`)

### Objective

Wire keyboard commands `d` and `m` to operate on the focused note when the cursor is inside a note callout, while preserving existing line/range behavior on diff rows.

### Files

- [`crates/diffuse-tui/src/app.rs`](../crates/diffuse-tui/src/app.rs)
- [`crates/diffuse-tui/src/app/notes.rs`](../crates/diffuse-tui/src/app/notes.rs)
- [`crates/diffuse-tui/src/app/dispatch.rs`](../crates/diffuse-tui/src/app/dispatch.rs)
- [`crates/diffuse-tui/src/event.rs`](../crates/diffuse-tui/src/event.rs)
- [`crates/diffuse-tui/src/app/notes/tests.rs`](../crates/diffuse-tui/src/app/notes/tests.rs)

### Steps

1. **Add `editing_note_id` to `AppState`**:
   ```rust
   pub struct AppState {
       ...
       pub editing_note_id: Option<String>,
   }
   ```

2. **Targeted Deletion (`d` / `TuiCommand::DeleteNote`)**:
   - In `AppState::delete_notes`:
     - If `self.visual_target_at(self.row_idx, self.vis_line)` is `VisualTarget::Note { note, .. }`:
       - Call `remove_note_by_id(&mut self.notes, &self.notes_key, &note.file, &note.id)`.
       - Persist via `save_notes`.
       - Sync file notes and call `self.bump_notes()`.
       - Set status message: `("deleted note".to_string(), true)`.
       - Clamp `self.vis_line` to the new row visual height.
     - Else: `d` on diff lines is disabled; status message `("no note here".to_string(), false)`, completely preventing accidental deletion of notes while browsing code lines.

3. **Targeted Editing (`m` / `TuiCommand::StartNoteDraft`)**:
   - In `dispatch.rs` / `AppState::start_note_draft`:
     - If `self.visual_target_at(...)` is `VisualTarget::Note { note, .. }`:
       - `self.editing_note_id = Some(note.id.clone());`
       - `self.note_draft = Some(note.text.clone());`
     - Else:
       - `self.editing_note_id = None;`
       - `self.note_draft = Some(String::new());`

4. **Commit Note Draft (`AppState::commit_note_draft`)**:
   - If `let Some(id) = self.editing_note_id.take()`:
     - Update text using `update_note_text(&mut self.notes, &self.notes_key, &file, &id, text)`.
     - Persist via `save_notes`, sync, and bump.
     - Set status message: `("updated note".to_string(), true)`.
   - Else: execute existing `add_note` logic for new drafts.

5. **Draft Cancellation (`Esc`)**:
   - `self.editing_note_id = None;`
   - `self.note_draft = None;`

6. **Test-First Discipline (`notes/tests.rs`)**:
   - Test that pressing `d` on line 2 of Note B (when Note A and Note B share the same line) deletes only Note B and leaves Note A intact.
   - Test that pressing `m` on a note opens draft with that note's exact text, and committing updates the note text without altering other notes.
   - Test editing and deleting notes attached to file header banners.

---

## 5. Phase 5 — Verification, Golden Review & Acceptance Gate

### Objective

Run full verification suites and verify that all invariants, performance budgets, and regression suites hold.

### Files

- Full workspace test suites
- [`tests/golden.rs`](../tests/golden.rs)

### Steps

1. **Automated Verification Gate**:
   - Execute `./scripts/check.sh`.
   - Verify nightly `rustfmt`, `clippy --fix` (0 warnings), unit tests, and both golden legs (library and binary) pass cleanly.
2. **Performance Budgets**:
   - Unit tests `< 0.5s`.
   - Full gate `< 3s` on warm builds.
3. **Interactive Validation**:
   - Test on narrow viewport (height 20, width 60): verify smooth scroll through multi-line notes and wrapped diff lines.
   - Verify `d` deletes only the focused note and `m` edits in-place.
4. **Sign-off and Plan Archive**:
   - Archive plan to `plan/done/Sub-Row Visual Line Navigation & Note Selection.md` upon completion.
