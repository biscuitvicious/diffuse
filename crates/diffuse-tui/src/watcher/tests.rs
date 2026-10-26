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
