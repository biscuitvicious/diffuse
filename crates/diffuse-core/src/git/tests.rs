use super::*;

#[test]
fn test_load_git_repo_non_repo_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let result = load_git_repo(tmp.path(), &[], &[], false, 6);
    assert!(
        result.is_err(),
        "a non-git directory must error instead of silently yielding an empty diff"
    );
}

#[test]
fn test_resolve_refs_two_dot() {
    let refs = resolve_refs(Path::new("."), &["main..feature".into()], false);
    assert!(!refs.worktree_new);
    assert_eq!(refs.new_ref.as_deref(), Some("feature"));
    assert_eq!(refs.old_ref.as_deref(), Some("main"));
}

#[test]
fn test_resolve_refs_single_ref() {
    let refs = resolve_refs(Path::new("."), &["main".into()], false);
    assert!(refs.worktree_new);
    assert_eq!(refs.new_ref, None);
    assert_eq!(refs.old_ref.as_deref(), Some("main"));
}

#[test]
fn test_resolve_refs_caret_exclamation() {
    let refs = resolve_refs(Path::new("."), &["abc1234^!".into()], false);
    assert!(!refs.worktree_new);
    assert_eq!(refs.new_ref.as_deref(), Some("abc1234"));
    assert_eq!(refs.old_ref.as_deref(), Some("abc1234^"));
}

#[test]
fn test_fixed_commit_range_excludes_untracked_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {:?} failed", args);
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    std::fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit1"]);

    std::fs::write(dir.join("a.rs"), "fn a() { 1 }\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit2"]);

    // Untracked file — must NOT appear when diffing a fixed range
    std::fs::write(dir.join("bogus_untracked.txt"), "I should not appear\n").unwrap();

    let range = "HEAD~1..HEAD".to_string();
    let res = load_git_repo(dir, std::slice::from_ref(&range), &[], false, 4).unwrap();
    assert!(
        !res.diff.contains("bogus_untracked.txt"),
        "fixed commit range must not include untracked files in diff"
    );
    assert!(
        !res.new_content.contains_key("bogus_untracked.txt"),
        "fixed commit range must not include untracked files in new_content"
    );

    // Also verify via the streaming path
    let (producer, consumer) = crate::streaming::spawn_streaming_pipeline(
        Some(1),
        crate::pipeline::PipelineOptions::default(),
    );
    let (meta_tx, _meta_rx) = std::sync::mpsc::channel();
    stream_git_repo_to_producer(dir, &[range], &[], false, 4, producer, &meta_tx);
    let files: Vec<_> = consumer.collect();
    assert!(
        files.iter().all(|f| f.path != "bogus_untracked.txt"),
        "streaming: fixed commit range must not include untracked files"
    );

    // Clean up
    std::fs::remove_file(dir.join("bogus_untracked.txt")).unwrap();
}

#[test]
fn test_load_git_repo_includes_untracked_files_with_synthetic_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success());
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    std::fs::write(dir.join("tracked.rs"), "fn tracked() {}\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);

    // Create untracked file
    std::fs::write(dir.join("untracked.rs"), "fn untracked() {}\n").unwrap();

    let res = load_git_repo(dir, &[], &[], false, 6).unwrap();
    assert!(
        res.diff.contains("untracked.rs"),
        "diff must contain synthetic diff for untracked.rs"
    );
    assert!(
        res.new_content.contains_key("untracked.rs"),
        "new_content must contain untracked.rs"
    );

    let out = crate::pipeline::run_pipeline_full(
        &res.diff,
        Some(res.new_content),
        Some(res.old_content),
        crate::pipeline::PipelineOptions::default(),
    );
    let untracked_file = out
        .files
        .iter()
        .find(|f| f.path == "untracked.rs")
        .expect("untracked.rs must be in pipeline output");
    assert_eq!(untracked_file.status, "untracked");
    assert!(!untracked_file.hunks.is_empty());
    assert_eq!(untracked_file.hunks[0].rows.len(), 1);
    assert_eq!(untracked_file.hunks[0].rows[0].text, "fn untracked() {}");
}

#[test]
fn test_load_git_blobs_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success());
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    std::fs::write(dir.join("f1.txt"), "content 1\n").unwrap();
    std::fs::write(dir.join("f2.txt"), "content 2\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit1"]);

    std::fs::write(dir.join("f1.txt"), "content 1 mod\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit2"]);

    let specs = vec![
        "HEAD:f1.txt".to_string(),
        "HEAD:f2.txt".to_string(),
        "HEAD~1:f1.txt".to_string(),
        "HEAD:nonexistent.txt".to_string(),
    ];

    let blobs = load_git_blobs_batch(dir, &specs);
    assert_eq!(
        blobs.get("HEAD:f1.txt").map(|s| s.as_str()),
        Some("content 1 mod\n")
    );
    assert_eq!(
        blobs.get("HEAD:f2.txt").map(|s| s.as_str()),
        Some("content 2\n")
    );
    assert_eq!(
        blobs.get("HEAD~1:f1.txt").map(|s| s.as_str()),
        Some("content 1\n")
    );
    assert!(!blobs.contains_key("HEAD:nonexistent.txt"));
}

#[test]
fn test_load_git_blobs_native() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success());
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    std::fs::write(dir.join("f1.txt"), "hello native\n").unwrap();
    std::fs::write(dir.join("f2.txt"), "world native\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "native1"]);

    std::fs::write(dir.join("f1.txt"), "hello native mod\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "native2"]);

    let specs = vec![
        "HEAD:f1.txt".to_string(),
        "HEAD:f2.txt".to_string(),
        "HEAD~1:f1.txt".to_string(),
        "HEAD:missing.txt".to_string(),
    ];

    let blobs = load_git_blobs_native(dir, &specs);
    assert_eq!(
        blobs.get("HEAD:f1.txt").map(|s| s.as_str()),
        Some("hello native mod\n")
    );
    assert_eq!(
        blobs.get("HEAD:f2.txt").map(|s| s.as_str()),
        Some("world native\n")
    );
    assert_eq!(
        blobs.get("HEAD~1:f1.txt").map(|s| s.as_str()),
        Some("hello native\n")
    );
    assert!(!blobs.contains_key("HEAD:missing.txt"));
}

#[test]
fn test_git_diff_additions_only_still_fetches_old_size() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success());
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    let initial_content = "# Initial Title\nLine 1\nLine 2\n";
    std::fs::write(dir.join("doc.md"), initial_content).unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "commit1"]);

    // Append a line: only additions (+1), 0 deletions (-0)
    let appended_content = "# Initial Title\nLine 1\nLine 2\nLine 3 added\n";
    std::fs::write(dir.join("doc.md"), appended_content).unwrap();

    // 1. Test load_git_repo
    let res = load_git_repo(dir, &[], &[], false, 4).unwrap();
    assert!(
        res.old_content.contains_key("doc.md"),
        "old_content must be fetched even when diff has 0 deletions"
    );
    assert_eq!(res.old_content.get("doc.md").unwrap(), initial_content);

    // 2. Test stream_git_repo_to_producer
    let (producer, consumer) = crate::streaming::spawn_streaming_pipeline(
        Some(1),
        crate::pipeline::PipelineOptions::default(),
    );
    let (meta_tx, _meta_rx) = std::sync::mpsc::channel();
    stream_git_repo_to_producer(dir, &[], &[], false, 4, producer, &meta_tx);
    let files: Vec<_> = consumer.collect();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].old_bytes, Some(initial_content.len()));
    assert_eq!(
        files[0].old_content,
        initial_content
            .lines()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_resolve_commit_range_in_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {:?} failed", args);
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    std::fs::write(dir.join("a.rs"), "fn a() {}\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "c1"]);
    let c1 = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    let c1_hash = String::from_utf8_lossy(&c1.stdout).trim().to_string();

    std::fs::write(dir.join("a.rs"), "fn a() { 1 }\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "c2"]);
    let c2 = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(dir)
        .output()
        .unwrap();
    let c2_hash = String::from_utf8_lossy(&c2.stdout).trim().to_string();

    // Two-dot range
    let r1 = resolve_commit_range(dir, &["HEAD~1..HEAD".into()]);
    assert_eq!(r1, Some(format!("{c1_hash}..{c2_hash}")));

    // Three-dot range
    let r2 = resolve_commit_range(dir, &["HEAD~1...HEAD".into()]);
    assert_eq!(r2, Some(format!("{c1_hash}...{c2_hash}")));

    // Caret-exclamation
    let r3 = resolve_commit_range(dir, &["HEAD^!".into()]);
    assert_eq!(r3, Some(format!("{c1_hash}..{c2_hash}")));

    // Two positional commit args
    let r4 = resolve_commit_range(dir, &["HEAD~1".into(), "HEAD".into()]);
    assert_eq!(r4, Some(format!("{c1_hash}..{c2_hash}")));

    // Non-commit ranges:
    assert_eq!(resolve_commit_range(dir, &[]), None);
    assert_eq!(resolve_commit_range(dir, &["--cached".into()]), None);
    assert_eq!(resolve_commit_range(dir, &["HEAD".into()]), None);
    assert_eq!(resolve_commit_range(dir, &["a.rs".into()]), None);

    // Also test that load_git_repo populates commit_range
    let res = load_git_repo(dir, &["HEAD~1..HEAD".into()], &[], false, 4).unwrap();
    assert_eq!(res.commit_range, Some(format!("{c1_hash}..{c2_hash}")));
}
