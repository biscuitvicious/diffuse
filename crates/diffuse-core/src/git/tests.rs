use super::*;

#[test]
fn test_load_git_repo_non_repo_errors() {
    let dir = std::env::temp_dir().join(format!("diffuse-nonrepo-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let result = load_git_repo(&dir, &[], &[], false, 6);
    let _ = std::fs::remove_dir_all(&dir);
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
fn test_load_git_repo_includes_untracked_files_with_synthetic_diff() {
    let dir = std::env::temp_dir().join(format!("diffuse-untracked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
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

    let res = load_git_repo(&dir, &[], &[], false, 6).unwrap();
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

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_load_git_blobs_batch() {
    let dir = std::env::temp_dir().join(format!("diffuse-batch-blob-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
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

    let blobs = load_git_blobs_batch(&dir, &specs);
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

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_load_git_blobs_native() {
    let dir = std::env::temp_dir().join(format!("diffuse-native-blob-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
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

    let blobs = load_git_blobs_native(&dir, &specs);
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

    let _ = std::fs::remove_dir_all(&dir);
}
