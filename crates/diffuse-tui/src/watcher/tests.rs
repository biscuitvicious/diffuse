use std::path::Path;

use super::is_relevant_repo_path;

#[test]
fn test_relevant_path_accepts_working_tree_files() {
    let root = Path::new("/workspace/project");
    assert!(is_relevant_repo_path(
        Path::new("/workspace/project/src/lib.rs"),
        root
    ));
    assert!(is_relevant_repo_path(
        Path::new("/workspace/project/README.md"),
        root
    ));
    assert!(is_relevant_repo_path(
        Path::new("/workspace/project/crates/core/src/parser.rs"),
        root
    ));
}

#[test]
fn test_relevant_path_accepts_git_index_and_head() {
    let root = Path::new("/workspace/project");
    assert!(is_relevant_repo_path(
        Path::new("/workspace/project/.git/index"),
        root
    ));
    assert!(is_relevant_repo_path(
        Path::new("/workspace/project/.git/HEAD"),
        root
    ));
    assert!(is_relevant_repo_path(
        Path::new("/workspace/project/.git/MERGE_HEAD"),
        root
    ));
}

#[test]
fn test_relevant_path_accepts_git_branch_refs() {
    let root = Path::new("/workspace/project");
    assert!(is_relevant_repo_path(
        Path::new("/workspace/project/.git/refs/heads/main"),
        root
    ));
    assert!(is_relevant_repo_path(
        Path::new("/workspace/project/.git/refs/heads/feature"),
        root
    ));
}

#[test]
fn test_relevant_path_rejects_git_objects_and_pack() {
    let root = Path::new("/workspace/project");
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/.git/objects/1a/2b3c4d"),
        root
    ));
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/.git/objects/pack/pack-1234.pack"),
        root
    ));
}

#[test]
fn test_relevant_path_rejects_git_logs_and_hooks() {
    let root = Path::new("/workspace/project");
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/.git/logs/HEAD"),
        root
    ));
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/.git/logs/refs/heads/main"),
        root
    ));
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/.git/hooks/pre-commit"),
        root
    ));
}

#[test]
fn test_relevant_path_rejects_target_and_build() {
    let root = Path::new("/workspace/project");
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/target/debug/diffuse"),
        root
    ));
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/node_modules/package/index.js"),
        root
    ));
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/dist/bundle.js"),
        root
    ));
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project/.cache/rustc/hash"),
        root
    ));
}

#[test]
fn test_relevant_path_rejects_outside_repo_or_root() {
    let root = Path::new("/workspace/project");
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/other/file.txt"),
        root
    ));
    assert!(!is_relevant_repo_path(
        Path::new("/workspace/project"),
        root
    ));
}

#[test]
fn test_repo_watcher_spawns_and_detects_mutation() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = super::RepoWatcher::spawn(root.clone(), tx).expect("spawn watcher");

    // Write a file in the tempdir
    let file = root.join("src").join("test.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "hello\n").unwrap();

    // Receive event within reasonable duration (e.g. 1000ms)
    let res = rx.recv_timeout(std::time::Duration::from_millis(1000));
    assert!(res.is_ok(), "must receive change pulse on file write");
}

#[test]
fn test_watchable_dirs_skip_gitignored_trees() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let repo = diffuse_git::testing::init_repo(&root).unwrap();
    diffuse_git::testing::commit_files(
        &repo,
        "init",
        &[
            (".gitignore", "target/\nignored/\n"),
            ("src/lib.rs", "fn f() {}\n"),
        ],
    )
    .unwrap();
    for dir in [
        "target/debug/deps",
        "ignored/nested",
        "crates/core/src",
        "src",
    ] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }

    let dirs: Vec<String> = super::watchable_dirs(&root, &root)
        .iter()
        .filter_map(|path| path.strip_prefix(&root).ok())
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    assert!(
        dirs.iter().any(|dir| dir == "src"),
        "content directories stay watched: {dirs:?}"
    );
    assert!(
        dirs.iter().any(|dir| dir == "crates/core/src"),
        "nested content directories stay watched: {dirs:?}"
    );
    assert!(
        !dirs.iter().any(|dir| dir.starts_with("target")),
        "gitignored trees are pruned: {dirs:?}"
    );
    assert!(
        !dirs.iter().any(|dir| dir.starts_with("ignored")),
        "gitignored trees are pruned: {dirs:?}"
    );
}

#[test]
fn test_repo_watcher_watches_directories_created_after_spawn() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = super::RepoWatcher::spawn(root.clone(), tx).expect("spawn watcher");

    let nested = root.join("pkg/nested");
    std::fs::create_dir_all(&nested).unwrap();
    // A directory is registered from its creation event, so give that a moment
    // before writing inside it.
    std::thread::sleep(std::time::Duration::from_millis(100));
    std::fs::write(nested.join("file.rs"), "fn main() {}\n").unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(super::FsChange::File(path)) if path.ends_with("pkg/nested/file.rs") => break,
            Ok(_) => continue,
            Err(err) => panic!("no event for a file written in a new directory: {err}"),
        }
    }
}

#[test]
fn test_repo_watcher_watches_directories_moved_into_the_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = super::RepoWatcher::spawn(root.clone(), tx).expect("spawn watcher");

    let incoming = tmp.path().join("incoming");
    std::fs::create_dir_all(&incoming).unwrap();
    std::fs::write(incoming.join("a.rs"), "fn a() {}\n").unwrap();
    std::fs::rename(&incoming, root.join("moved")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    std::fs::write(root.join("moved/after.rs"), "fn b() {}\n").unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(remaining) {
            Ok(super::FsChange::File(path)) if path.ends_with("moved/after.rs") => break,
            Ok(_) => continue,
            Err(err) => panic!("no event for a file written in a moved directory: {err}"),
        }
    }
}

#[test]
fn test_classify_repo_path_working_tree_vs_global() {
    use super::{FsChange, classify_repo_path};
    let root = Path::new("/workspace/project");

    assert_eq!(
        classify_repo_path(Path::new("/workspace/project/src/lib.rs"), root),
        Some(FsChange::File("src/lib.rs".into()))
    );
    assert_eq!(
        classify_repo_path(Path::new("/workspace/project/.git/index"), root),
        Some(FsChange::Global)
    );
    assert_eq!(
        classify_repo_path(Path::new("/workspace/project/.git/HEAD"), root),
        Some(FsChange::Global)
    );
    assert_eq!(
        classify_repo_path(Path::new("/workspace/project/target/debug/bin"), root),
        None
    );
}

#[test]
fn test_change_collector_debouncing_and_global_upgrade() {
    use super::{FsChange, FsChangeCollector, FsEvent};
    use std::time::{Duration, Instant};

    let mut collector = FsChangeCollector::new();
    let t0 = Instant::now();
    let window = Duration::from_millis(150);

    // Initial state: nothing
    assert_eq!(collector.take_debounced(t0, window), None);

    // Record file 1
    collector.record(FsChange::File("a.txt".into()), t0);
    // Before window expires: returns None
    assert_eq!(
        collector.take_debounced(t0 + Duration::from_millis(100), window),
        None
    );

    // Record file 2 at t0 + 100ms
    let t1 = t0 + Duration::from_millis(100);
    collector.record(FsChange::File("b.txt".into()), t1);

    // At t1 + 100ms (t0 + 200ms): window from t1 hasn't expired yet
    assert_eq!(
        collector.take_debounced(t1 + Duration::from_millis(100), window),
        None
    );

    // At t1 + 160ms: quiet window has passed -> emits Paths([a.txt, b.txt])
    match collector.take_debounced(t1 + Duration::from_millis(160), window) {
        Some(FsEvent::Paths(mut paths)) => {
            paths.sort();
            assert_eq!(paths, vec!["a.txt".to_string(), "b.txt".to_string()]);
        }
        other => panic!("expected FsEvent::Paths, got {other:?}"),
    }

    // Now test upgrade to Global:
    let t2 = Instant::now();
    collector.record(FsChange::File("c.txt".into()), t2);
    collector.record(FsChange::Global, t2);

    match collector.take_debounced(t2 + Duration::from_millis(160), window) {
        Some(FsEvent::Global) => {}
        other => panic!("expected FsEvent::Global, got {other:?}"),
    }
}

#[test]
fn test_is_mutation_event_classification() {
    use super::is_mutation_event;
    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, EventKind, MetadataKind, ModifyKind,
        RemoveKind,
    };

    assert!(is_mutation_event(&EventKind::Create(CreateKind::File)));
    assert!(is_mutation_event(&EventKind::Remove(RemoveKind::File)));
    assert!(is_mutation_event(&EventKind::Any));
    assert!(is_mutation_event(&EventKind::Modify(ModifyKind::Data(
        DataChange::Content
    ))));
    assert!(is_mutation_event(&EventKind::Modify(ModifyKind::Name(
        notify::event::RenameMode::Any
    ))));
    assert!(is_mutation_event(&EventKind::Modify(ModifyKind::Metadata(
        MetadataKind::WriteTime
    ))));

    // Access time updates and file access (reads/opens) must be rejected
    assert!(!is_mutation_event(&EventKind::Modify(
        ModifyKind::Metadata(MetadataKind::AccessTime)
    )));
    assert!(!is_mutation_event(&EventKind::Access(AccessKind::Open(
        AccessMode::Any
    ))));
    assert!(!is_mutation_event(&EventKind::Access(AccessKind::Read)));
    assert!(!is_mutation_event(&EventKind::Other));
}

#[test]
fn test_read_operations_do_not_trigger_fs_change() {
    let dir = tempfile::tempdir().unwrap();
    let repo_root = dir.path().to_path_buf();
    let repo = diffuse_git::testing::init_repo(&repo_root).unwrap();
    diffuse_git::testing::commit_files(&repo, "init", &[("file.txt", "hello\n")]).unwrap();

    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = super::RepoWatcher::spawn(repo_root.clone(), tx).unwrap();

    // Now perform git read operations as diffuse does on reload:
    let _res = diffuse_core::git::load_git_repo(&repo_root, &[], &[], false, 4).unwrap();

    // Give inotify a moment
    std::thread::sleep(std::time::Duration::from_millis(100));

    let mut events = Vec::new();
    while let Ok(evt) = rx.try_recv() {
        events.push(evt);
    }
    assert!(
        events.is_empty(),
        "Read operations must NOT produce FsChange events, but got: {events:?}"
    );
}

#[test]
fn test_is_notes_path() {
    use super::is_notes_path;

    let dir = tempfile::tempdir().unwrap();
    let notes_path = dir.path().join("notes.json");
    let other_path = dir.path().join("other.json");
    let nested_notes = dir.path().join("sub/notes.json");

    assert!(is_notes_path(&notes_path, &notes_path));
    assert!(!is_notes_path(&other_path, &notes_path));
    assert!(!is_notes_path(&nested_notes, &notes_path));

    // When file exists on disk
    std::fs::write(&notes_path, "{}").unwrap();
    assert!(is_notes_path(&notes_path, &notes_path));
    assert!(!is_notes_path(&other_path, &notes_path));
}

#[test]
fn test_collector_notes_and_mixed_events() {
    use super::{FsChange, FsChangeCollector, FsDiffEvent, FsEvent};
    use std::time::{Duration, Instant};

    let mut collector = FsChangeCollector::new();
    let t0 = Instant::now();
    let window = Duration::from_millis(150);

    // Only notes change
    collector.record(FsChange::Notes, t0);
    assert_eq!(
        collector.take_debounced(t0 + Duration::from_millis(160), window),
        Some(FsEvent::Notes)
    );

    // Mixed notes + file change
    let t1 = Instant::now();
    collector.record(FsChange::File("file.rs".into()), t1);
    collector.record(FsChange::Notes, t1);
    match collector.take_debounced(t1 + Duration::from_millis(160), window) {
        Some(FsEvent::DiffAndNotes { diff }) => {
            assert_eq!(diff, FsDiffEvent::Paths(vec!["file.rs".into()]));
        }
        other => panic!("expected DiffAndNotes, got {other:?}"),
    }

    // Mixed notes + global change
    let t2 = Instant::now();
    collector.record(FsChange::File("file.rs".into()), t2);
    collector.record(FsChange::Global, t2);
    collector.record(FsChange::Notes, t2);
    match collector.take_debounced(t2 + Duration::from_millis(160), window) {
        Some(FsEvent::DiffAndNotes { diff }) => {
            assert_eq!(diff, FsDiffEvent::Global);
        }
        other => panic!("expected DiffAndNotes with Global, got {other:?}"),
    }
}
