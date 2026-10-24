//! Unit tests for diffuse-git repository operations and types.

use std::{fs, path::Path, process::Command};

use crate::{
    GitBlob, GitDiffOptions, RefPair, Repo, Repository, ReviewTarget, RevisionRange,
    parse_git_rev_and_pathspecs,
};

fn run_cmd(cwd: &Path, prog: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(prog)
        .current_dir(cwd)
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        String::from_utf8(out.stdout).ok()
    } else {
        None
    }
}

fn init_test_git_repo(dir: &Path) -> (String, String, String, String) {
    run_cmd(dir, "git", &["init"]).unwrap();
    run_cmd(dir, "git", &["config", "user.name", "Test"]).unwrap();
    run_cmd(dir, "git", &["config", "user.email", "test@example.com"]).unwrap();
    run_cmd(dir, "git", &["config", "commit.gpgsign", "false"]).unwrap();

    let write_and_commit = |msg: &str, content: &str| -> String {
        fs::write(dir.join("file.txt"), content).unwrap();
        run_cmd(dir, "git", &["add", "file.txt"]).unwrap();
        run_cmd(dir, "git", &["commit", "-m", msg]).unwrap();
        run_cmd(dir, "git", &["rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string()
    };

    let c1 = write_and_commit("c1", "line 1\n");
    let c2 = write_and_commit("c2", "line 1\nline 2\n");
    let c3 = write_and_commit("c3", "line 1\nline 2\nline 3\n");
    let c4 = write_and_commit("c4", "line 1\nline 2\nline 3\nline 4\n");

    (c1, c2, c3, c4)
}

#[test]
fn test_git_diff_options_defaults() {
    let opts = GitDiffOptions::default();
    assert_eq!(opts.context_lines, 4);
    assert!(opts.find_renames);
    assert_eq!(opts.rename_threshold, 50);
    assert!(!opts.ignore_indentation);
}

#[test]
fn test_git_blob_constructors() {
    let blob = GitBlob::new("abc1234", b"hello world".to_vec());
    assert_eq!(blob.id, "abc1234");
    assert_eq!(blob.data, b"hello world");
    assert_eq!(blob.as_str(), Some("hello world"));

    let invalid_utf8 = GitBlob::new("xyz", vec![0xFF, 0xFE, 0xFD]);
    assert_eq!(invalid_utf8.as_str(), None);
}

#[test]
fn test_repo_discovery_and_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (_c1, _c2, _c3, c4) = init_test_git_repo(dir);

    // Repo from root
    let repo = Repo::from_root(dir);
    assert_eq!(repo.root(), Some(dir));
    assert!(repo.is_work_tree());
    assert_eq!(repo.head_commit(), Some(c4.clone()));
    assert!(!repo.branch().is_empty());

    // Repo discover from subpath
    let sub = dir.join("nested/subdir");
    fs::create_dir_all(&sub).unwrap();
    let discovered = Repo::discover(&sub).expect("discovery from subdir");
    assert_eq!(discovered.root(), Some(dir));
    assert_eq!(discovered.head_commit(), Some(c4));

    // Discover outside repo
    let non_repo = tempfile::tempdir().unwrap();
    assert!(Repo::discover(non_repo.path()).is_none());

    // With name and name_opt
    let repo_with_name = repo.clone().with_name("my-repo");
    assert_eq!(repo_with_name.name(), "my-repo");
    let repo_opt = repo.with_name_opt(Some("opt-repo".to_string()));
    assert_eq!(repo_opt.name(), "opt-repo");
}

#[test]
fn test_repository_native_methods() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (c1, c2, _c3, c4) = init_test_git_repo(dir);

    let repo = Repository::open(dir).unwrap();
    assert_eq!(repo.work_dir(), Some(dir));
    assert_eq!(repo.head_commit_hash(), Some(c4.clone()));
    assert_eq!(
        repo.head_commit_id().map(|id| id.to_string()),
        Some(c4.clone())
    );

    // Commit check
    assert!(repo.is_commit("HEAD"));
    assert!(repo.is_commit(&c1));
    assert!(!repo.is_commit("nonexistent-rev-xyz"));
    assert!(!repo.is_commit(""));

    // Hash resolution
    let short_head = repo.rev_to_short_hash("HEAD").unwrap();
    assert_eq!(short_head.len(), 7);
    assert!(c4.starts_with(&short_head));
    assert_eq!(repo.rev_to_full_hash("HEAD"), Some(c4.clone()));
    assert_eq!(repo.rev_to_short_hash(""), None);
    assert_eq!(repo.rev_to_full_hash(""), None);

    // Commit range resolution
    let range_str = repo.resolve_commit_range(&[format!("{c1}..{c2}")]).unwrap();
    assert_eq!(range_str, format!("{}..{}", &c1[..7], &c2[..7]));

    let two_arg_range = repo
        .resolve_commit_range(&[c1.clone(), c2.clone()])
        .unwrap();
    assert_eq!(two_arg_range, format!("{}..{}", &c1[..7], &c2[..7]));

    // Notes commit resolution
    let notes_commit = repo.notes_commit(&[format!("{c1}..{c4}")]);
    assert!(notes_commit.is_some());
}

#[test]
fn test_revision_range_and_ref_pair() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (c1, c2, _, _) = init_test_git_repo(dir);

    let range = RevisionRange::resolve(dir, &[format!("{c1}..{c2}")]).unwrap();
    assert_eq!(range.bottom(), &c1[..7]);
    assert_eq!(range.top(), &c2[..7]);
    assert_eq!(format!("{range}"), format!("{}..{}", &c1[..7], &c2[..7]));

    // RefPair resolution
    let pair = RefPair::resolve(dir, &[format!("{c1}..{c2}")], false);
    assert!(!pair.worktree_new);
    assert_eq!(pair.old_ref, Some(c1));
    assert_eq!(pair.new_ref, Some(c2));
}

#[test]
fn test_parse_git_rev_and_pathspecs_native() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (c1, _c2, _c3, _c4) = init_test_git_repo(dir);

    // Explicit separator --
    let (revs, paths) = parse_git_rev_and_pathspecs(
        Some(dir),
        &["HEAD".to_string(), "--".to_string(), "file.txt".to_string()],
    );
    assert_eq!(revs, vec!["HEAD"]);
    assert_eq!(paths, vec!["file.txt"]);

    // Disambiguation: commit hash vs file
    let (revs2, paths2) =
        parse_git_rev_and_pathspecs(Some(dir), &[c1.clone(), "file.txt".to_string()]);
    assert_eq!(revs2, vec![c1]);
    assert_eq!(paths2, vec!["file.txt"]);
}

#[test]
fn test_review_target_native() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (c1, c2, _, _) = init_test_git_repo(dir);

    let repo = Repo::from_root(dir);
    let target = ReviewTarget::from_repo_args(repo, vec![format!("{c1}..{c2}")], false);
    assert!(target.is_range());
    assert_eq!(target.branch(), "master".to_string().trim());
    assert!(target.is_work_tree());
    assert!(!target.is_staged());
    assert_eq!(
        target.commit_range(),
        Some(format!("{}..{}", &c1[..7], &c2[..7]))
    );

    // Path matching
    assert!(target.matches_path("src/lib.rs", dir));
}
