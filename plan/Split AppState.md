# Split AppState — Modularizing the TUI God Object

- [x] Phase 1 — Scaffolding & Flat Model
- [x] Phase 2 — Geometry & Navigation
- [x] Phase 3 — Selection, Notes, Finder & Collapse
- [x] Phase 4 — Anchoring & Context Expansion
- [x] Phase 5 — Rendering & Dispatch

---

## Executive Summary

`crates/diffuse-tui/src/app.rs` is `6464` lines: a god object coupling `AppState:29` construction, flat-row construction, row queries, visual geometry, viewport navigation, selection/notes/finder, viewport anchoring, hunk context expansion, frame rendering, and command dispatch. Every change recompiles and retests the whole surface, and `render_frame:779` alone spans `~400` lines.

This plan decomposes `app.rs` into `crates/diffuse-tui/src/app/` as a parent module. `app/mod.rs` retains only `AppState` struct definition and trivial constructors; each child module contributes a single `impl AppState` with one clear responsibility. Child modules can access `AppState` privates (`hunk_meta:53`, `note_id_counter:54`, `scroll_pin:70`) without `pub(crate)` leakage that sibling modules would require. Tests move per-module (`app/flat/tests.rs`, `app/geometry/tests.rs`, …) to respect the 400–500 row hint ("less is more") and keep `./scripts/check.sh` sub-`3s`.

Invariants: `flat` + `hunk_starts` + `hunk_meta` stay consistent across every mutation; `visual_geometry` remains pure (`content_width -> starts/counts/seg_counts/total`); viewport pins (`scroll_pin`) survive only until new input or cursor move; `flat.row_idx/vis_line/scroll` always clamped to `visual_geometry` output.

---

## Table of Contents

1. [Phase 1 — Scaffolding & Flat Model](#1-phase-1--scaffolding--flat-model)
2. [Phase 2 — Geometry & Navigation](#2-phase-2--geometry--navigation)
3. [Phase 3 — Selection, Notes, Finder & Collapse](#3-phase-3--selection-notes-finder--collapse)
4. [Phase 4 — Anchoring & Context Expansion](#4-phase-4--anchoring--context-expansion)
5. [Phase 5 — Rendering & Dispatch](#5-phase-5--rendering--dispatch)

---

## 1. Phase 1 — Scaffolding & Flat Model

**Goal:** Introduce `app/` directory without behavior change. Isolate what rows exist.

**Files:**
- `crates/diffuse-tui/src/app.rs` → `crates/diffuse-tui/src/app/mod.rs` (struct + `new:73`, `with_expand_lines:133`, `with_repo_context:138`, `current_file_idx:551`, `current_file:555`, `current_flat_row:559`)
- `crates/diffuse-tui/src/app/flat.rs` — `build_flat:283`, `build_visual_flat:226`, `build_raw_flat:238`, `build_hunk_starts:402`, `build_file_hunk_starts_for_file:332`, `build_hunk_meta:446`, `file_header_row:320`, `first_row_of_file:2986`, `last_row_of_file:2993`, `next/prev_coord_row:3000` helpers
- `crates/diffuse-tui/src/app/query.rs` — `row_of:144`, `row_from_output:148`, `flat_coord:216`, `flat_row_coord:220`, `row_coord:563`, `flat_pos_of_coord:1801`, `get_line_runs:1744` (file-content → runs)
- `crates/diffuse-tui/src/app/sync.rs` — `append_file:454`, `reload:2886`, `rebuild_flat_preserving_cursor:2858`, `adjust_scroll:2876` (flat-rebuilding mutations)
- `crates/diffuse-tui/src/lib.rs:14` — `pub use app::AppState` re-export unchanged

**Steps:**
1. `mkdir crates/diffuse-tui/src/app`, move `app.rs` to `app/mod.rs`, add `mod flat; mod query; mod sync;` and `impl AppState` blocks in children via `use super::AppState`.
2. Relocate corresponding `#[test]` helpers (`test_hunk_starts_and_navigation`, `test_streaming_append_file_*`, `test_untracked_files_collapsed_by_default` flat setup) to `app/flat/tests.rs` and `app/query/tests.rs`; keep one smoke in `app/mod.rs`.
3. Verify `cargo test -p diffuse-tui`, `cargo clippy`, `wc -l crates/diffuse-tui/src/app/*.rs` each `< 400`, `./scripts/check.sh` clean.

## 2. Phase 2 — Geometry & Navigation

**Goal:** Isolate width-dependent layout (pure) from cursor/scroll mutation (impure).

**Files:**
- `crates/diffuse-tui/src/app/geometry.rs` — `visual_geometry:1299`, `row_seg_count:1219`, `cursor_visual_line:1506`, `hunk_ctx_extent_rows:1460`, `hunk_leading_gap:2066`, `hunk_trailing_gap:2107`, `note_callouts:737` stays callout-generation but consumed here; pure functions over `&self` + `content_width`
- `crates/diffuse-tui/src/app/navigation.rs` — `viewport_margin:678`, `jump_to_row:685`, `goto_line:712`, `goto_coord:2940`, `goto_file_idx:1214`, `next_file:2964`/`prev_file:2975`, `next_hunk:1286`/`prev_hunk:1429`, `move_up:1276`/`move_down:1266`, `move_top:3226`/`move_bottom:3230`, `page_up:3172`/`page_down:3113`, `scroll_to_visual:1376`, `adjust_scroll` (if not in sync)
- `crates/diffuse-tui/src/app/view_mode.rs` (optional, or in `navigation.rs`) — `toggle_raw_diff:1556` (relies on geometry + anchor)

**Steps:**
1. Extract geometry pure methods; add unit tests for `visual_geometry` segment counts, `row_seg_count` wrapping via `row_seg_count` existing tests (`test_word_wrap_and_visual_subline_navigation`, `test_raw_diff_word_wrap_and_marker`).
2. Extract navigation mutators; preserve `scroll_pin` invalidation invariants (cleared on movement except `EndMouseDrag`).
3. Move `test_viewport_margin_rounding_half_up`, `test_goto_file_lands_on_banner_with_viewport_margin`, `test_goto_line_preserves_viewport_margin_above`, `test_move_bottom_preserves_viewport_margin_below`, `test_page_down_up_scrolls_one_third_viewport` into `app/navigation/tests.rs`.
4. Gates: `cargo test -p diffuse-tui --lib navigation geometry`, `wc -l` `< 450` each, `./scripts/check.sh`.

## 3. Phase 3 — Selection, Notes, Finder & Collapse

**Goal:** Isolate user-state overlays that sit on top of geometry/navigation.

**Files:**
- `crates/diffuse-tui/src/app/selection.rs` — `compute_selection:3023`, `start_or_confirm_selection:3059`, `band_rows:3077` (selection band), state fields `anchor:42`, `sel_pivot:43`, `confirmed_sel:44`
- `crates/diffuse-tui/src/app/notes.rs` — `commit_note_draft:570`, `delete_notes:624`, `note_callouts:737` (if not in geometry), fields `note_draft:41`, `notes:50`, `note_id_counter:54`
- `crates/diffuse-tui/src/app/collapse.rs` — `toggle_file_collapse:1730`, `toggle_current_file_collapse:1739`, respects `collapsed_files:59`
- `crates/diffuse-tui/src/app/finder.rs` — `open_file_finder:1182`, `cancel_file_finder:1193`, `confirm_file_finder:1200`, `track_finder_selection:1204` delegating to existing `finder::FileFinderState:234`

**Steps:**
1. Move selection logic with its `Selection` type (still defined in `types.rs:6`) and band computation; relocate `test_selection_*` trio and `test_selection_v_confirm_clears_anchor_and_band_renders` to `app/selection/tests.rs`.
2. Move notes lifecycle; relocate `test_note_commit_persists_and_deletes` and note callout rendering assertions to `app/notes/tests.rs`.
3. Move collapse + finder integration; relocate `test_untracked_files_collapsed_by_default` collapse assertions and finder flow into `app/collapse/tests.rs` / `app/finder/tests.rs`.
4. Gates: per-module `cargo test`, `wc -l` `< 350` each, `check.sh`.

## 4. Phase 4 — Anchoring & Context Expansion

**Goal:** Isolate the most subtle invariant: viewport pinning around hunk merges/splits.

**Files:**
- `crates/diffuse-tui/src/app/anchor.rs` — `CursorAnchor:1785`, `DividerFallback:1767`, `DividerAnchors:1776`, `capture_cursor_anchor:1823`, `restore_cursor_anchor:1848`, `capture_divider_anchor:1901`, `restore_divider_pivot:1963`, `scroll_pin:70` handling
- `crates/diffuse-tui/src/app/context.rs` — `expand_hunk_above:2323`/`_mutate:2334`, `expand_hunk_below:2504`/`_mutate:2537`, `expand_hunk_both:2699`, `expand_all_hidden:2131`, `reset_hunk_at:2166`, `reset_current_hunk:2157`, `reset_all_context:2204`, `reset_hunk_slices:2221`, `count_output_hunk_ctx:2291`, gap helpers if not in geometry

**Steps:**
1. Extract anchor primitives first; they are pure viewport math with no hunk mutation, reusable by `toggle_raw_diff` and `reload`.
2. Extract context mutators; each public method captures anchors, calls `_mutate`, then restores via `anchor` module. Preserve invariants: `h1_new_end`/`gap_count` arithmetic, `hunk_meta` updates, `f.rows` rebuild, and `scroll_pin = Some((new_scroll, row_idx))` on merge vs. divider-survives.
3. Relocate `test_separator_expand_by_enter_and_click_and_merge`, `test_hunk_divider_arrow_click_and_syntax_highlighted_expansion`, `test_expand_all_and_reset_context_re_inserts_hunk_dividers`, `test_h1_divider_down_arrow_click_expands_top_and_stays_in_view`, `test_hunk_divider_both_arrow_click_merges_and_pivots_below_row`, `test_wrapped_divider_arrow_click_at_scrollbar_column_expands`, `test_hunk_divider_up_arrow_pinned_even_with_cursor_above`, `test_two_up_arrow_clicks_merge_with_wrapped_rows_and_lock_below_row`, plus `test_mirage_golden_cache_rs_hunk_divider` into `app/context/tests.rs` / `app/anchor/tests.rs`.
4. Gates: `cargo test -p diffuse-tui --lib context anchor`, wrapped-row merge pinning tests are the key regression suite, `wc -l` `anchor.rs ~280`, `context.rs ~450`.

## 5. Phase 5 — Rendering & Dispatch

**Goal:** Slim the orchestrator; leave `AppState` as data + `impl`s, rendering and input as thin adapters.

**Files:**
- `crates/diffuse-tui/src/app/render.rs` — `render_frame:779` (~400 lines today: header/status `793`, viewport geometry `858`, cursor clamp `889`, viewport loop `917` with 5 branches, scrollbar `1099`, ribbon `1137`, modals `1142`). Split helpers: `render_viewport_rows`, `render_divider_row`, `render_file_header_row`, `render_content_row` delegating to `view/diff_grid.rs:321` and `view/header.rs:245`
- `crates/diffuse-tui/src/app/dispatch.rs` — `execute:3237` match on `TuiCommand:5` (`command.rs:76`), mouse hit-testing extracted: `hit_test_divider:3456` (`▲▼/▲▲/▼▼` at `3465` + wrapped `eff_col:3462`), `hit_test_scrollbar:3408`, `hit_test_file_header_triangle:3498`, `handle_mouse_click:3398`, `handle_mouse_drag:3528`
- `crates/diffuse-tui/src/app/mod.rs` + `lib.rs:29` — wire `run_streaming_interactive_tui` to call `state.render_frame` + `event::handle_event` → `state.execute`

**Steps:**
1. Extract dispatch with zero behavior change; each arm delegates to `navigation`/`context`/`selection`/`notes`/`actions` modules. Keep `actions.rs:465` for git stage/unstage/discard side-effects.
2. Extract render orchestration; keep `view/*` as pure ANSI builders, `app/render.rs` as scroll/pin-aware composition.
3. Relocate remaining frame tests (`test_app_state_scroll_through_all_rows`, `test_hunk_divider_renders_and_extent_lift`, `test_render_frame_lines_do_not_overflow_width`, `test_current_line_highlight_covers_gutter`, `test_raw_diff_*`, `test_clicking_file_header_triangle_toggles_collapse`) to `app/render/tests.rs` / `app/dispatch/tests.rs`.
4. Final gates: `wc -l crates/diffuse-tui/src/app/*.rs` all `< 500` (hint, not hard cap), `cargo test -p diffuse-tui` ~71 tests still green, `./scripts/check.sh` all clear, `./scripts/install.sh` + visual smoke (`/review` on `mirage-2025-08`), golden corpus unchanged (`tests/golden/*.wire.json.gz` byte-identical).
