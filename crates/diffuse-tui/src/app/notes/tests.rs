use crate::{
    app::AppState,
    core::{
        PipelineOptions,
        notes::{
            LineHash, ReviewNote, Side, add_note, hash_line_content, load_notes, notes_for_file,
        },
        pipeline::run_pipeline_full,
        wire::PipelineOutput,
    },
    render::ColorExt,
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

    state.delete_notes();
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
fn test_note_commit_records_line_hash() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(sample_output(), "test_repo".into(), notes_path);

    let file = state.current_file().unwrap().path.clone();
    state.note_draft = Some("note with hash".into());
    state.commit_note_draft();

    let notes: Vec<_> = notes_for_file(&state.notes, "test_repo", &file).collect();
    assert_eq!(notes.len(), 1);
    let hash = notes[0].line_hash.expect("line_hash must be recorded");
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
    let mut state = AppState::new(sample_output(), "test_repo".into(), notes_path);
    let file = state.current_file().unwrap().path.clone();
    let banner_idx = state.first_row_of_file(state.current_file_idx());

    // 1. Note with matching line_hash anchors cleanly -> inline callout
    //    produced
    let matching_hash = hash_line_content("    let x = 1;");
    let valid_note = ReviewNote {
        id: "valid".into(),
        file: file.clone(),
        side: Side::New,
        start: 1025,
        end: 1025,
        text: "valid note".into(),
        created_at: 100,
        line_hash: Some(matching_hash),
    };
    add_note(&mut state.notes, &state.notes_key, valid_note);
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

    // 2. Note with mismatching line_hash -> displayed on file banner
    state.notes.clear();
    let mismatch_note = ReviewNote {
        id: "mismatch".into(),
        file: file.clone(),
        side: Side::New,
        start: 1025,
        end: 1025,
        text: "mismatch note".into(),
        created_at: 101,
        line_hash: Some(LineHash::from_hex("deadbeef").unwrap()),
    };
    add_note(&mut state.notes, &state.notes_key, mismatch_note);
    let callouts_mismatch = state.note_callouts(80);
    assert_eq!(
        callouts_mismatch.len(),
        1,
        "mismatched line_hash note must produce a banner callout"
    );
    assert!(
        callouts_mismatch.contains_key(&banner_idx),
        "mismatched line_hash note must display attached to the file banner"
    );

    // 3. Note on nonexistent line number -> displayed on file banner
    state.notes.clear();
    let nonexistent_line_note = ReviewNote {
        id: "nonexistent".into(),
        file: file.clone(),
        side: Side::New,
        start: 99999,
        end: 99999,
        text: "orphan note".into(),
        created_at: 102,
        line_hash: Some(LineHash::from_hex("87654321").unwrap()),
    };
    add_note(&mut state.notes, &state.notes_key, nonexistent_line_note);
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

    // 4. Note without line_hash -> displayed on file banner
    state.notes.clear();
    let legacy_note = ReviewNote {
        id: "legacy".into(),
        file: file.clone(),
        side: Side::New,
        start: 1025,
        end: 1025,
        text: "legacy note".into(),
        created_at: 103,
        line_hash: None,
    };
    add_note(&mut state.notes, &state.notes_key, legacy_note);
    let callouts_legacy = state.note_callouts(80);
    assert_eq!(
        callouts_legacy.len(),
        1,
        "legacy note without hash must produce a banner callout"
    );
    assert!(
        callouts_legacy.contains_key(&banner_idx),
        "legacy note without hash must display attached to the file banner"
    );
}

#[test]
fn test_notes_dynamic_sync_updates_counts_and_preserves_banner_notes() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(sample_output(), "test_repo".into(), notes_path);
    let file = state.current_file().unwrap().path.clone();
    let banner_idx = state.first_row_of_file(state.current_file_idx());

    // Initially zero notes
    assert_eq!(state.output.files[0].counts.notes, 0);
    assert_eq!(state.output.total_counts().notes, 0);

    // Commit note via draft -> file counts and total counts dynamically update
    // to 1
    state.note_draft = Some("first dynamic note".into());
    state.commit_note_draft();
    assert_eq!(state.output.files[0].counts.notes, 1);
    assert_eq!(state.output.total_counts().notes, 1);
    assert_eq!(state.output.files[0].notes.len(), 1);

    // Add an unanchored note (mismatched hash) directly to notes DB and sync
    add_note(
        &mut state.notes,
        &state.notes_key,
        ReviewNote {
            id: "banner-note".into(),
            file: file.clone(),
            side: Side::New,
            start: 99999,
            end: 99999,
            text: "unanchored note".into(),
            created_at: 104,
            line_hash: None,
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
    state.delete_notes();
    assert_eq!(state.output.files[0].counts.notes, 1);
    assert_eq!(state.output.total_counts().notes, 1);
}

#[test]
fn test_note_callouts_render_across_all_files_and_banners() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(multi_file_output(), "test_repo".into(), notes_path);

    // Initial cursor is on file 0 (first.rs)
    assert_eq!(state.current_file_idx(), 0);

    let banner_1 = state.first_row_of_file(1);

    // Add unanchored note to second.rs (file 1)
    add_note(
        &mut state.notes,
        &state.notes_key,
        ReviewNote {
            id: "note-file-1".into(),
            file: "second.rs".into(),
            side: Side::New,
            start: 999,
            end: 999,
            text: "second file banner note".into(),
            created_at: 200,
            line_hash: None,
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
    let mut state = AppState::new(multi_file_output(), "test_repo".into(), notes_path);

    let matching_hash = hash_line_content("    let a = 20;");
    add_note(
        &mut state.notes,
        &state.notes_key,
        ReviewNote {
            id: "inline-note-file-1".into(),
            file: "second.rs".into(),
            side: Side::New,
            start: 2,
            end: 2,
            text: "inline note on second.rs".into(),
            created_at: 201,
            line_hash: Some(matching_hash),
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
    let mut state = AppState::new(multi_file_output(), "test_repo".into(), notes_path);

    let note_text = "Test *Markdown* **formatting:**\n\n`this is a code span` with a comment.\n\n* List item one\n* List item two with a very long line that should certainly wrap when the terminal width is constrained\n\n```bash\nif [ -f hello.txt ]; then\n  cat hello.txt\nfi\n```";

    let banner_idx = state.first_row_of_file(0);
    add_note(
        &mut state.notes,
        &state.notes_key,
        ReviewNote {
            id: "md-note".into(),
            file: "first.rs".into(),
            side: Side::New,
            start: 9999,
            end: 9999,
            text: note_text.into(),
            created_at: 300,
            line_hash: None,
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
    let mut state = AppState::new(multi_file_output(), "test_repo".into(), notes_path);
    let banner_idx = state.first_row_of_file(0);

    add_note(
        &mut state.notes,
        &state.notes_key,
        ReviewNote {
            id: "gutter-note".into(),
            file: "first.rs".into(),
            side: Side::New,
            start: 9999,
            end: 9999,
            text: "This is\na 4-line\nnote:\n`let a = 17;`".into(),
            created_at: 301,
            line_hash: None,
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
    let muted = state.theme.palette.note_tint_bg.bg_code();
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
    assert_eq!(plain[0], "▌███ 💬 This is a 4-line note: let a = 17;");
}

#[test]
fn test_note_callout_empty_text_renders_one_line() {
    let notes_file = tempfile::NamedTempFile::new().unwrap();
    let notes_path = notes_file.path().to_path_buf();
    let mut state = AppState::new(multi_file_output(), "test_repo".into(), notes_path);
    let banner_idx = state.first_row_of_file(0);

    add_note(
        &mut state.notes,
        &state.notes_key,
        ReviewNote {
            id: "empty-note".into(),
            file: "first.rs".into(),
            side: Side::New,
            start: 9999,
            end: 9999,
            text: String::new(),
            created_at: 302,
            line_hash: None,
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
    let mut state = AppState::new(multi_file_output(), "test_repo".into(), notes_path);
    let banner_idx = state.first_row_of_file(0);

    // A very long single line that will exceed the gutter + text_cols at
    // content_width = 50
    let long_line = "a".repeat(120);
    let note_text = format!("Short line\n{long_line}\nAnother line");

    add_note(
        &mut state.notes,
        &state.notes_key,
        ReviewNote {
            id: "wrap-test".into(),
            file: "first.rs".into(),
            side: Side::New,
            start: 9999,
            end: 9999,
            text: note_text,
            created_at: 400,
            line_hash: None,
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
