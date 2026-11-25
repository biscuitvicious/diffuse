use std::{
    collections::HashMap,
    fs,
    path::Path,
    time::{Duration, Instant},
};

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
};

fn sample_output() -> PipelineOutput {
    let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,3 +1025,4 @@ fn test() {
     let x = 1;
-    // old comment — em dash
+    // new comment — em dash updated
+    let y = 2;
 }
"#;
    let prefix: String = (1..1025).map(|i| format!("line {i}\n")).collect();
    let new_content = [(
        "filesystem.rs".to_string(),
        format!(
            "{prefix}    let x = 1;\n    // new comment — em dash updated\n    let y = 2;\n}}\n"
        ),
    )]
    .into_iter()
    .collect();
    let old_content = [(
        "filesystem.rs".to_string(),
        format!("{prefix}    let x = 1;\n    // old comment — em dash\n}}\n"),
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

/// Primary display number of a selection bound (new side preferred).
fn coord_num(c: diffuse_core::notes::DiffCoord) -> u32 {
    c.new_line_num
        .or(c.old_line_num)
        .map(|n| n.get())
        .unwrap_or(0)
}

#[test]
fn test_with_theme_builder_sets_theme() {
    let state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_theme(crate::render::DiffuseTheme::dark());
    assert_eq!(state.theme, crate::render::DiffuseTheme::dark());
}

#[test]
fn test_clicking_file_header_triangle_toggles_collapse() {
    let diff = r#"diff --git a/filesystem.rs b/filesystem.rs
index 1111111..2222222 100644
--- a/filesystem.rs
+++ b/filesystem.rs
@@ -1025,3 +1025,4 @@ fn test() {
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
        crate::render::DiffuseTheme::dark(),
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
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path.clone(),
        crate::render::DiffuseTheme::dark(),
    );

    let file = state.current_file().unwrap().path.clone();
    state.note_draft = Some("check this **carefully**".into());
    state.commit_note_draft().expect("note commit must succeed");
    assert!(state.note_draft.is_none());

    let notes: Vec<_> = notes_for_file(&state.notes, "test_repo", "", &file).collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, "check this **carefully**");
    assert!(
        notes[0].start() > 0,
        "note must anchor to a real row coordinate"
    );

    let on_disk = load_notes(&notes_path);
    assert_eq!(
        notes_for_file(&on_disk, "test_repo", "", &file).count(),
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
        notes_for_file(&state.notes, "test_repo", "", &file)
            .next()
            .is_none(),
        "delete must remove the note from the DB"
    );
    let on_disk2 = load_notes(&notes_path);
    assert!(
        notes_for_file(&on_disk2, "test_repo", "", &file)
            .next()
            .is_none(),
        "delete must persist the removal"
    );
}

#[test]
fn test_help_modal_toggles_and_renders() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    assert!(with_help.contains("page down"));
}

#[test]
fn test_selection_anchor_span_snaps_to_coord_rows() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    assert!(
        coord_num(sel.start) < coord_num(sel.end),
        "range must span two rows"
    );
    assert_eq!(coord_num(sel.start), 1025);
    assert_eq!(coord_num(sel.end), 1027);
    assert!(
        sel.start.new_line_num.is_some() && sel.end.new_line_num.is_some(),
        "a range led by a new-side row carries new-side numbers on both bounds"
    );
}

#[test]
fn test_selection_v_confirm_clears_anchor_and_band_renders() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    assert_eq!(coord_num(sel.start), 1025);
    assert_eq!(coord_num(sel.end), 1027);

    let frame2 = state.render_frame(100, 30);
    assert!(
        frame2.contains("\x1b[48;5;235m"),
        "a confirmed selection must keep banding its coordinate range"
    );
}

#[test]
fn test_selection_requires_coord_rows() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.row_idx = 0;
    state.vis_line = 0;
    state.start_or_confirm_selection();
    state.move_down();
    let sel = state
        .compute_selection()
        .expect("anchor over the header must snap to a coordinate row");
    assert_eq!(
        coord_num(sel.start),
        1022,
        "snap to the first padded ctx row"
    );
    assert_eq!(coord_num(sel.end), 1022);
}

#[test]
fn test_raw_diff_uncollapses_mod_and_renders() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
                .old_text()
                .contains("old comment"),
        "the mod pair must un-collapse into a - line"
    );
    let add = state.flat.iter().find(|fr| {
        state
            .row_of(fr)
            .is_some_and(|r| r.kind == RowKind::Add && r.new_text().contains("new comment"))
    });
    assert!(add.is_some(), "the mod pair must un-collapse into a + line");

    let sep = state.flat.iter().find(|fr| fr.is_hunk_divider());
    assert!(sep.is_some());

    let frame = state.render_frame(120, 40);
    assert!(frame.contains("@@ -1025,3 +1025,4 @@"), "raw @@ header");
    // Raw markers render in kind colors. Asserted on visible text plus
    // live color, not byte adjacency (the optimizer drops redundant
    // repeats, carrying the color forward instead).
    for (marker, color) in [("-", "\x1b[38;5;203m"), ("+", "\x1b[38;5;120m")] {
        let raw_line = frame
            .lines()
            .find(|l| crate::render::ansi::strip_ansi(l).contains(marker) && l.contains("comment"))
            .unwrap_or_else(|| panic!("raw {marker} marker line must render"));
        assert!(
            raw_line.contains(color),
            "raw {marker} lines render {color} marker and text: {raw_line:?}"
        );
    }
}

#[test]
fn test_raw_diff_toggle_anchors_cursor() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.toggle_raw_diff();
    assert!(state.raw_diff);

    let kinds: Vec<RowKind> = state
        .flat
        .iter()
        .filter(|r| !r.is_file_header() && !r.is_hunk_divider())
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
        "diff --git a/test.rs b/test.rs\n--- a/test.rs\n+++ b/test.rs\n@@ -1,50 +1,50 @@\n",
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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 20;
    state.toggle_raw_diff();
    let frame = state.render_frame(80, 20);
    let lines: Vec<&str> = frame.lines().collect();

    // Check the viewport rows (excluding header/ribbon) do not draw a scrollbar
    for line in &lines[2..lines.len() - 2] {
        assert!(
            !line.contains('█') && !line.contains('│'),
            "raw diff must not draw scrollbar"
        );
    }
}

#[test]
fn test_raw_diff_wrapped_line_no_false_trailing_highlight() {
    let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,0 +1,1 @@
+6. Segment Rule: Each upper captures the contiguous run of read-only layers directly beneath it.
"#;
    let new_content = [(
            "test.rs".to_string(),
            "6. Segment Rule: Each upper captures the contiguous run of read-only layers directly beneath it.\n".to_string(),
        )]
        .into_iter()
        .collect();
    let out = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
@@ -80,6 +86,6 @@
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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
        !current_row.is_file_header(),
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
@@ -76,57 +82,104 @@ impl ResolutionFingerprint {
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
    let mut state = AppState::new(
        out,
        "mirage".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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

fn load_opt1_wire_output() -> PipelineOutput {
    let base_dir = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/opt-1-cache-regex-window-slice"
    ));
    let wire_path = base_dir.join("expect/wire.jsonl");
    let jsonl_str = fs::read_to_string(wire_path).expect("must read opt-1 wire.jsonl");
    let mut files = crate::core::wire::jsonl::decode_jsonl_str(&jsonl_str).unwrap();
    let fixture_new = base_dir.join("fixture/new");
    for f in &mut files {
        let p = fixture_new.join(&f.path);
        if let Ok(content) = fs::read_to_string(&p) {
            f.new_content = content.lines().map(String::from).collect();
        }
    }
    PipelineOutput {
        files,
        branch: String::new(),
        untracked_skipped: vec![],
    }
}

#[test]
fn test_separator_expand_by_enter_and_click_and_merge() {
    let output = load_opt1_wire_output();

    let mut state = AppState::new(
        output,
        "opt_1".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_expand_lines(6);

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
        frame.contains("──── 2/"),
        "H2 divider must still exist before fully merged"
    );
    assert!(
        frame.contains("more lines"),
        "H2 divider must reflect hidden gap"
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
    let output = load_opt1_wire_output();

    let mut state = AppState::new(
        output,
        "opt_1".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_expand_lines(6);

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

    let (_, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let div_segs = counts[h2_hunk_start.flat].saturating_sub(seg_counts[h2_hunk_start.flat]);
    let arrow_screen_row =
        state.last_viewport_start + (h2_div_vis + div_segs).saturating_sub(state.scroll);

    // Click on the gutter '▲' column (col 0)
    state.execute(TuiCommand::MouseClick {
        row: arrow_screen_row,
        col: 0,
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
@@ -1,3 +1,3 @@
 fn h1() {
-    let x = 1;
+    let x = 10;
 }
@@ -30,3 +30,3 @@
 fn h2() {
-    let y = 2;
+    let y = 20;
 }
@@ -60,3 +60,3 @@
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
    let new_content: HashMap<String, String> = [("cache.rs".to_string(), new_lines.join("\n"))]
        .into_iter()
        .collect();
    let output = run_pipeline_full(diff, Some(new_content), None, PipelineOptions::default());

    let mut state = AppState::new(
        output,
        "mirage".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_expand_lines(6);

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
        .position(|fr| fr.file_idx == fi && !fr.is_file_header())
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
    assert!(frame.contains("──── 1/3"));
    assert!(frame.contains("──── 2/3"));
    assert!(frame.contains("──── 3/3"));
}

#[test]
fn test_h1_divider_down_arrow_click_expands_top_and_stays_in_view() {
    let diff = r#"diff --git a/TODO.md b/TODO.md
index 1111111..2222222 100644
--- a/TODO.md
+++ b/TODO.md
@@ -10,3 +10,5 @@
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
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    let fi = 0;
    let gap = state.hunk_leading_gap(fi, 0);
    assert_eq!(gap, 6, "6 lines hidden before H1");

    // Render initial frame with small viewport (14 rows)
    let frame = state.render_frame(80, 14);
    assert!(frame.contains("6 more lines"));

    // Click up arrow on H1 first row gutter
    let (starts, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let h1_start = state
        .hunk_starts
        .iter()
        .find(|hs| hs.file == fi && hs.hunk == 0)
        .unwrap();
    let h1_div_vis = starts[h1_start.flat];
    let div_segs = counts[h1_start.flat].saturating_sub(seg_counts[h1_start.flat]);
    let click_row =
        state.last_viewport_start + (h1_div_vis + div_segs).saturating_sub(state.scroll);

    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 0,
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
        frame_after.contains("──── 1/1"),
        "H1 divider must remain visible on screen"
    );
    assert!(
        frame_after.contains("// line 1"),
        "expanded line 1 must be visible on screen"
    );
}

#[test]
fn test_single_line_hunk_wrapped_and_unwrapped_gutter_icons() {
    let diff = r#"diff --git a/sample.rs b/sample.rs
index 1111111..2222222 100644
--- a/sample.rs
+++ b/sample.rs
@@ -5,1 +5,1 @@
-old line 5 content that is very long and will wrap nicely across multiple segments on narrow terminals
+new line 5 content that is very long and will wrap nicely across multiple segments on narrow terminals
"#;
    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();
    for i in 1..=20 {
        if i == 5 {
            old_lines.push(
                "old line 5 content that is very long and will wrap nicely across multiple segments on narrow terminals".to_string(),
            );
            new_lines.push(
                "new line 5 content that is very long and will wrap nicely across multiple segments on narrow terminals".to_string(),
            );
        } else {
            old_lines.push(format!("// line {i}"));
            new_lines.push(format!("// line {i}"));
        }
    }
    let contents = [("sample.rs".to_string(), new_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let old_contents = [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
        .into_iter()
        .collect();
    let opts_u0 = PipelineOptions {
        visual_context: 0,
        ..Default::default()
    };
    let output = PipelineInput::new(diff)
        .with_new_content(contents)
        .with_old_content(old_contents)
        .with_options(opts_u0)
        .run();

    // 1. Unwrapped: wide screen (140 cols). Single-line hunk collapses icon to ⇕
    let mut state = AppState::new(
        output.clone(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let frame_wide = state.render_frame(140, 30);
    assert!(
        frame_wide.contains('⇕'),
        "unwrapped 1-line hunk with context in both directions must display ⇕"
    );

    // Clicking ⇕ (col 0) expands both directions
    let (starts, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let hunk_start = state.hunk_starts[0].flat;
    let div_segs = counts[hunk_start].saturating_sub(seg_counts[hunk_start]);
    let click_row = state.last_viewport_start + (starts[hunk_start] + div_segs);
    let gap_above_pre = state.hunk_leading_gap(0, 0);
    let gap_below_pre = state.hunk_trailing_gap(0);
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 0,
        shift: false,
    });
    assert!(
        state.hunk_leading_gap(0, 0) < gap_above_pre,
        "clicking ⇕ must expand above"
    );
    assert!(
        state.hunk_trailing_gap(0) < gap_below_pre,
        "clicking ⇕ must expand below"
    );

    // 2. Wrapped: narrow screen (40 cols). Single-line hunk with wrapped segments
    // displays ⇑ on segment 0 and ⇓ on final segment.
    let mut state_narrow = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let frame_narrow = state_narrow.render_frame(40, 30);
    assert!(
        frame_narrow.contains('⇑'),
        "wrapped 1-line hunk must display ⇑ on segment 0"
    );
    assert!(
        frame_narrow.contains('⇓'),
        "wrapped 1-line hunk must display ⇓ on final segment"
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
    // Narrow: the divider (~33 chars with the gap tail) wraps to 2 segs.
    let (w, h) = (24, 16);
    let mut state = AppState::new(
        output.clone(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
        let mut probe = AppState::new(
            state.output.clone(),
            "test_repo".into(),
            temp_notes_path(),
            crate::render::DiffuseTheme::dark(),
        );
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
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
fn test_scrollbar_drawn_column_matches_hit_column() {
    // The bar glyph must sit exactly in the column the click hit-test
    // uses (`last_content_width`), across the whole viewport.
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,60 @@\n-old\n");
    for i in 1..=60 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    let (counts, _, _, total) = state.visual_geometry(cw);
    let header_vis = counts.first().copied().unwrap_or(1);
    let max_scroll = total.saturating_sub(vh);

    // Clicking the track seeks exactly according to the formula: the mapping
    // uses the pointer row on the full-height scrollbar track.
    let (thumb_off, _) = AppState::scrollbar_thumb_for(track_h, vh, total, scrolled, header_vis)
        .expect("scrollbar must apply");
    let track_row = if thumb_off > 2 { 1 } else { track_h - 2 };
    let want = AppState::scrollbar_scroll_for_row(track_h, vh, total, header_vis, track_row);
    state.execute(TuiCommand::MouseClick {
        row: track_row,
        col: cw,
        shift: false,
    });
    assert!(
        state.is_dragging_scrollbar,
        "bar-column click must start a drag"
    );
    assert_eq!(
        state.scroll, want,
        "bar-column click on empty track must seek to the pointer row"
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
fn test_scrollbar_click_on_thumb_does_not_snap() {
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,60 @@\n-old\n");
    for i in 1..=60 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.render_frame(80, 30);
    let vh = state.viewport_height;
    let track_h = state.screen_height;
    state.scroll_to_visual(20);
    state.render_frame(80, 30);
    let scrolled = state.scroll;
    assert_eq!(scrolled, 20);
    let cw = state.last_content_width;
    let (counts, _, _, total) = state.visual_geometry(cw);
    let header_vis = counts.first().copied().unwrap_or(1);
    let (thumb_off, thumb_size) =
        AppState::scrollbar_thumb_for(track_h, vh, total, scrolled, header_vis)
            .expect("scrollbar must apply");
    assert!(thumb_size >= 3, "need thumb size of at least 3 rows");

    // Click near the bottom of the thumb (low on the thumb)
    let click_row = thumb_off + thumb_size - 1;
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: cw,
        shift: false,
    });

    assert!(
        state.is_dragging_scrollbar,
        "click must start scrollbar drag"
    );
    assert_eq!(
        state.scrollbar_drag_grab,
        Some(thumb_size - 1),
        "grab offset must record distance from thumb top"
    );
    assert_eq!(
        state.scroll, 20,
        "clicking on the thumb must not adjust scroll position (no snap)"
    );

    // Dragging down by 1 row moves thumb by 1 offset
    state.execute(TuiCommand::MouseDrag {
        row: click_row + 1,
        col: cw,
        shift: false,
    });
    let want_offset = thumb_off + 1;
    let want_scroll =
        AppState::scrollbar_scroll_for_offset(track_h, vh, total, header_vis, want_offset);
    assert_eq!(
        state.scroll, want_scroll,
        "dragging must move relative to grab offset"
    );

    // Releasing clears drag and grab state
    state.execute(TuiCommand::EndMouseDrag);
    assert!(!state.is_dragging_scrollbar);
    assert_eq!(state.scrollbar_drag_grab, None);

    // Clicking outside the thumb on the track (at top row 0) MUST snap immediately
    assert!(thumb_off > 0);
    state.execute(TuiCommand::MouseClick {
        row: 0,
        col: cw,
        shift: false,
    });
    assert!(state.is_dragging_scrollbar);
    assert_eq!(state.scroll, 0, "clicking on empty track must snap");
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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.render_frame(80, 24);
    let vh = state.viewport_height;
    let track_h = state.screen_height;
    state.scroll_to_visual(20);
    state.render_frame(80, 24);
    assert!(state.scroll > 0, "must be scrolled into the interior");
    let cw = state.last_content_width;
    let (counts, _, _, total) = state.visual_geometry(cw);
    let header_vis = counts.first().copied().unwrap_or(1);
    let max_scroll = total.saturating_sub(vh);
    assert!(max_scroll >= track_h, "need room for a strict sweep");

    let (thumb_off, _) =
        AppState::scrollbar_thumb_for(track_h, vh, total, state.scroll, header_vis)
            .expect("scrollbar must apply");
    state.execute(TuiCommand::MouseClick {
        row: thumb_off,
        col: cw,
        shift: false,
    });
    assert!(state.is_dragging_scrollbar);
    assert_eq!(state.scrollbar_drag_grab, Some(0));

    let mut prev = usize::MAX;
    let mut first = true;
    for r in 0..track_h {
        state.execute(TuiCommand::MouseDrag {
            row: r,
            col: cw,
            shift: false,
        });
        let want = AppState::scrollbar_scroll_for_offset(track_h, vh, total, header_vis, r);
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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
fn test_scrollbar_touch_scroll_inverts_and_scales_speed() {
    let mut diff =
        String::from("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,100 @@\n-old\n");
    for i in 1..=100 {
        diff.push_str(&format!("+line {i}\n"));
    }
    let out = run_pipeline_full(&diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.render_frame(50, 20);
    let cw = state.last_content_width;
    let track_h = state.screen_height;
    let vh = state.viewport_height;
    let (_, _, _, total_vis) = state.cached_geometry_tuple(cw);
    let expected_step = AppState::scrollbar_step(track_h, vh, total_vis, 3);

    assert_eq!(state.scroll, 0);

    // In scrollbar column (col >= cw), MouseScrollUp (finger moving down on mobile
    // touch) scrolls DOWN by the scaled track step:
    state.execute(TuiCommand::MouseScrollUp {
        n: 3,
        row: 10,
        col: cw,
    });
    assert_eq!(state.scroll, expected_step);

    // In penultimate column (col == cw - 1), MouseScrollDown also operates the
    // scrollbar during drag gestures:
    state.execute(TuiCommand::MouseScrollDown {
        n: 3,
        row: 10,
        col: cw.saturating_sub(1),
    });
    assert_eq!(state.scroll, 0);

    // In content area further in (col < cw - 1), normal viewport scrolling applies:
    state.execute(TuiCommand::MouseScrollDown {
        n: 3,
        row: 10,
        col: cw.saturating_sub(2),
    });
    assert_eq!(state.scroll, 3);

    // Tapping / clicking in the penultimate column (col == cw - 1) must NOT
    // activate scrollbar dragging:
    state.execute(TuiCommand::MouseClick {
        row: 10,
        col: cw.saturating_sub(1),
        shift: false,
    });
    assert!(
        !state.is_dragging_scrollbar,
        "clicking penultimate column must not be treated as scrollbar"
    );

    // Tapping / clicking in the scrollbar column (col >= cw) DOES activate
    // scrollbar:
    state.execute(TuiCommand::MouseClick {
        row: 10,
        col: cw,
        shift: false,
    });
    assert!(
        state.is_dragging_scrollbar,
        "clicking scrollbar column must activate scrollbar"
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
    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_expand_lines(6);

    let fi = 0;

    // Park the cursor on an EARLY row of H1 (far above the H2 divider).
    // Small viewport so max_scroll > 0 and the render-time cursor clamp
    // has room to fight the divider pivot.
    let early_row = state
        .flat
        .iter()
        .position(|fr| {
            fr.file_idx == fi
                && !fr.is_file_header()
                && !fr.is_hunk_divider()
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
    let (_, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let div_segs = counts[div_flat].saturating_sub(seg_counts[div_flat]);
    let arrow_screen_row = state.last_viewport_start + div_screen_before + div_segs;
    state.execute(TuiCommand::MouseClick {
        row: arrow_screen_row,
        col: 0,
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
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_expand_lines(6);

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
        // Park scroll so the divider sits mid-viewport with enough room below
        // to expand without clamping at max_scroll.
        state.scroll = div_vis.saturating_sub(8);
        let (starts, _counts2, _seg_counts2, _) = state.visual_geometry(cw);
        let dsy = starts[div_flat].saturating_sub(state.scroll);
        let div_segs = counts[div_flat].saturating_sub(seg_counts[div_flat]);
        let anchor_screen_pre = state
            .flat_pos_of_coord(fi, Some(anchor_new_line), None)
            .map(|p| starts[p] + div_segs - state.scroll)
            .expect("anchor must exist");

        let last_h0_flat = div_flat - 1;
        let last_h0_vis = starts[last_h0_flat] + seg_counts[last_h0_flat] - 1;
        let last_h0_sy = last_h0_vis.saturating_sub(state.scroll);
        state.execute(TuiCommand::MouseClick {
            row: state.last_viewport_start + last_h0_sy,
            col: 0,
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
@@ -1,3 +1,3 @@
 fn hello() {
-    println!("old");
+    println!("new");
 }
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.set_status_message("Saved note".to_string(), true);

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

    // No separate message row: the message renders at the front of the
    // secondary bar at row 1, and the diff starts at row 2.
    let mut line1 = String::new();
    for x in 0..80 {
        line1.push_str(buffer[(x, 1)].symbol());
    }
    assert!(line1.contains("Saved note"));
    assert!(!line1.contains("hunk 1/1"));
    assert!(!line1.contains("file 1/1"));

    let mut line2 = String::new();
    for x in 0..80 {
        line2.push_str(buffer[(x, 2)].symbol());
    }
    assert!(!line2.contains("Saved note"));
    assert!(!line2.contains("hunk 1/1"));
    assert!(!line2.contains("file 1/1"));
    assert!(line2.contains("src/lib.rs") || line2.contains("fn hello()"));

    // Viewport area must contain diff grid lines (e.g. file header or hunk
    // content)
    let mut grid_text = String::new();
    for y in 2..21 {
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
@@ -1,3 +1,3 @@
 fn hello() {
-    println!("old");
+    println!("new");
 }
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
@@ -1,3 +1,3 @@
-fn old_one() {}
-fn old_two() {}
-fn old_three() {}
+fn new_one() {}
+fn new_two() {}
+fn new_three() {}
"#;
    let out = run_pipeline_full(diff, None, None, PipelineOptions::default());
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();

    assert_eq!(
        state.last_viewport_start, 2,
        "Viewport starts after header + reserved secondary"
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
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    let output = load_opt1_wire_output();

    let mut state = AppState::new(
        output,
        "opt_1".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    // A move row renders `≈` with no `+`/`-` sign: the row coloring already
    // identifies the move side.
    let mov_line = |frame: &str, num: &str| -> String {
        frame
            .lines()
            .find(|line| line.contains(num) && line.contains('≈'))
            .map(str::to_owned)
            .unwrap_or_else(|| panic!("no move row {num} in frame:\n{frame}"))
    };
    assert!(!mov_line(&raw_frame, "96").contains("≈+"));

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
    assert!(!mov_line(&raw_frame_src, "127").contains("≈-"));
    assert!(
        raw_frame_src
            .lines()
            .any(|line| line.contains("135") && line.contains('-') && !line.contains('≈')),
        "trailing blank line 135 must be regular deletion '-', not a move row"
    );

    // Also verify collapsed gutter at narrow width (< 100 diff content cols)
    let frame_narrow = state.render_frame(80, 40);
    let raw_frame_narrow = crate::render::strip_ansi(&frame_narrow);
    assert!(!mov_line(&raw_frame_narrow, "127").contains("≈-"));
    assert!(
        raw_frame_narrow
            .lines()
            .any(|line| line.contains("135") && line.contains('-') && !line.contains('≈')),
        "narrow frame must render line 135 as a regular deletion, not a move row"
    );
}

#[test]
fn test_gutter_expand_icon_hit_area_and_swallow() {
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
    let contents: HashMap<String, String> =
        [("sample.rs".to_string(), old_lines.join("\n") + "\n")]
            .into_iter()
            .collect();
    let output = PipelineInput::new(diff)
        .with_new_content(contents.clone())
        .with_old_content(contents)
        .run();
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_expand_lines(6);
    state.render_frame(120, 30);
    let fi = 0;
    let gap = state.hunk_leading_gap(fi, 1);
    assert_eq!(gap, 14);

    let (starts, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let h2_hunk_start = state.hunk_starts[1].flat;
    let h2_div_vis = starts[h2_hunk_start];
    let div_segs = counts[h2_hunk_start].saturating_sub(seg_counts[h2_hunk_start]);
    let click_row =
        state.last_viewport_start + (h2_div_vis + div_segs).saturating_sub(state.scroll);

    // Park cursor on row 0
    state.row_idx = 0;
    state.vis_line = 0;

    // 1. Click at col 0 (on the ▲ icon): expands and swallows click (cursor stays
    //    on row 0)
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 0,
        shift: false,
    });
    let gap_after_col0 = state.hunk_leading_gap(fi, 1);
    assert!(
        gap_after_col0 < gap,
        "col 0 click on expand arrow must trigger expansion"
    );
    assert_eq!(
        state.row_idx, 0,
        "expand icon click must be swallowed and not move cursor"
    );

    // 2. Click at col 1 (1 column right of the icon): expands and swallows click
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 1,
        shift: false,
    });
    let gap_after_col1 = state.hunk_leading_gap(fi, 1);
    assert!(
        gap_after_col1 < gap_after_col0,
        "col 1 click within hit area must trigger expansion"
    );
    assert_eq!(
        state.row_idx, 0,
        "expand icon hit area click must be swallowed and not move cursor"
    );

    // 3. Click at col 2 (outside the 2-col hit area): does NOT expand, moves cursor
    state.execute(TuiCommand::MouseClick {
        row: click_row,
        col: 2,
        shift: false,
    });
    assert_eq!(
        state.hunk_leading_gap(fi, 1),
        gap_after_col1,
        "col 2 click outside hit area must not expand"
    );
    assert_ne!(
        state.row_idx, 0,
        "click outside hit area must move cursor to clicked row"
    );
}

#[test]
fn test_file_header_indicator_hit_rectangle_three_chars_wide() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.row_idx = 1;
    state.vis_line = 0;
    state.note_draft = Some("Review this line".into());
    state.commit_note_draft().expect("note commit must succeed");

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
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.row_idx = 1;
    state.vis_line = 0;
    state.note_draft = Some("Line 1\n\nLine 2".into());
    state.commit_note_draft().expect("note commit must succeed");

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
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
fn test_file_badge_omits_total_while_streaming() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let output = crate::core::pipeline::run_pipeline_full(
        diff,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    assert_eq!(output.files.len(), 2, "both files must be present");
    let mut state = AppState::new(
        output,
        "repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    state.is_streaming = true;
    let streaming = state.render_frame(120, 20);
    let plain_streaming = crate::render::strip_ansi(&streaming);
    assert!(
        plain_streaming.contains("a.rs 1 "),
        "streaming banner keeps the position:\n{plain_streaming}"
    );
    assert!(
        !plain_streaming.contains("a.rs 1/2"),
        "streaming banner must omit the still-growing total:\n{plain_streaming}"
    );

    state.is_streaming = false;
    let loaded = state.render_frame(120, 20);
    let plain_loaded = crate::render::strip_ansi(&loaded);
    assert!(
        plain_loaded.contains("a.rs 1/2"),
        "the total appears once the diff is loaded:\n{plain_loaded}"
    );
}

#[test]
fn test_no_diffs_renders_header_footer_and_secondary_message_frame() {
    let empty_output = PipelineOutput {
        files: Vec::new(),
        branch: "main".into(),
        untracked_skipped: Vec::new(),
    };
    let mut state = AppState::new(
        empty_output,
        "my_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let frame = state.render_frame(120, 20);

    // Header bar with repo name
    assert!(frame.contains("my_repo"), "frame must include repo name");

    // Secondary status bar carries the persistent empty-state notice when
    // there are no files (plus any transient segments), with a reserved
    // slot so hide/show never shifts content.
    assert!(!frame.contains("file 0/0"), "frame must omit file badge");
    assert!(
        !frame.contains("hunk 0/0"),
        "frame must omit hunk badge when secondary is hidden"
    );
    // Header still carries repo + branch glyph.
    let header_row = frame.lines().next().unwrap_or_default().to_string();
    assert!(
        header_row.contains('\u{2387}'),
        "header must show branch glyph"
    );
    assert!(header_row.contains("main"), "header must show branch name");

    // Empty-state notice lives in the secondary bar (persistent, no
    // timeout); the main area stays blank.
    let rows: Vec<String> = frame.lines().map(|l| l.to_string()).collect();
    assert!(
        rows.get(1)
            .is_some_and(|r| r.contains("No diff to display.")),
        "secondary bar (row 1) must carry the empty-state notice"
    );
    let main_rows = &rows[state.last_viewport_start
        ..(state.last_viewport_start + state.viewport_height).min(rows.len())];
    assert!(
        main_rows.iter().all(|r| !r.contains("No diff to display.")),
        "main area must stay blank once the notice moves to the secondary bar"
    );

    // Bottom ribbon footer
    assert!(frame.contains("[j/k]move"), "footer must include [j/k]move");
    assert!(frame.contains("[q]uit"), "footer must include [q]uit");
    assert!(frame.contains("[?]help"), "footer must include [?]help");
}

#[test]
fn test_no_diffs_renders_header_footer_and_secondary_message_ratatui() {
    let empty_output = PipelineOutput {
        files: Vec::new(),
        branch: "feature-branch".into(),
        untracked_skipped: Vec::new(),
    };
    let mut state = AppState::new(
        empty_output,
        "demo_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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

    // Row 0: Header bar (still carries repo + branch glyph).
    assert!(
        row_strings[0].contains("demo_repo"),
        "Row 0 must be header bar: {}",
        row_strings[0]
    );
    assert!(
        row_strings[0].contains('\u{2387}'),
        "Row 0 header must show branch glyph: {}",
        row_strings[0]
    );
    assert!(
        row_strings[0].contains("feature-branch"),
        "Row 0 header must show branch name: {}",
        row_strings[0]
    );

    // Secondary bar carries the persistent empty-state notice when there
    // are no files, so no row may carry hunk/file badges.
    for (i, row) in row_strings.iter().enumerate() {
        assert!(
            !row.contains("hunk 0/0") && !row.contains("file 0/0"),
            "Row {i} must not carry hunk/file badge when secondary hidden: {row}"
        );
    }

    // Row 1: secondary bar carries the persistent empty-state notice
    // (no timeout).
    assert!(
        row_strings[1].contains("No diff to display."),
        "Row 1 secondary bar must carry the empty-state notice: {:?}",
        row_strings[1]
    );

    // Row 2: main area stays blank once the notice moves to the bar.
    assert!(
        !row_strings[2].contains("No diff to display."),
        "Row 2 main area must stay blank: {}",
        row_strings[2]
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

    let mut state = AppState::new(
        output,
        "demo_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
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
fn test_app_single_commit_range_caret_bang_in_render_ratatui() {
    use ratatui::{Terminal, backend::TestBackend};

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    let output = PipelineOutput {
        files: vec![],
        branch: "work".into(),
        untracked_skipped: vec![],
    };

    let mut state = AppState::new(
        output,
        "demo_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_commit_range(Some("7e0caab^!".into()));

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
        row_0.contains("7e0caab^!"),
        "Header bar must show single-commit range with caret-bang: {}",
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
        diffuse_core::Repo::from_key("my_repo"),
        vec![],
        true,
    );
    let mut state = AppState::new(
        output,
        "my_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
    .with_target(target);
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
    let mut state = AppState::new(
        output,
        "repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // When not streaming, dot is always "●"
    state.is_streaming = false;
    assert_eq!(state.streaming_dot(), "●");

    // When streaming, but no data received yet: steady dot "●"
    state.is_streaming = true;
    state.last_stream_activity = None;
    assert_eq!(state.streaming_dot(), "●");

    // When data is actively arriving (< 500ms):
    let t0 = Instant::now();
    state.start_time = t0;
    state.record_stream_activity_at(t0);

    // Active, phase 0 (0..200ms): dot is "●"
    let t_100ms = t0 + Duration::from_millis(100);
    assert_eq!(state.streaming_dot_at(t_100ms), "●");

    // Active, phase 1 (200..400ms): dot is " " (blinking off)
    let t_250ms = t0 + Duration::from_millis(250);
    assert_eq!(state.streaming_dot_at(t_250ms), " ");

    // Active, phase 2 (400..500ms): dot is "●" (blinking on)
    let t_450ms = t0 + Duration::from_millis(450);
    assert_eq!(state.streaming_dot_at(t_450ms), "●");

    // Delay reached (>= 500ms without new data): pauses blinking (steady dot
    // "●")
    let t_600ms = t0 + Duration::from_millis(600);
    assert_eq!(state.streaming_dot_at(t_600ms), "●");

    // New data arrives at 600ms!
    state.record_stream_activity_at(t_600ms);
    // Active again at 600ms..800ms: phase at 700ms is (700/200)%2 = 1 -> " "
    // (resumes blinking!)
    let t_700ms = t0 + Duration::from_millis(700);
    assert_eq!(state.streaming_dot_at(t_700ms), " ");
}

#[test]
fn test_viewport_line_origin_describes_file_and_row() {
    use crate::types::FlatRow;
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

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
    state.flat.push(FlatRow::diff_row(999, 0, 0));
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
    let mut state = AppState::new(
        out,
        "diffuse".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    // Transient-only secondary bar: force it visible with the `go` buffer
    // (raw_diff would disable the scrollbar column).
    state.num_buffer = "42".to_string();

    let backend = TestBackend::new(48, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let main_width = state.last_content_width;
    // The reserved secondary slot lives at row 1; find the transient row
    // dynamically.
    let mut found = None;
    for y in 0..24 {
        let mut row = String::new();
        for x in 0..48 {
            row.push_str(buffer[(x, y)].symbol());
        }
        if row.contains("go 42") {
            found = Some((y, row));
            break;
        }
    }
    let (y, row) = found.expect("status bar must show transient go segment");
    assert!(
        !row.contains("hunk 1/12") && !row.contains("hunk"),
        "transient-only status bar must not contain hunk badge: {row:?} (row {y})"
    );
    let chars: Vec<char> = row.chars().collect();
    assert!(
        chars.len() > main_width,
        "status row must reach the scrollbar column (main_width {main_width}): {row:?}"
    );
    assert!(
        chars[main_width] == '█' || chars[main_width] == '│',
        "status bar must be narrowed to preserve scrollbar column: {row:?}"
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
    // The inserted line shifts new-side numbering by one: new line (i)
    // carries old line (i-1)'s text below the insertion, keeping the tape's
    // declared matching consistent (padding positions agree on both sides).
    let new_lines: Vec<String> = (1..=15)
        .map(|i| {
            if i == 3 {
                "// inserted".to_string()
            } else if i < 3 {
                format!("// line {i}")
            } else {
                format!("// line {}", i - 1)
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
    let mut state = AppState::new(
        trailing_gap_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.render_frame(120, 40);

    let fi = 0;
    let gap = state.hunk_trailing_gap(fi);
    assert!(gap > 0, "must have trailing gap");

    let last_row = state.last_row_of_file(fi);
    let (starts, _, _, _) = state.visual_geometry(state.last_content_width);
    let bottom_vis = starts[last_row];
    let click_row = state.last_viewport_start + bottom_vis.saturating_sub(state.scroll);

    // Initial cursor is at top
    state.row_idx = 0;
    state.vis_line = 0;

    // Click on col 0 of the end of hunks divider
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
        state.vis_line, 0,
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
    let state = AppState::new(
        shrunk_eof_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
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
        crate::render::DiffuseTheme::dark(),
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
    let mut state = AppState::new(
        two_hunk_gap_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
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

    // Content row before bottom divider: not on bottom divider
    let last_diff = state.last_row_of_file(fi) - 1;
    state.row_idx = last_diff;
    state.vis_line = 0;
    assert_eq!(state.is_cursor_on_bottom_divider(), None);

    // Bottom divider row itself
    let last_row = state.last_row_of_file(fi);
    state.row_idx = last_row;
    state.vis_line = 0;
    assert_eq!(state.is_cursor_on_bottom_divider(), Some(fi));

    // Raw diff mode returns None
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
    let mut state = AppState::new(
        two_hunk_gap_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    )
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
    let (_, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
    let div_segs = counts[div_flat].saturating_sub(seg_counts[div_flat]);
    let click_row = state.last_viewport_start + div_screen_y + div_segs;
    let col = 0;

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
fn test_hunk_bar_click_retains_cursor_visual_position_outside_screen_above_and_below() {
    let fi = 0;

    // 1. Cursor is ABOVE the screen (scrolled off top)
    {
        let mut state = AppState::new(
            two_hunk_gap_output(),
            "test_repo".into(),
            temp_notes_path(),
            crate::render::DiffuseTheme::dark(),
        )
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
        let (_, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
        let div_segs = counts[div_flat].saturating_sub(seg_counts[div_flat]);
        let click_row =
            (state.last_viewport_start as isize + div_screen_y + div_segs as isize) as usize;
        let col = 0;

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
        let mut state = AppState::new(
            two_hunk_gap_output(),
            "test_repo".into(),
            temp_notes_path(),
            crate::render::DiffuseTheme::dark(),
        )
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
        let (_, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
        let div_segs = counts[div_flat].saturating_sub(seg_counts[div_flat]);
        let click_row =
            (state.last_viewport_start as isize + div_screen_y + div_segs as isize) as usize;
        let col = 0;

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
        let mut state = AppState::new(
            two_hunk_gap_output(),
            "test_repo".into(),
            temp_notes_path(),
            crate::render::DiffuseTheme::dark(),
        )
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
        let mut state = AppState::new(
            two_hunk_gap_output(),
            "test_repo".into(),
            temp_notes_path(),
            crate::render::DiffuseTheme::dark(),
        )
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
        let mut state = AppState::new(
            two_hunk_gap_output(),
            "test_repo".into(),
            temp_notes_path(),
            crate::render::DiffuseTheme::dark(),
        )
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
    let base_state = AppState::new(
        two_hunk_gap_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    for test_row in 0..base_state.flat.len() {
        let mut s = AppState::new(
            two_hunk_gap_output(),
            "test_repo".into(),
            temp_notes_path(),
            crate::render::DiffuseTheme::dark(),
        );
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
            let mut state = AppState::new(
                two_hunk_gap_output(),
                "test_repo".into(),
                temp_notes_path(),
                crate::render::DiffuseTheme::dark(),
            )
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

        // Test B: click ⇓ (expand context below hunk 0) retains visual cursor
        // position
        {
            let mut state = AppState::new(
                two_hunk_gap_output(),
                "test_repo".into(),
                temp_notes_path(),
                crate::render::DiffuseTheme::dark(),
            )
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
            let last_hunk0_flat = div_flat - 1;
            let row_vis = starts[last_hunk0_flat];
            let row_screen_y = (row_vis as isize) - (state.scroll as isize);
            let click_row = (state.last_viewport_start as isize + row_screen_y) as usize;

            state.execute(TuiCommand::MouseClick {
                row: click_row,
                col: 0,
                shift: false,
            });
            state.render_frame(120, 16);

            let cursor_vis_after = state.cursor_visual_line();
            let cursor_rel_after = (cursor_vis_after as isize) - (state.scroll as isize);
            assert_eq!(
                cursor_rel_after, cursor_rel_before,
                "click ⇓: cursor visual position must be retained for row {test_row}"
            );
        }

        // Test C: click ⇑ on hunk 1 first row retains visual cursor position
        {
            let mut state = AppState::new(
                two_hunk_gap_output(),
                "test_repo".into(),
                temp_notes_path(),
                crate::render::DiffuseTheme::dark(),
            )
            .with_expand_lines(4);
            state.render_frame(120, 16);
            state.scroll = 2;
            state.row_idx = test_row;
            state.vis_line = 0;
            let cursor_vis_before = state.cursor_visual_line();
            let cursor_rel_before = (cursor_vis_before as isize) - (state.scroll as isize);

            let (starts, counts, seg_counts, _) = state.visual_geometry(state.last_content_width);
            let div_flat = state
                .hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == 1)
                .unwrap()
                .flat;
            let div_vis = starts[div_flat];
            let div_segs = counts[div_flat].saturating_sub(seg_counts[div_flat]);
            let click_row = (state.last_viewport_start as isize
                + (div_vis as isize - state.scroll as isize)
                + div_segs as isize) as usize;

            state.execute(TuiCommand::MouseClick {
                row: click_row,
                col: 0,
                shift: false,
            });
            state.render_frame(120, 16);

            let cursor_vis_after = state.cursor_visual_line();
            let cursor_rel_after = (cursor_vis_after as isize) - (state.scroll as isize);
            assert_eq!(
                cursor_rel_after, cursor_rel_before,
                "click ⇑: cursor visual position must be retained for row {test_row}"
            );
        }
    }
}

#[test]
fn test_app_toggle_theme_command() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    assert_eq!(state.theme, crate::render::DiffuseTheme::dark());

    state.execute(TuiCommand::ToggleTheme);
    assert_eq!(state.theme, crate::render::DiffuseTheme::light());

    state.execute(TuiCommand::ToggleTheme);
    assert_eq!(state.theme, crate::render::DiffuseTheme::dark());
}

#[test]
fn test_sticky_file_banner_sticks_to_top_when_scrolling_down() {
    let mut lines_a = String::new();
    let mut diff_a = String::new();
    for i in 1..=30 {
        lines_a.push_str(&format!("line {i}\n"));
        diff_a.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}"
    );
    let new_content = [("a.rs".to_string(), lines_a)].into_iter().collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 10;
    state.last_content_width = 80;

    // At scroll = 0, row 0 is a.rs header.
    let (lines_0, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(
        lines_0[0].to_string().contains("a.rs"),
        "row 0 must show a.rs banner at scroll 0"
    );

    // Scroll down inside a.rs (scroll = 5).
    state.scroll_to_visual(5);
    let (lines_5, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(
        lines_5[0].to_string().contains("a.rs"),
        "row 0 must stick to a.rs banner when scrolled down into content"
    );
    assert!(
        lines_5[1].to_string().contains(" 1/1 "),
        "screen row 1 must show sticky hunk divider beneath file banner, got: {:?}",
        lines_5[1]
    );
    assert!(
        lines_5[2].to_string().contains("line 6"),
        "screen row 2 must show visual line 7 (line 6) beneath the sticky headers, got: {:?}",
        lines_5[2]
    );
}

#[test]
fn test_sticky_file_banner_pushed_upwards_by_next_banner() {
    let mut lines_a = String::new();
    let mut lines_b = String::new();
    let mut diff_a = String::new();
    let mut diff_b = String::new();
    for i in 1..=10 {
        lines_a.push_str(&format!("a line {i}\n"));
        lines_b.push_str(&format!("b line {i}\n"));
        diff_a.push_str(&format!("+a line {i}\n"));
        diff_b.push_str(&format!("+b line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,10 @@\n{diff_a}diff --git a/b.rs b/b.rs\nindex 222..333 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -0,0 +1,10 @@\n{diff_b}"
    );
    let new_content = [("a.rs".to_string(), lines_a), ("b.rs".to_string(), lines_b)]
        .into_iter()
        .collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 10;
    state.last_content_width = 80;

    let b_banner_row = state.first_row_of_file(1);
    let (starts, _, _, _) = state.visual_geometry(80);
    let b_vis = starts[b_banner_row];

    // When scroll = b_vis - 1:
    // b.rs banner is at screen row 1, pushing File A's hunk divider off.
    // a.rs sticky banner is at screen row 0. They meet!
    state.scroll_to_visual(b_vis.saturating_sub(1));
    let (lines_meet, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(
        lines_meet[0].to_string().contains("a.rs"),
        "screen row 0 must show a.rs banner"
    );
    assert!(
        lines_meet[1].to_string().contains("b.rs"),
        "screen row 1 must show b.rs banner meeting it"
    );

    // When scroll = b_vis:
    // b.rs banner reaches screen row 0, pushing a.rs banner completely off-screen!
    state.scroll_to_visual(b_vis);
    let (lines_pushed, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(
        lines_pushed[0].to_string().contains("b.rs"),
        "b.rs banner must now be at screen row 0"
    );

    // On further scrolling into b.rs:
    // b.rs banner and hunk divider stick to the top!
    state.scroll_to_visual(b_vis + 1);
    let (lines_b_sticky, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(
        lines_b_sticky[0].to_string().contains("b.rs"),
        "b.rs banner must now stick to row 0"
    );
    assert!(
        lines_b_sticky[1].to_string().contains(" 1/1 "),
        "screen row 1 must show sticky hunk divider beneath b.rs banner, got: {:?}",
        lines_b_sticky[1]
    );
    assert!(
        lines_b_sticky[2].to_string().contains("b line 2"),
        "b.rs content must scroll beneath it"
    );
}

#[test]
fn test_clicking_sticky_header_toggles_collapse() {
    let mut lines_a = String::new();
    let mut diff_a = String::new();
    for i in 1..=30 {
        lines_a.push_str(&format!("line {i}\n"));
        diff_a.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}"
    );
    let new_content = [("a.rs".to_string(), lines_a)].into_iter().collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 10;
    state.last_content_width = 80;
    state.last_viewport_start = 2;
    state.scroll_to_visual(8);

    assert!(
        !state.collapsed_files.contains(&0),
        "initially not collapsed"
    );

    // Click the collapse indicator on row 0 of the viewport (which shows sticky
    // a.rs header).
    let col = crate::view::diff_grid::FILE_HEADER_INDICATOR_COL;
    state.execute(TuiCommand::MouseClick {
        row: 2, // header offset (2) + viewport row (0) = 2
        col,
        shift: false,
    });

    assert!(
        state.collapsed_files.contains(&0),
        "clicking sticky header indicator must toggle collapse"
    );
}

#[test]
fn test_sticky_hunk_header_sticks_below_file_header() {
    let mut lines_a = String::new();
    let mut diff_a = String::new();
    for i in 1..=30 {
        lines_a.push_str(&format!("line {i}\n"));
        diff_a.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}"
    );
    let new_content = [("a.rs".to_string(), lines_a)].into_iter().collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 10;
    state.last_content_width = 80;

    // At scroll = 0: row 0 is a.rs header, row 1 is H1 divider.
    let (lines_0, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_0[0].to_string().contains("a.rs"));
    assert!(lines_0[1].to_string().contains(" 1/1 "));

    // Scroll down inside a.rs (scroll = 5).
    state.scroll_to_visual(5);
    let (lines_5, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(
        lines_5[0].to_string().contains("a.rs"),
        "row 0 must stick to a.rs banner"
    );
    assert!(
        lines_5[1].to_string().contains(" 1/1 "),
        "row 1 must stick to hunk divider Hunk 1/1 immediately below file banner, got: {:?}",
        lines_5[1]
    );
    assert!(
        lines_5[2].to_string().contains("line 6"),
        "screen row 2 must show content beneath the sticky hunk divider, got: {:?}",
        lines_5[2]
    );

    // viewport_row_to_visual mapping
    assert_eq!(
        state.viewport_row_to_visual(0),
        0,
        "screen row 0 maps to file banner"
    );
    assert_eq!(
        state.viewport_row_to_visual(1),
        1,
        "screen row 1 maps to hunk divider"
    );
    assert_eq!(
        state.viewport_row_to_visual(2),
        7,
        "screen row 2 maps to line 6 (visual line 7)"
    );
}

#[test]
fn test_sticky_hunk_header_pushed_by_next_hunk() {
    let mut diff_1 = String::new();
    let mut diff_2 = String::new();
    for i in 1..=15 {
        diff_1.push_str(&format!("+h1 line {i}\n"));
        diff_2.push_str(&format!("+h2 line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,15 @@\n{diff_1}@@ -30,0 +30,15 @@\n{diff_2}"
    );
    let mut state = AppState::new(
        run_pipeline_full(&diff, None, None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 10;
    state.last_content_width = 80;

    let (starts, _, _, _) = state.visual_geometry(80);
    let h2_row = state.hunk_starts[1].flat;
    let h2_vis = starts[h2_row];

    // Scroll right before H2 pushes H1 off.
    // Screen row 0: a.rs (sticky)
    // Screen row 1: H1 (sticky)
    // Screen row 2: H2 meeting it!
    state.scroll_to_visual(h2_vis.saturating_sub(2));
    state.row_idx = h2_row; // Cursor on H2 so keep_cursor_visible doesn't clamp scroll up
    let (lines_meet, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_meet[0].to_string().contains("a.rs"));
    assert!(
        lines_meet[1].to_string().contains(" 1/2 "),
        "1/2 still sticky at row 1"
    );
    assert!(
        lines_meet[2].to_string().contains(" 2/2 "),
        "2/2 meeting at row 2"
    );

    // Scroll 1 line further: H2 reaches row 1, pushing H1 completely off!
    state.scroll_to_visual(h2_vis.saturating_sub(1));
    state.row_idx = h2_row;
    let (lines_pushed, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_pushed[0].to_string().contains("a.rs"));
    assert!(
        lines_pushed[1].to_string().contains(" 2/2 "),
        "2/2 now sticky at row 1"
    );
    assert!(lines_pushed[6].to_string().contains("h2 line 1"));
}

#[test]
fn test_sticky_hunk_header_pushed_by_next_file() {
    let mut diff_a = String::new();
    let mut diff_b = String::new();
    for i in 1..=15 {
        diff_a.push_str(&format!("+a line {i}\n"));
        diff_b.push_str(&format!("+b line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,15 @@\n{diff_a}diff --git a/b.rs b/b.rs\nindex 222..333 100644\n--- a/b.rs\n+++ b/b.rs\n@@ -0,0 +1,15 @@\n{diff_b}"
    );
    let mut state = AppState::new(
        run_pipeline_full(&diff, None, None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 10;
    state.last_content_width = 80;

    let b_banner_row = state.first_row_of_file(1);
    let (starts, _, _, _) = state.visual_geometry(80);
    let b_vis = starts[b_banner_row];

    // Scroll so b.rs header is at row 2 (below sticky a.rs at row 0 and sticky H1
    // at row 1)
    state.scroll_to_visual(b_vis.saturating_sub(2));
    state.row_idx = b_banner_row;
    let (lines_meet, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_meet[0].to_string().contains("a.rs"));
    assert!(lines_meet[1].to_string().contains(" 1/1 "));
    assert!(lines_meet[2].to_string().contains("b.rs"));

    // Scroll 1 line further: b.rs reaches row 1, pushing H1 off!
    state.scroll_to_visual(b_vis.saturating_sub(1));
    state.row_idx = b_banner_row;
    let (lines_hunk_pushed, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_hunk_pushed[0].to_string().contains("a.rs"));
    assert!(lines_hunk_pushed[1].to_string().contains("b.rs"));

    // Scroll 1 line further: b.rs reaches row 0, pushing a.rs off!
    state.scroll_to_visual(b_vis);
    state.row_idx = b_banner_row;
    let (lines_file_pushed, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_file_pushed[0].to_string().contains("b.rs"));
    assert!(lines_file_pushed[1].to_string().contains(" 1/1 ")); // b.rs's Hunk 1/1!
}

#[test]
fn test_clicking_sticky_hunk_header_selects_hunk() {
    let mut lines_a = String::new();
    let mut diff_a = String::new();
    for i in 1..=30 {
        lines_a.push_str(&format!("line {i}\n"));
        diff_a.push_str(&format!("+line {i}\n"));
    }
    let diff = format!(
        "diff --git a/a.rs b/a.rs\nindex 000..111 100644\n--- a/a.rs\n+++ b/a.rs\n@@ -0,0 +1,30 @@\n{diff_a}"
    );
    let new_content = [("a.rs".to_string(), lines_a)].into_iter().collect();

    let mut state = AppState::new(
        run_pipeline_full(&diff, Some(new_content), None, PipelineOptions::default()),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.viewport_height = 10;
    state.last_content_width = 80;
    state.last_viewport_start = 2;
    state.scroll_to_visual(8);
    state.row_idx = 8;

    // Viewport row 1 is the sticky hunk header.
    // Screen row is header offset (2) + viewport row (1) = 3.
    state.execute(TuiCommand::MouseClick {
        row: 3,
        col: 10,
        shift: false,
    });

    let hunk_row = state.hunk_starts[0].flat;
    assert_eq!(
        state.row_idx, hunk_row,
        "clicking sticky hunk header must place cursor on hunk divider flat row"
    );
}

#[test]
fn test_secondary_status_hide_show_does_not_shift_content() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    // Hidden: no transient segments.
    state.num_buffer.clear();
    let frame_hidden = state.render_frame(100, 30);
    let start_hidden = state.last_viewport_start;
    let vh_hidden = state.viewport_height;
    let lines_hidden: Vec<String> = frame_hidden.lines().map(|l| l.to_string()).collect();

    // Shown: `go` buffer forces one status segment without touching diff.
    state.num_buffer = "42".to_string();
    let frame_shown = state.render_frame(100, 30);
    let start_shown = state.last_viewport_start;
    let vh_shown = state.viewport_height;
    let lines_shown: Vec<String> = frame_shown.lines().map(|l| l.to_string()).collect();

    assert_eq!(
        start_hidden, start_shown,
        "viewport top must not move when secondary appears"
    );
    assert_eq!(
        vh_hidden, vh_shown,
        "viewport height must not change when secondary appears"
    );
    assert_eq!(
        lines_hidden.len(),
        lines_shown.len(),
        "frame line count must stay stable (reserved slot)"
    );
    let diff_hidden =
        &lines_hidden[start_hidden..(start_hidden + vh_hidden).min(lines_hidden.len())];
    let diff_shown = &lines_shown[start_shown..(start_shown + vh_shown).min(lines_shown.len())];
    assert_eq!(
        diff_hidden, diff_shown,
        "diff rows must not shift up/down when secondary appears"
    );
    assert!(
        lines_shown.iter().any(|l| l.contains("go 42")),
        "shown frame must contain the status segment"
    );
}

#[test]
fn test_status_message_moves_to_secondary_and_expires() {
    use std::time::{Duration, Instant};

    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.num_buffer.clear();
    let baseline = state.render_frame(100, 30);
    let base_start = state.last_viewport_start;
    let base_vh = state.viewport_height;
    let base_lines: Vec<String> = baseline.lines().map(|l| l.to_string()).collect();

    // Message goes to the secondary bar at the top, not a separate row.
    state.set_status_message("staged hunk".to_string(), true);
    let frame = state.render_frame(100, 30);
    assert_eq!(
        state.last_viewport_start, base_start,
        "message must not add a row above the viewport"
    );
    assert_eq!(
        state.viewport_height, base_vh,
        "message must not shrink the viewport"
    );
    let lines: Vec<String> = frame.lines().map(|l| l.to_string()).collect();
    assert_eq!(lines.len(), base_lines.len(), "frame stays stable");
    assert!(
        lines[..base_start]
            .iter()
            .any(|l| l.contains("staged hunk")),
        "message must render in the secondary slot above diff"
    );
    assert!(
        !lines
            .iter()
            .skip(base_start + base_vh)
            .any(|l| l.contains("staged hunk")),
        "message must not render below the viewport"
    );

    // `go` leads status message segment.
    state.num_buffer = "42".to_string();
    let frame2 = state.render_frame(100, 30);
    let msg_line = frame2
        .lines()
        .map(crate::render::strip_ansi)
        .find(|l| l.contains("staged hunk"))
        .expect("message line must exist");
    let (mi, gi) = (
        msg_line.find("staged hunk").unwrap(),
        msg_line.find("go 42").unwrap(),
    );
    assert!(gi < mi, "`go` segment leads message: {msg_line}");

    // Expiry: ok messages time out, errors linger longer but also expire.
    state.set_status_message_at(
        "staged hunk".to_string(),
        true,
        Instant::now() - Duration::from_secs(60),
    );
    assert!(
        state.visible_status_message().is_none(),
        "stale ok message must read as expired"
    );
    let frame3 = state.render_frame(100, 30);
    assert!(
        !frame3.contains("staged hunk"),
        "expired message must disappear from the frame"
    );

    state.set_status_message_at(
        "no staged changes".to_string(),
        false,
        Instant::now() - Duration::from_secs(5),
    );
    assert!(
        state.visible_status_message().is_some(),
        "recent error must still be visible"
    );
    state.set_status_message_at(
        "no staged changes".to_string(),
        false,
        Instant::now() - Duration::from_secs(60),
    );
    assert!(
        state.visible_status_message().is_none(),
        "stale error must eventually expire"
    );
    assert!(
        state.poll_status_expiry(Instant::now()),
        "poll must report clearing an expired message"
    );
    assert!(state.status_message.is_none());
    assert!(
        !state.poll_status_expiry(Instant::now()),
        "poll on empty must report nothing cleared"
    );
    state.set_status_message("fresh".to_string(), true);
    assert!(
        !state.poll_status_expiry(Instant::now()),
        "poll on fresh message must keep it"
    );
}

#[test]
fn test_secondary_status_bar_keeps_background_when_empty() {
    use crate::render::ColorExt;

    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.num_buffer.clear();
    let bg_code = crate::render::DiffuseTheme::dark()
        .palette
        .ui
        .status_bar_bg
        .bg_code();

    // ANSI path: empty slot keeps the status background.
    let hidden = state.render_frame(100, 30);
    let hidden_lines: Vec<&str> = hidden.lines().collect();
    assert!(
        hidden_lines[1].contains(&bg_code),
        "empty secondary must keep status bg: {:?}",
        hidden_lines[1]
    );
    assert!(
        crate::render::strip_ansi(hidden_lines[1]).trim().is_empty(),
        "empty secondary carries no text"
    );
    state.num_buffer = "42".to_string();
    let shown = state.render_frame(100, 30);
    let shown_lines: Vec<&str> = shown.lines().collect();
    assert!(
        shown_lines[1].contains(&bg_code),
        "shown secondary must use the same bg"
    );

    // Ratatui path: cell backgrounds match in both states.
    state.num_buffer.clear();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();
    let expected =
        crate::view::to_color(crate::render::DiffuseTheme::dark().palette.ui.status_bar_bg);
    for x in 0..80 {
        assert_eq!(
            terminal.backend().buffer()[(x, 1)].bg,
            expected,
            "empty secondary cell {x} must keep status bg"
        );
    }
    state.num_buffer = "42".to_string();
    terminal
        .draw(|f| {
            state.render_ratatui_frame(f);
        })
        .unwrap();
    for x in 0..80 {
        assert_eq!(
            terminal.backend().buffer()[(x, 1)].bg,
            expected,
            "shown secondary cell {x} must use the same bg"
        );
    }
}

#[test]
fn test_tui_frames_emit_optimal_ansi() {
    // The TUI must not emit redundant SGR state: every frame line is
    // already a fixed point of the optimizer (no-op codes would bloat
    // every keystroke's worth of terminal bytes).
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    for (w, h) in [(100, 30), (80, 24), (48, 24)] {
        let frame = state.render_frame(w, h);
        for line in frame.lines() {
            let optimized = crate::render::ansi::optimize_ansi(line);
            assert_eq!(
                optimized.as_ref(),
                line,
                "frame line carries redundant SGR at {w}x{h}: {line:?}"
            );
        }
        state.move_down();
    }
}

#[test]
fn test_is_reload_deferred_guards_active_interactions() {
    let mut state = AppState::new(
        sample_output(),
        "repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // Initial state: not deferred
    assert!(!state.is_reload_deferred());

    // 1. Note draft
    state.note_draft = Some("Drafting...".into());
    assert!(state.is_reload_deferred());
    state.note_draft = None;
    assert!(!state.is_reload_deferred());

    // 2. Help modal
    state.show_help = true;
    assert!(state.is_reload_deferred());
    state.show_help = false;
    assert!(!state.is_reload_deferred());

    // 3. File finder
    state.file_finder = Some(crate::finder::FileFinderState::new(
        &state.output.files,
        0,
        0,
    ));
    assert!(state.is_reload_deferred());
    state.file_finder = None;
    assert!(!state.is_reload_deferred());

    // 4. Scrollbar drag
    state.is_dragging_scrollbar = true;
    assert!(state.is_reload_deferred());
    state.is_dragging_scrollbar = false;
    assert!(!state.is_reload_deferred());

    // 5. Selection anchor
    state.anchor = Some(2);
    assert!(state.is_reload_deferred());
    state.anchor = None;
    assert!(!state.is_reload_deferred());

    // 6. Pending editor
    state.pending_editor = Some(crate::editor::EditorRequest {
        file: "a.rs".into(),
        line: 1,
        range: None,
    });
    assert!(state.is_reload_deferred());
    state.pending_editor = None;
    assert!(!state.is_reload_deferred());
}

#[test]
fn test_header_file_count_waits_for_the_stream() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let output = crate::core::pipeline::run_pipeline_full(
        diff,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    let mut state = AppState::new(
        output,
        "repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    state.is_streaming = true;
    assert_eq!(
        state.header_file_count(),
        crate::view::HeaderFileCount::Streaming
    );
    assert_eq!(state.file_badge(0).total, None);
    let streaming = state.render_frame(120, 20);
    assert!(
        !streaming.contains("2 files") && !streaming.contains("1 file"),
        "a growing total is not announced:\n{streaming}"
    );
    assert!(
        streaming.contains("? files"),
        "initial streaming displays '? files':\n{streaming}"
    );

    // Single-file re-streaming retains the constant file badge.
    state.loading_files.insert(0);
    assert_eq!(
        state.header_file_count(),
        crate::view::HeaderFileCount::Exact(2)
    );
    assert_eq!(state.file_badge(0).total, Some(2));
    let single_file_streaming = state.render_frame(120, 20);
    assert!(
        single_file_streaming.contains("2 files"),
        "single-file streaming preserves exact file badge:\n{single_file_streaming}"
    );
    assert!(
        !single_file_streaming.contains("? files"),
        "single-file streaming does not show '? files':\n{single_file_streaming}"
    );
    state.loading_files.clear();

    state.is_streaming = false;
    assert_eq!(
        state.header_file_count(),
        crate::view::HeaderFileCount::Exact(2)
    );
    assert_eq!(state.file_badge(0).total, Some(2));
    let loaded = state.render_frame(120, 20);
    assert!(
        loaded.contains("2 files"),
        "the loaded total is announced:\n{loaded}"
    );

    let empty_output = PipelineOutput {
        files: Vec::new(),
        branch: "main".into(),
        untracked_skipped: Vec::new(),
    };
    let empty_state = AppState::new(
        empty_output,
        "repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    assert_eq!(
        empty_state.header_file_count(),
        crate::view::HeaderFileCount::None
    );
}

#[test]
fn test_flat_coord_for_expanded_mod() {
    let diff = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,1 @@\n-hello old world\n+hello new world\n";
    let output = crate::core::pipeline::run_pipeline_full(
        diff,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    let fr_del = FlatRow::expanded_mod(
        0,
        0,
        0,
        diffuse_core::ModSide::Del,
        diffuse_core::ModGlyph::Top,
    );
    let fr_add = FlatRow::expanded_mod(
        0,
        0,
        0,
        diffuse_core::ModSide::Add,
        diffuse_core::ModGlyph::Bottom,
    );
    assert_eq!(AppState::flat_coord(&output, false, &fr_del), Some(1));
    assert_eq!(AppState::flat_coord(&output, false, &fr_add), Some(1));
}

#[test]
fn test_adaptive_line_number_width_from_hunk_headers() {
    let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,3 @@
 fn main() {
-    let x = 1;
+    let x = "this is a string";
 }
"#;
    let output = crate::core::pipeline::run_pipeline_full(
        diff,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );

    let state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // Initial state calculated directly from hunk header: max 3, width 1
    assert_eq!(state.new_line_num.max_num, 3);
    assert_eq!(state.new_line_num.width, 1);
    assert_eq!(state.old_line_num.max_num, 3);
    assert_eq!(state.old_line_num.width, 1);

    // Now test with larger line numbers (e.g. line 42)
    let diff_large = r#"diff --git a/big.rs b/big.rs
--- a/big.rs
+++ b/big.rs
@@ -40,3 +40,3 @@
 fn large() {
-    let old = 1;
+    let new = 2;
 }
"#;
    let output_large = crate::core::pipeline::run_pipeline_full(
        diff_large,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    let state_large = AppState::new(
        output_large,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    assert_eq!(state_large.new_line_num.max_num, 42);
    assert_eq!(state_large.new_line_num.width, 2);
    assert_eq!(state_large.old_line_num.max_num, 42);
    assert_eq!(state_large.old_line_num.width, 2);
}

#[test]
fn test_adaptive_width_on_file_expand_reload_and_append() {
    let diff_small = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,3 +1,3 @@\n line 1\n-line 2\n+line 2b\n line 3\n";
    let output_small = crate::core::pipeline::run_pipeline_full(
        diff_small,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    let mut state = AppState::new(
        output_small,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    assert_eq!(state.new_line_num.max_num, 3);
    assert_eq!(state.new_line_num.width, 1);

    // Append file with line numbers in the thousands
    let diff_huge = "diff --git a/huge.rs b/huge.rs\n--- a/huge.rs\n+++ b/huge.rs\n@@ -1000,3 +1000,3 @@\n line 1000\n-line 1001\n+line 1001b\n line 1002\n";
    let output_huge = crate::core::pipeline::run_pipeline_full(
        diff_huge,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    state.append_file(output_huge.files[0].clone());

    // Immediately adapted from the appended file's final hunk header
    assert_eq!(state.new_line_num.max_num, 1002);
    assert_eq!(state.new_line_num.width, 4);

    // Collapsing file 1 should adapt width back down to 1
    state.toggle_file_collapse(1);
    assert_eq!(state.new_line_num.max_num, 3);
    assert_eq!(state.new_line_num.width, 1);

    // Uncollapsing file 1 adapts width back up to 4
    state.toggle_file_collapse(1);
    assert_eq!(state.new_line_num.max_num, 1002);
    assert_eq!(state.new_line_num.width, 4);

    // Reloading diff back to a small file adapts back down
    let diff_reload = "diff --git a/c.rs b/c.rs\n--- a/c.rs\n+++ b/c.rs\n@@ -1,2 +1,2 @@\n-line 1\n+line 1b\n line 2\n";
    let output_reload = crate::core::pipeline::run_pipeline_full(
        diff_reload,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    state.reload(output_reload);
    assert_eq!(state.new_line_num.max_num, 2);
    assert_eq!(state.new_line_num.width, 1);
}

#[test]
#[should_panic(
    expected = "terminal content width (4) is too narrow: line number gutter requires 7 columns"
)]
fn test_content_width_less_than_or_equal_to_gutter_width_fails_with_informative_error() {
    let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1000,1 +1000,1 @@
-old
+new
"#;
    let output = crate::core::pipeline::run_pipeline_full(
        diff,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // Max line number 1000 -> line_num_width = 4 -> gutter_width = 7.
    assert_eq!(state.gutter_width(), 7);

    // Call render_diff_viewport_lines with width 5 (which is <= gutter_width 7)
    let _ = state.render_diff_viewport_lines(5, 10, 10);
}

#[test]
fn test_explicit_line_num_width_override() {
    let diff = r#"diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -1000,2 +1000,2 @@
-old 1
-old 2
+new 1
+new 2
"#;
    let output = crate::core::pipeline::run_pipeline_full(
        diff,
        None,
        None,
        crate::core::PipelineOptions::default(),
    );
    let mut state = AppState::new(
        output,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );

    // Default adaptive: max line is 1001, width is 4, gutter is 7
    assert_eq!(state.new_line_num.width, 4);
    assert_eq!(state.gutter_width(), 7);

    // Override with 0 (hide line numbers entirely)
    state = state.with_line_num_width(Some(0));
    assert_eq!(state.new_line_num.width, 0);
    assert_eq!(state.gutter_width(), 3);

    let (lines, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 0);
    // Find a rendered row (e.g. line with "old 1")
    let del_line = lines
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.contains("old 1")))
        .expect("must find del row");
    // With line_num_width 0, gutter has 3 columns: expand icon column (" "), space
    // span, and '-' span
    assert_eq!(del_line.spans[0].content, " ");
    assert_eq!(del_line.spans[1].content, " ");
    assert_eq!(del_line.spans[2].content, "-");

    // Override with 2 (numbers >= 100 truncate with ellipsis)
    state = state.with_line_num_width(Some(2));
    assert_eq!(state.new_line_num.width, 2);
    assert_eq!(state.gutter_width(), 5);

    let (lines_w2, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 0);
    let del_line_w2 = lines_w2
        .iter()
        .find(|l| l.spans.iter().any(|s| s.content.contains("old 1")))
        .expect("must find del row");
    // Expand column " ", then 1000 truncated to width 2 becomes "1…" followed by "
    // " and "-"
    assert_eq!(del_line_w2.spans[0].content, " ");
    assert_eq!(del_line_w2.spans[1].content, "1…");
    assert_eq!(del_line_w2.spans[2].content, " ");
    assert_eq!(del_line_w2.spans[3].content, "-");

    // Override with 16
    state = state.with_line_num_width(Some(16));
    assert_eq!(state.new_line_num.width, 16);
    assert_eq!(state.gutter_width(), 19);

    // Reset override to None -> restores adaptive dynamic calculation
    state = state.with_line_num_width(None);
    assert_eq!(state.new_line_num.width, 4);
    assert_eq!(state.gutter_width(), 7);
}
