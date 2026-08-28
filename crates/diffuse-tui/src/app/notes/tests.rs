use crate::{
    app::AppState,
    core::{
        PipelineOptions,
        notes::{load_notes, notes_for_file},
        pipeline::run_pipeline_full,
        wire::PipelineOutput,
    },
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
fn test_note_commit_persists_and_deletes() {
    let notes_path = std::env::temp_dir().join(format!(
        "diffuse-test-note-lifecycle-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&notes_path);
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

    let _ = std::fs::remove_file(notes_path);
}
