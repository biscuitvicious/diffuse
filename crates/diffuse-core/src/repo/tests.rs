use std::path::Path;

use diffuse_git::testing::{commit_files, init_repo};

use crate::repo::{Repo, ReviewTarget, ReviewTargetNotesExt, RevisionRange};

fn init_test_git_repo(dir: &Path) -> (String, String, String, String) {
    let repo = init_repo(dir).unwrap();
    let c1 = commit_files(&repo, "c1", &[("file.txt", "line 1\n")])
        .unwrap()
        .to_string();
    let c2 = commit_files(&repo, "c2", &[("file.txt", "line 1\nline 2\n")])
        .unwrap()
        .to_string();
    let c3 = commit_files(&repo, "c3", &[("file.txt", "line 1\nline 2\nline 3\n")])
        .unwrap()
        .to_string();
    let c4 = commit_files(
        &repo,
        "c4",
        &[("file.txt", "line 1\nline 2\nline 3\nline 4\n")],
    )
    .unwrap()
    .to_string();

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

#[test]
fn test_review_target_revisions_and_pathspecs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (c1, _c2, _c3, c4) = init_test_git_repo(dir);
    let repo = Repo::from_root(dir).with_name("myrepo");

    // 1. Single commit argument `c1`:
    //    - git_revs = [c1], pathspecs = []
    //    - notes_commit = c1, notes_key = myrepo@c1
    let target_c1 = ReviewTarget::from_repo_args(repo.clone(), vec![c1.clone()], false);
    assert_eq!(target_c1.git_revs, vec![c1.clone()]);
    assert!(target_c1.pathspecs.is_empty());
    assert_eq!(target_c1.single_commit(), Some(c1.clone()));
    assert_eq!(target_c1.notes_commit(), Some(c1.clone()));
    assert_eq!(target_c1.notes_key(), format!("myrepo@{c1}"));

    // 2. Pathspec argument `file.txt` only:
    //    - git_revs = [], pathspecs = [file.txt]
    //    - notes_commit = c4 (live HEAD), notes_key = myrepo@c4
    let target_path = ReviewTarget::from_repo_args(repo.clone(), vec!["file.txt".into()], false);
    assert!(target_path.git_revs.is_empty());
    assert_eq!(target_path.pathspecs, vec!["file.txt"]);
    assert_eq!(target_path.notes_commit(), Some(c4.clone()));
    assert_eq!(target_path.notes_key(), format!("myrepo@{c4}"));
    assert!(target_path.matches_path("file.txt", dir));
    assert!(!target_path.matches_path("other.rs", dir));

    // 3. Commit + pathspec `c1 file.txt`:
    let target_both =
        ReviewTarget::from_repo_args(repo.clone(), vec![c1.clone(), "file.txt".into()], false);
    assert_eq!(target_both.git_revs, vec![c1.clone()]);
    assert_eq!(target_both.pathspecs, vec!["file.txt"]);
    assert_eq!(target_both.notes_commit(), Some(c1.clone()));
    assert!(target_both.matches_path("file.txt", dir));

    // 4. Using `--` separator syntax:
    let target_sep = ReviewTarget::from_repo_args(
        repo.clone(),
        vec![c1.clone(), "--".into(), "file.txt".into()],
        false,
    );
    assert_eq!(target_sep.git_revs, vec![c1.clone()]);
    assert_eq!(target_sep.pathspecs, vec!["file.txt"]);

    // 5. Database keys matching:
    let mut db = crate::notes::NotesDB::new();
    let note = crate::notes::ReviewNote {
        id: "n1".into(),
        file: "file.txt".into(),
        location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(1)),
        text: "Note".into(),
        ..Default::default()
    };
    crate::notes::add_note(&mut db, &format!("myrepo@{c1}"), note.clone());
    crate::notes::add_note(&mut db, &format!("myrepo@{c4}"), note.clone());
    crate::notes::add_note(&mut db, "myrepo", note);

    let keys_c1 = target_c1.matched_notes_db_keys(&db);
    assert_eq!(keys_c1, vec![format!("myrepo@{c1}")]);

    let keys_head = target_path.matched_notes_db_keys(&db);
    assert_eq!(keys_head, vec![format!("myrepo@{c4}")]);

    // Target with no notes on its commit returns empty keys (no fallback to bare
    // repo)
    let target_empty = ReviewTarget::from_repo_args(repo.clone(), vec![_c2.clone()], false);
    assert!(target_empty.matched_notes_db_keys(&db).is_empty());

    // Unrooted target matches bare repo
    let unrooted_target = ReviewTarget::from_repo(Repo::from_name("myrepo"));
    assert_eq!(unrooted_target.matched_notes_db_keys(&db), vec!["myrepo"]);
}
