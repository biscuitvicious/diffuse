use std::collections::HashMap;

use crate::{
    app::{AppState, temp_notes_path},
    core::{
        PipelineOptions,
        notes::{LineHash, NoteStatus, ReviewNote, add_note, load_notes, notes_for_file},
        pipeline::run_pipeline_full,
        wire::PipelineOutput,
    },
    render::ColorExt,
};

/// The output row at a new-side line number in the given file.
fn row_at_new(state: &AppState, file_idx: usize, n: u32) -> &crate::core::wire::OutputRow {
    let f = &state.output.files[file_idx];
    f.rows
        .iter()
        .chain(f.hunks.iter().flat_map(|h| h.rows.iter()))
        .find(|r| r.new_line == Some(n))
        .unwrap_or_else(|| panic!("no output row at new line {n}"))
}

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

fn multi_file_output() -> PipelineOutput {
    let diff = r#"diff --git a/first.rs b/first.rs
index 1111111..2222222 100644
--- a/first.rs
+++ b/first.rs
@@ -1,3 +1,3 @@
 fn first() {
-    let x = 1;
+    let x = 2;
 }
diff --git a/second.rs b/second.rs
index 3333333..4444444 100644
--- a/second.rs
+++ b/second.rs
@@ -1,3 +1,3 @@
 fn second() {
-    let a = 10;
+    let a = 20;
 }
"#;
    let new_content = [
        (
            "first.rs".to_string(),
            "fn first() {\n    let x = 2;\n}\n".to_string(),
        ),
        (
            "second.rs".to_string(),
            "fn second() {\n    let a = 20;\n}\n".to_string(),
        ),
    ]
    .into_iter()
    .collect();
    let old_content = [
        (
            "first.rs".to_string(),
            "fn first() {\n    let x = 1;\n}\n".to_string(),
        ),
        (
            "second.rs".to_string(),
            "fn second() {\n    let a = 10;\n}\n".to_string(),
        ),
    ]
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
fn test_note_commit_records_content_hash() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );

    let file = state.current_file().unwrap().path.clone();
    state.note_draft = Some("note with hash".into());
    state.commit_note_draft().expect("note commit must succeed");

    let notes: Vec<_> = notes_for_file(&state.notes, "test_repo", "", &file).collect();
    assert_eq!(notes.len(), 1);
    let hash = notes[0]
        .content_hash
        .expect("content_hash must be recorded");
    // Short hash: exactly 8 hex chars (max 4-8 chars)
    let hex = hash.to_hex();
    assert_eq!(hex.len(), 8);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(format!("{hash}"), hex);
}

#[test]
fn test_note_callout_anchoring_and_banner_placement() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );
    let file = state.current_file().unwrap().path.clone();
    let banner_idx = state.first_row_of_file(state.current_file_idx());

    // 1. Note whose complete row hash matches the row at its exact coordinates
    //    anchors cleanly -> inline callout produced
    let anchor_row = row_at_new(&state, 0, 1025);
    let coord = crate::core::notes::DiffCoord::from_row(anchor_row).unwrap();
    let valid_note = ReviewNote {
        id: "valid".into(),
        file: file.clone(),
        location: crate::core::notes::NoteLocation::Line { coord },
        text: "valid note".into(),
        created_at: 100,
        content_hash: LineHash::of_rows(std::slice::from_ref(anchor_row)),
        original_source_rows: vec![anchor_row.clone()],
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", valid_note);
    let callouts = state.note_callouts(80);
    assert_eq!(
        callouts.len(),
        1,
        "cleanly anchored note must produce a callout"
    );
    assert!(
        !callouts.contains_key(&banner_idx),
        "cleanly anchored note must anchor inline, not on file banner"
    );

    // 2. Note with mismatching content_hash -> displayed on file banner
    state.notes.clear();
    let mismatch_note = ReviewNote {
        id: "mismatch".into(),
        file: file.clone(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            1025,
        )),
        text: "mismatch note".into(),
        created_at: 101,
        content_hash: Some(LineHash::from_hex("deadbeef").unwrap()),
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", mismatch_note);
    let callouts_mismatch = state.note_callouts(80);
    assert_eq!(
        callouts_mismatch.len(),
        1,
        "mismatched content_hash note must produce a banner callout"
    );
    assert!(
        callouts_mismatch.contains_key(&banner_idx),
        "mismatched content_hash note must display attached to the file banner"
    );

    // 3. Note on nonexistent line number -> displayed on file banner
    state.notes.clear();
    let nonexistent_line_note = ReviewNote {
        id: "nonexistent".into(),
        file: file.clone(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            99999,
        )),
        text: "orphan note".into(),
        created_at: 102,
        content_hash: Some(LineHash::from_hex("87654321").unwrap()),
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", nonexistent_line_note);
    let callouts_orphan = state.note_callouts(80);
    assert_eq!(
        callouts_orphan.len(),
        1,
        "orphan note must produce a banner callout"
    );
    assert!(
        callouts_orphan.contains_key(&banner_idx),
        "orphan note must display attached to the file banner"
    );

    // 4. Note without content_hash -> displayed on file banner
    state.notes.clear();
    let unhashed_note = ReviewNote {
        id: "unhashed".into(),
        file: file.clone(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            1025,
        )),
        text: "unhashed note".into(),
        created_at: 103,
        content_hash: None,
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", unhashed_note);
    let callouts_unhashed = state.note_callouts(80);
    assert_eq!(
        callouts_unhashed.len(),
        1,
        "unhashed note must produce a banner callout"
    );
    assert!(
        callouts_unhashed.contains_key(&banner_idx),
        "unhashed note must display attached to the file banner"
    );
}

#[test]
fn test_multiline_note_anchors_to_last_line() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );
    let file = state.current_file().unwrap().path.clone();
    let banner_idx = state.first_row_of_file(state.current_file_idx());

    // Multi-line range 1025..=1027: complete hash over all captured rows
    let rows = vec![
        row_at_new(&state, 0, 1025).clone(),
        row_at_new(&state, 0, 1026).clone(),
        row_at_new(&state, 0, 1027).clone(),
    ];
    let valid_multiline = ReviewNote {
        id: "multi-valid".into(),
        file: file.clone(),
        location: crate::core::notes::NoteLocation::Range {
            start: crate::core::notes::DiffCoord::from_row(&rows[0]).unwrap(),
            end: crate::core::notes::DiffCoord::from_row(rows.last().unwrap()).unwrap(),
        },
        text: "multiline note".into(),
        created_at: 100,
        content_hash: LineHash::of_rows(&rows),
        original_source_rows: rows,
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", valid_multiline);
    let callouts = state.note_callouts(80);
    assert_eq!(
        callouts.len(),
        1,
        "cleanly anchored multiline note must produce a callout"
    );
    assert!(
        !callouts.contains_key(&banner_idx),
        "multiline note anchored to last line must anchor inline, not on file banner"
    );

    // A hash over only the first row does not match the 3-row window
    state.notes.clear();
    let first_row = row_at_new(&state, 0, 1025).clone();
    let mismatch_multiline = ReviewNote {
        id: "multi-mismatch".into(),
        file,
        location: crate::core::notes::NoteLocation::Range {
            start: crate::core::notes::DiffCoord::from_row(&first_row).unwrap(),
            end: crate::core::notes::DiffCoord::from_row(row_at_new(&state, 0, 1027)).unwrap(),
        },
        text: "first line hash note".into(),
        created_at: 101,
        content_hash: LineHash::of_rows(std::slice::from_ref(&first_row)),
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", mismatch_multiline);
    let callouts_mismatch = state.note_callouts(80);
    assert!(
        callouts_mismatch.contains_key(&banner_idx),
        "note with single-line hash must fail compound anchor across 3 lines and be placed on file banner"
    );
}

#[test]
fn test_notes_dynamic_sync_updates_counts_and_preserves_banner_notes() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );
    let file = state.current_file().unwrap().path.clone();
    let banner_idx = state.first_row_of_file(state.current_file_idx());

    // Initially zero notes
    assert_eq!(state.output.files[0].counts.notes, 0);
    assert_eq!(state.output.total_counts().notes, 0);

    // Commit note via draft -> file counts and total counts dynamically update
    // to 1
    state.note_draft = Some("first dynamic note".into());
    state.commit_note_draft().expect("note commit must succeed");
    assert_eq!(state.output.files[0].counts.notes, 1);
    assert_eq!(state.output.total_counts().notes, 1);
    assert_eq!(state.output.files[0].notes.len(), 1);

    // Add an unanchored note (mismatched hash) directly to notes DB and sync
    add_note(
        &mut state.notes,
        "test_repo",
        "",
        ReviewNote {
            id: "banner-note".into(),
            file: file.clone(),
            location: crate::core::notes::NoteLocation::line(
                crate::core::notes::DiffCoord::new_side(99999),
            ),
            text: "unanchored note".into(),
            created_at: 104,
            content_hash: None,
            ..Default::default()
        },
    );
    state.sync_all_notes();

    // Both notes count towards the file total and global total
    assert_eq!(state.output.files[0].counts.notes, 2);
    assert_eq!(state.output.total_counts().notes, 2);
    assert_eq!(state.output.files[0].notes.len(), 2);

    // The unanchored note displays attached to the file banner
    let callouts = state.note_callouts(100);
    assert!(
        callouts.contains_key(&banner_idx),
        "unanchored note must display attached to the file banner"
    );
    assert_eq!(callouts.get(&banner_idx).unwrap().len(), 1);

    // Delete the first note -> counts dynamically decrease to 1
    state.move_down();
    state.delete_note();
    assert_eq!(state.output.files[0].counts.notes, 1);
    assert_eq!(state.output.total_counts().notes, 1);
}

#[test]
fn test_note_callouts_render_across_all_files_and_banners() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );

    // Initial cursor is on file 0 (first.rs)
    assert_eq!(state.current_file_idx(), 0);

    let banner_1 = state.first_row_of_file(1);

    // Add unanchored note to second.rs (file 1)
    add_note(
        &mut state.notes,
        "test_repo",
        "",
        ReviewNote {
            id: "note-file-1".into(),
            file: "second.rs".into(),
            location: crate::core::notes::NoteLocation::line(
                crate::core::notes::DiffCoord::new_side(999),
            ),
            text: "second file banner note".into(),
            created_at: 200,
            content_hash: None,
            ..Default::default()
        },
    );
    state.sync_all_notes();

    // Callouts must include banner_1 even when cursor is on file 0!
    let callouts = state.note_callouts(100);
    assert!(
        callouts.contains_key(&banner_1),
        "note on second.rs must be attached to second.rs file banner (index {banner_1}), even when cursor is on file 0"
    );
    assert_eq!(callouts.get(&banner_1).unwrap().len(), 1);
    assert!(callouts.get(&banner_1).unwrap()[0].contains("second file banner note"));

    // Rendering a frame that includes second.rs file banner must show the
    // callout
    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("second file banner note"),
        "frame must render banner note on second.rs: {:.300}",
        frame
    );
}

#[test]
fn test_note_callouts_on_collapsed_file_render_on_banner() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );

    let anchor_row = row_at_new(&state, 1, 2).clone();
    let coord = crate::core::notes::DiffCoord::from_row(&anchor_row).unwrap();
    add_note(
        &mut state.notes,
        "test_repo",
        "",
        ReviewNote {
            id: "inline-note-file-1".into(),
            file: "second.rs".into(),
            location: crate::core::notes::NoteLocation::Line { coord },
            text: "inline note on second.rs".into(),
            created_at: 201,
            content_hash: LineHash::of_rows(std::slice::from_ref(&anchor_row)),
            original_source_rows: vec![anchor_row],
            ..Default::default()
        },
    );
    state.sync_all_notes();

    let banner_1 = state.first_row_of_file(1);
    let callouts_expanded = state.note_callouts(100);
    assert!(
        !callouts_expanded.contains_key(&banner_1),
        "when expanded and cleanly anchored, note should not be on the banner"
    );

    // Collapse file 1 -> note anchors to file 1's banner because hunk rows are
    // absent from flat
    state.toggle_file_collapse(1);
    let banner_1_collapsed = state.first_row_of_file(1);
    let callouts_collapsed = state.note_callouts(100);
    assert!(
        callouts_collapsed.contains_key(&banner_1_collapsed),
        "when collapsed, cleanly anchored note must display attached to the file banner"
    );

    let frame = state.render_frame(100, 30);
    assert!(
        frame.contains("inline note on second.rs"),
        "frame must render the note on the collapsed banner: {:.300}",
        frame
    );
}

#[test]
fn test_note_callouts_full_markdown_multiline_and_wrap() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );

    let note_text = "Test *Markdown* **formatting:**\n\n`this is a code span` with a comment.\n\n* List item one\n* List item two with a very long line that should certainly wrap when the terminal width is constrained\n\n```bash\nif [ -f hello.txt ]; then\n  cat hello.txt\nfi\n```";

    let banner_idx = state.first_row_of_file(0);
    add_note(
        &mut state.notes,
        "test_repo",
        "",
        ReviewNote {
            id: "md-note".into(),
            file: "first.rs".into(),
            location: crate::core::notes::NoteLocation::line(
                crate::core::notes::DiffCoord::new_side(9999),
            ),
            text: note_text.into(),
            created_at: 300,
            content_hash: None,
            ..Default::default()
        },
    );
    state.sync_all_notes();

    // Callouts at content_width = 50:
    // text_cols = 50 - 8 = 42 columns.
    let callouts = state.note_callouts(50);
    let note_lines = callouts
        .get(&banner_idx)
        .expect("must have callout on file banner");

    // Must have multiple lines
    assert!(
        note_lines.len() > 10,
        "note must expand into multiple visual lines: found {}",
        note_lines.len()
    );

    // First line must start with the 💬 icon, markers rendered away.
    let clean_first = crate::render::strip_ansi(&note_lines[0]);
    assert_eq!(
        clean_first, "▌███ 💬 Test Markdown formatting:",
        "first line must have correct prefix and content: {}",
        clean_first
    );

    // Bullets draw as • instead of source dashes.
    assert!(
        note_lines
            .iter()
            .any(|l| crate::render::strip_ansi(l) == "▌███ │  • List item one"),
        "must render bullet list items: {:?}",
        note_lines
            .iter()
            .map(|l| crate::render::strip_ansi(l))
            .collect::<Vec<_>>()
    );

    // Continuation lines must start with the gutter line (│)
    for (i, line) in note_lines
        .iter()
        .enumerate()
        .skip(1)
        .take(note_lines.len() - 2)
    {
        let clean = crate::render::strip_ansi(line);
        assert!(
            clean.starts_with("▌███ │  "),
            "continuation line {i} must be indented with gutter line: {line:?}"
        );
    }

    // Last line must start with the final corner (╰, no dash).
    if let Some(last) = note_lines.last() {
        let clean = crate::render::strip_ansi(last);
        assert!(
            clean.starts_with("▌███ ╰  "),
            "last line must start with final corner: {last:?}"
        );
    }

    // Check code fence syntax styling
    let bash_line = note_lines
        .iter()
        .find(|l| crate::render::strip_ansi(l).contains("cat hello.txt"))
        .expect("must find bash fence line");
    assert!(
        bash_line.contains("cat"),
        "bash fence line must contain cat: {}",
        bash_line
    );

    // Ensure render_frame works cleanly without visual overflow or panic
    let frame = state.render_frame(50, 40);
    assert!(
        crate::render::strip_ansi(&frame).contains("Test"),
        "rendered frame must contain note content"
    );
    assert!(
        crate::render::strip_ansi(&frame).contains("cat hello.txt"),
        "rendered frame must contain code fence content"
    );
}

#[test]
fn test_note_callout_gutter_band_and_terminal_line() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );
    let banner_idx = state.first_row_of_file(0);

    add_note(
        &mut state.notes,
        "test_repo",
        "",
        ReviewNote {
            id: "gutter-note".into(),
            file: "first.rs".into(),
            location: crate::core::notes::NoteLocation::line(
                crate::core::notes::DiffCoord::new_side(9999),
            ),
            text: "This is\na 4-line\nnote:\n`let a = 17;`".into(),
            created_at: 301,
            content_hash: None,
            ..Default::default()
        },
    );
    state.sync_all_notes();

    let callouts = state.note_callouts(80);
    let note_lines = callouts
        .get(&banner_idx)
        .expect("must have callout on file banner");
    // Soft-wrapped source lines join into one reflowed paragraph (no blank
    // source lines): "This is a 4-line note: let a = 17;" fits text_cols =
    // 80 - 8 = 72 on a single visual line, code span without backticks.
    assert_eq!(note_lines.len(), 1);

    let amber = crate::render::Color::Rgb(250, 75, 40).fg_code();
    let yellow = crate::render::Color::Rgb(220, 220, 20).fg_code();
    let note_bg = crate::render::Color::Fixed(234).bg_code();
    let white = crate::render::Color::Fixed(255).fg_code();
    // Legacy note-tint foreground guard (Rgb(76, 64, 20)); the cursor-tiered
    // schema dropped the unconsumed tint slot.
    let muted = crate::render::Color::Rgb(76, 64, 20).bg_code();
    for line in note_lines {
        assert!(
            line.starts_with(&note_bg),
            "note background must start at the line start: {line:?}"
        );
        assert!(line.contains(&amber), "note stripe must be amber: {line:?}");
        assert!(
            line.contains(&yellow),
            "note gutter must use filled yellow blocks: {line:?}"
        );
        assert!(
            !line.contains(&muted),
            "note content must not use the muted note background: {line:?}"
        );
    }
    assert!(
        note_lines[0].contains(&white),
        "speech bubble must be pure white: {:?}",
        note_lines[0]
    );

    let plain: Vec<_> = note_lines
        .iter()
        .map(|line| crate::render::strip_ansi(line))
        .collect();
    assert_eq!(
        plain[0].trim_end(),
        "▌███ 💬 This is a 4-line note: let a = 17;"
    );
    assert_eq!(crate::render::display_width(&plain[0]), 80);
}

#[test]
fn test_note_callout_empty_text_renders_one_line() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );
    let banner_idx = state.first_row_of_file(0);

    add_note(
        &mut state.notes,
        "test_repo",
        "",
        ReviewNote {
            id: "empty-note".into(),
            file: "first.rs".into(),
            location: crate::core::notes::NoteLocation::line(
                crate::core::notes::DiffCoord::new_side(9999),
            ),
            text: String::new(),
            created_at: 302,
            content_hash: None,
            ..Default::default()
        },
    );
    state.sync_all_notes();

    let callouts = state.note_callouts(80);
    let note_lines = callouts
        .get(&banner_idx)
        .expect("must have callout on file banner");
    assert_eq!(note_lines.len(), 1);
}

#[test]
fn test_note_callout_long_line_wrapping() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        multi_file_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );
    let banner_idx = state.first_row_of_file(0);

    // A very long single line that will exceed the gutter + text_cols at
    // content_width = 50
    let long_line = "a".repeat(120);
    let note_text = format!("Short line\n{long_line}\nAnother line");

    add_note(
        &mut state.notes,
        "test_repo",
        "",
        ReviewNote {
            id: "wrap-test".into(),
            file: "first.rs".into(),
            location: crate::core::notes::NoteLocation::line(
                crate::core::notes::DiffCoord::new_side(9999),
            ),
            text: note_text,
            created_at: 400,
            content_hash: None,
            ..Default::default()
        },
    );
    state.sync_all_notes();

    let content_width = 50;
    let callouts = state.note_callouts(content_width);
    let note_lines = callouts
        .get(&banner_idx)
        .expect("must have callout on file banner");

    // Verify every rendered line fits within the content_width (including
    // gutter)
    for line in note_lines {
        let visual_width = crate::view::visible_width(line);
        assert!(
            visual_width <= content_width,
            "line exceeds content_width: {} > {}: {:?}",
            visual_width,
            content_width,
            line
        );
    }

    // The long line should be split into at least two visual segments
    assert!(
        note_lines.len() >= 4,
        "expected wrapping to produce multiple lines, got {}",
        note_lines.len()
    );
}

#[test]
fn test_table_note_width_at_narrow_content_width() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path,
        crate::render::DiffuseTheme::dark(),
    );
    let banner_idx = state.first_row_of_file(0);
    let table = "| State & Style | Alignment & Code | Metrics | Long Tokens & Wrapping |\n| :--- | :---: | ---: | :--- |\n| ✅ **Ready** | `cargo test` | 100.0% | Normal text with multiple wrapped words |\n| 🔄 *Active* | [diffuse](https://diffuse.rs) | +42 ms | Supercalifragilisticexpialidocious |\n| ⬜ Pending | ~~deprecated~~ | $1,234.56 | `diffuse::render::terminal::wrap_cell` |\n| 🦀 **Mixed** 🚀 | *italic* & **bold** | -99 | **Bold across multiple lines when squashed** |\n| ⚠️ Warning | | 0.00 | Unicode: 👩‍💻 and 🇩🇪 |";
    add_note(
        &mut state.notes,
        "test_repo",
        "",
        ReviewNote {
            id: "table-note".into(),
            file: "filesystem.rs".into(),
            location: crate::core::notes::NoteLocation::line(
                crate::core::notes::DiffCoord::new_side(1025),
            ),
            text: table.to_string(),
            created_at: 0,
            content_hash: None,
            ..Default::default()
        },
    );
    state.sync_all_notes();

    let content_width = 49;
    let callouts = state.note_callouts(content_width);
    let note_lines = callouts
        .get(&banner_idx)
        .expect("must have callout on file banner");

    for (i, line) in note_lines.iter().enumerate() {
        let visual_width = crate::view::visible_width(line);
        assert!(
            visual_width <= content_width,
            "line {i} exceeds content_width: {visual_width} > {content_width}: {line:?}"
        );
    }
}

#[test]
fn test_targeted_note_deletion_multiple_notes_on_same_row() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path.clone(),
        crate::render::DiffuseTheme::dark(),
    );
    let file = state.current_file().unwrap().path.clone();

    // Commit two notes on the first code row
    state.note_draft = Some("First note".into());
    state.commit_note_draft().expect("note commit must succeed");
    state.note_draft = Some("Second note".into());
    state.commit_note_draft().expect("note commit must succeed");

    let note_row = 1;
    state.row_idx = note_row;
    let layout = state.note_callout_layout_for_row(note_row, 80);
    assert_eq!(layout.len(), 2, "both notes must be laid out");

    // The second note starts after the first note's visual lines
    // row 1 has: vis_line 0 = hunk divider, vis_line 1 = code segment
    let note2_vis_line = 1 + 1 + layout[0].1;
    state.vis_line = note2_vis_line;

    let target = state.visual_target_at(state.row_idx, state.vis_line);
    let note2_id = match target {
        Some(crate::app::notes::VisualTarget::Note { note, .. }) => {
            assert_eq!(note.text, "Second note");
            note.id.clone()
        }
        other => panic!(
            "expected VisualTarget::Note for second note, got {:?}",
            other
        ),
    };

    // Delete targeted note
    state.delete_note();

    let remaining: Vec<_> = notes_for_file(&state.notes, "test_repo", "", &file).collect();
    assert_eq!(remaining.len(), 1);
    assert_ne!(remaining[0].id, note2_id);
    assert_eq!(remaining[0].text, "First note");
}

#[test]
fn test_targeted_note_editing_preserves_id_and_updates_text() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path.clone(),
        crate::render::DiffuseTheme::dark(),
    );
    let file = state.current_file().unwrap().path.clone();

    state.note_draft = Some("Original comment text".into());
    state.commit_note_draft().expect("note commit must succeed");

    let note_row = 1;
    state.row_idx = note_row;
    state.vis_line = 2; // vis_line 0 = hunk div, 1 = code seg, 2 = note line

    // Targeted edit via start_note_draft
    state.start_note_draft();
    let orig_id = state
        .editing_note_id
        .clone()
        .expect("must set editing_note_id");
    assert_eq!(state.note_draft.as_deref(), Some("Original comment text"));

    // Modify the draft text
    state.note_draft = Some("Updated comment text!".into());
    state.commit_note_draft().expect("note commit must succeed");

    assert!(state.note_draft.is_none());
    assert!(state.editing_note_id.is_none());

    let notes: Vec<_> = notes_for_file(&state.notes, "test_repo", "", &file).collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].id, orig_id, "editing must preserve note id");
    assert_eq!(notes[0].text, "Updated comment text!");
}

#[test]
fn test_delete_note_disabled_on_diff_row_without_selection() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path.clone(),
        crate::render::DiffuseTheme::dark(),
    );
    let file = state.current_file().unwrap().path.clone();

    state.note_draft = Some("Keep me safe".into());
    state.commit_note_draft().expect("note commit must succeed");

    // Cursor is on row 1, vis_line 1 (code row, not the note)
    state.row_idx = 1;
    state.vis_line = 1;
    state.delete_note();

    assert_eq!(
        state.status_message.as_ref().map(|(s, _, _)| s.as_str()),
        Some("no note here")
    );
    let notes: Vec<_> = notes_for_file(&state.notes, "test_repo", "", &file).collect();
    assert_eq!(notes.len(), 1, "note must not be deleted from code line");
}

#[test]
fn test_style_focused_note_line() {
    use crate::app::notes::style_focused_note_line;
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.note_draft = Some("test note".into());
    state.commit_note_draft().expect("note commit must succeed");
    let callouts = state.note_callouts(80);
    let note_line = callouts.values().next().unwrap().first().unwrap();

    // Non-current line of focused note keeps note_bg (234) with cyan stripe
    let focused_non_current =
        style_focused_note_line(note_line, false, &crate::render::DiffuseTheme::dark());
    assert!(
        focused_non_current.contains("\x1b[38;5;81m"),
        "must switch left stripe to cyan ANSI color"
    );
    assert!(
        focused_non_current.contains("\x1b[48;5;234m"),
        "non-current line keeps note_bg 234"
    );
    assert!(
        focused_non_current.contains("💬"),
        "must preserve original note callout glyph"
    );

    // Current line of focused note gets cursor_bg (237) background highlight
    let focused_current =
        style_focused_note_line(note_line, true, &crate::render::DiffuseTheme::dark());
    assert!(
        focused_current.contains("\x1b[38;5;81m"),
        "must switch left stripe to cyan ANSI color"
    );
    assert!(
        focused_current.contains("\x1b[48;5;237m"),
        "current line receives cursor background 237"
    );
    assert!(
        focused_current.contains("💬"),
        "must preserve original note callout glyph"
    );
}

#[test]
fn test_style_focused_note_line_in_progress_and_resolved() {
    use crate::app::notes::style_focused_note_line;
    let theme = crate::render::DiffuseTheme::dark();

    // Test InProgress status with 3 lines to exercise first_prefix, line_prefix,
    // final_prefix
    let in_progress_lines = crate::render::render_note_callout_lines(
        NoteStatus::InProgress,
        "- Line one\n- Line two\n- Line three",
        &[],
        false,
        true,
        &theme,
        80,
    );
    assert_eq!(in_progress_lines.len(), 3);

    // First line (InProgress)
    let focused_first = style_focused_note_line(
        &in_progress_lines[0],
        true,
        &crate::render::DiffuseTheme::dark(),
    );
    assert!(
        focused_first.contains("\x1b[38;5;81m"),
        "in-progress first line must switch left stripe to cyan"
    );
    assert!(
        focused_first.contains("\x1b[48;5;237m"),
        "current line receives cursor background 237"
    );
    assert!(
        focused_first.contains("🔄"),
        "must preserve InProgress 🔄 glyph"
    );

    // Middle line (InProgress, line_prefix)
    let focused_middle = style_focused_note_line(
        &in_progress_lines[1],
        false,
        &crate::render::DiffuseTheme::dark(),
    );
    assert!(
        focused_middle.contains("\x1b[38;5;81m"),
        "in-progress continuation line must switch left stripe to cyan"
    );
    assert!(
        focused_middle.contains("\x1b[48;5;234m"),
        "non-current line keeps note_bg 234"
    );
    assert!(focused_middle.contains('│'), "must preserve connector │");

    // Final line (InProgress, final_prefix)
    let focused_final = style_focused_note_line(
        &in_progress_lines[2],
        false,
        &crate::render::DiffuseTheme::dark(),
    );
    assert!(
        focused_final.contains("\x1b[38;5;81m"),
        "in-progress final line must switch left stripe to cyan"
    );
    assert!(
        focused_final.contains("\x1b[48;5;234m"),
        "non-current line keeps note_bg 234"
    );
    assert!(focused_final.contains('╰'), "must preserve connector ╰");

    // Test Resolved status
    let resolved_lines = crate::render::render_note_callout_lines(
        NoteStatus::Resolved,
        "- Res one\n- Res two",
        &[],
        false,
        true,
        &theme,
        80,
    );
    assert_eq!(resolved_lines.len(), 2);

    let focused_res = style_focused_note_line(
        &resolved_lines[0],
        true,
        &crate::render::DiffuseTheme::dark(),
    );
    assert!(
        focused_res.contains("\x1b[38;5;81m"),
        "resolved line must switch left stripe to cyan"
    );
    assert!(
        focused_res.contains("\x1b[48;5;237m"),
        "current line receives cursor background 237"
    );
    assert!(
        focused_res.contains("✅"),
        "must preserve Resolved ✅ glyph"
    );

    // Non-note string falls through unchanged
    let non_note = "arbitrary line content";
    assert_eq!(
        style_focused_note_line(non_note, true, &crate::render::DiffuseTheme::dark()),
        non_note
    );
}

#[test]
fn test_multiline_note_stripe_entirely_cyan_when_cursor_on_any_line() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.note_draft = Some("- First line\n- Second line\n- Third line".into());
    state.commit_note_draft().expect("note commit must succeed");

    // Row 1 has: vis_line 0 = hunk div, vis_line 1 = code seg,
    // vis_line 2 = note line 0, 3 = note line 1, 4 = note line 2
    state.row_idx = 1;
    state.vis_line = 3; // hover cursor on the middle line of the note

    let frame = state.render_frame(80, 20);
    let cyan_count = frame.matches("\x1b[38;5;81m").count();
    assert_eq!(
        cyan_count, 3,
        "all 3 lines of the focused note must have cyan stripe, got {cyan_count}"
    );

    // The current line of the note must have the cursor background 237
    // highlight
    assert!(
        frame.contains("\x1b[48;5;237m"),
        "current line of note must receive cursor background 237"
    );
}

#[test]
fn test_note_status_cycling_open_progress_resolved() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.row_idx = 1;
    state.note_draft = Some("Cycle me".into());
    state.commit_note_draft().expect("note commit must succeed");

    let file_notes: Vec<_> =
        notes_for_file(&state.notes, "test_repo", "", "filesystem.rs").collect();
    assert_eq!(file_notes.len(), 1);
    assert_eq!(file_notes[0].status, NoteStatus::Open);
    assert_eq!(state.output.files[0].counts.notes, 1);
    assert_eq!(state.output.files[0].counts.in_progress_notes, 0);
    assert_eq!(state.output.files[0].counts.resolved_notes, 0);

    // Position cursor on the note
    state.vis_line = 2;

    // Cycle 1: Open -> InProgress
    let cycled = state.cycle_note_status();
    assert!(cycled);
    let n = notes_for_file(&state.notes, "test_repo", "", "filesystem.rs")
        .next()
        .unwrap();
    assert_eq!(n.status, NoteStatus::InProgress);
    assert_eq!(state.output.files[0].counts.notes, 0);
    assert_eq!(state.output.files[0].counts.in_progress_notes, 1);
    assert_eq!(state.output.files[0].counts.resolved_notes, 0);
    assert!(
        state
            .status_message
            .as_ref()
            .unwrap()
            .0
            .contains("in progress")
    );

    // Callouts reflect InProgress glyph
    let callouts = state.note_callouts(80);
    let note_rendered = &callouts.values().next().unwrap()[0];
    assert!(note_rendered.contains('🔄'));

    // Cycle 2: InProgress -> Resolved
    let cycled2 = state.cycle_note_status();
    assert!(cycled2);
    let n = notes_for_file(&state.notes, "test_repo", "", "filesystem.rs")
        .next()
        .unwrap();
    assert_eq!(n.status, NoteStatus::Resolved);
    assert_eq!(state.output.files[0].counts.notes, 0);
    assert_eq!(state.output.files[0].counts.in_progress_notes, 0);
    assert_eq!(state.output.files[0].counts.resolved_notes, 1);
    assert!(
        state
            .status_message
            .as_ref()
            .unwrap()
            .0
            .contains("resolved")
    );

    // Callouts hide resolved note by default
    let callouts_hidden = state.note_callouts(80);
    assert!(
        callouts_hidden.is_empty(),
        "resolved note must be hidden by default"
    );

    // Cycle 3: Resolved -> Open
    // Even when hidden in callouts, cycling while on coordinate still works
    let cycled3 = state.cycle_note_status();
    assert!(cycled3);
    let n = notes_for_file(&state.notes, "test_repo", "", "filesystem.rs")
        .next()
        .unwrap();
    assert_eq!(n.status, NoteStatus::Open);
    assert_eq!(state.output.files[0].counts.notes, 1);
    assert_eq!(state.output.files[0].counts.in_progress_notes, 0);
    assert_eq!(state.output.files[0].counts.resolved_notes, 0);
    assert!(state.status_message.as_ref().unwrap().0.contains("open"));

    let callouts_reopened = state.note_callouts(80);
    assert_eq!(callouts_reopened.len(), 1);
    assert!(callouts_reopened.values().next().unwrap()[0].contains('💬'));
}

#[test]
fn test_toggle_show_resolved_notes() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.row_idx = 1;
    state.note_draft = Some("Resolved note test".into());
    state.commit_note_draft().expect("note commit must succeed");

    // Mark resolved
    state.vis_line = 2;
    state.cycle_note_status(); // InProgress
    state.cycle_note_status(); // Resolved

    assert!(!state.show_resolved_notes);
    assert!(state.note_callouts(80).is_empty());

    // Toggle on
    state.toggle_show_resolved_notes();
    assert!(state.show_resolved_notes);
    let callouts = state.note_callouts(80);
    assert_eq!(callouts.len(), 1);
    assert!(callouts.values().next().unwrap()[0].contains('✅'));

    // Toggle off
    state.toggle_show_resolved_notes();
    assert!(!state.show_resolved_notes);
    assert!(state.note_callouts(80).is_empty());
}

#[test]
fn test_toggle_show_note_source() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    // Create an unanchored note on the banner (with original_source_rows)
    let banner_idx = state.first_row_of_file(state.current_file_idx());
    let file = state.output.files[state.current_file_idx()].path.clone();
    let unanchored_note = ReviewNote {
        id: "unanchored-src-1".into(),
        file: file.clone(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            9999,
        )),
        text: "Feedback on unanchored snippet".into(),
        created_at: 100,
        original_source_rows: vec![diffuse_core::wire::OutputRow {
            kind: diffuse_core::parse::RowKind::Add,
            mov_side: None,
            new_line: Some(9999),
            old_line: None,
            runs: vec![diffuse_core::wire::OutputRun::new(
                diffuse_core::merge::RunKind::Add,
                "let test_var = 123;".to_string(),
            )],
            wholesale: false,
            group: None,
        }],
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", unanchored_note);
    state.bump_notes();

    let src = "let test_var = 123;";

    assert!(!state.show_note_source);
    let callouts_no_src = state.note_callouts(80);
    let lines_no_src = &callouts_no_src[&banner_idx];
    assert!(
        !lines_no_src
            .iter()
            .any(|l| crate::render::strip_ansi(l).contains(src.trim()))
    );
    assert!(
        lines_no_src
            .iter()
            .any(|l| l.contains("Feedback on unanchored snippet"))
    );

    // Toggle on
    state.toggle_show_note_source();
    assert!(state.show_note_source);
    let callouts_src = state.note_callouts(80);
    let lines_src = &callouts_src[&banner_idx];
    assert!(
        lines_src
            .iter()
            .any(|l| crate::render::strip_ansi(l).contains(src.trim()))
    );
    assert!(
        lines_src
            .iter()
            .any(|l| l.contains("Feedback on unanchored snippet"))
    );

    // Toggle off
    state.toggle_show_note_source();
    assert!(!state.show_note_source);
    let callouts_off = state.note_callouts(80);
    let lines_off = &callouts_off[&banner_idx];
    assert!(
        !lines_off
            .iter()
            .any(|l| crate::render::strip_ansi(l).contains(src.trim()))
    );
    assert!(
        lines_off
            .iter()
            .any(|l| l.contains("Feedback on unanchored snippet"))
    );
}

#[test]
fn test_show_source_only_on_unanchored_notes() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let content_row = state
        .flat
        .iter()
        .position(|r| {
            state
                .row_of(r)
                .is_some_and(|row| !row.new_text().trim().is_empty())
        })
        .unwrap();
    state.row_idx = content_row;
    state.note_draft = Some("Anchored note".into());
    state.commit_note_draft().expect("note commit must succeed");

    // Also create an unanchored note on the banner (with original_source_rows)
    let banner_idx = state.first_row_of_file(state.current_file_idx());
    let file = state.output.files[state.current_file_idx()].path.clone();
    let unanchored_note = ReviewNote {
        id: "unanchored-note-1".into(),
        file: file.clone(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            9999,
        )),
        text: "Unanchored note on banner".into(),
        created_at: 200,
        original_source_rows: vec![diffuse_core::wire::OutputRow {
            kind: diffuse_core::parse::RowKind::Add,
            mov_side: None,
            new_line: Some(9999),
            old_line: None,
            runs: vec![diffuse_core::wire::OutputRun::new(
                diffuse_core::merge::RunKind::Add,
                "unanchored_original_code()".to_string(),
            )],
            wholesale: false,
            group: None,
        }],
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", unanchored_note);
    state.bump_notes();

    // Toggle show_note_source on
    state.toggle_show_note_source();
    assert!(state.show_note_source);
    let callouts_on = state.note_callouts(80);

    // Anchored note at content_row must NOT display original source rows!
    let anchored_callout = &callouts_on[&content_row];
    let anchored_src = {
        let n = notes_for_file(&state.notes, "test_repo", "", &file)
            .find(|n| n.text == "Anchored note")
            .unwrap();
        n.original_source_rows[0].text()
    };
    assert!(
        !anchored_callout
            .iter()
            .any(|l| crate::render::strip_ansi(l).contains(anchored_src.trim())),
        "anchored notes must NEVER display source lines in the TUI even when show_note_source is on"
    );

    // Unanchored note at banner_idx MUST display original source rows *above* the
    // note callout!
    let banner_callout_on = &callouts_on[&banner_idx];
    let src_idx = banner_callout_on
        .iter()
        .position(|l| crate::render::strip_ansi(l).contains("unanchored_original_code()"))
        .expect("unanchored original code must be present");
    let note_idx = banner_callout_on
        .iter()
        .position(|l| l.contains("Unanchored note on banner"))
        .expect("note text must be present");
    assert!(
        src_idx < note_idx,
        "original source line (idx {src_idx}) must appear above the note callout (idx {note_idx})"
    );

    // Layout helper for row must also reflect this distinction
    let anchored_layout = state.note_callout_layout_for_row(content_row, 80);
    assert_eq!(anchored_layout.len(), 1);
    assert_eq!(
        anchored_layout[0].1, 1,
        "anchored note callout line count must not include source rows"
    );

    let banner_layout = state.note_callout_layout_for_row(banner_idx, 80);
    assert!(
        banner_layout
            .iter()
            .any(|(n, lines)| n.id == "unanchored-note-1" && *lines > 1),
        "unanchored note callout line count must include source rows when show_note_source is true"
    );
}

#[test]
fn test_event_keybindings_x_z_and_shift_s() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.row_idx = 1;
    state.note_draft = Some("Keybinding test".into());
    state.commit_note_draft().expect("note commit must succeed");

    // Position on note
    state.vis_line = 2;

    // Press 'x': cycle note status
    crate::event::handle_key_event(
        &mut state,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
    );
    let n = notes_for_file(&state.notes, "test_repo", "", "filesystem.rs")
        .next()
        .unwrap();
    assert_eq!(n.status, NoteStatus::InProgress);

    // Press 'z': toggle show resolved notes
    assert!(!state.show_resolved_notes);
    crate::event::handle_key_event(
        &mut state,
        KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
    );
    assert!(state.show_resolved_notes);

    // Press 'S' (Shift+S): toggle show source lines
    assert!(!state.show_note_source);
    crate::event::handle_key_event(
        &mut state,
        KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT),
    );
    assert!(state.show_note_source);
}

#[test]
fn test_sticky_banner_note_scrolls_line_by_line_beneath_sticky_header() {
    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    let banner_idx = state.first_row_of_file(state.current_file_idx());
    let file = state.output.files[state.current_file_idx()].path.clone();

    // Add a multi-line note on the banner (unanchored)
    let note = ReviewNote {
        id: "banner-multi".into(),
        file: file.clone(),
        location: crate::core::notes::NoteLocation::line(
            crate::core::notes::DiffCoord::new_side(9999),
        ),
        text: "Ambiguity guard test: line content matches but chunk is not unique across the file. The uniqueness check prevents false re-anchoring and safely places the note onto the file header banner.".into(),
        created_at: 100,
        ..Default::default()
    };
    add_note(&mut state.notes, "test_repo", "", note);
    state.bump_notes();

    let callouts = state.note_callouts(80);
    assert!(callouts.contains_key(&banner_idx));
    let note_lines = callouts.get(&banner_idx).unwrap();
    assert!(
        note_lines.len() >= 3,
        "must render into multiple callout lines: got {}",
        note_lines.len()
    );

    // At scroll = 0, screen row 0 is the file banner.
    // Screen row 1 is line 0 of the note ("Ambiguity guard test").
    // Screen row 2 is line 1 of the note ("uniqueness check").
    state.scroll = 0;
    let (lines_0, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_0[0].contains("════"), "row 0 must be file banner");
    assert!(
        lines_0[1].contains("Ambiguity guard test"),
        "row 1 must be note line 0, got: {:?}",
        lines_0[1]
    );
    assert!(
        lines_0[2].contains("uniqueness check"),
        "row 2 must be note line 1, got: {:?}",
        lines_0[2]
    );

    // Now scroll down by 1 line (scroll = 1):
    // Sticky banner activates on row 0.
    // Note line 0 scrolls under the sticky banner.
    // Screen row 1 must smoothly show note line 1 ("uniqueness check")!
    state.scroll = 1;
    let (lines_1, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_1[0].contains("════"), "row 0 must be sticky banner");
    assert!(
        lines_1[1].contains("uniqueness check"),
        "row 1 must smoothly show note line 1 beneath the sticky header, got: {:?}",
        lines_1[1]
    );

    // Scroll down by 2 lines (scroll = 2):
    // Screen row 1 must smoothly show note line 2 ("places the note")!
    state.scroll = 2;
    let (lines_2, _, _, _, _) = state.render_diff_viewport_lines(80, 10, 10);
    assert!(lines_2[0].contains("════"), "row 0 must be sticky banner");
    assert!(
        lines_2[1].contains("places the note"),
        "row 1 must smoothly show note line 2 beneath the sticky header, got: {:?}",
        lines_2[1]
    );
}

#[test]
fn test_cross_hunk_selection_bridges_and_merges_hunks() {
    let diff = "\
diff --git a/test.rs b/test.rs
--- a/test.rs
+++ b/test.rs
@@ -10,3 +10,3 @@
 line 10
-line 11 old
+line 11 new
 line 12
@@ -30,3 +30,3 @@
 line 30
-line 31 old
+line 31 new
 line 32
";
    let file_lines: Vec<String> = (1..=50)
        .map(|i| {
            if i == 11 {
                "line 11 new".to_string()
            } else if i == 31 {
                "line 31 new".to_string()
            } else {
                format!("line {i}")
            }
        })
        .collect();
    let content = file_lines.join("\n") + "\n";
    let mut new_map = HashMap::new();
    new_map.insert("test.rs".to_string(), content.clone());

    // Without notes, pipeline produces 2 hunks: @@ -10,3 @@ and @@ -30,3 @@
    let base_out = run_pipeline_full(
        diff,
        Some(new_map.clone()),
        None,
        PipelineOptions::default(),
    );
    assert_eq!(
        base_out.files[0].hunks.len(),
        2,
        "must initially have 2 hunks"
    );

    // Add note spanning lines 11 to 31 (spans both hunks and the gap between
    // them!). Provisional capture: plain context rows over the span, exactly
    // as a fresh note records its selection before relocation.
    let provisional_rows: Vec<crate::core::wire::OutputRow> = (11u32..=31)
        .map(|n| crate::core::wire::OutputRow {
            kind: crate::core::parse::RowKind::Ctx,
            mov_side: None,
            old_line: Some(n),
            new_line: Some(n),
            runs: vec![crate::core::wire::OutputRun::new(
                crate::core::merge::RunKind::Ctx,
                file_lines[(n - 1) as usize].clone(),
            )],
            wholesale: false,
            group: None,
        })
        .collect();
    let note = ReviewNote {
        id: "cross-hunk".into(),
        file: "test.rs".into(),
        location: crate::core::notes::NoteLocation::Range {
            start: crate::core::notes::DiffCoord::from_row(&provisional_rows[0]).unwrap(),
            end: crate::core::notes::DiffCoord::from_row(provisional_rows.last().unwrap()).unwrap(),
        },
        text: "Spans across hunk 1 and hunk 2".into(),
        original_source_rows: provisional_rows,
        content_hash: None,
        ..Default::default()
    };

    let mut db = HashMap::new();
    add_note(&mut db, "test_repo", "", note);

    let opts = PipelineOptions::default()
        .with_notes(Some(db.clone()), Some("test_repo".to_string()), None)
        .with_visual_context(4);

    let out = run_pipeline_full(diff, Some(new_map), None, opts);
    // The synthetic context bridges the gap, causing merge_adjacent_hunks to merge
    // them into 1!
    assert_eq!(
        out.files[0].hunks.len(),
        1,
        "bridged hunks must merge into a single hunk"
    );

    // Relocation: re-anchor the note to the actual output rows (the same
    // flow `diffuse notes create` applies after pipeline synthesis).
    let f = &out.files[0];
    let window: Vec<crate::core::wire::OutputRow> = f
        .rows
        .iter()
        .filter(|r| matches!(r.new_line, Some(n) if (11..=31).contains(&n)))
        .cloned()
        .collect();
    assert_eq!(window.len(), 21, "the merged hunk must cover the full span");
    let relocated = ReviewNote {
        location: crate::core::notes::NoteLocation::Range {
            start: crate::core::notes::DiffCoord::from_row(&window[0]).unwrap(),
            end: crate::core::notes::DiffCoord::from_row(window.last().unwrap()).unwrap(),
        },
        original_source_rows: window.clone(),
        content_hash: LineHash::of_rows(&window),
        ..db["test_repo"][""]["test.rs"][0].clone()
    };
    let mut db = HashMap::new();
    add_note(&mut db, "test_repo", "", relocated);

    let mut state = AppState::new(
        out,
        "test_repo".into(),
        temp_notes_path(),
        crate::render::DiffuseTheme::dark(),
    );
    state.notes = db;

    // The note must anchor inline to line 31, not on the file banner!
    let banner_idx = state.first_row_of_file(0);
    let callouts = state.note_callouts(80);
    assert!(
        !callouts.contains_key(&banner_idx),
        "must not be on file banner"
    );

    let target_row = state.note_target_row(0, &state.notes["test_repo"][""]["test.rs"][0]);
    assert_ne!(
        target_row, banner_idx,
        "target row must be inside the merged hunk"
    );
    assert!(
        callouts.contains_key(&target_row),
        "callout must be placed at target row"
    );
}

#[test]
fn test_diff_notes_db_resolved_and_unresolved() {
    let mut old_db = HashMap::new();
    let note1 = ReviewNote {
        id: "note-1".into(),
        file: "src/lib.rs".into(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            10,
        )),
        text: "Please fix this".into(),
        status: NoteStatus::Open,
        ..Default::default()
    };
    add_note(&mut old_db, "repo", "", note1.clone());

    let mut new_db = old_db.clone();
    let n = new_db
        .get_mut("repo")
        .unwrap()
        .get_mut("")
        .unwrap()
        .get_mut("src/lib.rs")
        .unwrap()
        .get_mut(0)
        .unwrap();
    n.status = NoteStatus::Resolved;

    let summary = AppState::diff_notes_db(&old_db, &new_db, Some("repo"));
    assert_eq!(summary.active_delta, -1);
    assert_eq!(summary.resolved_delta, 1);
    assert_eq!(
        summary.format_message(),
        "↻ notes updated (-1 active, +1 resolved)"
    );

    // And back: un-resolving note
    let rev_summary = AppState::diff_notes_db(&new_db, &old_db, Some("repo"));
    assert_eq!(rev_summary.active_delta, 1);
    assert_eq!(rev_summary.resolved_delta, -1);
    assert_eq!(
        rev_summary.format_message(),
        "↻ notes updated (+1 active, -1 resolved)"
    );
}

#[test]
fn test_diff_notes_db_add_delete_edit() {
    let mut db1 = HashMap::new();
    let note1 = ReviewNote {
        id: "note-1".into(),
        file: "src/lib.rs".into(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            10,
        )),
        text: "Original text".into(),
        status: NoteStatus::Open,
        ..Default::default()
    };
    add_note(&mut db1, "repo", "", note1);

    // Addition
    let mut db2 = db1.clone();
    let note2 = ReviewNote {
        id: "note-2".into(),
        file: "src/lib.rs".into(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            20,
        )),
        text: "New note".into(),
        status: NoteStatus::Open,
        ..Default::default()
    };
    add_note(&mut db2, "repo", "", note2);

    let summary_add = AppState::diff_notes_db(&db1, &db2, Some("repo"));
    assert_eq!(summary_add.added, 1);
    assert_eq!(summary_add.format_message(), "↻ notes updated (+1 note)");

    // Deletion
    let summary_del = AppState::diff_notes_db(&db2, &db1, Some("repo"));
    assert_eq!(summary_del.deleted, 1);
    assert_eq!(summary_del.format_message(), "↻ notes updated (-1 note)");

    // Edit
    let mut db3 = db1.clone();
    db3.get_mut("repo")
        .unwrap()
        .get_mut("")
        .unwrap()
        .get_mut("src/lib.rs")
        .unwrap()
        .get_mut(0)
        .unwrap()
        .text = "Updated text".into();
    let summary_edit = AppState::diff_notes_db(&db1, &db3, Some("repo"));
    assert_eq!(summary_edit.edited, 1);
    assert_eq!(
        summary_edit.format_message(),
        "↻ notes updated (1 note edited)"
    );
}

#[test]
fn test_reload_notes_from_disk_updates_counts_and_hides_resolved() {
    use diffuse_core::notes::save_notes;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let notes_path = tmp.path().to_path_buf();

    let mut db = HashMap::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "filesystem.rs".into(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            1025,
        )),
        text: "Review feedback".into(),
        status: NoteStatus::Open,
        ..Default::default()
    };
    add_note(&mut db, "test_repo", "", note);
    save_notes(&notes_path, &db).unwrap();

    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path.clone(),
        crate::render::DiffuseTheme::dark(),
    );
    state.notes = db.clone();
    state.sync_all_notes();

    assert_eq!(state.output.files[0].counts.notes, 1);
    assert_eq!(state.output.files[0].counts.resolved_notes, 0);
    assert_eq!(state.output.total_counts().notes, 1);

    // Callout visible when not resolved
    let callouts = state.note_callouts(80);
    assert!(!callouts.is_empty());

    // No change on disk -> reload returns None
    assert_eq!(state.reload_notes_from_disk(), None);

    // Now resolve note on disk
    let mut resolved_db = db.clone();
    resolved_db
        .get_mut("test_repo")
        .unwrap()
        .get_mut("")
        .unwrap()
        .get_mut("filesystem.rs")
        .unwrap()
        .get_mut(0)
        .unwrap()
        .status = NoteStatus::Resolved;
    save_notes(&notes_path, &resolved_db).unwrap();

    // Reload from disk
    let msg = state.reload_notes_from_disk();
    assert_eq!(
        msg,
        Some("↻ notes updated (-1 active, +1 resolved)".to_string())
    );

    // Verify file counts updated correctly (-1 active, +1 resolved)
    assert_eq!(state.output.files[0].counts.notes, 0);
    assert_eq!(state.output.files[0].counts.resolved_notes, 1);

    // When show_resolved_notes is false (default), resolved note is hidden/removed
    assert!(!state.show_resolved_notes);
    let callouts_after = state.note_callouts(80);
    assert!(
        callouts_after.is_empty(),
        "resolved note must be hidden when show_resolved_notes is false"
    );

    // When show_resolved_notes is true, resolved note appears
    state.show_resolved_notes = true;
    let callouts_resolved = state.note_callouts(80);
    assert!(
        !callouts_resolved.is_empty(),
        "resolved note must appear when show_resolved_notes is true"
    );
}

#[test]
fn test_reload_notes_from_disk_cursor_on_resolved_note_is_clamped_safely() {
    use diffuse_core::notes::save_notes;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let notes_path = tmp.path().to_path_buf();

    let mut db = HashMap::new();
    let note = ReviewNote {
        id: "n1".into(),
        file: "filesystem.rs".into(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            1025,
        )),
        text: "Line 1\nLine 2".into(),
        status: NoteStatus::Open,
        ..Default::default()
    };
    add_note(&mut db, "test_repo", "", note);
    save_notes(&notes_path, &db).unwrap();

    let mut state = AppState::new(
        sample_output(),
        "test_repo".into(),
        notes_path.clone(),
        crate::render::DiffuseTheme::dark(),
    );
    state.notes = db.clone();
    state.sync_all_notes();

    let target_row = state.note_target_row(0, &state.notes["test_repo"][""]["filesystem.rs"][0]);
    state.row_idx = target_row;
    state.vis_line = 1;

    // Resolve note on disk
    let mut resolved_db = db.clone();
    resolved_db
        .get_mut("test_repo")
        .unwrap()
        .get_mut("")
        .unwrap()
        .get_mut("filesystem.rs")
        .unwrap()
        .get_mut(0)
        .unwrap()
        .status = NoteStatus::Resolved;
    save_notes(&notes_path, &resolved_db).unwrap();

    // Reload from disk (show_resolved_notes is false)
    let _ = state.reload_notes_from_disk();

    // vis_line must be clamped back to 0 (parent row), without out-of-bounds index
    assert_eq!(state.vis_line, 0);
}

#[test]
fn test_diff_notes_db_in_progress_and_plural_formatting() {
    let mut db1 = HashMap::new();
    let note1 = ReviewNote {
        id: "note-1".into(),
        file: "src/lib.rs".into(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            10,
        )),
        text: "Note 1".into(),
        status: NoteStatus::Open,
        ..Default::default()
    };
    let note2 = ReviewNote {
        id: "note-2".into(),
        file: "src/lib.rs".into(),
        location: crate::core::notes::NoteLocation::line(crate::core::notes::DiffCoord::new_side(
            20,
        )),
        text: "Note 2".into(),
        status: NoteStatus::Open,
        ..Default::default()
    };
    add_note(&mut db1, "repo", "", note1);
    add_note(&mut db1, "repo", "", note2);

    // Transition note1 from Open -> InProgress
    let mut db2 = db1.clone();
    db2.get_mut("repo")
        .unwrap()
        .get_mut("")
        .unwrap()
        .get_mut("src/lib.rs")
        .unwrap()
        .get_mut(0)
        .unwrap()
        .status = NoteStatus::InProgress;

    let summary = AppState::diff_notes_db(&db1, &db2, Some("repo"));
    assert_eq!(summary.in_progress_delta, 1);
    assert_eq!(summary.format_message(), "↻ notes updated (+1 in progress)");

    // Plural additions and deletions
    let empty_db = HashMap::new();
    let add_two = AppState::diff_notes_db(&empty_db, &db1, Some("repo"));
    assert_eq!(add_two.added, 2);
    assert_eq!(add_two.format_message(), "↻ notes updated (+2 notes)");

    let del_two = AppState::diff_notes_db(&db1, &empty_db, Some("repo"));
    assert_eq!(del_two.deleted, 2);
    assert_eq!(del_two.format_message(), "↻ notes updated (-2 notes)");
}
