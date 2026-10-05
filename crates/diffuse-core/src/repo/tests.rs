use std::{fs, path::Path};

use crate::{
    git::run_git,
    repo::{Repo, ReviewTarget, RevisionRange},
};

fn init_test_git_repo(dir: &Path) -> (String, String, String, String) {
    run_git(dir, &["init"]).unwrap();
    run_git(dir, &["config", "user.name", "Test"]).unwrap();
    run_git(dir, &["config", "user.email", "test@example.com"]).unwrap();
    run_git(dir, &["config", "commit.gpgsign", "false"]).unwrap();

    let write_and_commit = |msg: &str, content: &str| -> String {
        fs::write(dir.join("file.txt"), content).unwrap();
        run_git(dir, &["add", "file.txt"]).unwrap();
        run_git(dir, &["commit", "-m", msg]).unwrap();
        run_git(dir, &["rev-parse", "HEAD"])
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
fn test_repo_construction_and_queries() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (_c1, _, _, c4) = init_test_git_repo(dir);

    // 1. From root: infers name from dir name
    let repo = Repo::from_root(dir);
    assert_eq!(repo.root(), Some(dir));
    let expected_name = dir.file_name().unwrap().to_str().unwrap();
    assert_eq!(repo.name(), expected_name);
    assert!(repo.is_work_tree());
    assert_eq!(repo.head_commit(), Some(c4.clone()));

    // 2. Overridden name
    let repo_named = repo.clone().with_name("custom-repo");
    assert_eq!(repo_named.name(), "custom-repo");

    // 3. From name only (unrooted)
    let unrooted = Repo::from_name("unrooted-repo");
    assert_eq!(unrooted.root(), None);
    assert_eq!(unrooted.name(), "unrooted-repo");
    assert!(!unrooted.is_work_tree());
    assert_eq!(unrooted.head_commit(), None);
    assert_eq!(unrooted.branch(), "");

    // 4. Discover from inside work tree
    let discovered = Repo::discover(dir);
    assert!(discovered.is_some());
    let discovered = discovered.unwrap();
    assert_eq!(discovered.head_commit(), Some(c4));
}

#[test]
fn test_revision_range_tuple_struct() {
    let range = RevisionRange::new("c1", "c2");
    assert_eq!(range.bottom(), "c1");
    assert_eq!(range.top(), "c2");
    assert_eq!(range.0, "c1");
    assert_eq!(range.1, "c2");
    assert_eq!(range.to_string(), "c1..c2");

    // Pattern matching
    let RevisionRange(b, t) = &range;
    assert_eq!(b, "c1");
    assert_eq!(t, "c2");
}

#[test]
fn test_revision_range_resolution() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (c1, c2, c3, _c4) = init_test_git_repo(dir);
    let (s1, s2, s3) = (&c1[..7], &c2[..7], &c3[..7]);

    // 1. Single-arg range c1..c2
    let r1 = RevisionRange::resolve(dir, &[format!("{c1}..{c2}")]).unwrap();
    assert_eq!(r1.to_string(), format!("{s1}..{s2}"));

    // 2. Two-arg range [c1, c3]
    let r2 = RevisionRange::resolve(dir, &[c1.clone(), c3.clone()]).unwrap();
    assert_eq!(r2.bottom(), s1);
    assert_eq!(r2.top(), s3);

    // 3. Single-commit notation c3^! -> parent..c3
    let r3 = RevisionRange::resolve(dir, &[format!("{c3}^!")]).unwrap();
    assert_eq!(r3.bottom(), s2);
    assert_eq!(r3.top(), s3);

    // 4. Working tree diffs (empty args, --staged, --cached, single ref without
    //    range) -> None
    assert_eq!(RevisionRange::resolve(dir, &[]), None);
    assert_eq!(RevisionRange::resolve(dir, &["--staged".into()]), None);
    assert_eq!(RevisionRange::resolve(dir, &["--cached".into()]), None);
}

#[test]
fn test_review_target_queries() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (c1, _c2, c3, c4) = init_test_git_repo(dir);
    let (s1, s4) = (&c1[..7], &c4[..7]);

    let repo = Repo::from_root(dir).with_name("myrepo");

    // 1. Reviewing a commit range c1..c4
    let range_args = vec![format!("{c1}..{c4}")];
    let target_range = ReviewTarget::from_repo_args(repo.clone(), range_args, false);
    assert!(target_range.is_range());
    assert_eq!(
        target_range.range(),
        Some(&RevisionRange(s1.to_string(), s4.to_string()))
    );
    assert_eq!(target_range.commit_range(), Some(format!("{s1}..{s4}")));
    assert_eq!(target_range.repo_name(), "myrepo");
    assert_eq!(target_range.repo_root(), Some(dir));
    // Notes commit for range c1..c4 (next newest) is full c3
    assert_eq!(target_range.notes_commit(), Some(c3.clone()));
    assert_eq!(target_range.notes_key(), format!("myrepo@{c3}"));
    assert!(!target_range.is_staged());
    assert!(!target_range.is_cached());

    // 2. Reviewing working tree (empty args)
    let target_wt = ReviewTarget::from_repo_args(repo, vec![], false);
    assert!(!target_wt.is_range());
    assert_eq!(target_wt.range(), None);
    assert_eq!(target_wt.commit_range(), None);
    assert_eq!(target_wt.notes_commit(), Some(c4.clone()));
    assert_eq!(target_wt.notes_key(), format!("myrepo@{c4}"));
    assert!(!target_wt.is_staged());

    // 3. Staged target
    let target_staged = ReviewTarget::from_repo_args(Repo::from_root(dir), vec![], true);
    assert!(target_staged.is_staged());
    assert!(target_staged.is_cached());

    // 4. Unrooted review target
    let unrooted_repo = Repo::from_name("piped-repo");
    let target_unrooted = ReviewTarget::from_repo(unrooted_repo);
    assert!(!target_unrooted.is_range());
    assert_eq!(target_unrooted.notes_commit(), None);
    assert_eq!(target_unrooted.notes_key(), "piped-repo");
    assert_eq!(target_unrooted.repo_root(), None);
    assert_eq!(target_unrooted.branch(), "");
    assert!(!target_unrooted.is_work_tree());

    // 5. from_root constructor and branch / worktree forwarding
    let target_from_root = ReviewTarget::from_root(dir);
    assert_eq!(target_from_root.repo_root(), Some(dir));
    assert!(target_from_root.is_work_tree());
    assert!(!target_from_root.branch().is_empty());
}

#[test]
fn test_repo_edge_cases_and_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // Discover in non-git directory returns None
    assert_eq!(Repo::discover(dir), None);

    // Empty override name falls back to root dir name or "diffuse"
    let repo_empty_name = Repo::from_root(dir).with_name("");
    let expected = dir.file_name().unwrap().to_str().unwrap();
    assert_eq!(repo_empty_name.name(), expected);

    let unrooted_empty = Repo::from_name("");
    assert_eq!(unrooted_empty.name(), "diffuse");

    // with_target
    let target = unrooted_empty.with_target(
        Some(RevisionRange::new("a", "b")),
        vec!["a..b".into()],
        false,
    );
    assert!(target.is_range());
    assert_eq!(target.commit_range(), Some("a..b".into()));

    // Triple-dot resolution
    let (c1, _, _, c4) = init_test_git_repo(dir);
    let s1 = &c1[..7];
    let s4 = &c4[..7];
    let r_dot3 = RevisionRange::resolve(dir, &[format!("{c1}...{c4}")]).unwrap();
    assert_eq!(r_dot3.bottom(), s1);
    assert_eq!(r_dot3.top(), s4);

    // Single branch name without range (working tree comparison against branch)
    assert_eq!(RevisionRange::resolve(dir, &["HEAD".into()]), None);
}
