use std::{fs, path::Path};

use ratatui::{Terminal, backend::TestBackend};

use super::*;
use crate::{
    command::TuiCommand,
    core::{
        PipelineOptions,
        notes::{load_notes, notes_for_file},
        parse::RowKind,
        pipeline::{PipelineInput, run_pipeline_full},
        wire::PipelineOutput,
    },
    render::Badge,
    view::hunk_divider_text,
};

fn sample_output() -> PipelineOutput {
    let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
     let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
    let new_content = [(
        "filesystem.rs".to_string(),
        "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n"
            .to_string(),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "filesystem.rs".to_string(),
        "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
    )]
    .into_iter()
    .collect();
    run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    )
}

#[test]
fn test_with_theme_builder_sets_theme() {
    let state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path())
        .with_theme(crate::render::DiffuseTheme::DEFAULT);
    assert_eq!(state.theme, crate::render::DiffuseTheme::DEFAULT);
}

#[test]
fn test_clicking_file_header_triangle_toggles_collapse() {
    let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,6 +1025,8 @@ fn test() {
     let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
    let new_content = [(
        "filesystem.rs".to_string(),
        "fn test() {\n    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}\n"
            .to_string(),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "filesystem.rs".to_string(),
        "fn test() {\n    let x = 1;\n    // old comment — em dash\n}\n".to_string(),
    )]
    .into_iter()
    .collect();
    let mut state = AppState::new(
        run_pipeline_full(
            diff,
            Some(new_content),
            Some(old_content),
            PipelineOptions::default(),
        ),
        "test_repo".into(),
        temp_notes_path(),
    );
    state.render_frame(120, 30);

    assert!(!state.collapsed_files.contains(&0), "starts expanded");
    let initial_len = state.flat.len();

    // Click exactly on the ▼ triangle (char col 12) on the header row
    let header_vis = state
        .visual_geometry(state.last_content_width)
        .0
        .first()
        .copied()
        .unwrap_or(0);
    let click_row = state.last_viewport_start + header_vis.saturating_sub(state.scroll);

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL,
        shift: false,
    });
    assert!(
        state.collapsed_files.contains(&0),
        "clicking the triangle must collapse the file"
    );
    assert_eq!(state.flat.len(), 1, "collapsed to header only");

    // Expand again via the (now ▸) triangle
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL,
        shift: false,
    });
    assert!(
        !state.collapsed_files.contains(&0),
        "clicking again must expand the file"
    );
    assert_eq!(state.flat.len(), initial_len);

    // A click elsewhere on the header must NOT toggle — it selects.
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: crate::view::diff_grid::FILE_HEADER_INDICATOR_COL + 10,
        shift: false,
    });
    assert!(
        !state.collapsed_files.contains(&0),
        "non-triangle header clicks must not collapse"
    );
}

#[test]
fn test_note_commit_persists_and_deletes() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(sample_output(), "test_repo".into(), notes_path.clone());

    let file = state.current_file().unwrap().path.clone();
    state.note_draft = Some("check this **carefully**".into());
    state.commit_note_draft();
    assert!(state.note_draft.is_none());

    let notes: Vec<_> = notes_for_file(&state.notes, "test_repo", &file).collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, "check this **carefully**");
    assert!(
        notes[0].start > 0,
        "note must anchor to a real row coordinate"
    );

    let on_disk = load_notes(&notes_path);
    assert_eq!(
        notes_for_file(&on_disk, "test_repo", &file).count(),
        1,
        "commit must persist through the notes DB file"
    );

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("💬"),
        "a committed note must render a callout: {:.200}",
        frame
    );

    state.move_down();
    state.delete_note();
    assert!(
        notes_for_file(&state.notes, "test_repo", &file)
            .next()
            .is_none(),
        "delete must remove the note from the DB"
    );
    let on_disk2 = load_notes(&notes_path);
    assert!(
        notes_for_file(&on_disk2, "test_repo", &file)
            .next()
            .is_none(),
        "delete must persist the removal"
    );
}

#[test]
fn test_help_modal_toggles_and_renders() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());

    let plain = state.render_frame(100, 30);
    assert!(
        !plain.contains("KEY BINDINGS"),
        "help modal must be hidden by default"
    );

    state.show_help = true;
    let with_help = state.render_frame(100, 30);
    assert!(
        with_help.contains("KEY BINDINGS"),
        "help modal must render when enabled"
    );
    assert!(with_help.contains("toggle syntax"));
}

#[test]
fn test_selection_anchor_span_snaps_to_coord_rows() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    // flat[0] is the file header (no coordinate); AppState starts on the
    // first coordinate row (1020). The engine pads the hunk with empty
    // above-context rows (1022-1024), then the real rows: ctx
    // " let x = 1;" (1025), the mod pair (1026), the add (1027). Walk to
    // the first real row, anchor, extend over the mod.
    for _ in 0..3 {
        state.move_down();
    }
    state.start_or_confirm_selection();
    state.move_down();
    state.move_down();

    let sel = state
        .compute_selection()
        .expect("anchor over coordinate rows must yield a selection");
    assert_eq!(sel.file_idx, 0);
    assert!(sel.start < sel.end, "range must span two rows");
    assert_eq!(sel.start, 1025);
    assert_eq!(sel.end, 1027);
    assert!(
        sel.side_is_new,
        "a range led by a new-side row is a new-side selection"
    );
}

#[test]
fn test_selection_v_confirm_clears_anchor_and_band_renders() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    for _ in 0..3 {
        state.move_down();
    }
    state.start_or_confirm_selection();
    state.move_down();
    state.move_down();

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("\x1b[48;5;235m"),
        "an active anchor must band the rows between anchor and cursor"
    );

    state.start_or_confirm_selection();
    assert!(state.anchor.is_none(), "confirming clears the anchor");
    let sel = state
        .confirmed_sel
        .expect("confirm must record a selection");
    assert_eq!(sel.start, 1025);
    assert_eq!(sel.end, 1027);

    let frame2 = state.render_frame(100, 30);
    assert!(
        frame2.contains("\x1b[48;5;235m"),
        "a confirmed selection must keep banding its coordinate range"
    );
}

#[test]
fn test_selection_requires_coord_rows() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    // Anchor on the file header (no coordinate) and confirm in place: the
    // range collapses onto the header which carries no line number, so
    // the reference computes no selection.
    state.row_idx = 0;
    state.vis_line = 0;
    state.start_or_confirm_selection();
    state.start_or_confirm_selection();
    assert!(state.anchor.is_none());
    assert!(state.confirmed_sel.is_none(), "header has no coordinate");

    // Anchoring on the header then walking onto a coordinate row snaps
    // the low end down to the first coordinate row of the file.
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    state.row_idx = 0;
    state.vis_line = 0;
    state.start_or_confirm_selection();
    state.move_down();
    let sel = state
        .compute_selection()
        .expect("anchor over the header must snap to a coordinate row");
    assert_eq!(sel.start, 1022, "snap to the first padded ctx row");
    assert_eq!(sel.end, 1022);
}

#[test]
fn test_raw_diff_uncollapses_mod_and_renders() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    assert!(!state.raw_diff);
    state.toggle_raw_diff();
    assert!(state.raw_diff);

    let del = state
        .flat
        .iter()
        .find(|fr| state.row_of(fr).is_some_and(|r| r.kind == RowKind::Del));
    assert!(
        del.is_some()
            && state
                .row_of(del.unwrap())
                .unwrap()
                .text
                .contains("old comment"),
        "the mod pair must un-collapse into a - line"
    );
    let add = state.flat.iter().find(|fr| {
        state
            .row_of(fr)
            .is_some_and(|r| r.kind == RowKind::Add && r.text.contains("new comment"))
    });
    assert!(add.is_some(), "the mod pair must un-collapse into a + line");

    let sep = state.flat.iter().find(|fr| fr.is_hunk_divider);
    assert!(sep.is_some());

    let frame = state.render_frame(120, 40);
    assert!(frame.contains("@@ -1025,6 +1025,8 @@"), "raw @@ header");
    assert!(
        frame.contains("\x1b[38;5;203m-\x1b[39m\x1b[23m\x1b[38;5;203m"),
        "raw deleted lines render red marker and red text"
    );
    assert!(
        frame.contains("\x1b[38;5;120m+\x1b[39m\x1b[23m\x1b[38;5;120m"),
        "raw added lines render green marker and green text"
    );
}

#[test]
fn test_raw_diff_toggle_anchors_cursor() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    // flat[5] is the mod pair (new 1026); walk onto it.
    for _ in 0..4 {
        state.move_down();
    }
    let r = state.row_of(&state.flat[state.row_idx]).unwrap();
    assert_eq!(r.kind, RowKind::Mod);
    assert_eq!(r.new_line, Some(1026));

    state.toggle_raw_diff();
    let r = state.row_of(&state.flat[state.row_idx]).unwrap();
    assert_eq!(
        r.kind,
        RowKind::Add,
        "the cursor resolves to the + line of the pair"
    );
    assert_eq!(r.new_line, Some(1026), "same coordinate after the switch");

    state.toggle_raw_diff();
    let r = state.row_of(&state.flat[state.row_idx]).unwrap();
    assert_eq!(
        AppState::row_coord(r),
        Some(1026),
        "round-tripping lands back on the same line"
    );
    assert!(!state.raw_diff);
}

#[test]
fn test_raw_diff_exact_input_reproduction() {
    let diff = "diff --git a/cache.rs b/cache.rs\n--- a/cache.rs\n+++ b/cache.rs\n@@ -1,4 +1,4 @@\n-fn a() {\n-    let x = 1;\n-    let y = 2;\n+fn a() {\n+    let x = 10;\n+    let y = 20;\n }\n";
    let new_content = [(
        "cache.rs".to_string(),
        "fn a() {\n    let x = 10;\n    let y = 20;\n}\n".to_string(),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "cache.rs".to_string(),
        "fn a() {\n    let x = 1;\n    let y = 2;\n}\n".to_string(),
    )]
    .into_iter()
    .collect();

    let out = run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.toggle_raw_diff();
    assert!(state.raw_diff);

    let kinds: Vec<RowKind> = state
        .flat
        .iter()
        .filter(|r| !r.is_file_header && !r.is_hunk_divider)
        .map(|r| state.row_of(r).unwrap().kind)
        .collect();

    // Exact match with diff input: 3 deletions, 3 additions, 1 context line
    assert_eq!(
        kinds,
        vec![
            RowKind::Del,
            RowKind::Del,
            RowKind::Del,
            RowKind::Add,
            RowKind::Add,
            RowKind::Add,
            RowKind::Ctx,
        ],
        "raw diff must reproduce the exact hunk row sequence from the input diff"
    );
}

#[test]
fn test_toggle_raw_diff_preserves_cursor_screen_pivot() {
    let mut diff = String::from(
        "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,40 +1,40 @@\n",
    );
    let mut lines = Vec::new();
    for i in 1..=50 {
        if i == 25 {
            diff.push_str("- line 25\n+ line 25 mod\n");
            lines.push(" line 25 mod".to_string());
        } else {
            diff.push_str(&format!(" line {i}\n"));
            lines.push(format!(" line {i}"));
        }
    }
    let new_content = [("test.rs".to_string(), lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.viewport_height = 10;
    state.last_content_width = 80;
    state.render_frame(80, 10);

    // Position cursor at modified line (row 7) and scroll down by 2
    state.scroll = 2;
    state.row_idx = 7;
    let visual_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);

    state.toggle_raw_diff();
    assert!(state.raw_diff);

    let raw_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);
    assert_eq!(
        visual_screen_y, raw_screen_y,
        "cursor screen-y position relative to viewport top must be preserved across raw diff toggle"
    );

    // Toggle back and verify round-trip stability
    state.toggle_raw_diff();
    assert!(!state.raw_diff);
    let roundtrip_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);
    assert_eq!(
        visual_screen_y, roundtrip_screen_y,
        "cursor screen-y position must match after roundtrip toggle"
    );
}

#[test]
fn test_raw_diff_no_right_padding_or_scrollbar() {
    let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,3 @@
 -let short = 1;
 +let short = 2;
 "#;
    let new_content = [("test.rs".to_string(), "let short = 2;\n".to_string())]
        .into_iter()
        .collect();
    let out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.viewport_height = 20;
    state.toggle_raw_diff();
    assert!(state.raw_diff);

    let frame = state.render_frame(80, 20);
    let lines: Vec<&str> = frame.lines().collect();

    // Check the diff rows (excluding banner/ribbon)
    for line in &lines[1..3] {
        // Must not contain scrollbar characters
        assert!(
            !line.contains('█') && !line.contains('│'),
            "raw diff must not draw scrollbar"
        );
        // Must not be right-padded with 50+ trailing spaces before \x1b[K
        assert!(
            !line.contains("                                                  "),
            "raw diff must not right-pad with spaces"
        );
    }
}

#[test]
fn test_raw_diff_wrapped_line_no_false_trailing_highlight() {
    let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,1 +1,1 @@
+6. Segment Rule: Each upper captures the contiguous run of read-only layers directly beneath it.
"#;
    let new_content = [(
            "test.rs".to_string(),
            "6. Segment Rule: Each upper captures the contiguous run of read-only layers directly beneath it.\n".to_string(),
        )]
        .into_iter()
        .collect();
    let out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.viewport_height = 20;
    state.toggle_raw_diff();
    assert!(state.raw_diff);

    // Render at width 40 so the line wraps into multiple segments
    let frame = state.render_frame(40, 20);
    let lines: Vec<&str> = frame.lines().collect();

    // The first segment of the wrapped line must not contain green background
    // highlight \x1b[48;2;28;99;52m (default add_fill_bg)
    for line in &lines[2..4] {
        assert!(
            !line.contains("\x1b[48;2;28;99;52m"),
            "wrapped line segments without trailing spaces must not highlight intermediate spaces: {line}"
        );
    }
}

#[test]
fn test_toggle_raw_diff_from_deleted_line_to_merged_mod_line() {
    let diff = r#"diff --git a/cache.rs b/cache.rs
--- a/cache.rs
+++ b/cache.rs
@@ -80,5 +86,5 @@
  context line 80
  context line 81
-/// LRU cache for path resolution results with modtime validation.
+/// LRU cache for path resolution results with modtime validation and
  context line 83
  context line 84
 "#;
    let new_content = [(
            "cache.rs".to_string(),
            "context line 80\ncontext line 81\n/// LRU cache for path resolution results with modtime validation and\ncontext line 83\ncontext line 84\n".to_string(),
        )]
        .into_iter()
        .collect();
    let old_content = [(
            "cache.rs".to_string(),
            "context line 80\ncontext line 81\n/// LRU cache for path resolution results with modtime validation.\ncontext line 83\ncontext line 84\n".to_string(),
        )]
        .into_iter()
        .collect();
    let out = run_pipeline_full(
        diff,
        Some(new_content),
        Some(old_content),
        PipelineOptions::default(),
    );
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.viewport_height = 20;

    // Switch to raw diff
    state.toggle_raw_diff();
    assert!(state.raw_diff);

    // Find the deleted line in raw diff (old_line = Some(82))
    let del_idx = state
        .flat
        .iter()
        .position(|r| {
            state
                .row_of(r)
                .is_some_and(|row| row.kind == RowKind::Del && row.old_line == Some(82))
        })
        .expect("must find deleted line 82 in raw diff");
    state.row_idx = del_idx;

    // Hit R to toggle back to visual diff
    state.toggle_raw_diff();
    assert!(!state.raw_diff);

    let current_row = &state.flat[state.row_idx];
    assert!(
        !current_row.is_file_header,
        "must not jump to file header row"
    );
    let current_out = state.row_of(current_row).unwrap();
    assert_eq!(
        current_out.old_line,
        Some(82),
        "selected row in visual diff must have old_line 82"
    );
    assert_eq!(
        current_out.new_line,
        Some(88),
        "selected row in visual diff must be the merged mod line 88"
    );
}

#[test]
fn test_cache_rs_lines_115_and_141_stick_to_screen() {
    let diff = r#"diff --git a/src/tracer/vfs/cache.rs b/src/tracer/vfs/cache.rs
index 6f6222d..4aa7a1d 100644
--- a/src/tracer/vfs/cache.rs
+++ b/src/tracer/vfs/cache.rs
@@ -1,7 +1,13 @@
-use std::{cell::RefCell, num::NonZeroUsize, os::unix::fs::MetadataExt, path::PathBuf};
+use std::{
+    cell::RefCell,
+    num::NonZeroUsize,
+    os::unix::fs::MetadataExt,
+    path::PathBuf,
+    sync::atomic::{AtomicU64, Ordering},
+};

 use lru::LruCache;

 use super::{Intent, Links, ResolutionResult};

 // ── Cache Key
@@ -76,55 +82,104 @@ impl ResolutionFingerprint {
 }

 // ── Resolution Cache ────────────────────────────────────────────────────────

 const DEFAULT_CAPACITY: usize = 2048;

-/// LRU cache for path resolution results with modtime validation.
+/// LRU cache for path resolution results with modtime validation and
+/// **epoch-based invalidation**.
+///
+/// Every entry is tagged with the epoch snapshot taken when its resolution
+/// *started* (`VFS::resolve` reads the epoch before lookup). A mode flip /
+/// mount mutation bumps the epoch atomically:
+///
+/// - `lookup` rejects any entry whose tag differs from the caller's snapshot
+///   — a result computed under a pre-flip mount state is never served.
+/// - `insert` drops any entry whose tag is stale *at insert time* — a
+///   resolution that started before a flip but finishes its (possibly
+///   copy-up) computation after the bump cannot re-populate the cache with a
+///   pre-flip result (the in-flight-insert race). The epoch read at resolve
+///   start is the coherence anchor: an insert under an old epoch means the
+///   computation spanned a flip, so the result is discarded.
+///
+/// The epoch is a single `AtomicU64` — when the VFS becomes shared
+/// (multi-threaded multi-guest, plan/TOKIO_MULTITHREADED_HYPERVISOR.md) the
+/// bump stays a single atomic store; the `set_mode` + bump pair must then sit
+/// in one critical section (the per-guest `Mutex` from phase M2).
 #[derive(Debug)]
 pub(super) struct ResolutionCache {
     cache: RefCell<LruCache<CacheKey, ResolutionFingerprint>>,
-    results: RefCell<LruCache<CacheKey, ResolutionResult>>,
+    results: RefCell<LruCache<CacheKey, (u64, ResolutionResult)>>,
+    epoch: AtomicU64,
 }

 impl Clone for ResolutionCache {
     fn clone(&self) -> Self {
         Self {
             cache: RefCell::new(LruCache::new(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())),
             results: RefCell::new(LruCache::new(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())),
+            epoch: AtomicU64::new(self.epoch.load(Ordering::Relaxed)),
         }
     }
 }

 impl ResolutionCache {
     pub fn new() -> Self {
         Self {
             cache: RefCell::new(LruCache::new(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())),
             results: RefCell::new(LruCache::new(NonZeroUsize::new(DEFAULT_CAPACITY).unwrap())),
+            epoch: AtomicU64::new(0),
         }
     }

-    pub fn lookup(&self, key: &CacheKey) -> Option<ResolutionResult> {
+    /// Current epoch. A resolution must snapshot this *before* its first
+    /// lookup and pass it to both `lookup` and `insert`.
+    pub fn epoch(&self) -> u64 {
+        self.epoch.load(Ordering::Relaxed)
+    }
+
+    pub fn lookup(&self, key: &CacheKey, epoch: u64) -> Option<ResolutionResult> {
         let fingerprint = self.cache.borrow().peek(key)?.clone();
-        if fingerprint.is_valid() {
-            return self.results.borrow_mut().get(key).cloned();
+        if !fingerprint.is_valid() {
+            self.cache.borrow_mut().pop(key);
+            self.results.borrow_mut().pop(key);
+            return None;
         }
-        self.cache.borrow_mut().pop(key);
-        self.results.borrow_mut().pop(key);
-        None
+        let (entry_epoch, result) = self.results.borrow_mut().get(key)?.clone();
+        if entry_epoch != epoch {
+            // Computed under an older mount state: the epoch bump (mode flip,
+            // mount mutation) invalidated it.
+            self.cache.borrow_mut().pop(key);
+            self.results.borrow_mut().pop(key);
+            return None;
         }
         Some(result)
     }

     pub fn insert(
         &self,
         key: CacheKey,
         result: ResolutionResult,
         fingerprint: ResolutionFingerprint,
+        epoch: u64,
     ) {
+        // The epoch read at resolve start is the coherence anchor. If it is
+        // already stale, the computation spanned a mode flip / mount change —
+        // discard the result rather than cache a mixed-state entry.
+        if self.epoch.load(Ordering::Relaxed) != epoch {
+            return;
+        }
         self.cache.borrow_mut().put(key.clone(), fingerprint);
-        self.results.borrow_mut().put(key, result);
+        self.results.borrow_mut().put(key, (epoch, result));
     }

+    /// Invalidate every cached entry. Epoch-based: a single atomic bump makes
+    /// all entries computed under an older epoch stale at lookup — race-free
+    /// even against an in-flight resolution inserting after this call (the
+    /// insert carries the pre-bump epoch and is dropped or never served).
+    /// The physical clear additionally bounds memory.
     pub fn clear(&self) {
+        self.epoch.fetch_add(1, Ordering::Relaxed);
         self.cache.borrow_mut().clear();
         self.results.borrow_mut().clear();
     }
 }
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "mirage".into(), temp_notes_path());

    // Exact reproduction test matching user's terminal geometry (74 rows, 300
    // cols) on mirage.work
    let vis_idx_115 = state
        .flat
        .iter()
        .position(|r| {
            r.file_idx == 3 && state.row_of(r).is_some_and(|row| row.new_line == Some(115))
        })
        .unwrap_or_else(|| {
            state
                .flat
                .iter()
                .position(|r| state.row_of(r).is_some_and(|row| row.new_line == Some(115)))
                .unwrap_or_else(|| panic!("must find line 115"))
        });
    state.row_idx = vis_idx_115;
    state.vis_line = 0;

    // Set scroll so line 90 is at the top of the viewport (matching visual
    // screenshot)
    let vis_idx_90 = state
        .flat
        .iter()
        .position(|r| {
            r.file_idx == state.flat[vis_idx_115].file_idx
                && state.row_of(r).is_some_and(|row| row.new_line == Some(90))
        })
        .unwrap_or_else(|| panic!("must find line 90"));
    state.scroll = state.visual_geometry(300).0[vis_idx_90];

    let frame_vis = state.render_frame(300, 74);
    let vis_cursor_line = frame_vis
        .lines()
        .enumerate()
        .skip(2)
        .find(|(_, l)| l.contains("115"))
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("line 115 not in visual frame"));

    state.toggle_raw_diff();
    let frame_raw = state.render_frame(300, 74);
    let raw_cursor_line = frame_raw
        .lines()
        .enumerate()
        .skip(2)
        .find(|(_, l)| l.contains("115"))
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("line 115 not in raw frame"));

    assert_eq!(
        vis_cursor_line, raw_cursor_line,
        "line 115 cursor screen line must not jump on 74x300 terminal: vis={vis_cursor_line}, raw={raw_cursor_line}"
    );

    // Switch back to visual mode for the loop test
    if state.raw_diff {
        state.toggle_raw_diff();
    }

    // Test line 115 and line 141 specifically, across different viewport
    // heights and scroll positions
    for target_line in [115, 141] {
        for vh in [25, 30, 40, 50] {
            let vis_idx = state
                .flat
                .iter()
                .position(|r| {
                    state.row_of(r).is_some_and(|row| {
                        row.new_line == Some(target_line) || row.old_line == Some(target_line)
                    })
                })
                .unwrap_or_else(|| panic!("must find line {target_line} in visual flat"));
            state.row_idx = vis_idx;
            state.vis_line = 0;
            state.scroll = 0;

            let _ = state.render_frame(120, vh);
            let vis_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);

            state.toggle_raw_diff();
            let _ = state.render_frame(120, vh);
            let raw_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);

            assert_eq!(
                vis_screen_y, raw_screen_y,
                "line {target_line} (vh={vh}) screen Y must match on toggle to raw: vis={vis_screen_y}, raw={raw_screen_y}"
            );

            state.toggle_raw_diff();
            let _ = state.render_frame(120, vh);
            let roundtrip_screen_y = state.cursor_visual_line().saturating_sub(state.scroll);

            assert_eq!(
                vis_screen_y, roundtrip_screen_y,
                "line {target_line} (vh={vh}) screen Y must match on roundtrip toggle: vis={vis_screen_y}, roundtrip={roundtrip_screen_y}"
            );
        }
    }

    // Test line 115 in raw diff
    if !state.raw_diff {
        state.toggle_raw_diff();
    }
    assert!(state.raw_diff);

    let raw_idx_115 = state
        .flat
        .iter()
        .position(|r| state.row_of(r).is_some_and(|row| row.new_line == Some(115)))
        .unwrap_or_else(|| panic!("must find line 115 in raw flat"));
    state.row_idx = raw_idx_115;
    state.scroll = 30;

    let frame_raw_115 = state.render_frame(120, 30);
    let is_cursor_line = |l: &&str| {
        l.contains("\x1b[48;5;237m")
            || l.contains("\x1b[48;2;54;54;54m")
            || l.contains("\x1b[48;2;36;56;37m")
            || l.contains("\x1b[48;2;68;24;26m")
    };
    let raw_115_cursor_line = frame_raw_115
        .lines()
        .enumerate()
        .skip(2)
        .find(|(_, l)| is_cursor_line(l))
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("raw 115 cursor line not found"));

    state.toggle_raw_diff();
    assert!(!state.raw_diff);

    let frame_vis_115 = state.render_frame(120, 30);
    let vis_115_cursor_line = frame_vis_115
        .lines()
        .enumerate()
        .skip(2)
        .find(|(_, l)| is_cursor_line(l))
        .map(|(i, _)| i)
        .unwrap_or_else(|| panic!("visual 115 cursor line not found"));

    assert_eq!(
        raw_115_cursor_line, vis_115_cursor_line,
        "cursor screen line on line 115 must not move when switching from raw to visual diff"
    );
}

#[test]
fn test_separator_expand_by_enter_and_click_and_merge() {
    let wire_path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/opt-1-cache-regex-window-slice/wire.jsonl"
    ));
    let jsonl_str = fs::read_to_string(wire_path).expect("must read opt-1 wire.jsonl");
    let files = crate::core::wire::jsonl::decode_jsonl_str(&jsonl_str).unwrap();
    let output = PipelineOutput {
        files,
        branch: String::new(),
        untracked_skipped: vec![],
    };

    let mut state = AppState::new(output, "opt_1".into(), temp_notes_path()).with_expand_lines(6);

    // Find collapse/mod.rs file index
    let fi = state
        .output
        .files
        .iter()
        .position(|f| f.path.contains("collapse/mod.rs"))
        .expect("collapse/mod.rs must exist");

    // Initial hunk count for collapse/mod.rs
    let init_hunk_count = state.output.files[fi].hunks.len();
    assert_eq!(init_hunk_count, 8, "collapse/mod.rs starts with 8 hunks");

    // Jump to H2 to expand context around the divider via Enter
    let h2_start = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;

    state.jump_to_row(h2_start, false);
    assert!(
        state.is_cursor_on_hunk_divider().is_some(),
        "cursor must be on H2 divider"
    );

    let initial_h1_below = state.hunk_meta[fi][0].below;
    let initial_h2_above = state.hunk_meta[fi][1].above;

    // Press Enter on H2 divider to expand context around divider by 6 lines
    state.execute(TuiCommand::ToggleCollapseFile(None));

    assert!(
        state.hunk_meta[fi][0].below >= initial_h1_below + 6,
        "H1 below context must increase by 6"
    );
    assert!(
        state.hunk_meta[fi][1].above >= initial_h2_above + 6,
        "H2 above context must increase by 6"
    );

    // Jump to H2 divider after expansion
    if let Some(hs) = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
    {
        state.jump_to_row(hs.flat, false);
    }

    let frame = state.render_frame(140, 80);
    assert!(
        frame.contains("──── H2/"),
        "H2 divider must still exist before fully merged"
    );
    assert!(
        frame.contains("more lines: ⬍ · ▲ · ▼"),
        "H2 divider must reflect hidden gap with ⬍ · ▲ · ▼ buttons"
    );

    // Keep expanding until H1 and H2 merge
    while state.output.files[fi].hunks.len() == init_hunk_count {
        if let Some(hs) = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
        {
            state.jump_to_row(hs.flat, false);
            state.execute(TuiCommand::ToggleCollapseFile(None));
        } else {
            break;
        }
    }

    assert_eq!(
        state.output.files[fi].hunks.len(),
        init_hunk_count - 1,
        "expanding entire gap must merge H1 and H2 into 1 hunk"
    );
}

#[test]
fn test_hunk_divider_arrow_click_and_syntax_highlighted_expansion() {
    let wire_path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/opt-1-cache-regex-window-slice/wire.jsonl"
    ));
    let jsonl_str = fs::read_to_string(wire_path).expect("must read opt-1 wire.jsonl");
    let files = crate::core::wire::jsonl::decode_jsonl_str(&jsonl_str).unwrap();
    let output = PipelineOutput {
        files,
        branch: String::new(),
        untracked_skipped: vec![],
    };

    let mut state = AppState::new(output, "opt_1".into(), temp_notes_path()).with_expand_lines(6);

    // Find collapse/mod.rs file index
    let fi = state
        .output
        .files
        .iter()
        .position(|f| f.path.contains("collapse/mod.rs"))
        .expect("collapse/mod.rs must exist");

    let initial_h2_above = state.hunk_meta[fi][1].above;

    // Expand H2 above directly
    assert!(state.expand_hunk_above(fi, 1));
    assert_eq!(
        state.hunk_meta[fi][1].above,
        initial_h2_above + 6,
        "H2 above context must increase by 6"
    );

    // Verify prepended context rows in H2 have syntax runs
    let first_row = &state.output.files[fi].hunks[1].rows[0];
    assert_eq!(first_row.kind, RowKind::Ctx);
    assert!(
        !first_row.runs.is_empty(),
        "expanded context row must have syntax token runs"
    );
    assert!(
        first_row.runs.iter().any(|r| r.color.is_some()),
        "expanded context row must have colored syntax tokens"
    );

    let initial_h1_below = state.hunk_meta[fi][0].below;
    // Expand H1 below directly
    assert!(state.expand_hunk_below(fi, 0));
    assert_eq!(
        state.hunk_meta[fi][0].below,
        initial_h1_below + 6,
        "H1 below context must increase by 6"
    );

    // Verify appended context rows in H1 have syntax runs
    let last_h1_row = state.output.files[fi].hunks[0].rows.last().unwrap();
    assert_eq!(last_h1_row.kind, RowKind::Ctx);
    assert!(
        !last_h1_row.runs.is_empty(),
        "expanded context row must have syntax token runs"
    );

    // Verify that clicking an arrow on a hunk divider via MouseClick does NOT
    // move cursor
    let target_line = 100;
    let line_flat_idx = state
        .flat
        .iter()
        .position(|fr| {
            fr.file_idx == fi
                && state
                    .row_of(fr)
                    .is_some_and(|r| r.new_line == Some(target_line))
        })
        .expect("must find target line in flat");
    state.jump_to_row(line_flat_idx, false);
    assert_eq!(state.row_idx, line_flat_idx);

    // Render frame with sufficient height so both H2 divider and target line
    // are in viewport
    state.render_frame(140, 80);
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let h2_hunk_start = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap();
    let h2_div_vis = starts[h2_hunk_start.flat];
    let click_row = state.last_viewport_start + h2_div_vis.saturating_sub(state.scroll);

    // Compute exact visual column of '▲' on H2 divider
    let h = &state.output.files[fi].hunks[1];
    let hidden_gap = state.hunk_leading_gap(fi, 1);
    let txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: state.output.files[fi].hunks.len(),
        },
        h,
        hidden_gap,
    );
    let idx = txt.find("▲").unwrap();
    let col = txt[..idx].chars().count();

    // Click on the '▲' column
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col,
        shift: false,
    });

    // Verify cursor is on the expanded hunk divider and divider line y position
    // is invariant
    let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
    let h2_div_vis_after = starts_after[state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat];
    let click_row_after = state.last_viewport_start + h2_div_vis_after.saturating_sub(state.scroll);
    assert_eq!(
        click_row, click_row_after,
        "divider line screen y position must remain exactly the same when expanding context"
    );
}

#[test]
fn test_expand_all_and_reset_context_re_inserts_hunk_dividers() {
    let diff = r#"diff --git a/cache.rs b/cache.rs
index 111..222 100644
--- a/cache.rs
+++ b/cache.rs
@@ -1,5 +1,5 @@
 fn h1() {
-    let x = 1;
+    let x = 10;
 }
@@ -30,5 +30,5 @@
 fn h2() {
-    let y = 2;
+    let y = 20;
 }
@@ -60,5 +60,5 @@
 fn h3() {
-    let z = 3;
+    let z = 30;
 }
"#;
    let mut new_lines = Vec::new();
    for i in 1..=80 {
        if i == 2 {
            new_lines.push("    let x = 10;".to_string());
        } else if i == 31 {
            new_lines.push("    let y = 20;".to_string());
        } else if i == 61 {
            new_lines.push("    let z = 30;".to_string());
        } else {
            new_lines.push(format!("// context line {i}"));
        }
    }
    let new_content: std::collections::HashMap<String, String> =
        [("cache.rs".to_string(), new_lines.join("\n"))]
            .into_iter()
            .collect();
    let output = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());

    let mut state = AppState::new(output, "mirage".into(), temp_notes_path()).with_expand_lines(6);

    let fi = state
        .output
        .files
        .iter()
        .position(|f| f.path.contains("cache.rs"))
        .expect("cache.rs must exist");

    let init_hunks = state.output.files[fi].hunks.len();
    assert_eq!(init_hunks, 3, "cache.rs starts with 3 hunks");

    // 1. Expand all hidden context lines via 'a' shortcut
    state.execute(TuiCommand::ExpandAllHidden);
    assert_eq!(
        state.output.files[fi].hunks.len(),
        1,
        "expanding all context merges cache.rs into 1 contiguous hunk"
    );

    // 2. Reset context back to -U via 'c' shortcut (ResetCurrentHunkContext)
    let cache_file_row = state
        .flat
        .iter()
        .position(|fr| fr.file_idx == fi && !fr.is_file_header)
        .expect("row in cache.rs");
    state.jump_to_row(cache_file_row, false);
    state.execute(TuiCommand::ResetCurrentHunkContext);

    assert_eq!(
        state.output.files[fi].hunks.len(),
        3,
        "resetting context splits hunk back wherever interior ctx > 2 * -U, restoring 3 hunks"
    );

    // Verify dividers are re-rendered
    let frame = state.render_frame(140, 200);
    assert!(frame.contains("──── H1/3 ·"));
    assert!(frame.contains("──── H2/3 ·"));
    assert!(frame.contains("──── H3/3 ·"));
}

#[test]
fn test_h1_divider_down_arrow_click_expands_top_and_stays_in_view() {
    let diff = r#"diff --git a/TODO.md b/TODO.md
index 1111111..2222222 100644
--- a/TODO.md
+++ b/TODO.md
@@ -10,4 +10,6 @@
 // line 10
+// line 11 added
+// line 12 added
 // line 13
"#;
    let mut old_lines = Vec::new();
    for i in 1..=25 {
        old_lines.push(format!("// line {i}"));
    }
    let mut new_lines = old_lines.clone();
    new_lines.insert(10, "// line 11 added".to_string());
    new_lines.insert(11, "// line 12 added".to_string());

    let contents = [("TODO.md".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("TODO.md".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run();
    let mut state = AppState::new(output, "test_repo".into(), temp_notes_path());

    let fi = 0;
    let gap = state.hunk_leading_gap(fi, 0);
    assert_eq!(gap, 6, "6 lines hidden before H1");

    // Render initial frame with small viewport (14 rows)
    let frame = state.render_frame(80, 14);
    assert!(frame.contains("6 more lines: ▼"));
    assert!(!frame.contains("▲"));

    // Click down arrow on H1 divider
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let h1_start = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 0)
        .unwrap();
    let h1_div_vis = starts[h1_start.flat];
    let click_row = state.last_viewport_start + h1_div_vis.saturating_sub(state.scroll);

    let h = &state.output.files[fi].hunks[0];
    let txt = hunk_divider_text(
        Badge {
            idx: 0,
            total: state.output.files[fi].hunks.len(),
        },
        h,
        6,
    );
    let idx = txt.find("▼").unwrap();
    let col = txt[..idx].chars().count();

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col,
        shift: false,
    });

    // Frame after clicking down arrow: H1 should now start at line 1 and file
    // header must remain visible!
    let frame_after = state.render_frame(80, 14);
    assert!(
        frame_after.contains("TODO.md"),
        "file header must remain visible on screen"
    );
    assert!(
        frame_after.contains("──── H1/1 ·"),
        "H1 divider must remain visible on screen"
    );
    assert!(
        frame_after.contains("// line 1"),
        "expanded line 1 must be visible on screen"
    );
}

#[test]
fn test_hunk_divider_both_arrow_click_merges_and_pivots_below_row() {
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -23,9 +24,9 @@
 // line 23
 // line 24
 // line 25
 // line 26
-// line 27 old
+// line 27 new
 // line 28
 // line 29
 // line 30
 // line 31
"#;
    let mut old_lines = Vec::new();
    for i in 1..=40 {
        if i == 27 {
            old_lines.push("// line 27 old".to_string());
        } else {
            old_lines.push(format!("// line {i}"));
        }
    }
    let mut new_lines = Vec::new();
    for i in 1..=40 {
        if i == 5 {
            new_lines.push("// inserted one".to_string());
        }
        if i == 28 {
            new_lines.push("// line 27 new".to_string());
        } else {
            new_lines.push(format!("// line {i}"));
        }
    }

    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run();
    let mut state =
        AppState::new(output, "test_repo".into(), temp_notes_path()).with_expand_lines(12);

    let fi = 0;
    assert_eq!(state.output.files[fi].hunks.len(), 2);
    let gap = state.hunk_leading_gap(fi, 1);
    assert_eq!(
        gap, 14,
        "14 lines hidden between H1 and H2; step=12 leaves 2 for the second call"
    );
    assert!(
        gap <= 2 * state.expand_lines,
        "gap must be fully consumed by the two sequential expansions"
    );

    // Render initial frame so layout state is populated
    state.render_frame(80, 20);
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);

    // Locate H2 divider and the boundary row: with n hidden lines split
    // (n+1)/2 up / n-(n+1)/2 down, the first downward-side row is the
    // original H2 first row minus the downward take.
    let div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .map(|hs| hs.flat)
        .expect("H2 divider must exist in flat");
    let h2_first_new_line = state.output.files[fi].hunks[1]
        .rows
        .first()
        .and_then(|r| r.new_line)
        .expect("H2 must have rows");
    let top_take = gap.div_ceil(2).min(state.expand_lines);
    let bottom_take = gap - top_take;
    let below_row_new_line = h2_first_new_line - bottom_take as u32;

    let div_screen_y = starts[div_flat].saturating_sub(state.scroll);

    // Click the ↕↕ (expand both) arrow on the H2 divider: the gap splits
    // (n+1)/2 up / n-(n+1)/2 down, and on merge everything pivots around
    // the line where the divider was — scroll stays put.
    let scroll_before = state.scroll;
    // Reference row ABOVE the divider (last row of H1) — must not move.
    let ref_new_line = state.output.files[fi].hunks[0]
        .rows
        .iter()
        .rev()
        .find_map(|r| r.new_line)
        .expect("H1 must have rows");
    let (starts_pre, _c_pre, _s_pre, _) = state.visual_geometry(state.last_content_width);
    let ref_vis_before = state
        .flat_pos_of_coord(fi, Some(ref_new_line), None)
        .and_then(|pos| starts_pre.get(pos))
        .copied()
        .expect("H1 last row in flat");

    let h = &state.output.files[fi].hunks[1];
    let txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: state.output.files[fi].hunks.len(),
        },
        h,
        gap,
    );
    let both_idx = txt.find("⬍").expect("divider text must contain ⬍");
    let col = txt[..both_idx].chars().count();

    state.execute(TuiCommand::MouseClick {
        row: state.last_viewport_start + div_screen_y,
        col,
        shift: false,
    });

    // The whole gap was consumed: H2 merged into H1
    assert_eq!(
        state.output.files[fi].hunks.len(),
        1,
        "fully consumed gap must merge H2 into H1"
    );

    // The boundary between the upward and downward halves sits exactly
    // where the divider was on screen; the top half pushed the content
    // above up by its visual length.
    let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
    let boundary_vis_after = state
        .flat_pos_of_coord(fi, Some(below_row_new_line), None)
        .and_then(|pos| starts_after.get(pos))
        .copied()
        .expect("boundary row must exist after merge");
    assert_eq!(
        boundary_vis_after.saturating_sub(state.scroll),
        div_screen_y,
        "the up/down boundary must pivot at the divider's old screen y"
    );
    let shift = state.scroll.saturating_sub(scroll_before);
    // Content above the divider moved up by exactly the scroll shift.
    let ref_vis_after = state
        .flat_pos_of_coord(fi, Some(ref_new_line), None)
        .and_then(|pos| starts_after.get(pos))
        .copied()
        .expect("reference row must exist after merge");
    assert_eq!(
        ref_vis_after, ref_vis_before,
        "content above keeps its absolute layout"
    );
    assert_eq!(
        ref_vis_after.saturating_sub(state.scroll),
        ref_vis_before
            .saturating_sub(scroll_before)
            .saturating_sub(shift),
        "content above the divider slides up by exactly the top half"
    );
}

#[test]
fn test_wrapped_divider_arrow_click_at_scrollbar_column_expands() {
    // Regression: on narrow screens the divider wraps so the ▲▲ arrows sit
    // flush against the border/scrollbar column; tapping them there must
    // expand the gap instead of triggering a scrollbar seek that jumps to
    // another file.
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -17,9 +18,9 @@
 // line 17
 // line 18
 // line 19
 // line 20
-// line 21 old
+// line 21 new
 // line 22
 // line 23
 // line 24
 // line 25
"#;
    let mut old_lines = Vec::new();
    for i in 1..=40 {
        if i == 21 {
            old_lines.push("// line 21 old".to_string());
        } else {
            old_lines.push(format!("// line {i}"));
        }
    }
    let mut new_lines = Vec::new();
    for i in 1..=40 {
        if i == 5 {
            new_lines.push("// inserted one".to_string());
        }
        if i == 22 {
            new_lines.push("// line 21 new".to_string());
        } else {
            new_lines.push(format!("// line {i}"));
        }
    }
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run();
    let mut state =
        AppState::new(output, "test_repo".into(), temp_notes_path()).with_expand_lines(6);

    let fi = 0;
    // Narrow terminal: divider text (~60 chars) wraps across two segments
    state.render_frame(48, 30);
    assert!(
        state.last_content_width < 60,
        "content width must be narrower than the divider text for this scenario"
    );

    let (starts, _counts, _seg, _) = state.visual_geometry(state.last_content_width);
    let div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;
    let gap_before = state.hunk_leading_gap(fi, 1);
    let hk = &state.output.files[fi].hunks[1];
    let txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: state.output.files[fi].hunks.len(),
        },
        hk,
        gap_before,
    );

    // Find the visual segment holding the '▲▲' button and its column
    // within that segment.
    let cw = state.last_content_width;
    let up_char_col = txt[..txt.find("▲").unwrap()].chars().count();
    let seg_off = up_char_col / cw;
    let col_in_seg = up_char_col % cw;

    let div_screen = starts[div_flat].saturating_sub(state.scroll) + seg_off;

    state.execute(TuiCommand::MouseClick {
        row: state.last_viewport_start + div_screen,
        col: col_in_seg,
        shift: false,
    });

    assert_eq!(
        state.hunk_leading_gap(fi, 1),
        gap_before.saturating_sub(6),
        "tap on wrapped ▲ must expand the gap"
    );
    // Clicking a divider button must never trigger a scrollbar seek: the
    // same file's divider must remain in the viewport.
    let (starts2, _c2, _s2, _t2) = state.visual_geometry(state.last_content_width);
    let div_screen_after = starts2[div_flat].saturating_sub(state.scroll);
    assert!(
        div_screen_after < state.viewport_height,
        "clicking a divider button must never trigger a scrollbar seek"
    );
}

#[test]
fn test_seek_then_render_never_yanks_scroll() {
    // Seek stability across a render pass at a narrow width where the
    // divider wraps: scroll_to_visual must place the cursor exactly where
    // the render's cursor-follow expects it, or the render yanks scroll
    // on the next frame. Targets landing inside a divider's own segments
    // are excluded: showing a whole divider (scroll snapped to its start)
    // is the render's standing behavior for divider rows.
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -17,9 +18,9 @@
 // line 17
 // line 18
 // line 19
 // line 20
-// line 21 old
+// line 21 new
 // line 22
 // line 23
 // line 24
 // line 25
"#;
    let mut old_lines = Vec::new();
    for i in 1..=40 {
        if i == 21 {
            old_lines.push("// line 21 old".to_string());
        } else {
            old_lines.push(format!("// line {i}"));
        }
    }
    let mut new_lines = Vec::new();
    for i in 1..=40 {
        if i == 5 {
            new_lines.push("// inserted one".to_string());
        }
        if i == 22 {
            new_lines.push("// line 21 new".to_string());
        } else {
            new_lines.push(format!("// line {i}"));
        }
    }
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run();
    // Narrow: the divider (~60 chars with the gap tail) wraps to 2 segs.
    let (w, h) = (48, 16);
    let mut state = AppState::new(output.clone(), "test_repo".into(), temp_notes_path());
    state.render_frame(w, h);
    let vh = state.viewport_height;
    let (_, _, _, total) = state.visual_geometry(state.last_content_width);
    let max_scroll = total.saturating_sub(vh);
    assert!(max_scroll > 5, "need scroll room, got {max_scroll}");
    let div_segs = state.hunk_div_seg_count(0, 1, state.last_content_width);
    assert!(
        div_segs >= 2,
        "divider must wrap for this scenario, got {div_segs}"
    );

    for t in [0, 5, max_scroll / 2, max_scroll] {
        // Fresh state per target: cursor placement must not leak across
        // probes. Skip targets inside a divider's own segments (whole-
        // divider display is standing behavior there).
        let mut probe = AppState::new(state.output.clone(), "test_repo".into(), temp_notes_path());
        probe.render_frame(w, h);
        let (starts, _, _, _) = probe.visual_geometry(probe.last_content_width);
        let on_div_seg = probe.hunk_starts.iter().any(|hs| {
            let s = starts[hs.flat];
            let n = probe.hunk_div_seg_count(hs.file, hs.hunk, probe.last_content_width);
            t > s && t < s + n
        });
        if on_div_seg {
            continue;
        }
        probe.scroll_to_visual(t);
        probe.render_frame(w, h);
        assert_eq!(
            probe.scroll,
            t.min(max_scroll),
            "render must not yank scroll after seeking to {t}"
        );
    }
    // At least the undisputed targets must have been exercised.
    assert!(max_scroll >= 2);
}

#[test]
fn test_scrollbar_column_click_on_divider_row_seeks() {
    // The scrollbar owns its column: a click there seeks even when it
    // lands on a divider row, and must not expand the gap.
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/a.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -17,9 +18,9 @@
 // line 17
 // line 18
 // line 19
 // line 20
-// line 21 old
+// line 21 new
 // line 22
 // line 23
 // line 24
 // line 25
"#;
    let mut old_lines = Vec::new();
    for i in 1..=40 {
        if i == 21 {
            old_lines.push("// line 21 old".to_string());
        } else {
            old_lines.push(format!("// line {i}"));
        }
    }
    let mut new_lines = Vec::new();
    for i in 1..=40 {
        if i == 5 {
            new_lines.push("// inserted one".to_string());
        }
        if i == 22 {
            new_lines.push("// line 21 new".to_string());
        } else {
            new_lines.push(format!("// line {i}"));
        }
    }
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run();
    let mut state = AppState::new(output, "test_repo".into(), temp_notes_path());

    state.render_frame(80, 16);
    let cw = state.last_content_width;
    let ho = state.last_viewport_start;
    let vh = state.viewport_height;
    let (starts, _, _, total) = state.visual_geometry(cw);
    let max_scroll = total.saturating_sub(vh);
    assert!(
        max_scroll >= 2,
        "need scroll room for a seek, got {max_scroll}"
    );
    let div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == 0 && hs.hunk == 1)
        .unwrap()
        .flat;
    let div_screen = starts[div_flat].saturating_sub(state.scroll);
    let track_h = 16;
    let (_, _, thumb_off, thumb_size, _) = state.render_diff_viewport_lines(80, vh, track_h);
    assert!(
        div_screen < vh,
        "divider must be in the viewport for this scenario"
    );
    assert!(
        ho + div_screen >= thumb_off + thumb_size,
        "divider must sit on a track row (below the top thumb), got screen {div_screen}"
    );
    let gap_before = state.hunk_leading_gap(0, 1);
    assert!(gap_before > 0, "need a hidden gap to prove no-expand");

    state.execute(TuiCommand::MouseClick {
        row: ho + div_screen,
        col: cw,
        shift: false,
    });

    assert!(
        state.is_dragging_scrollbar,
        "scrollbar column must start a drag even on divider rows"
    );
    assert_ne!(
        state.scroll, 0,
        "scrollbar column click on a divider row must seek"
    );
    assert_eq!(
        state.hunk_leading_gap(0, 1),
        gap_before,
        "scrollbar column click must not expand the gap"
    );
}

#[test]
fn test_divider_arrow_hit_area_wraps_with_segments() {
    // Arrow hit areas live in the unwrapped divider text, so the ±1
    // tolerance wraps across visual segments without ever touching the
    // scrollbar column:
    // 1) glyph at a segment end is hit from the next segment's first cell;
    // 2&3) glyph at a segment start is hit from the previous segment's
    //    last cell.
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/a.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -17,9 +18,9 @@
 // line 17
 // line 18
 // line 19
 // line 20
-// line 21 old
+// line 21 new
 // line 22
 // line 23
 // line 24
 // line 25
"#;
    let mut old_lines = Vec::new();
    for i in 1..=40 {
        if i == 21 {
            old_lines.push("// line 21 old".to_string());
        } else {
            old_lines.push(format!("// line {i}"));
        }
    }
    let mut new_lines = Vec::new();
    for i in 1..=40 {
        if i == 5 {
            new_lines.push("// inserted one".to_string());
        }
        if i == 22 {
            new_lines.push("// line 21 new".to_string());
        } else {
            new_lines.push(format!("// line {i}"));
        }
    }
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run();

    // Character index of the ▲ glyph in hunk 2's divider text.
    let hk = output.files[0].hunks[1].clone();
    let gap_probe = {
        let tmp = AppState::new(output.clone(), "test_repo".into(), temp_notes_path());
        tmp.hunk_leading_gap(0, 1)
    };
    assert!(gap_probe > 0, "need a hidden gap for arrow hits");
    let txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: output.files[0].hunks.len(),
        },
        &hk,
        gap_probe,
    );
    let up_idx = txt.find('▲').expect("divider must carry ▲");
    let c = txt[..up_idx].chars().count();

    // Case 1: glyph at a segment end (width c+1) is hit from the next
    // segment's first cell.
    let mut state = AppState::new(output.clone(), "test_repo".into(), temp_notes_path());
    let width_end = c + 1;
    state.render_frame(width_end, 40);
    assert!(
        state.last_content_width == width_end,
        "no scrollbar may appear or the column math shifts"
    );
    let cw = state.last_content_width;
    let ho = state.last_viewport_start;
    let (starts, _, _, _) = state.visual_geometry(cw);
    let div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.hunk == 1)
        .unwrap()
        .flat;
    let div_screen = starts[div_flat].saturating_sub(state.scroll);
    assert!(
        state.hunk_div_seg_count(0, 1, cw) >= 2,
        "divider must wrap for this scenario"
    );
    state.execute(TuiCommand::MouseClick {
        row: ho + div_screen + 1,
        col: 0,
        shift: false,
    });
    assert_eq!(
        state.hunk_leading_gap(0, 1),
        gap_probe.saturating_sub(state.expand_lines.max(1)),
        "next-segment first cell must hit a line-end ▲"
    );

    // Cases 2&3: glyph at a segment start (width c) is hit from the
    // previous segment's last cell.
    let mut state = AppState::new(output, "test_repo".into(), temp_notes_path());
    state.render_frame(c, 40);
    assert!(
        state.last_content_width == c,
        "no scrollbar may appear or the column math shifts"
    );
    let cw = state.last_content_width;
    let ho = state.last_viewport_start;
    let (starts, _, _, _) = state.visual_geometry(cw);
    let div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.hunk == 1)
        .unwrap()
        .flat;
    let div_screen = starts[div_flat].saturating_sub(state.scroll);
    assert!(
        state.hunk_div_seg_count(0, 1, cw) >= 2,
        "divider must wrap for this scenario"
    );
    state.execute(TuiCommand::MouseClick {
        row: ho + div_screen,
        col: cw - 1,
        shift: false,
    });
    assert_eq!(
        state.hunk_leading_gap(0, 1),
        gap_probe.saturating_sub(state.expand_lines.max(1)),
        "previous-segment last cell must hit a wrapped ▲"
    );
}

#[test]
fn test_scrollbar_drawn_column_matches_hit_column() {
    // The bar glyph must sit exactly in the column the click hit-test
    // uses (`last_content_width`), across the whole viewport.
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,60 @@\n-old\n");
    for i in 1..=60 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    let frame = state.render_frame(80, 30);
    let cw = state.last_content_width;
    let ho = state.last_viewport_start;
    let vh = state.viewport_height;
    let mut checked = 0;
    for line in frame.lines().skip(ho).take(vh) {
        let plain = crate::render::strip_ansi(line);
        let chars: Vec<char> = plain.chars().collect();
        assert!(
            chars.len() > cw,
            "viewport line must reach the scrollbar column"
        );
        assert!(
            chars[cw] == '█' || chars[cw] == '│',
            "scrollbar cell must hold the bar glyph, got {:?}",
            chars[cw]
        );
        checked += 1;
    }
    assert_eq!(checked, vh, "must check every viewport row");
}

#[test]
fn test_scrollbar_click_seeks_from_pointer_row() {
    // Click and drag share one exact mapping: every bar-column click
    // seeks to the pointer row's scroll, thumb or track alike.
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,60 @@\n-old\n");
    for i in 1..=60 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.render_frame(80, 30);
    let vh = state.viewport_height;
    let track_h = state.screen_height;
    state.scroll_to_visual(20);
    state.render_frame(80, 30);
    let scrolled = state.scroll;
    assert!(
        scrolled > 0,
        "must be scrolled into the interior, got {scrolled}"
    );
    let cw = state.last_content_width;
    let (_, _, _, total) = state.visual_geometry(cw);
    let max_scroll = total.saturating_sub(vh);

    // Clicking the thumb row seeks exactly like a track click.
    let thumb_row = track_h / 2;
    let want = AppState::scrollbar_scroll_for_row(track_h, max_scroll, thumb_row);
    state.execute(TuiCommand::MouseClick {
        row: thumb_row,
        col: cw,
        shift: false,
    });
    assert!(
        state.is_dragging_scrollbar,
        "bar-column click must start a drag"
    );
    assert_eq!(
        state.scroll, want,
        "bar-column click must seek to the pointer row"
    );

    state.execute(TuiCommand::MouseClick {
        row: 0,
        col: cw,
        shift: false,
    });
    assert_eq!(state.scroll, 0, "top row click must seek to the top");

    state.execute(TuiCommand::MouseClick {
        row: track_h - 1,
        col: cw,
        shift: false,
    });
    assert_eq!(
        state.scroll, max_scroll,
        "last row click must seek to max scroll"
    );
}

#[test]
fn test_scrollbar_drag_follows_formula_monotonically() {
    // Dragging maps pointer rows to scrolls exactly, monotonically, with
    // exact extremes — no flats, no jumps.
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,60 @@\n-old\n");
    for i in 1..=60 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.render_frame(80, 24);
    let vh = state.viewport_height;
    let track_h = state.screen_height;
    let header_offset = state.last_viewport_start;
    state.scroll_to_visual(20);
    state.render_frame(80, 24);
    assert!(state.scroll > 0, "must be scrolled into the interior");
    let cw = state.last_content_width;
    let (_, _, _, total) = state.visual_geometry(cw);
    let max_scroll = total.saturating_sub(vh);
    assert!(max_scroll >= track_h, "need room for a strict sweep");

    state.execute(TuiCommand::MouseClick {
        row: track_h / 2,
        col: cw,
        shift: false,
    });
    assert!(state.is_dragging_scrollbar);

    let mut prev = usize::MAX;
    let mut first = true;
    for r in 0..track_h {
        state.execute(TuiCommand::MouseDrag {
            row: r,
            col: cw,
            shift: false,
        });
        let row_relative = r.saturating_sub(header_offset);
        let want = AppState::scrollbar_scroll_for_row(vh, max_scroll, row_relative);
        assert_eq!(
            state.scroll, want,
            "drag to row {r} must follow the formula"
        );
        if first {
            assert_eq!(want, 0, "row 0 must yield scroll 0");
            first = false;
        } else {
            assert!(
                state.scroll >= prev,
                "dragging down must never move up, row {r}"
            );
        }
        prev = state.scroll;
    }
    assert_eq!(prev, max_scroll, "last row must yield max scroll");
}

#[test]
fn test_scrollbar_drag_extremes_from_any_grab() {
    // Pointer rows 0 and H-1 always mean scroll 0 and max scroll,
    // regardless of where the thumb was grabbed.
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,60 @@\n-old\n");
    for i in 1..=60 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());
    state.render_frame(80, 24);
    let vh = state.viewport_height;
    let track_h = state.screen_height;
    state.scroll_to_visual(20);
    state.render_frame(80, 24);
    let scrolled = state.scroll;
    assert!(scrolled > 1, "must start mid-content, got {scrolled}");
    let cw = state.last_content_width;
    let (counts, _, _, total) = state.visual_geometry(cw);
    let header_vis = counts.first().copied().unwrap_or(1);
    let max_scroll = total.saturating_sub(vh);
    let (off, size) = AppState::scrollbar_thumb_for(track_h, vh, total, scrolled, header_vis)
        .expect("scrollbar must apply");
    assert!(size >= 2, "need a small thumb for this scenario");

    // Grab the thumb's top edge, then drag to both extremes: the
    // formula answers from the pointer row alone.
    state.execute(TuiCommand::MouseClick {
        row: off,
        col: cw,
        shift: false,
    });
    state.execute(TuiCommand::MouseDrag {
        row: 0,
        col: cw,
        shift: false,
    });
    assert_eq!(state.scroll, 0, "row 0 must yield scroll 0 from any grab");

    state.execute(TuiCommand::MouseDrag {
        row: track_h - 1,
        col: cw,
        shift: false,
    });
    assert_eq!(
        state.scroll, max_scroll,
        "last row must yield max scroll from any grab"
    );
}

#[test]
fn test_scrollbar_column_spans_full_height() {
    // The bar column must run the whole terminal height, including the
    // header and ribbon rows — not just the viewport.
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,60 @@\n-old\n");
    for i in 1..=60 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "test_repo".into(), temp_notes_path());

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut thumb_rows = 0;
    for y in 0..20 {
        let sym = buffer[(79, y)].symbol();
        assert!(
            sym == "█" || sym == "│",
            "every screen row must carry the bar column, row {y} has {sym:?}"
        );
        if sym == "█" {
            thumb_rows += 1;
        }
    }
    assert!(
        thumb_rows >= 2,
        "thumb must be at least 2 rows high, got {thumb_rows}"
    );
}

#[test]
fn test_hunk_divider_up_arrow_pinned_even_with_cursor_above() {
    // Regression: cursor sits on a row ABOVE the divider (typical after
    // scrolling/navigating); the render-time cursor clamp must not drag
    // scroll back and slide the divider down after an up-arrow expansion.
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -17,9 +18,9 @@
 // line 17
 // line 18
 // line 19
 // line 20
-// line 21 old
+// line 21 new
 // line 22
 // line 23
 // line 24
 // line 25
"#;
    let mut old_lines = Vec::new();
    for i in 1..=40 {
        if i == 21 {
            old_lines.push("// line 21 old".to_string());
        } else {
            old_lines.push(format!("// line {i}"));
        }
    }
    let mut new_lines = Vec::new();
    for i in 1..=40 {
        if i == 5 {
            new_lines.push("// inserted one".to_string());
        }
        if i == 22 {
            new_lines.push("// line 21 new".to_string());
        } else {
            new_lines.push(format!("// line {i}"));
        }
    }
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run();
    let mut state =
        AppState::new(output, "test_repo".into(), temp_notes_path()).with_expand_lines(6);

    let fi = 0;

    // Park the cursor on an EARLY row of H1 (far above the H2 divider).
    // Small viewport so max_scroll > 0 and the render-time cursor clamp
    // has room to fight the divider pivot.
    let early_row = state
        .flat
        .iter()
        .position(|fr| {
            fr.file_idx == fi
                && !fr.is_file_header
                && !fr.is_hunk_divider
                && state.row_of(fr).is_some_and(|r| r.new_line == Some(1))
        })
        .expect("first content row must exist");
    state.jump_to_row(early_row, false);

    state.render_frame(300, 20);
    let div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let div_screen_before = starts[div_flat].saturating_sub(state.scroll);
    assert!(
        div_screen_before < state.viewport_height,
        "divider must be on screen for this scenario"
    );
    let cursor_before = state.row_idx;
    let cursor_screen_before = starts[cursor_before].saturating_sub(state.scroll);
    let gap_before = state.hunk_leading_gap(fi, 1);
    let hk = &state.output.files[fi].hunks[1];
    let txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: state.output.files[fi].hunks.len(),
        },
        hk,
        gap_before,
    );
    let idx = txt.find("▲").unwrap();
    let col = txt[..idx].chars().count();
    state.execute(TuiCommand::MouseClick {
        row: state.last_viewport_start + div_screen_before,
        col,
        shift: false,
    });

    // Render like the real event loop does — this is where the clamp fired
    state.render_frame(300, 20);

    assert_eq!(
        state.hunk_leading_gap(fi, 1),
        gap_before.saturating_sub(6),
        "up arrow must have expanded 6 lines into the gap"
    );

    // Re-locate the divider: flat indices shifted with the inserted rows
    let div_flat_after = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;
    let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
    let div_screen_after = starts_after[div_flat_after].saturating_sub(state.scroll);
    assert_eq!(
        div_screen_after, div_screen_before,
        "divider must stay pinned even when the cursor is above it"
    );
    let cursor_screen_after = starts_after[state.row_idx].saturating_sub(state.scroll);
    assert_eq!(
        cursor_screen_after, cursor_screen_before,
        "cursor must retain its visual screen position when clicking an expansion arrow"
    );
}

#[test]
fn test_two_up_arrow_clicks_merge_with_wrapped_rows_and_lock_below_row() {
    // Static conversion of the exploratory worktree probe: two sequential
    // \u{25b2}\u{25b2} presses on a narrow screen with LONG context rows (they
    // wrap into several visual segments). The second press consumes the
    // remaining gap and merges; the row directly below the removed divider
    // must stay locked at its own pre-click screen position.
    let diff = r#"diff --git a/wide.rs b/wide.rs
index 1111111..2222222 100644
--- a/wide.rs
+++ b/wide.rs
@@ -1,8 +1,9 @@
 // line 1 -- padding padding padding padding padding padding padding padding
 // line 2 -- padding padding padding padding padding padding padding padding
 // line 3 -- padding padding padding padding padding padding padding padding
 // line 4 -- padding padding padding padding padding padding padding padding
+// inserted marker -- padding padding padding padding padding padding padding
 // line 5 -- padding padding padding padding padding padding padding padding
 // line 6 -- padding padding padding padding padding padding padding padding
 // line 7 -- padding padding padding padding padding padding padding padding
 // line 8 -- padding padding padding padding padding padding padding padding
@@ -19,9 +20,9 @@
 // line 19 -- padding padding padding padding padding padding padding padding
 // line 20 -- padding padding padding padding padding padding padding padding
 // line 21 -- padding padding padding padding padding padding padding padding
 // line 22 -- padding padding padding padding padding padding padding padding
-// line 23 old -- padding padding padding padding padding padding padding pad
+// line 23 new -- padding padding padding padding padding padding padding pad
 // line 24 -- padding padding padding padding padding padding padding padding
 // line 25 -- padding padding padding padding padding padding padding padding
 // line 26 -- padding padding padding padding padding padding padding padding
 // line 27 -- padding padding padding padding padding padding padding padding
"#;
    let pad = " -- padding padding padding padding padding padding padding padding";
    let mut old_lines = Vec::new();
    for i in 1..=40 {
        if i == 23 {
            old_lines.push(format!("// line 23 old{pad}"));
        } else {
            old_lines.push(format!("// line {i}{pad}"));
        }
    }
    let mut new_lines = Vec::new();
    for i in 1..=40 {
        if i == 5 {
            new_lines.push(format!("// inserted marker{pad}"));
        }
        if i == 24 {
            new_lines.push(format!("// line 23 new{pad}"));
        } else {
            new_lines.push(format!("// line {i}{pad}"));
        }
    }
    let contents = [("wide.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("wide.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .run();
    let mut state =
        AppState::new(output, "test_repo".into(), temp_notes_path()).with_expand_lines(6);

    let fi = 0;
    assert_eq!(state.output.files[fi].hunks.len(), 2);
    // Anchor = first content row of H2 (the line directly below divider)
    let anchor_new_line = state.output.files[fi].hunks[1].rows[0]
        .new_line
        .expect("H2 must have rows");

    state.render_frame(50, 30);
    let cw = state.last_content_width;
    // Rows are long enough that they wrap into several visual segments.
    assert!(cw < 80, "scenario requires a narrow viewport");

    for click in 1..=2 {
        let gap = state.hunk_leading_gap(fi, 1);
        assert!(gap > 0, "click #{click}: gap must still be positive");
        let div_flat = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;
        let (starts, counts, seg_counts, _) = state.visual_geometry(cw);
        let div_vis = starts[div_flat];
        // Park scroll so the divider sits mid-viewport
        state.scroll = div_vis.saturating_sub(6);
        let (starts, _counts2, _seg_counts2, _) = state.visual_geometry(cw);
        let dsy = starts[div_flat].saturating_sub(state.scroll);
        let div_segs = counts[div_flat].saturating_sub(seg_counts[div_flat]);
        let anchor_screen_pre = state
            .flat_pos_of_coord(fi, Some(anchor_new_line), None)
            .map(|p| starts[p] + div_segs - state.scroll)
            .expect("anchor must exist");

        let hk = state.output.files[fi].hunks[1].clone();
        let txt = hunk_divider_text(
            Badge {
                idx: 1,
                total: state.output.files[fi].hunks.len(),
            },
            &hk,
            gap,
        );
        let idx = txt.find("▲").unwrap();
        let up_col = txt[..idx].chars().count();
        state.execute(TuiCommand::MouseClick {
            row: state.last_viewport_start + dsy + up_col / cw,
            col: up_col % cw,
            shift: false,
        });

        let merged = state.output.files[fi].hunks.len() == 1;
        let (starts2, _c2, _s2, _) = state.visual_geometry(cw);
        let anchor_screen_post = state
            .flat_pos_of_coord(fi, Some(anchor_new_line), None)
            .map(|p| starts2[p].saturating_sub(state.scroll));

        if !merged {
            // Divider survived: it must stay pinned at its screen y.
            let div_vis2 = state
                .hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == 1)
                .map(|hs| starts2[hs.flat])
                .unwrap();
            assert_eq!(
                div_vis2.saturating_sub(state.scroll),
                dsy,
                "click #{click}: divider must stay pinned"
            );
        } else {
            // Merge: the below-row must keep its own screen position.
            assert_eq!(
                anchor_screen_post,
                Some(anchor_screen_pre),
                "merged: row below the removed divider must stay locked"
            );
        }
    }
    assert_eq!(
        state.output.files[fi].hunks.len(),
        1,
        "two presses on a 10-line gap must fully consume and merge it"
    );
}

#[test]
fn test_render_ratatui_frame_layout() {
    use ratatui::{Terminal, backend::TestBackend};

    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,2 @@
 fn hello() {
-    println!("old");
+    println!("new");
 }
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());
    state.status_message = Some(("Saved note".to_string(), true));

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    // First line holds repo name and global counts; the file moved down.
    let mut line0 = String::new();
    for x in 0..80 {
        line0.push_str(buffer[(x, 0)].symbol());
    }
    assert!(line0.contains("diffuse"));
    assert!(!line0.contains("src/lib.rs"));

    // Second line must contain file path plus files and hunks badges.
    let mut line1 = String::new();
    for x in 0..80 {
        line1.push_str(buffer[(x, 1)].symbol());
    }
    assert!(line1.contains("src/lib.rs"));
    assert!(line1.contains("file 1/1"));
    assert!(line1.contains("hunk 1/1"));

    // Third line must contain the status message "Saved note"
    let mut line2 = String::new();
    for x in 0..80 {
        line2.push_str(buffer[(x, 2)].symbol());
    }
    assert!(line2.contains("Saved note"));

    // Viewport area must contain diff grid lines (e.g. file header or hunk
    // content)
    let mut grid_text = String::new();
    for y in 3..22 {
        for x in 0..80 {
            grid_text.push_str(buffer[(x, y)].symbol());
        }
        grid_text.push('\n');
    }
    assert!(grid_text.contains("fn hello()") || grid_text.contains("src/lib.rs"));
}

#[test]
fn test_render_ratatui_frame_modals_overlay() {
    use ratatui::{Terminal, backend::TestBackend};

    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,2 @@
 fn hello() {
-    println!("old");
+    println!("new");
 }
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());

    // 1. Help Modal Active
    state.show_help = true;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..24 {
        for x in 0..80 {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("KEY BINDINGS"));
    assert!(text.contains("? / Esc close"));

    // 2. File Finder Modal Active
    state.show_help = false;
    state.open_file_finder();
    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..24 {
        for x in 0..80 {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(text.contains("FIND FILE"));
    assert!(text.contains("src/lib.rs"));
}

#[test]
fn test_render_ratatui_frame_scrolling_clean_spans() {
    use ratatui::{Terminal, backend::TestBackend};

    let diff = r#"diff --git a/Cargo.lock b/Cargo.lock
--- a/Cargo.lock
+++ b/Cargo.lock
@@ -1,5 +1,5 @@
 [[package]]
-name = "old_dep"
+name = "new_dep"
 version = "0.1.0"
-checksum = "1111"
+checksum = "2222"
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Scroll down and up
    for _ in 0..5 {
        state.move_down();
        terminal
            .draw(|f| {
                state.render_ratatui_frame(f);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut full_text = String::new();
        for y in 0..24 {
            for x in 0..80 {
                full_text.push_str(buffer[(x, y)].symbol());
            }
            full_text.push('\n');
        }

        // Must NOT contain raw ANSI escapes or leaked SGR numbers
        assert!(!full_text.contains("\x1b["), "Must not leak raw ESC");
        assert!(!full_text.contains("48;5;"), "Must not leak bg SGR codes");
        assert!(!full_text.contains("38;5;"), "Must not leak fg SGR codes");
        assert!(
            !full_text.contains("29m"),
            "Must not leak strikethrough reset SGR"
        );
    }
}

#[test]
fn test_render_ratatui_frame_note_modal_overlay_with_range_selection() {
    use ratatui::{Terminal, backend::TestBackend};

    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,5 +1,5 @@
-fn old_one() {}
-fn old_two() {}
-fn old_three() {}
+fn new_one() {}
+fn new_two() {}
+fn new_three() {}
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());

    // Select range
    state.start_or_confirm_selection();
    state.move_down();
    state.move_down();
    state.note_draft = Some("multiline\nnote draft".into());

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..24 {
        for x in 0..80 {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }

    assert!(
        text.contains("NOTE on src/lib.rs:1-3"),
        "Must show range 1-3 in modal title: {text}"
    );
    assert!(text.contains("multiline"), "Must show multiline content");
    assert!(text.contains("note draft"), "Must show second line");
}

#[test]
fn test_render_ratatui_frame_mouse_hit_testing() {
    use ratatui::{Terminal, backend::TestBackend};

    let diff = r#"diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn one() {}
-fn two_old() {}
+fn two_new() {}
 fn three() {}
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    assert_eq!(
        state.last_viewport_start, 2,
        "Viewport starts after header and status bars"
    );
    assert!(state.viewport_height > 0);

    // Click on row 3 (which is viewport row 1, selecting row_idx 1)
    state.execute(TuiCommand::MouseClick {
        row: 3,
        col: 10,
        shift: false,
    });
    assert_eq!(state.row_idx, 1);
}

#[test]
fn test_event_paste_into_note_modal() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());

    state.execute(TuiCommand::StartNoteDraft);
    assert_eq!(state.note_draft, Some(String::new()));

    crate::event::handle_event(
        &mut state,
        crossterm::event::Event::Paste(
            "This is a pasted multiline note\nwith multiple lines of text.".to_string(),
        ),
    );

    assert_eq!(
        state.note_draft.as_deref(),
        Some("This is a pasted multiline note\nwith multiple lines of text.")
    );
}

#[test]
fn test_event_paste_into_file_finder() {
    let diff = "diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());

    state.execute(TuiCommand::OpenFileFinder);
    assert!(state.file_finder.is_some());

    crate::event::handle_event(
        &mut state,
        crossterm::event::Event::Paste("main".to_string()),
    );

    assert_eq!(state.file_finder.as_ref().unwrap().query, "main");
}

#[test]
fn test_note_modal_margins_with_scrollbar() {
    // Generate enough lines to trigger a scrollbar
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,50 @@\n-old\n");
    for i in 1..=50 {
        diff.push_str(&format!("+new line {i}\n"));
    }

    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());

    state.execute(TuiCommand::StartNoteDraft);
    assert!(state.note_draft.is_some());

    let backend = TestBackend::new(50, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    // Locate the top border of the note modal
    let mut found_border_y = None;
    for y in 0..20 {
        if buffer[(1, y)].symbol() == "╭" {
            found_border_y = Some(y);
            break;
        }
    }

    let y = found_border_y.expect("must find top border ╭ at col 1");
    assert_eq!(
        buffer[(0, y)].symbol(),
        " ",
        "Col 0 must be left margin space"
    );
    assert_eq!(buffer[(1, y)].symbol(), "╭", "Col 1 must be left border ╭");
    assert_eq!(
        buffer[(47, y)].symbol(),
        "╮",
        "Col 47 must be right border ╮"
    );
    assert_eq!(
        buffer[(48, y)].symbol(),
        " ",
        "Col 48 must be right margin space before scrollbar"
    );
    assert!(
        buffer[(49, y)].symbol() == "█" || buffer[(49, y)].symbol() == "│",
        "Col 49 must carry the scrollbar column behind the modal margin"
    );
}

#[test]
fn test_mov_target_and_source_render_in_tui() {
    let wire_path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/opt-1-cache-regex-window-slice/wire.jsonl"
    ));
    let jsonl_str = fs::read_to_string(wire_path).expect("must read opt-1 wire.jsonl");
    let files = crate::core::wire::jsonl::decode_jsonl_str(&jsonl_str).unwrap();
    let output = PipelineOutput {
        files,
        branch: String::new(),
        untracked_skipped: vec![],
    };

    let mut state = AppState::new(output, "opt_1".into(), temp_notes_path());

    // Find collapse/mod.rs file index
    let fi = state
        .output
        .files
        .iter()
        .position(|f| f.path.contains("collapse/mod.rs"))
        .expect("collapse/mod.rs must exist");

    // Jump to collapse/mod.rs
    let file_start = state.flat.iter().position(|fr| fr.file_idx == fi).unwrap();
    state.jump_to_row(file_start, false);

    // Find position of row with old_line or new_line around 96
    let tgt_pos = state
        .flat
        .iter()
        .position(|fr| state.row_of(fr).is_some_and(|r| r.new_line == Some(96)));
    assert!(tgt_pos.is_some(), "target move row 96 must exist in flat");
    state.jump_to_row(tgt_pos.unwrap(), false);
    let frame = state.render_frame(120, 40);
    let raw_frame = crate::render::strip_ansi(&frame);
    assert!(
        raw_frame.contains("96≈+"),
        "frame must render target move row 96≈+"
    );

    let src_pos = state.flat.iter().position(|fr| {
        state.row_of(fr).is_some_and(|r| {
            r.kind == RowKind::Mov
                && r.mov_side == Some(crate::core::wire::MovSide::Src)
                && r.old_line == Some(127)
        })
    });
    assert!(src_pos.is_some(), "source move row 127 must exist in flat");

    state.jump_to_row(src_pos.unwrap(), false);
    let frame_src = state.render_frame(120, 40);
    let raw_frame_src = crate::render::strip_ansi(&frame_src);
    assert!(
        raw_frame_src.contains("127≈-"),
        "frame must render source move row 127≈-"
    );
    assert!(
        raw_frame_src.contains("135 -") && !raw_frame_src.contains("135≈-"),
        "trailing blank line 135 must be regular deletion '-', not move '≈-'"
    );
}

#[test]
fn test_hunk_divider_arrow_hit_rectangles_and_dot_exclusion() {
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,3 +1,3 @@
 1
 2
 3
@@ -20,3 +20,3 @@
 20
 21
 22
"#;
    let old_lines: Vec<String> = (1..=30).map(|i| i.to_string()).collect();
    let contents: std::collections::HashMap<String, String> =
        [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents.clone())
        .with_old_content(contents)
        .run();
    let mut state =
        AppState::new(output, "test_repo".into(), temp_notes_path()).with_expand_lines(6);
    state.render_frame(120, 30);
    let fi = 0;
    let gap = state.hunk_leading_gap(fi, 1);
    assert_eq!(gap, 14);

    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let h2_div_vis = starts[state.hunk_starts[1].flat];
    let click_row = state.last_viewport_start + h2_div_vis.saturating_sub(state.scroll);

    let h = &state.output.files[fi].hunks[1];
    let txt = hunk_divider_text(
        Badge {
            idx: 1,
            total: state.output.files[fi].hunks.len(),
        },
        h,
        gap,
    );

    // Locate arrows and separating dots
    let both_idx = txt.find('⬍').unwrap();
    let both_col = txt[..both_idx].chars().count();
    let up_idx = txt.find('▲').unwrap();
    let up_col = txt[..up_idx].chars().count();

    // The dot between both and up is exactly mid-way (both_col + 2 == up_col -
    // 2)
    let dot_col = both_col + 2;
    assert_eq!(dot_col, up_col - 2);

    // Clicking the separating dot must NOT trigger any expansion
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: dot_col,
        shift: false,
    });
    assert_eq!(
        state.hunk_leading_gap(fi, 1),
        gap,
        "clicking the separating dot must not trigger expansion"
    );

    // Clicking both_col - 1 (leading space of ⬍) MUST hit both
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: both_col - 1,
        shift: false,
    });
    let gap_after_both = state.hunk_leading_gap(fi, 1);
    assert!(
        gap_after_both < gap,
        "clicking leading space of ⬍ must trigger both-expansion"
    );
}

#[test]
fn test_file_header_indicator_hit_rectangle_three_chars_wide() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    state.render_frame(80, 20);
    let click_row = state.last_viewport_start;
    let c = crate::view::diff_grid::FILE_HEADER_INDICATOR_COL;

    // Col c - 2 (outside the 3-char hit rect) must NOT toggle collapse
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: c - 2,
        shift: false,
    });
    assert!(
        !state.collapsed_files.contains(&0),
        "col c-2 must not trigger file collapse"
    );

    // Col c + 2 (outside the 3-char hit rect) must NOT toggle collapse
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: c + 2,
        shift: false,
    });
    assert!(
        !state.collapsed_files.contains(&0),
        "col c+2 must not trigger file collapse"
    );

    // Col c - 1 (leading space) MUST toggle collapse
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: c - 1,
        shift: false,
    });
    assert!(
        state.collapsed_files.contains(&0),
        "col c-1 (leading space) must trigger file collapse"
    );

    // Col c + 1 (trailing space) MUST toggle collapse back
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: c + 1,
        shift: false,
    });
    assert!(
        !state.collapsed_files.contains(&0),
        "col c+1 (trailing space) must toggle file collapse back"
    );
}

#[test]
fn test_mouse_click_hunk_header_selects_hunk_divider() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    state.render_frame(120, 40);

    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let target_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == 0 && hs.hunk == 0)
        .expect("must find hunk 0")
        .flat;
    let h0_div_vis = starts[target_flat];
    let click_row = state.last_viewport_start + h0_div_vis.saturating_sub(state.scroll);

    // Initial cursor is on row 0 (file header)
    state.row_idx = 0;
    state.vis_line = 0;

    // Click on column 0 of the hunk divider line (outside of any expand arrow)
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 0,
        shift: false,
    });

    assert_eq!(
        state.row_idx, target_flat,
        "cursor must move to hunk start row"
    );
    assert_eq!(state.vis_line, 0, "vis_line must be on hunk divider");
    assert!(
        matches!(
            state.visual_target_at(state.row_idx, state.vis_line),
            Some(crate::app::notes::VisualTarget::HunkDivider {
                file_idx: 0,
                hunk_idx: 0
            })
        ),
        "visual target must be HunkDivider"
    );
}

#[test]
fn test_mouse_click_note_line_selects_note() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    state.row_idx = 1;
    state.vis_line = 0;
    state.note_draft = Some("Review this line".into());
    state.commit_note_draft();

    state.render_frame(120, 40);
    let (starts, _, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let cw = state.last_content_width;
    let div_lines = state.hunk_div_seg_count(0, 0, cw);
    let row_segs = seg_counts[1];

    let note_vis = starts[1] + div_lines + row_segs;
    let click_row = state.last_viewport_start + note_vis.saturating_sub(state.scroll);

    state.row_idx = 0;
    state.vis_line = 0;

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 10,
        shift: false,
    });

    assert_eq!(state.row_idx, 1, "cursor must move to row with the note");
    assert_eq!(
        state.vis_line,
        div_lines + row_segs,
        "vis_line must be at note callout line"
    );
    match state.visual_target_at(state.row_idx, state.vis_line) {
        Some(crate::app::notes::VisualTarget::Note { note, line_idx, .. }) => {
            assert_eq!(note.text, "Review this line");
            assert_eq!(line_idx, 0);
        }
        other => panic!("expected VisualTarget::Note, got {:?}", other),
    }
}

#[test]
fn test_mouse_click_multiline_note_selects_specific_line() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    state.row_idx = 1;
    state.vis_line = 0;
    state.note_draft = Some("Line 1\n\nLine 2".into());
    state.commit_note_draft();

    state.render_frame(120, 40);
    let (starts, _, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let cw = state.last_content_width;
    let div_lines = state.hunk_div_seg_count(0, 0, cw);
    let row_segs = seg_counts[1];

    let layout = state.note_callout_layout_for_row(1, cw);
    assert!(layout[0].1 >= 2, "note must have at least 2 visual lines");

    // Click on the second line of the note callout: div_lines + row_segs + 1
    let note_vis_l1 = starts[1] + div_lines + row_segs + 1;
    let click_row = state.last_viewport_start + note_vis_l1.saturating_sub(state.scroll);

    state.row_idx = 0;
    state.vis_line = 0;

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 10,
        shift: false,
    });

    assert_eq!(state.row_idx, 1);
    assert_eq!(state.vis_line, div_lines + row_segs + 1);
    match state.visual_target_at(state.row_idx, state.vis_line) {
        Some(crate::app::notes::VisualTarget::Note { note, line_idx, .. }) => {
            assert_eq!(note.text, "Line 1\n\nLine 2");
            assert_eq!(line_idx, 1);
        }
        other => panic!("expected VisualTarget::Note line 1, got {:?}", other),
    }
}

#[test]
fn test_mouse_click_diff_row_with_hunk_divider_selects_code_row() {
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());
    state.render_frame(120, 40);

    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let cw = state.last_content_width;
    let div_lines = state.hunk_div_seg_count(0, 0, cw);
    assert_eq!(div_lines, 1);

    // Click on the code row (after the hunk divider: starts[1] + div_lines)
    let code_vis = starts[1] + div_lines;
    let click_row = state.last_viewport_start + code_vis.saturating_sub(state.scroll);

    state.row_idx = 0;
    state.vis_line = 0;

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 10,
        shift: false,
    });

    assert_eq!(state.row_idx, 1);
    assert_eq!(state.vis_line, div_lines);
    assert!(
        matches!(
            state.visual_target_at(state.row_idx, state.vis_line),
            Some(crate::app::notes::VisualTarget::DiffRow { seg_idx: 0, .. })
        ),
        "visual target must be DiffRow, not HunkDivider"
    );
}

#[test]
fn test_no_diffs_renders_header_footer_and_main_message_frame() {
    let empty_output = PipelineOutput {
        files: Vec::new(),
        branch: "main".into(),
        untracked_skipped: Vec::new(),
    };
    let mut state = AppState::new(empty_output, "my_repo".into(), temp_notes_path());
    let frame = state.render_frame(120, 20);

    // Header bar with repo name
    assert!(frame.contains("my_repo"), "frame must include repo name");

    // Status bar with file 0/0 and hunk 0/0
    assert!(frame.contains("file 0/0"), "status bar must show file 0/0");
    assert!(frame.contains("hunk 0/0"), "status bar must show hunk 0/0");
    assert!(
        frame.contains('\u{2387}'),
        "status bar must show branch glyph"
    );
    assert!(frame.contains("main"), "status bar must show branch name");

    // Main area message
    assert!(
        frame.contains("No diff to display."),
        "main area must contain no diff message"
    );

    // Bottom ribbon footer
    assert!(frame.contains("───"), "footer must include divider");
    assert!(frame.contains("[j/k]move"), "footer must include [j/k]move");
    assert!(frame.contains("[q]uit"), "footer must include [q]uit");
    assert!(frame.contains("[?]help"), "footer must include [?]help");
}

#[test]
fn test_no_diffs_renders_header_footer_and_main_message_ratatui() {
    let empty_output = PipelineOutput {
        files: Vec::new(),
        branch: "feature-branch".into(),
        untracked_skipped: Vec::new(),
    };
    let mut state = AppState::new(empty_output, "demo_repo".into(), temp_notes_path());
    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let row_strings: Vec<String> = (0..20)
        .map(|y| {
            let mut line = String::new();
            for x in 0..120 {
                line.push_str(buffer[(x, y)].symbol());
            }
            line
        })
        .collect();

    // Row 0: Header bar
    assert!(
        row_strings[0].contains("demo_repo"),
        "Row 0 must be header bar: {}",
        row_strings[0]
    );

    // Row 1: Secondary status bar
    assert!(
        row_strings[1].contains("file 0/0") && row_strings[1].contains("hunk 0/0"),
        "Row 1 must be status bar: {}",
        row_strings[1]
    );

    // Row 2: Main area with message
    assert!(
        row_strings[2].contains("No diff to display."),
        "Row 2 must be main area with message: {}",
        row_strings[2]
    );

    // Row 18: Divider
    assert!(
        row_strings[18].contains("───"),
        "Row 18 must be divider: {}",
        row_strings[18]
    );

    // Row 19 (bottom): Ribbon with shortcuts
    assert!(
        row_strings[19].contains("[j/k]move")
            && row_strings[19].contains("[q]uit")
            && row_strings[19].contains("[?]help"),
        "Row 19 must be bottom ribbon: {}",
        row_strings[19]
    );
}

#[test]
fn test_app_commit_range_in_render_ratatui() {
    use ratatui::{Terminal, backend::TestBackend};

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    let output = PipelineOutput {
        files: vec![],
        branch: "work".into(),
        untracked_skipped: vec![],
    };

    let mut state = AppState::new(output, "demo_repo".into(), temp_notes_path())
        .with_commit_range(Some("3f05a71..7e0caab".into()));

    let frame = terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    let buffer = frame.buffer;

    // Commit range should appear in the header bar (row 0), not the status bar
    let mut row_0 = String::new();
    for x in 0..80 {
        row_0.push_str(buffer[(x, 0)].symbol());
    }
    assert!(
        row_0.contains("3f05a71..7e0caab"),
        "Header bar must show commit range: {}",
        row_0
    );
    assert!(
        row_0.contains("\u{2387}"),
        "Header bar must show branch glyph with commit range: {}",
        row_0
    );
    assert!(
        !row_0.contains("work"),
        "Header bar must NOT show branch name when commit range is set: {}",
        row_0
    );
}

#[test]
fn test_app_header_bar_shows_staged_badge() {
    let output = PipelineOutput {
        files: vec![],
        branch: "work".into(),
        untracked_skipped: vec![],
    };
    let target = diffuse_core::ReviewTarget::from_repo_args(
        diffuse_core::Repo::from_name("my_repo"),
        vec![],
        true,
    );
    let mut state = AppState::new(output, "my_repo".into(), temp_notes_path()).with_target(target);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    let frame = terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();
    let buffer = frame.buffer;
    let mut row_0 = String::new();
    for x in 0..80 {
        row_0.push_str(buffer[(x, 0)].symbol());
    }
    assert!(
        row_0.contains("work (staged)"),
        "Header bar must show 'work (staged)' when target is staged: {row_0}"
    );
}

#[test]
fn test_app_streaming_dot_blinking_and_pause() {
    let output = PipelineOutput {
        files: vec![],
        branch: "work".into(),
        untracked_skipped: vec![],
    };
    let mut state = AppState::new(output, "repo".into(), temp_notes_path());

    // When not streaming, dot is always "●"
    state.is_streaming = false;
    assert_eq!(state.streaming_dot(), "●");

    // When streaming, but no data received yet: steady dot "●"
    state.is_streaming = true;
    state.last_stream_activity = None;
    assert_eq!(state.streaming_dot(), "●");

    // When data is actively arriving (< 500ms):
    let t0 = std::time::Instant::now();
    state.start_time = t0;
    state.record_stream_activity_at(t0);

    // Active, phase 0 (0..200ms): dot is "●"
    let t_100ms = t0 + std::time::Duration::from_millis(100);
    assert_eq!(state.streaming_dot_at(t_100ms), "●");

    // Active, phase 1 (200..400ms): dot is " " (blinking off)
    let t_250ms = t0 + std::time::Duration::from_millis(250);
    assert_eq!(state.streaming_dot_at(t_250ms), " ");

    // Active, phase 2 (400..500ms): dot is "●" (blinking on)
    let t_450ms = t0 + std::time::Duration::from_millis(450);
    assert_eq!(state.streaming_dot_at(t_450ms), "●");

    // Delay reached (>= 500ms without new data): pauses blinking (steady dot
    // "●")
    let t_600ms = t0 + std::time::Duration::from_millis(600);
    assert_eq!(state.streaming_dot_at(t_600ms), "●");

    // New data arrives at 600ms!
    state.record_stream_activity_at(t_600ms);
    // Active again at 600ms..800ms: phase at 700ms is (700/200)%2 = 1 -> " "
    // (resumes blinking!)
    let t_700ms = t0 + std::time::Duration::from_millis(700);
    assert_eq!(state.streaming_dot_at(t_700ms), " ");
}

#[test]
fn test_viewport_line_origin_describes_file_and_row() {
    use crate::types::FlatRow;
    let mut state = AppState::new(sample_output(), "test_repo".into(), temp_notes_path());

    // Banner row of the only file.
    let desc = state.viewport_line_origin(Some(0));
    assert!(
        desc.contains("filesystem.rs") && desc.contains('0'),
        "must name the file and flat row, got: {desc}"
    );

    // Content row deeper in the file.
    let desc = state.viewport_line_origin(Some(3));
    assert!(
        desc.contains("filesystem.rs") && desc.contains('3'),
        "must name the file and flat row, got: {desc}"
    );

    // Out-of-range row.
    let oob = state.flat.len() + 10;
    let desc = state.viewport_line_origin(Some(oob));
    assert!(
        desc.contains("out of range"),
        "must flag out-of-range rows, got: {desc}"
    );

    // Padding filler lines carry no source.
    assert_eq!(state.viewport_line_origin(None), "viewport padding");
    // Flat row pointing at a missing file.
    state.flat.push(FlatRow {
        file_idx: 999,
        hunk_idx: 0,
        row_idx: 0,
        is_file_header: false,
        is_hunk_divider: false,
    });
    let ghost = state.flat.len() - 1;
    let desc = state.viewport_line_origin(Some(ghost));
    assert!(
        desc.contains("999"),
        "must flag the unknown file index, got: {desc}"
    );
}

#[test]
fn test_escape_ansi_for_diagnostic() {
    use super::render::escape_ansi_for_diagnostic;
    assert_eq!(
        escape_ansi_for_diagnostic("\x1b[38;5;220mhi\x1b[0m"),
        "\\x1b[38;5;220mhi\\x1b[0m"
    );
    assert_eq!(escape_ansi_for_diagnostic("plain"), "plain");
}

#[test]
fn test_ratatui_status_bar_uses_scrollbar_narrowed_width() {
    use ratatui::{Terminal, backend::TestBackend};

    // Long path plus enough hunks to force the scrollbar column on: the
    // status bar must target the narrowed app column, so the truncated
    // tail keeps its final character instead of clipping mid-name.
    let mut hunks = String::new();
    for h in 0..12 {
        hunks.push_str(&format!(
            "@@ -{},1 +{},1 @@\n-old line {}\n+new line {}\n",
            h * 10 + 1,
            h * 10 + 1,
            h,
            h
        ));
    }
    let diff = format!(
        "diff --git a/crates/diffuse-core/src/lib.rs b/crates/diffuse-core/src/lib.rs\n--- a/crates/diffuse-core/src/lib.rs\n+++ b/crates/diffuse-core/src/lib.rs\n{hunks}"
    );
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(out, "diffuse".into(), temp_notes_path());

    let backend = TestBackend::new(48, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let mut row = String::new();
    for x in 0..48 {
        row.push_str(buffer[(x, 1)].symbol());
    }
    assert!(
        row.contains("lib.rs"),
        "status tail must keep its final character: {row:?}"
    );
}

fn trailing_gap_output() -> PipelineOutput {
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,5 +1,6 @@
 // line 1
 // line 2
+// inserted
 // line 3
 // line 4
 // line 5
"#;
    let new_lines: Vec<String> = (1..=15)
        .map(|i| {
            if i == 3 {
                "// inserted".to_string()
            } else {
                format!("// line {i}")
            }
        })
        .collect();
    let old_lines: Vec<String> = (1..=14).map(|i| format!("// line {i}")).collect();
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    run_pipeline_full(
        diff,
        Some(contents),
        Some(old_contents),
        PipelineOptions::default(),
    )
}

#[test]
fn test_mouse_click_end_of_hunks_divider_selects() {
    let mut state = AppState::new(trailing_gap_output(), "test_repo".into(), temp_notes_path());
    state.render_frame(120, 40);

    let fi = 0;
    let gap = state.hunk_trailing_gap(fi);
    assert!(gap > 0, "must have trailing gap");

    let last_row = state.last_row_of_file(fi);
    let (starts, _, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let cw = state.last_content_width;
    let div_lines = if state.hunk_starts.iter().any(|hs| hs.flat == last_row) {
        state.hunk_div_seg_count(fi, 0, cw)
    } else {
        0
    };
    let row_segs = seg_counts[last_row];
    let note_lines = state.note_callouts(cw).get(&last_row).map_or(0, Vec::len);
    let bottom_vis = starts[last_row] + div_lines + row_segs + note_lines;
    let click_row = state.last_viewport_start + bottom_vis.saturating_sub(state.scroll);

    // Initial cursor is at top
    state.row_idx = 0;
    state.vis_line = 0;

    // Click on col 0 of the end of hunks divider (outside the ▲ arrow)
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 0,
        shift: false,
    });

    assert_eq!(
        state.row_idx, last_row,
        "cursor row must be last row of file"
    );
    assert_eq!(
        state.vis_line,
        div_lines + row_segs + note_lines,
        "vis_line must target the end of hunks divider"
    );
    assert!(
        matches!(
            state.visual_target_at(state.row_idx, state.vis_line),
            Some(crate::app::notes::VisualTarget::BottomGapDivider { file_idx: 0 })
        ),
        "visual target must be BottomGapDivider"
    );
}

#[test]
fn test_mouse_click_end_of_hunks_divider_arrow_expands() {
    let mut state = AppState::new(trailing_gap_output(), "test_repo".into(), temp_notes_path())
        .with_expand_lines(5);
    state.render_frame(120, 40);

    let fi = 0;
    let gap_before = state.hunk_trailing_gap(fi);
    assert!(gap_before > 0);

    let last_row = state.last_row_of_file(fi);
    let (starts, _, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let cw = state.last_content_width;
    let div_lines = if state.hunk_starts.iter().any(|hs| hs.flat == last_row) {
        state.hunk_div_seg_count(fi, 0, cw)
    } else {
        0
    };
    let row_segs = seg_counts[last_row];
    let note_lines = state.note_callouts(cw).get(&last_row).map_or(0, Vec::len);
    let bottom_vis = starts[last_row] + div_lines + row_segs + note_lines;
    let click_row = state.last_viewport_start + bottom_vis.saturating_sub(state.scroll);

    let txt = crate::view::diff_grid::bottom_gap_divider_text(gap_before);
    let arrow_col = txt.find("▲").map(|i| txt[..i].chars().count()).unwrap();

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: arrow_col,
        shift: false,
    });

    let gap_after = state.hunk_trailing_gap(fi);
    assert!(
        gap_after < gap_before,
        "clicking ▲ must expand trailing context (before: {gap_before}, after: {gap_after})"
    );
}

fn shrunk_eof_output() -> PipelineOutput {
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -5,6 +5,1 @@
 // line 5
-// line 6
-// line 7
-// line 8
-// line 9
-// line 10
"#;
    let new_lines: Vec<String> = (1..=5).map(|i| format!("// line {i}")).collect();
    let old_lines: Vec<String> = (1..=10).map(|i| format!("// line {i}")).collect();
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    run_pipeline_full(
        diff,
        Some(contents),
        Some(old_contents),
        PipelineOptions::default(),
    )
}

#[test]
fn test_hunk_trailing_gap_when_file_shrunk_at_eof() {
    let state = AppState::new(shrunk_eof_output(), "test_repo".into(), temp_notes_path());
    let fi = 0;
    assert_eq!(
        state.hunk_trailing_gap(fi),
        0,
        "shrunk file ending at EOF must have 0 trailing gap, not a phantom gap"
    );
}

fn shrunk_eof_context_row_output() -> PipelineOutput {
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,5 +1,2 @@
 // line 1
-// line 2
-// line 3
-// line 4
 // line 5
"#;
    let new_lines: Vec<String> = vec!["// line 1".into(), "// line 5".into()];
    let old_lines: Vec<String> = (1..=5).map(|i| format!("// line {i}")).collect();
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    run_pipeline_full(
        diff,
        Some(contents),
        Some(old_contents),
        PipelineOptions::default(),
    )
}

#[test]
fn test_hunk_trailing_gap_when_file_shrunk_with_eof_context_row() {
    let mut state = AppState::new(
        shrunk_eof_context_row_output(),
        "test_repo".into(),
        temp_notes_path(),
    );
    let fi = 0;
    assert_eq!(
        state.hunk_trailing_gap(fi),
        0,
        "shrunk file ending at EOF with context line must have 0 trailing gap, not a phantom gap"
    );
    assert!(
        !state.expand_hunk_below(fi, 0),
        "expand_hunk_below must return false when trailing gap is 0"
    );
}

#[test]
fn test_is_cursor_on_bottom_divider_and_enter_key() {
    let mut state = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
        .with_expand_lines(4);
    let fi = 0;
    let gap_before = state.hunk_trailing_gap(fi);
    assert!(gap_before > 0);

    // On file header (row 0), cursor is not on bottom divider
    state.row_idx = 0;
    state.vis_line = 0;
    assert_eq!(state.is_cursor_on_bottom_divider(), None);

    // On a content row, cursor is not on bottom divider
    state.row_idx = 1;
    state.vis_line = 0;
    assert_eq!(state.is_cursor_on_bottom_divider(), None);

    // Last row of file
    let last_row = state.last_row_of_file(fi);
    state.row_idx = last_row;
    state.vis_line = 0;
    // Row segment itself is not on bottom divider
    assert_eq!(state.is_cursor_on_bottom_divider(), None);

    // Position vis_line onto the bottom divider
    let cw = state.effective_content_width();
    let row_segs = state.row_seg_count(last_row, cw);
    state.vis_line = row_segs;
    assert_eq!(state.is_cursor_on_bottom_divider(), Some(fi));

    // Raw diff mode returns None even if vis_line is set
    state.raw_diff = true;
    assert_eq!(state.is_cursor_on_bottom_divider(), None);
    state.raw_diff = false;
    assert_eq!(state.is_cursor_on_bottom_divider(), Some(fi));

    // Pressing Enter (ToggleCollapseFile(None)) expands the bottom gap
    state.execute(TuiCommand::ToggleCollapseFile(None));
    let gap_after = state.hunk_trailing_gap(fi);
    assert_eq!(gap_after, gap_before.saturating_sub(4));
}

fn two_hunk_gap_output() -> PipelineOutput {
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -1,8 +1,9 @@
 // line 1
 // line 2
 // line 3
 // line 4
+// inserted one
 // line 5
 // line 6
 // line 7
 // line 8
@@ -50,8 +51,9 @@
 // line 50
 // line 51
 // line 52
 // line 53
+// inserted two
 // line 54
 // line 55
 // line 56
 // line 57
"#;
    let new_lines: Vec<String> = (1..=70)
        .map(|i| {
            if i == 5 {
                "// inserted one".to_string()
            } else if i == 55 {
                "// inserted two".to_string()
            } else {
                format!("// line {i}")
            }
        })
        .collect();
    let old_lines: Vec<String> = (1..=68).map(|i| format!("// line {i}")).collect();
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    run_pipeline_full(
        diff,
        Some(contents),
        Some(old_contents),
        PipelineOptions::default(),
    )
}

#[test]
fn test_hunk_bar_click_pivots_and_retains_cursor_visual_position() {
    let mut state = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
        .with_expand_lines(4);
    state.render_frame(120, 16);
    state.scroll = 2;

    let fi = 0;
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;
    let div_vis = starts[div_flat];
    let div_screen_y = div_vis.saturating_sub(state.scroll);
    let click_row = state.last_viewport_start + div_screen_y;

    let h = &state.output.files[fi].hunks[1];
    let hidden_gap = state.hunk_leading_gap(fi, 1);
    let txt = hunk_divider_text(Badge { idx: 1, total: 2 }, h, hidden_gap);
    let col = txt.find("▲").map(|i| txt[..i].chars().count()).unwrap();

    // 1. Cursor is BEFORE the hunk bar (e.g. line 3 in hunk 0)
    let line_3_flat = state
        .flat
        .iter()
        .position(|fr| fr.file_idx == fi && state.row_of(fr).is_some_and(|r| r.new_line == Some(3)))
        .unwrap();
    state.row_idx = line_3_flat;
    state.vis_line = 0;
    let cursor_screen_before = state.cursor_visual_line().saturating_sub(state.scroll);

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col,
        shift: false,
    });

    let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
    let div_flat_after = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;
    let div_screen_after = starts_after[div_flat_after].saturating_sub(state.scroll);
    assert_eq!(
        div_screen_after, div_screen_y,
        "hunk bar must retain its visual screen position"
    );

    let cursor_screen_after = state.cursor_visual_line().saturating_sub(state.scroll);
    assert_eq!(
        cursor_screen_after, cursor_screen_before,
        "cursor must retain its visual screen position when before bar"
    );

    // 2. Cursor is AFTER the hunk bar (e.g. line 52 in hunk 1)
    let line_52_flat = state
        .flat
        .iter()
        .position(|fr| {
            fr.file_idx == fi && state.row_of(fr).is_some_and(|r| r.new_line == Some(52))
        })
        .unwrap();
    state.row_idx = line_52_flat;
    state.vis_line = 0;
    let cursor_screen_before2 = state.cursor_visual_line().saturating_sub(state.scroll);

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col,
        shift: false,
    });

    let (starts_after2, _, _, _) = state.visual_geometry(state.last_content_width);
    let div_flat_after2 = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;
    let div_screen_after2 = starts_after2[div_flat_after2].saturating_sub(state.scroll);
    assert_eq!(
        div_screen_after2, div_screen_y,
        "hunk bar must retain its visual screen position"
    );

    let cursor_screen_after2 = state.cursor_visual_line().saturating_sub(state.scroll);
    assert_eq!(
        cursor_screen_after2, cursor_screen_before2,
        "cursor must retain its visual screen position when after bar"
    );

    // 3. Cursor is ON the hunk bar
    state.row_idx = div_flat_after2;
    state.vis_line = 0;
    let cursor_screen_before3 = state.cursor_visual_line().saturating_sub(state.scroll);

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col,
        shift: false,
    });

    let cursor_screen_after3 = state.cursor_visual_line().saturating_sub(state.scroll);
    assert_eq!(
        cursor_screen_after3, cursor_screen_before3,
        "cursor must retain its visual screen position when on bar"
    );
    let current_div_flat = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 1)
        .unwrap()
        .flat;
    assert_eq!(
        state.row_idx, current_div_flat,
        "cursor must remain on the hunk bar"
    );
}

#[test]
fn test_bottom_divider_click_pivots_and_retains_cursor_visual_position() {
    let mut state = AppState::new(trailing_gap_output(), "test_repo".into(), temp_notes_path())
        .with_expand_lines(4);
    state.render_frame(120, 8);
    state.scroll = 1;

    let fi = 0;
    let gap_before = state.hunk_trailing_gap(fi);
    assert!(gap_before > 0);

    let last_row = state.last_row_of_file(fi);
    let (starts, _, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let cw = state.last_content_width;
    let div_lines = if state.hunk_starts.iter().any(|hs| hs.flat == last_row) {
        state.hunk_div_seg_count(fi, 0, cw)
    } else {
        0
    };
    let row_segs = seg_counts[last_row];
    let note_lines = state.note_callouts(cw).get(&last_row).map_or(0, Vec::len);
    let bottom_vis = starts[last_row] + div_lines + row_segs + note_lines;
    let bottom_screen_y = bottom_vis.saturating_sub(state.scroll);
    let click_row = state.last_viewport_start + bottom_screen_y;

    let txt = crate::view::diff_grid::bottom_gap_divider_text(gap_before);
    let arrow_col = txt.find("▲").map(|i| txt[..i].chars().count()).unwrap();

    // 1. Cursor before bottom divider (e.g. line 2)
    state.row_idx = 1;
    state.vis_line = 0;
    let cursor_screen_before = state.cursor_visual_line().saturating_sub(state.scroll);

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: arrow_col,
        shift: false,
    });

    let last_row_after = state.last_row_of_file(fi);
    let (starts_after, _, seg_counts_after, _) = state.visual_geometry(state.last_content_width);
    let div_lines_after = if state.hunk_starts.iter().any(|hs| hs.flat == last_row_after) {
        state.hunk_div_seg_count(fi, 0, cw)
    } else {
        0
    };
    let bottom_vis_after =
        starts_after[last_row_after] + div_lines_after + seg_counts_after[last_row_after];
    let bottom_screen_after = bottom_vis_after.saturating_sub(state.scroll);
    assert_eq!(
        bottom_screen_after, bottom_screen_y,
        "bottom divider must retain its visual screen position"
    );

    let cursor_screen_after = state.cursor_visual_line().saturating_sub(state.scroll);
    assert_eq!(
        cursor_screen_after, cursor_screen_before,
        "cursor must retain its visual screen position when expanding bottom divider"
    );
}

#[test]
fn test_hunk_bar_click_retains_cursor_visual_position_outside_screen_above_and_below() {
    let fi = 0;

    // 1. Cursor is ABOVE the screen (scrolled off top)
    {
        let mut state = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
            .with_expand_lines(4);
        state.render_frame(120, 16);
        state.scroll = 6;

        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let div_flat = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;
        let div_vis = starts[div_flat];
        let div_screen_y = (div_vis as isize) - (state.scroll as isize);
        let click_row = (state.last_viewport_start as isize + div_screen_y) as usize;

        let h = &state.output.files[fi].hunks[1];
        let hidden_gap = state.hunk_leading_gap(fi, 1);
        let txt = hunk_divider_text(Badge { idx: 1, total: 2 }, h, hidden_gap);
        let col = txt.find("▲").map(|i| txt[..i].chars().count()).unwrap();

        // Park cursor on line 1 (starts[1] == 1). With scroll == 6, rel_y = 1 -
        // 6 = -5.
        state.row_idx = 1;
        state.vis_line = 0;
        let cursor_rel_before = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert_eq!(
            cursor_rel_before, -5,
            "cursor should be 5 lines above screen"
        );

        state.execute(TuiCommand::MouseClick {
            row: click_row,
            col,
            shift: false,
        });

        let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
        let div_flat_after = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;
        let div_screen_after = (starts_after[div_flat_after] as isize) - (state.scroll as isize);
        assert_eq!(
            div_screen_after, div_screen_y,
            "hunk bar must retain its screen position"
        );

        let cursor_rel_after = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert_eq!(
            cursor_rel_after, cursor_rel_before,
            "cursor must retain its visual position when above screen"
        );
    }

    // 2. Cursor is BELOW the screen (scrolled past bottom)
    {
        let mut state = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
            .with_expand_lines(4);
        state.render_frame(120, 16);
        state.scroll = 2;

        let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
        let div_flat = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;
        let div_vis = starts[div_flat];
        let div_screen_y = (div_vis as isize) - (state.scroll as isize);
        let click_row = (state.last_viewport_start as isize + div_screen_y) as usize;

        let h = &state.output.files[fi].hunks[1];
        let hidden_gap = state.hunk_leading_gap(fi, 1);
        let txt = hunk_divider_text(Badge { idx: 1, total: 2 }, h, hidden_gap);
        let col = txt.find("▲").map(|i| txt[..i].chars().count()).unwrap();

        // Park cursor on last row in hunk 1. starts[last] is > 16. With scroll
        // == 2, rel_y > 16 (below screen).
        let last_hunk_row = state
            .flat
            .iter()
            .position(|fr| {
                fr.file_idx == fi && state.row_of(fr).is_some_and(|r| r.new_line == Some(57))
            })
            .unwrap();
        state.row_idx = last_hunk_row;
        state.vis_line = 0;
        let cursor_rel_before = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert!(
            cursor_rel_before >= 16,
            "cursor should be below screen (>= vh=16)"
        );

        state.execute(TuiCommand::MouseClick {
            row: click_row,
            col,
            shift: false,
        });

        let (starts_after, _, _, _) = state.visual_geometry(state.last_content_width);
        let div_flat_after = state
            .hunk_starts
            .iter()
            .find(|hs| hs.file == fi && hs.hunk == 1)
            .unwrap()
            .flat;
        let div_screen_after = (starts_after[div_flat_after] as isize) - (state.scroll as isize);
        assert_eq!(
            div_screen_after, div_screen_y,
            "hunk bar must retain its screen position"
        );

        let cursor_rel_after = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert_eq!(
            cursor_rel_after, cursor_rel_before,
            "cursor must retain its visual position when below screen"
        );
    }
}

#[test]
fn test_collapsing_hunk_retains_cursor_visual_position_on_screen_and_outside_screen() {
    let fi = 0;

    // 1. Cursor ON SCREEN when collapsing
    {
        let mut state = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
            .with_expand_lines(8);
        state.render_frame(120, 20);
        state.expand_hunk_below(fi, 0);

        state.scroll = 4;
        state.row_idx = 10;
        state.vis_line = 0;
        let cursor_rel_before = (state.cursor_visual_line() as isize) - (state.scroll as isize);

        state.execute(TuiCommand::ResetCurrentHunkContext);

        let cursor_rel_after = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert_eq!(
            cursor_rel_after, cursor_rel_before,
            "collapsing hunk must retain cursor visual position on screen"
        );
    }

    // 2. Cursor ABOVE SCREEN when collapsing
    {
        let mut state = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
            .with_expand_lines(8);
        state.render_frame(120, 10);
        state.expand_hunk_below(fi, 0);

        state.scroll = 10;
        state.row_idx = 2; // visual line ~2, scroll=10 -> rel_y ~ -8
        state.vis_line = 0;
        let cursor_rel_before = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert!(cursor_rel_before < 0, "cursor must be above screen");

        state.execute(TuiCommand::ResetCurrentHunkContext);

        let cursor_rel_after = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert_eq!(
            cursor_rel_after, cursor_rel_before,
            "collapsing hunk must retain cursor visual position above screen"
        );
    }

    // 3. Cursor BELOW SCREEN when collapsing
    {
        let mut state = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
            .with_expand_lines(8);
        state.render_frame(120, 16);
        state.expand_hunk_below(fi, 0);

        state.scroll = 2;
        let last_hunk_row = state
            .flat
            .iter()
            .position(|fr| {
                fr.file_idx == fi && state.row_of(fr).is_some_and(|r| r.new_line == Some(57))
            })
            .unwrap();
        state.row_idx = last_hunk_row;
        state.vis_line = 0;
        let cursor_rel_before = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert!(cursor_rel_before >= 16, "cursor must be below screen");

        state.execute(TuiCommand::ResetCurrentHunkContext);

        let cursor_rel_after = (state.cursor_visual_line() as isize) - (state.scroll as isize);
        assert_eq!(
            cursor_rel_after, cursor_rel_before,
            "collapsing hunk must retain cursor visual position below screen"
        );
    }
}

#[test]
fn test_expand_below_cursor_above_divider() {
    let fi = 0;

    // Verify cursor_visual_line exactly matches compute_geometry layout.
    let base_state = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path());
    for test_row in 0..base_state.flat.len() {
        let mut s = AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path());
        s.render_frame(120, 25);
        s.row_idx = test_row;
        s.vis_line = 0;
        let cv = s.cursor_visual_line();
        let (starts, _, _, _) = s.visual_geometry(s.last_content_width);
        let expected = starts[test_row] + s.vis_line;
        assert_eq!(
            cv, expected,
            "cursor_visual_line mismatch at row {test_row}: cv={cv}, expected={expected}"
        );
    }

    for test_row in 0..base_state.flat.len() {
        // Test A: expand_hunk_below API retains visual cursor position
        {
            let mut state =
                AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
                    .with_expand_lines(4);
            state.render_frame(120, 16);
            state.scroll = 2;
            state.row_idx = test_row;
            state.vis_line = 0;
            let cursor_vis_before = state.cursor_visual_line();
            let cursor_rel_before = (cursor_vis_before as isize) - (state.scroll as isize);

            state.expand_hunk_below(fi, 0);
            state.render_frame(120, 16);

            let cursor_vis_after = state.cursor_visual_line();
            let cursor_rel_after = (cursor_vis_after as isize) - (state.scroll as isize);
            assert_eq!(
                cursor_rel_after, cursor_rel_before,
                "expand_hunk_below: cursor visual position must be retained for row {test_row}"
            );
        }

        // Test B: click ▼ (expand context below divider) retains visual cursor
        // position
        {
            let mut state =
                AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
                    .with_expand_lines(4);
            state.render_frame(120, 16);
            state.scroll = 2;
            state.row_idx = test_row;
            state.vis_line = 0;
            let cursor_vis_before = state.cursor_visual_line();
            let cursor_rel_before = (cursor_vis_before as isize) - (state.scroll as isize);

            let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
            let div_flat = state
                .hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == 1)
                .unwrap()
                .flat;
            let div_vis = starts[div_flat];
            let div_screen_y = (div_vis as isize) - (state.scroll as isize);
            let click_row = (state.last_viewport_start as isize + div_screen_y) as usize;

            let h = &state.output.files[fi].hunks[1];
            let hidden_gap = state.hunk_leading_gap(fi, 1);
            let txt = hunk_divider_text(Badge { idx: 1, total: 2 }, h, hidden_gap);
            let col = txt.find("▼").map(|i| txt[..i].chars().count()).unwrap();

            state.execute(TuiCommand::MouseClick {
                row: click_row,
                col,
                shift: false,
            });
            state.render_frame(120, 16);

            let cursor_vis_after = state.cursor_visual_line();
            let cursor_rel_after = (cursor_vis_after as isize) - (state.scroll as isize);
            assert_eq!(
                cursor_rel_after, cursor_rel_before,
                "click ▼: cursor visual position must be retained for row {test_row}"
            );
        }

        // Test C: click ▲ on divider 1 retains visual cursor position
        {
            let mut state =
                AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
                    .with_expand_lines(4);
            state.render_frame(120, 16);
            state.scroll = 2;
            state.row_idx = test_row;
            state.vis_line = 0;
            let cursor_vis_before = state.cursor_visual_line();
            let cursor_rel_before = (cursor_vis_before as isize) - (state.scroll as isize);

            let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
            let div_flat = state
                .hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == 1)
                .unwrap()
                .flat;
            let div_vis = starts[div_flat];
            let div_screen_y = (div_vis as isize) - (state.scroll as isize);
            let click_row = (state.last_viewport_start as isize + div_screen_y) as usize;

            let h = &state.output.files[fi].hunks[1];
            let hidden_gap = state.hunk_leading_gap(fi, 1);
            let txt = hunk_divider_text(Badge { idx: 1, total: 2 }, h, hidden_gap);
            let col = txt.find("▲").map(|i| txt[..i].chars().count()).unwrap();

            state.execute(TuiCommand::MouseClick {
                row: click_row,
                col,
                shift: false,
            });
            state.render_frame(120, 16);

            let cursor_vis_after = state.cursor_visual_line();
            let cursor_rel_after = (cursor_vis_after as isize) - (state.scroll as isize);
            assert_eq!(
                cursor_rel_after, cursor_rel_before,
                "click ▲: cursor visual position must be retained for row {test_row}"
            );
        }

        // Test D: click bottom divider retains visual cursor position
        {
            let mut state =
                AppState::new(two_hunk_gap_output(), "test_repo".into(), temp_notes_path())
                    .with_expand_lines(4);
            state.render_frame(120, 16);
            state.scroll = 10;
            state.row_idx = test_row;
            state.vis_line = 0;
            let cursor_vis_before = state.cursor_visual_line();
            let cursor_rel_before = (cursor_vis_before as isize) - (state.scroll as isize);

            let (starts, _, seg_counts, _) = state.visual_geometry(state.last_content_width);
            let callouts = state.note_callouts(state.last_content_width);
            let div_vis = state
                .bottom_divider_vis_line_from_layout(
                    fi,
                    state.last_content_width,
                    &starts,
                    &seg_counts,
                    &callouts,
                )
                .unwrap();
            let div_screen_y = (div_vis as isize) - (state.scroll as isize);
            let click_row = (state.last_viewport_start as isize + div_screen_y) as usize;

            let gap = state.hunk_trailing_gap(fi);
            let txt = crate::view::diff_grid::bottom_gap_divider_text(gap);
            let col = txt.find("▲").map(|i| txt[..i].chars().count()).unwrap();

            state.execute(TuiCommand::MouseClick {
                row: click_row,
                col,
                shift: false,
            });
            state.render_frame(120, 16);

            let cursor_vis_after = state.cursor_visual_line();
            let cursor_rel_after = (cursor_vis_after as isize) - (state.scroll as isize);
            assert_eq!(
                cursor_rel_after, cursor_rel_before,
                "click bottom ▲: cursor visual position must be retained for row {test_row}"
            );
        }
    }
}
