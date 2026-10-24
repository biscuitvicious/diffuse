use std::{fs, path::PathBuf, process::Command};

use super::*;
use crate::core::git::load_git_repo;

#[test]
fn test_parse_hunk_header() {
    assert_eq!(
        parse_hunk_header("@@ -10,5 +20,8 @@ fn test()"),
        Some((10, 20))
    );
    assert_eq!(parse_hunk_header("@@ -1 +1 @@"), Some((1, 1)));
    assert_eq!(parse_hunk_header("invalid"), None);
}

fn setup_test_git_repo() -> (tempfile::TempDir, PathBuf) {
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

    let mut state = AppState::new(
        out,
        "test".into(),
        notes_path,
        crate::render::DiffuseTheme::DEFAULT,
    )
    .with_repo_context(repo_root.clone(), Vec::new(), false);

    assert_eq!(state.output.files.len(), 1);

    // Stage hunk
    let res = stage_action(&mut state, &repo_root, false);
    assert!(res.is_ok(), "staging hunk succeeded");

    // Reload diff for working directory - should be clean
    reload_diff(&mut state, &repo_root, &[]).unwrap();
    assert!(state.output.files.is_empty(), "working tree is now clean");

    // Reload diff with staged: true
    let git_res_staged = load_git_repo(&repo_root, &[], &[], true, ENGINE_CONTEXT).unwrap();
    let out_staged = run_pipeline_full(
        &git_res_staged.diff,
        Some(git_res_staged.new_content),
        Some(git_res_staged.old_content),
        PipelineOptions::default(),
    );
    state.reload(out_staged);
    state.target.staged = true;
    assert_eq!(state.output.files.len(), 1, "staged diff has 1 file");

    // Unstage hunk
    let res = unstage_action(&mut state, &repo_root, false);
    assert!(res.is_ok(), "unstaging succeeded");

    // Reload staged diff - should now be clean
    reload_diff(&mut state, &repo_root, &[]).unwrap();
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

    let mut state = AppState::new(
        out,
        "test".into(),
        notes_path,
        crate::render::DiffuseTheme::DEFAULT,
    )
    .with_repo_context(repo_root.clone(), Vec::new(), false);

    assert_eq!(state.output.files.len(), 1);

    // Discard hunk
    let res = discard_action(&mut state, &repo_root, false);
    assert!(res.is_ok(), "discard succeeded");

    // Reload diff - working tree must now be clean
    reload_diff(&mut state, &repo_root, &[]).unwrap();
    assert!(
        state.output.files.is_empty(),
        "working tree is clean after discard"
    );
}

#[test]
fn test_toggle_staged_action_flow() {
    let (repo_dir, notes_path) = setup_test_git_repo();
    let repo_root = repo_dir.path().to_path_buf();
    let git_res = load_git_repo(&repo_root, &[], &[], false, ENGINE_CONTEXT).unwrap();
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
        crate::render::DiffuseTheme::DEFAULT,
    )
    .with_repo_context(repo_root.clone(), Vec::new(), false);

    // 1. Initially unstaged: toggle when staged has NO changes -> Err("no staged
    //    changes")
    let git_args = state.git_args.clone();
    let err = toggle_staged_action(&mut state, &repo_root, &git_args).unwrap_err();
    assert_eq!(err, "no staged changes");
    assert!(!state.target.is_staged());

    // 2. Stage hunk
    stage_action(&mut state, &repo_root, false).unwrap();
    // After staging, index has staged changes.
    // Toggle to staged:
    let git_args = state.git_args.clone();
    toggle_staged_action(&mut state, &repo_root, &git_args).unwrap();
    assert!(state.target.is_staged());
    assert_eq!(state.output.files.len(), 1, "staged view has 1 file");

    // 3. Now in staged view: toggle back to unstaged (which is now clean) ->
    //    Err("no unstaged changes")
    let git_args = state.git_args.clone();
    let err2 = toggle_staged_action(&mut state, &repo_root, &git_args).unwrap_err();
    assert_eq!(err2, "no unstaged changes");
    assert!(state.target.is_staged(), "must remain in staged view");

    // 4. Add new unstaged change to working tree
    fs::write(
        repo_root.join("file.rs"),
        "fn one() {\n    println!(\"1\");\n}\nfn two() { println!(\"2\"); }\n",
    )
    .unwrap();

    // Now toggle back to unstaged:
    let git_args = state.git_args.clone();
    toggle_staged_action(&mut state, &repo_root, &git_args).unwrap();
    assert!(!state.target.is_staged());
    assert_eq!(state.output.files.len(), 1, "unstaged view has 1 file");

    // 5. Viewing a commit range -> Err
    state.commit_range = Some("HEAD~1..HEAD".into());
    let git_args = state.git_args.clone();
    let err3 = toggle_staged_action(&mut state, &repo_root, &git_args).unwrap_err();
    assert!(err3.contains("commit range"));
}
