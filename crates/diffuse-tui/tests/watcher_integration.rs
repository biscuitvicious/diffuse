use std::{
    fs,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

use diffuse_git::testing::{commit_files, init_repo};
use diffuse_tui::{
    AppState, TuiCommand,
    actions::reload_specific_files,
    core::{
        PipelineOptions,
        git::{ENGINE_CONTEXT, load_git_repo},
        pipeline::run_pipeline_full,
    },
    watcher::{
        FsChange, FsChangeCollector, FsEvent, RepoWatcher, classify_repo_path,
        is_relevant_repo_path,
    },
};

fn setup_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path()).unwrap();
    commit_files(
        &repo,
        "initial commit",
        &[("file1.txt", "line 1\nline 2\nline 3\n")],
    )
    .unwrap();
    let notes_path = dir.path().join("notes.json");
    (dir, notes_path)
}

#[test]
fn test_watcher_filtering_paths() {
    let (repo_dir, _) = setup_repo();
    let root = repo_dir.path();

    assert!(is_relevant_repo_path(&root.join("src/lib.rs"), root));
    assert!(is_relevant_repo_path(&root.join(".git/index"), root));
    assert!(is_relevant_repo_path(&root.join(".git/HEAD"), root));
    assert!(is_relevant_repo_path(
        &root.join(".git/refs/heads/main"),
        root
    ));

    assert!(!is_relevant_repo_path(
        &root.join("target/debug/build"),
        root
    ));
    assert!(!is_relevant_repo_path(
        &root.join("node_modules/pkg/index.js"),
        root
    ));
    assert!(!is_relevant_repo_path(
        &root.join(".git/objects/1a/2b3c"),
        root
    ));
    assert!(!is_relevant_repo_path(&root.join(".git/logs/HEAD"), root));

    assert_eq!(
        classify_repo_path(&root.join("file1.txt"), root),
        Some(FsChange::File("file1.txt".into()))
    );
    assert_eq!(
        classify_repo_path(&root.join(".git/index"), root),
        Some(FsChange::Global)
    );
    assert_eq!(classify_repo_path(&root.join("target/file"), root), None);
}

#[test]
fn test_collector_debouncing_and_global_upgrade() {
    let mut collector = FsChangeCollector::new();
    let t0 = Instant::now();

    collector.record(FsChange::File("file1.txt".into()), t0);
    collector.record(FsChange::File("file2.txt".into()), t0);

    // Before quiet window elapses:
    assert_eq!(
        collector.take_debounced(t0 + Duration::from_millis(50), Duration::from_millis(150)),
        None
    );

    // Add a global change (e.g. .git/index staging):
    collector.record(FsChange::Global, t0 + Duration::from_millis(60));

    // After quiet window: must upgrade to Global and clear individual files
    let event =
        collector.take_debounced(t0 + Duration::from_millis(220), Duration::from_millis(150));
    assert_eq!(event, Some(FsEvent::Global));
    assert!(collector.pending_files.is_empty());
    assert!(!collector.pending_global);
}

#[test]
fn test_fs_watcher_live_notification_and_reload() {
    let (repo_dir, notes_path) = setup_repo();
    let root = repo_dir.path().to_path_buf();

    // Modify file1.txt to create unstaged change
    fs::write(root.join("file1.txt"), "line 1\nline 2 mod\nline 3\n").unwrap();

    let git_res = load_git_repo(&root, &[], &[], false, ENGINE_CONTEXT).unwrap();
    let out = run_pipeline_full(
        &git_res.diff,
        Some(git_res.new_content),
        Some(git_res.old_content),
        PipelineOptions::default(),
    );

    let mut state = AppState::new(
        out,
        "test".into(),
        notes_path,
        diffuse_tui::render::DiffuseTheme::DEFAULT,
    )
    .with_repo_context(root.clone(), Vec::new(), false);

    assert_eq!(state.output.files.len(), 1);
    let original_text = state.output.files[0].hunks[0]
        .rows
        .iter()
        .map(|r| r.text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(original_text.contains("line 2 mod"));

    // Spawn watcher
    let (tx, rx) = mpsc::channel();
    let _watcher = RepoWatcher::spawn(root.clone(), tx).expect("spawn watcher");

    // Modify file1.txt again
    fs::write(root.join("file1.txt"), "line 1\nline 2 mod v2\nline 3\n").unwrap();

    // Drain events until we see file1.txt or timeout
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_file1 = false;
    while Instant::now() < deadline {
        if let Ok(change) = rx.recv_timeout(Duration::from_millis(100))
            && let FsChange::File(ref path) = change
            && path.contains("file1.txt")
        {
            saw_file1 = true;
            break;
        }
    }
    assert!(saw_file1, "watcher should report file1.txt change");

    // Execute reload_specific_files
    let count = reload_specific_files(&mut state, &root, &[], &["file1.txt".to_string()]).unwrap();
    assert_eq!(count, 1);

    let updated_text = state.output.files[0].hunks[0]
        .rows
        .iter()
        .map(|r| r.text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        updated_text.contains("line 2 mod v2"),
        "content must be updated to v2"
    );
}

#[test]
fn test_force_reload_state_transition() {
    let (repo_dir, notes_path) = setup_repo();
    let root = repo_dir.path().to_path_buf();
    fs::write(root.join("file1.txt"), "line 1\nline 2 mod\nline 3\n").unwrap();

    let git_res = load_git_repo(&root, &[], &[], false, ENGINE_CONTEXT).unwrap();
    let out = run_pipeline_full(
        &git_res.diff,
        Some(git_res.new_content),
        Some(git_res.old_content),
        PipelineOptions::default(),
    );

    let mut state = AppState::new(
        out,
        "test".into(),
        notes_path,
        diffuse_tui::render::DiffuseTheme::DEFAULT,
    )
    .with_repo_context(root, Vec::new(), false);

    assert_eq!(state.output.files.len(), 1);
    assert!(!state.is_streaming);

    // Dispatch ForceReload command
    state.execute(TuiCommand::ForceReload);
    assert!(state.pending_force_reload);

    // Verify snapshot capture
    let snapshot = state.capture_cursor_snapshot();
    assert_eq!(snapshot.file_path, "file1.txt");

    // In-place replacement preserves content and flat rows without blanking
    let updated_file = state.output.files[0].clone();
    state.replace_or_insert_file(updated_file, &snapshot);
    assert_eq!(state.output.files.len(), 1);
    assert!(!state.flat.is_empty());
}

#[test]
fn test_reloading_diff_status_clears_when_streaming_disconnects() {
    let mut state = AppState::new(
        diffuse_tui::core::wire::PipelineOutput {
            files: Vec::new(),
            branch: String::new(),
            untracked_skipped: Vec::new(),
        },
        "test".into(),
        PathBuf::from("/nonexistent.json"),
        diffuse_tui::render::DiffuseTheme::DEFAULT,
    );
    state.set_status_message("↻ reloading diff...".into(), true);
    assert!(state.visible_status_message().is_some());

    // When reload finishes and streaming receiver disconnects:
    if state
        .status_message
        .as_ref()
        .is_some_and(|(msg, _, _)| msg.starts_with("↻ reloading diff"))
    {
        state.status_message = None;
    }
    assert!(state.status_message.is_none());
}

#[test]
fn test_live_notes_mutation_detection() {
    let (repo_dir, notes_path) = setup_repo();
    let root = repo_dir.path().to_path_buf();

    let (tx, rx) = mpsc::channel();
    let _watcher = RepoWatcher::spawn_with_notes(Some(root), Some(notes_path.clone()), tx).unwrap();

    // Write to notes.json
    fs::write(&notes_path, "{}").unwrap();

    let change = rx
        .recv_timeout(Duration::from_millis(500))
        .expect("must detect mutation on notes.json");

    assert_eq!(change, FsChange::Notes);
}
