use std::fs;

use super::*;

#[test]
fn test_parse_hunk_header() {
    assert_eq!(
        parse_hunk_header("@@ -10,5 +20,8 @@ fn test()"),
        Some((10, 20))
    );
    assert_eq!(parse_hunk_header("@@ -1 +1 @@"), Some((1, 1)));
    assert_eq!(parse_hunk_header("invalid"), None);
}

fn setup_test_git_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();

    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };

    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "test"]);
    fs::write(dir.path().join("file.rs"), "fn one() {}\nfn two() {}\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);

    // Modify file.rs
    fs::write(
        dir.path().join("file.rs"),
        "fn one() {\n    println!(\"1\");\n}\nfn two() {}\n",
    )
    .unwrap();

    let notes_path = dir.path().join("notes.json");
    (dir, notes_path)
}

#[test]
fn test_git_stage_unstage_reload_flow() {
    let (repo_dir, notes_path) = setup_test_git_repo();
    let repo_root = repo_dir.path().to_path_buf();
    let git_res = load_git_repo(&repo_root, &[], &[], false, ENGINE_CONTEXT).unwrap();
    let out = run_pipeline_full(
        &git_res.diff,
        Some(git_res.new_content),
        Some(git_res.old_content),
        PipelineOptions::default(),
    );

    let mut state = AppState::new(out, "test".into(), notes_path)
        .with_repo_context(repo_root.clone(), Vec::new());

    assert_eq!(state.output.files.len(), 1);

    // Stage hunk
    let res = stage_action(&mut state, &repo_root, &[], false);
    assert!(res.is_ok(), "staging hunk succeeded");

    // Reload diff for working directory - should be clean
    reload_diff(&mut state, &repo_root, &[]).unwrap();
    assert!(state.output.files.is_empty(), "working tree is now clean");

    // Reload diff with --cached
    let cached_args = vec!["--cached".to_string()];
    let git_res_cached =
        load_git_repo(&repo_root, &cached_args, &[], false, ENGINE_CONTEXT).unwrap();
    let out_cached = run_pipeline_full(
        &git_res_cached.diff,
        Some(git_res_cached.new_content),
        Some(git_res_cached.old_content),
        PipelineOptions::default(),
    );
    state.reload(out_cached);
    assert_eq!(state.output.files.len(), 1, "cached diff has 1 file");

    // Unstage hunk
    let res = unstage_action(&mut state, &repo_root, &cached_args, false);
    assert!(res.is_ok(), "unstaging succeeded");

    // Reload cached diff - should now be clean
    reload_diff(&mut state, &repo_root, &cached_args).unwrap();
    assert!(state.output.files.is_empty(), "index is now clean");
}

#[test]
fn test_git_discard_flow() {
    let (repo_dir, notes_path) = setup_test_git_repo();
    let repo_root = repo_dir.path().to_path_buf();
    let git_res = load_git_repo(&repo_root, &[], &[], false, ENGINE_CONTEXT).unwrap();
    let out = run_pipeline_full(
        &git_res.diff,
        Some(git_res.new_content),
        Some(git_res.old_content),
        PipelineOptions::default(),
    );

    let mut state = AppState::new(out, "test".into(), notes_path)
        .with_repo_context(repo_root.clone(), Vec::new());

    assert_eq!(state.output.files.len(), 1);

    // Discard hunk
    let res = discard_action(&mut state, &repo_root, &[], false);
    assert!(res.is_ok(), "discard succeeded");

    // Reload diff - working tree must now be clean
    reload_diff(&mut state, &repo_root, &[]).unwrap();
    assert!(
        state.output.files.is_empty(),
        "working tree is clean after discard"
    );
}
