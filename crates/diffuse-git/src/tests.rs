//! Unit tests for diffuse-git repository operations and types.

use std::{fs, path::Path};

use crate::{
    GitBlob, GitDiffOptions, RefPair, Repo, Repository, ReviewTarget, RevisionRange,
    parse_git_rev_and_pathspecs,
};

fn init_test_git_repo(dir: &Path) -> (String, String, String, String) {
    let repo = crate::testing::init_repo(dir).unwrap();
    let c1 = crate::testing::commit_files(&repo, "c1", &[("file.txt", "line 1\n")])
        .unwrap()
        .to_string();
    let c2 = crate::testing::commit_files(&repo, "c2", &[("file.txt", "line 1\nline 2\n")])
        .unwrap()
        .to_string();
    let c3 = crate::testing::commit_files(&repo, "c3", &[("file.txt", "line 1\nline 2\nline 3\n")])
        .unwrap()
        .to_string();
    let c4 = crate::testing::commit_files(
        &repo,
        "c4",
        &[("file.txt", "line 1\nline 2\nline 3\nline 4\n")],
    )
    .unwrap()
    .to_string();

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
    let blob = GitBlob::new("abc1234", b"hello world".to_vec(), false);
    assert_eq!(blob.id, "abc1234");
    assert_eq!(blob.data, b"hello world");
    assert_eq!(blob.as_str(), Some("hello world"));
    assert!(!blob.is_binary());

    let invalid_utf8 = GitBlob::new("xyz", vec![0xFF, 0xFE, 0xFD], true);
    assert_eq!(invalid_utf8.as_str(), None);
    assert!(invalid_utf8.is_binary());
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

    // With key and key_opt
    let repo_with_key = repo.clone().with_key("my-repo");
    assert_eq!(repo_with_key.key(), Some("my-repo"));
    assert_eq!(repo_with_key.display_name(), "my-repo");
    let repo_opt = repo.clone().with_key_opt(Some("opt-repo".to_string()));
    assert_eq!(repo_opt.key(), Some("opt-repo"));
    assert_eq!(repo_opt.display_name(), "opt-repo");

    // Rooted without explicit key uses canonical path for key, file_name for
    // display
    let expected_root = dir.canonicalize().unwrap();
    let expected_name = dir.file_name().unwrap().to_str().unwrap();
    assert_eq!(repo.key(), Some(expected_root.to_str().unwrap()));
    assert_eq!(repo.display_name(), expected_name);

    // Unrooted without key returns None for key, "<unnamed>" for display
    let unrooted_empty = Repo {
        root: None,
        key: None,
    };
    assert_eq!(unrooted_empty.key(), None);
    assert_eq!(unrooted_empty.display_name(), "<unnamed>");

    // Unrooted with key
    let unrooted_key = Repo::from_key("custom");
    assert_eq!(unrooted_key.key(), Some("custom"));
    assert_eq!(unrooted_key.display_name(), "custom");
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

    // Commit range resolution: single-commit range c1..c2 resolves to c2^!
    let range_str = repo.resolve_commit_range(&[format!("{c1}..{c2}")]).unwrap();
    assert_eq!(range_str, format!("{}^!", &c2[..7]));

    let two_arg_range = repo
        .resolve_commit_range(&[c1.clone(), c2.clone()])
        .unwrap();
    assert_eq!(two_arg_range, format!("{}^!", &c2[..7]));

    // Multi-commit range c1..c4 remains c1..c4
    let multi_range = repo.resolve_commit_range(&[format!("{c1}..{c4}")]).unwrap();
    assert_eq!(multi_range, format!("{}..{}", &c1[..7], &c4[..7]));

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
    let pair = RefPair::resolve(&[format!("{c1}..{c2}")], false);
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
    let (c1, c2, _, c4) = init_test_git_repo(dir);

    let repo = Repo::from_root(dir);
    // Single-commit range c1..c2 displays c2^!
    let target = ReviewTarget::from_repo_args(repo.clone(), vec![format!("{c1}..{c2}")], false);
    assert!(target.is_range());
    assert_eq!(target.branch(), "main");
    assert!(target.is_work_tree());
    assert!(!target.is_staged());
    assert_eq!(target.commit_range(), Some(format!("{}^!", &c2[..7])));

    // Multi-commit range c1..c4 remains c1..c4
    let multi_target = ReviewTarget::from_repo_args(repo, vec![format!("{c1}..{c4}")], false);
    assert_eq!(
        multi_target.commit_range(),
        Some(format!("{}..{}", &c1[..7], &c4[..7]))
    );

    // Path matching
    assert!(target.matches_path("src/lib.rs", dir));
}

#[test]
fn test_repository_load_blob_and_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (c1, c2, c3, c4) = init_test_git_repo(dir);

    let repo = Repository::open(dir).unwrap();

    // 1. Load single blob by commit:path
    let blob_c1 = repo.load_blob(&c1, "file.txt").expect("blob c1");
    assert_eq!(blob_c1.as_str(), Some("line 1\n"));
    assert!(!blob_c1.is_binary());
    assert_eq!(blob_c1.size(), 7);

    let blob_c4 = repo.load_blob(&c4, "file.txt").expect("blob c4");
    assert_eq!(blob_c4.as_str(), Some("line 1\nline 2\nline 3\nline 4\n"));

    // 2. Load blob by ref spec ("HEAD:file.txt")
    let blob_head = repo.load_blob("HEAD", "file.txt").expect("blob head");
    assert_eq!(blob_head.as_str(), Some("line 1\nline 2\nline 3\nline 4\n"));

    // 3. Load non-existent file or ref
    assert!(repo.load_blob(&c1, "does_not_exist.txt").is_none());
    assert!(repo.load_blob("nonexistent-ref", "file.txt").is_none());

    // 4. Batch blob loading
    let specs = vec![
        format!("{c1}:file.txt"),
        format!("{c2}:file.txt"),
        format!("{c3}:file.txt"),
        format!("{c4}:file.txt"),
        "HEAD:file.txt".to_string(),
        format!("{c1}:missing.txt"),
    ];
    let loaded = repo.load_blobs_batch(&specs);
    assert_eq!(loaded.len(), 5);
    assert_eq!(
        loaded.get(&format!("{c1}:file.txt")).unwrap().as_str(),
        Some("line 1\n")
    );
    assert_eq!(
        loaded.get(&format!("{c2}:file.txt")).unwrap().as_str(),
        Some("line 1\nline 2\n")
    );
    assert_eq!(
        loaded.get(&format!("{c4}:file.txt")).unwrap().as_str(),
        Some("line 1\nline 2\nline 3\nline 4\n")
    );
    assert_eq!(
        loaded.get("HEAD:file.txt").unwrap().as_str(),
        Some("line 1\nline 2\nline 3\nline 4\n")
    );

    // 5. Batch text loading
    let texts = repo.load_blobs_text_batch(&specs);
    assert_eq!(texts.len(), 5);
    assert_eq!(
        texts.get(&format!("{c1}:file.txt")),
        Some(&"line 1\n".to_string())
    );
}

#[test]
fn test_repository_load_binary_blob() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_test_git_repo(dir);

    // Commit binary file
    let binary_data = vec![0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x00, 0x01, 0x00, 0x01];
    let repo = Repository::open(dir).unwrap();
    fs::write(dir.join("image.gif"), &binary_data).unwrap();
    crate::mutation::stage_file(&repo, "image.gif").unwrap();
    crate::testing::commit_index(&repo, "add image").unwrap();

    let blob = repo.load_blob("HEAD", "image.gif").expect("image blob");
    assert!(blob.is_binary());
    assert_eq!(blob.data, binary_data);
    assert_eq!(blob.as_str(), None);
}

#[test]
fn test_repository_load_index_blob() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_test_git_repo(dir);

    // Stage new content in index without committing
    fs::write(dir.join("staged.txt"), "staged content\n").unwrap();
    let repo = Repository::open(dir).unwrap();
    crate::mutation::stage_file(&repo, "staged.txt").unwrap();

    let blob = repo.load_blob("", "staged.txt").expect("index blob");
    assert_eq!(blob.as_str(), Some("staged content\n"));

    let blob_colon = repo.load_blob(":", "staged.txt").expect("colon index blob");
    assert_eq!(blob_colon.as_str(), Some("staged content\n"));
}

#[test]
fn test_repository_load_blob_gitattributes_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_test_git_repo(dir);

    // Configure .gitattributes to mark text files as binary via -diff and binary
    // macro
    fs::write(
        dir.join(".gitattributes"),
        "*.custombin -diff\n*.fakebin binary\n",
    )
    .unwrap();
    fs::write(
        dir.join("clean.custombin"),
        "Clean plain text without any null bytes.\n",
    )
    .unwrap();
    fs::write(
        dir.join("clean.fakebin"),
        "Another clean plain text without null bytes.\n",
    )
    .unwrap();

    let repo = Repository::open(dir).unwrap();
    crate::mutation::stage_file(&repo, ".gitattributes").unwrap();
    crate::mutation::stage_file(&repo, "clean.custombin").unwrap();
    crate::mutation::stage_file(&repo, "clean.fakebin").unwrap();
    crate::testing::commit_index(&repo, "attributes test").unwrap();

    // 1. Check clean.custombin - must be detected as binary via -diff metadata
    let custom_blob = repo
        .load_blob("HEAD", "clean.custombin")
        .expect("custombin blob");
    assert!(
        custom_blob.is_binary(),
        ".gitattributes -diff must force binary status"
    );
    assert_eq!(custom_blob.as_str(), None);

    // 2. Check clean.fakebin - must be detected as binary via binary macro metadata
    let fake_blob = repo
        .load_blob("HEAD", "clean.fakebin")
        .expect("fakebin blob");
    assert!(
        fake_blob.is_binary(),
        ".gitattributes binary must force binary status"
    );
    assert_eq!(fake_blob.as_str(), None);
}

#[test]
fn test_repository_untracked_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    init_test_git_repo(dir);

    let repo = Repository::open(dir).unwrap();
    // Add .gitignore
    fs::write(dir.join(".gitignore"), "*.ignored\nignored_dir/\n").unwrap();
    crate::mutation::stage_file(&repo, ".gitignore").unwrap();
    crate::testing::commit_index(&repo, "add gitignore").unwrap();

    // Create untracked text file
    fs::write(dir.join("untracked.txt"), "hello untracked\nsecond line\n").unwrap();

    // Create nested untracked file
    fs::create_dir_all(dir.join("nested/deep")).unwrap();
    fs::write(dir.join("nested/deep/file.txt"), "nested file\n").unwrap();

    // Create ignored files
    fs::write(dir.join("test.ignored"), "ignore me\n").unwrap();
    fs::create_dir_all(dir.join("ignored_dir")).unwrap();
    fs::write(dir.join("ignored_dir/foo.txt"), "ignored foo\n").unwrap();

    // Create untracked binary file with null bytes
    let binary_bytes = vec![0x00, 0x01, 0x02, 0x00, 0x03];
    fs::write(dir.join("untracked_binary.bin"), binary_bytes).unwrap();

    let repo = Repository::open(dir).unwrap();

    // 1. Untracked paths enumeration
    let untracked_paths = repo.untracked_paths(&[]).unwrap();
    let path_strings: Vec<String> = untracked_paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    assert!(path_strings.contains(&"untracked.txt".to_string()));
    assert!(path_strings.contains(&"nested/deep/file.txt".to_string()));
    assert!(path_strings.contains(&"untracked_binary.bin".to_string()));
    assert!(!path_strings.contains(&"test.ignored".to_string()));
    assert!(!path_strings.contains(&"ignored_dir/foo.txt".to_string()));
    assert!(!path_strings.contains(&"file.txt".to_string()));
    assert!(!path_strings.contains(&".gitignore".to_string()));

    // 2. Untracked DiffFiles with binary detection & skipping
    let out = repo.untracked_files(&[]).unwrap();
    assert_eq!(out.files.len(), 2);
    assert_eq!(out.skipped_binary, vec!["untracked_binary.bin".to_string()]);

    let f1 = out
        .files
        .iter()
        .find(|f| f.path == "untracked.txt")
        .unwrap();
    assert_eq!(f1.status, diffuse_parse::FileStatus::Untracked);
    assert_eq!(f1.hunks.len(), 1);
    assert_eq!(f1.hunks[0].rows.len(), 2);

    let f2 = out
        .files
        .iter()
        .find(|f| f.path == "nested/deep/file.txt")
        .unwrap();
    assert_eq!(f2.status, diffuse_parse::FileStatus::Untracked);
    assert_eq!(f2.hunks.len(), 1);
    assert_eq!(f2.hunks[0].rows.len(), 1);

    // 3. Pathspec filter
    let filtered = repo.untracked_files(&["nested/".to_string()]).unwrap();
    assert_eq!(filtered.files.len(), 1);
    assert_eq!(filtered.files[0].path, "nested/deep/file.txt");
    assert!(filtered.skipped_binary.is_empty());
}

#[test]
fn test_repository_file_statuses_unstaged_staged_and_range() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = crate::testing::init_repo(dir).unwrap();
    let c1 = crate::testing::commit_files(
        &repo,
        "initial commit",
        &[
            ("file1.txt", "hello 1\n"),
            ("file2.txt", "hello 2\n"),
            ("file3.txt", "hello 3\n"),
        ],
    )
    .unwrap()
    .to_string();

    // 1. Unstaged changes in working tree:
    // Modify file1.txt, delete file2.txt
    fs::write(dir.join("file1.txt"), "hello 1 modified\n").unwrap();
    fs::remove_file(dir.join("file2.txt")).unwrap();

    let unstaged_target = ReviewTarget {
        repo: Repo::from_root(dir),
        range: None,
        git_revs: vec![],
        pathspecs: vec![],
        staged: false,
        rev_key: None,
    };
    let statuses = repo.file_statuses(&unstaged_target).unwrap();
    let f1 = statuses
        .iter()
        .find(|s| s.path == "file1.txt")
        .expect("file1.txt status");
    assert_eq!(f1.status, diffuse_parse::FileStatus::Modified);

    let f2 = statuses
        .iter()
        .find(|s| s.path == "file2.txt")
        .expect("file2.txt status");
    assert_eq!(f2.status, diffuse_parse::FileStatus::Deleted);

    // 2. Stage changes:
    // Restore file2.txt, stage file1.txt, add added.txt, git mv file3.txt ->
    // renamed3.txt
    crate::mutation::discard_file(&repo, "file2.txt").unwrap();
    crate::mutation::stage_file(&repo, "file1.txt").unwrap();
    fs::write(dir.join("added.txt"), "brand new\n").unwrap();
    crate::mutation::stage_file(&repo, "added.txt").unwrap();
    fs::rename(dir.join("file3.txt"), dir.join("renamed3.txt")).unwrap();
    crate::mutation::stage_file(&repo, "file3.txt").unwrap();
    crate::mutation::stage_file(&repo, "renamed3.txt").unwrap();

    let staged_target = ReviewTarget {
        repo: Repo::from_root(dir),
        range: None,
        git_revs: vec![],
        pathspecs: vec![],
        staged: true,
        rev_key: None,
    };
    let staged_statuses = repo.file_statuses(&staged_target).unwrap();

    let sf1 = staged_statuses
        .iter()
        .find(|s| s.path == "file1.txt")
        .expect("file1.txt staged");
    assert_eq!(sf1.status, diffuse_parse::FileStatus::Modified);

    let s_add = staged_statuses
        .iter()
        .find(|s| s.path == "added.txt")
        .expect("added.txt staged");
    assert_eq!(s_add.status, diffuse_parse::FileStatus::Added);

    let s_ren = staged_statuses
        .iter()
        .find(|s| s.path == "renamed3.txt")
        .expect("renamed3.txt staged");
    assert_eq!(s_ren.status, diffuse_parse::FileStatus::Renamed);
    assert_eq!(s_ren.old_path.as_deref(), Some("file3.txt"));

    // 3. Commit staged changes to c2 and test commit range
    let c2 = crate::testing::commit_index(&repo, "commit c2")
        .unwrap()
        .to_string();

    let range_target = ReviewTarget {
        repo: Repo::from_root(dir),
        range: Some(RevisionRange(c1.clone(), c2.clone())),
        git_revs: vec![],
        pathspecs: vec![],
        staged: false,
        rev_key: None,
    };
    let range_statuses = repo.file_statuses(&range_target).unwrap();
    assert_eq!(range_statuses.len(), 3);
    assert!(
        range_statuses
            .iter()
            .any(|s| s.path == "file1.txt" && s.status == diffuse_parse::FileStatus::Modified)
    );
    assert!(
        range_statuses
            .iter()
            .any(|s| s.path == "added.txt" && s.status == diffuse_parse::FileStatus::Added)
    );
    assert!(range_statuses.iter().any(|s| s.path == "renamed3.txt"
        && s.status == diffuse_parse::FileStatus::Renamed
        && s.old_path.as_deref() == Some("file3.txt")));
}

#[test]
fn test_diff_text_identical() {
    let text = "fn foo() {\n    println!(\"hello\");\n}\n";
    let (hunks, patch) =
        crate::diff::diff_text(text, text, 3, crate::diff::WhitespaceMode::TrimEquality);
    assert!(hunks.is_empty());
    assert!(patch.is_empty());
}

#[test]
fn test_diff_text_trim_equality_leading_trailing() {
    let old = "fn foo() {\n    let a = 1;\n    let b = 2;\n}\n";
    let new = "fn foo() {\n        let a = 1;\n        let b = 2;\n}\n";

    // Exact mode catches indentation difference
    let (exact_hunks, exact_patch) =
        crate::diff::diff_text(old, new, 3, crate::diff::WhitespaceMode::Exact);
    assert_eq!(exact_hunks.len(), 1);
    assert!(exact_patch.contains("-    let a = 1;"));
    assert!(exact_patch.contains("+        let a = 1;"));

    // TrimEquality mode treats re-indented lines as equivalent (0 hunks)
    let (trim_hunks, trim_patch) =
        crate::diff::diff_text(old, new, 3, crate::diff::WhitespaceMode::TrimEquality);
    assert!(trim_hunks.is_empty());
    assert!(trim_patch.is_empty());
}

#[test]
fn test_diff_text_trim_equality_internal_whitespace_preserved() {
    let old = "let a = 1;\n";
    let new = "let  a = 1;\n";

    // Internal whitespace differences are preserved under TrimEquality
    let (hunks, patch) =
        crate::diff::diff_text(old, new, 3, crate::diff::WhitespaceMode::TrimEquality);
    assert_eq!(hunks.len(), 1);
    assert!(patch.contains("-let a = 1;\n"));
    assert!(patch.contains("+let  a = 1;\n"));
}

#[test]
fn test_diff_text_structure_and_context() {
    let old = "c1\nc2\nold_mid\nc3\nc4\n";
    let new = "c1\nc2\nnew_mid\nc3\nc4\n";

    let (hunks, patch) =
        crate::diff::diff_text(old, new, 2, crate::diff::WhitespaceMode::TrimEquality);
    assert_eq!(hunks.len(), 1);
    let h = &hunks[0];
    assert_eq!(h.old_start, 1);
    assert_eq!(h.old_count, 5);
    assert_eq!(h.new_start, 1);
    assert_eq!(h.new_count, 5);
    assert_eq!(h.rows.len(), 6); // c1, c2, -old_mid, +new_mid, c3, c4

    assert!(
        matches!(&h.rows[0], diffuse_parse::Row::Ctx(c) if c.old_pos == 1 && c.new_pos == 1 && c.text == "c1")
    );
    assert!(
        matches!(&h.rows[1], diffuse_parse::Row::Ctx(c) if c.old_pos == 2 && c.new_pos == 2 && c.text == "c2")
    );
    assert!(
        matches!(&h.rows[2], diffuse_parse::Row::Del { line: d, .. } if d.num == 3 && d.text == "old_mid")
    );
    assert!(
        matches!(&h.rows[3], diffuse_parse::Row::Add { line: a, .. } if a.num == 3 && a.text == "new_mid")
    );
    assert!(
        matches!(&h.rows[4], diffuse_parse::Row::Ctx(c) if c.old_pos == 4 && c.new_pos == 4 && c.text == "c3")
    );
    assert!(
        matches!(&h.rows[5], diffuse_parse::Row::Ctx(c) if c.old_pos == 5 && c.new_pos == 5 && c.text == "c4")
    );

    assert!(patch.starts_with("@@ -1,5 +1,5 @@\n"));
}

#[test]
fn test_generate_file_diff_scenarios() {
    use crate::diff::{FileDiffParams, WhitespaceMode, generate_file_diff};
    use diffuse_parse::FileStatus;

    // 1. Added text file
    let added = generate_file_diff(FileDiffParams {
        path: "added.rs",
        old_path: None,
        old_bytes: None,
        new_bytes: Some(b"fn new_func() {}\n"),
        status: FileStatus::Added,
        old_mode: None,
        new_mode: Some("100644"),
        is_binary: false,
        context: 3,
        mode: WhitespaceMode::TrimEquality,
    });
    assert_eq!(added.file.status, FileStatus::Added);
    assert_eq!(added.file.hunks.len(), 1);
    assert!(
        added
            .patch
            .contains("new file mode 100644\n--- /dev/null\n+++ b/added.rs\n")
    );
    assert!(added.patch.contains("+fn new_func() {}\n"));

    // 2. Deleted text file
    let deleted = generate_file_diff(FileDiffParams {
        path: "deleted.rs",
        old_path: None,
        old_bytes: Some(b"fn old_func() {}\n"),
        new_bytes: None,
        status: FileStatus::Deleted,
        old_mode: Some("100644"),
        new_mode: None,
        is_binary: false,
        context: 3,
        mode: WhitespaceMode::TrimEquality,
    });
    assert_eq!(deleted.file.status, FileStatus::Deleted);
    assert_eq!(deleted.file.hunks.len(), 1);
    assert!(
        deleted
            .patch
            .contains("deleted file mode 100644\n--- a/deleted.rs\n+++ /dev/null\n")
    );
    assert!(deleted.patch.contains("-fn old_func() {}\n"));

    // 3. Renamed file
    let renamed = generate_file_diff(FileDiffParams {
        path: "new_name.rs",
        old_path: Some("old_name.rs"),
        old_bytes: Some(b"same content\n"),
        new_bytes: Some(b"same content\n"),
        status: FileStatus::Renamed,
        old_mode: Some("100644"),
        new_mode: Some("100644"),
        is_binary: false,
        context: 3,
        mode: WhitespaceMode::TrimEquality,
    });
    assert_eq!(renamed.file.status, FileStatus::Renamed);
    assert_eq!(renamed.file.old_path.as_deref(), Some("old_name.rs"));
    assert!(renamed.file.hunks.is_empty());
    assert!(
        renamed
            .patch
            .contains("rename from old_name.rs\nrename to new_name.rs\n")
    );

    // 4. Binary file
    let binary = generate_file_diff(FileDiffParams {
        path: "image.png",
        old_path: None,
        old_bytes: Some(b"\0\0PNG"),
        new_bytes: Some(b"\0\0PNG2"),
        status: FileStatus::Modified,
        old_mode: Some("100644"),
        new_mode: Some("100644"),
        is_binary: true,
        context: 3,
        mode: WhitespaceMode::TrimEquality,
    });
    assert!(binary.file.is_binary);
    assert!(binary.file.hunks.is_empty());
    assert!(
        binary
            .patch
            .contains("Binary files a/image.png and b/image.png differ\n")
    );
}

#[test]
fn test_diff_repository_unstaged_staged_and_range() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = crate::testing::init_repo(dir).unwrap();
    crate::testing::commit_files(
        &repo,
        "initial commit",
        &[
            ("file1.txt", "line1\nline2\nline3\n"),
            ("file2.txt", "hello world\n"),
        ],
    )
    .unwrap();

    // Modify file1: change line2 to line2_mod
    std::fs::write(dir.join("file1.txt"), "line1\nline2_mod\nline3\n").unwrap();
    // Untracked file
    std::fs::write(dir.join("untracked.txt"), "new content\n").unwrap();

    let target = ReviewTarget {
        repo: Repo::from_root(dir),
        range: None,
        git_revs: vec![],
        pathspecs: vec![],
        staged: false,
        rev_key: None,
    };

    let res =
        crate::diff::diff_repository(&repo, &target, 3, crate::diff::WhitespaceMode::TrimEquality)
            .unwrap();
    assert_eq!(res.files.len(), 2);
    assert!(res.diff.contains("--- a/file1.txt\n+++ b/file1.txt\n"));
    assert!(res.diff.contains("-line2\n+line2_mod\n"));
    assert!(
        res.diff
            .contains("untracked file mode 100644\n--- /dev/null\n+++ b/untracked.txt\n")
    );
    assert_eq!(
        res.old_content.get("file1.txt").map(String::as_str),
        Some("line1\nline2\nline3\n")
    );
    assert_eq!(
        res.new_content.get("file1.txt").map(String::as_str),
        Some("line1\nline2_mod\nline3\n")
    );
    assert_eq!(
        res.new_content.get("untracked.txt").map(String::as_str),
        Some("new content\n")
    );
}

#[test]
fn test_diff_repository_on_cli_setup_repo() {
    let dir = tempfile::tempdir().unwrap();
    let repo = crate::testing::init_repo(dir.path()).unwrap();
    crate::testing::commit_files(&repo, "init", &[("a.txt", "one\ntwo\n")]).unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "hello\nworld\n").unwrap();

    let target = ReviewTarget {
        repo: Repo::from_root(dir.path()),
        range: None,
        git_revs: vec![],
        pathspecs: vec![],
        staged: false,
        rev_key: None,
    };

    let res =
        crate::diff::diff_repository(&repo, &target, 3, crate::diff::WhitespaceMode::TrimEquality)
            .unwrap();
    assert_eq!(res.files.len(), 2);
    let a_file = res.files.iter().find(|f| f.path == "a.txt").unwrap();
    assert!(!a_file.hunks.is_empty(), "a.txt must have hunks");
}

#[test]
fn test_submodule_gitlink_diff() {
    let parent = tempfile::tempdir().unwrap();
    let sub = tempfile::tempdir().unwrap();

    // Initialize submodule repo
    let sub_repo = crate::testing::init_repo(sub.path()).unwrap();
    let sub_c1 = crate::testing::commit_files(
        &sub_repo,
        "sub commit 1",
        &[("sub_file.txt", "hello sub\n")],
    )
    .unwrap();

    // Initialize parent repo with submodule gitlink
    let parent_repo = crate::testing::init_repo(parent.path()).unwrap();
    crate::testing::commit_tree_entries(
        &parent_repo,
        "add submodule",
        &[("my_sub", gix::object::tree::EntryKind::Commit, sub_c1)],
    )
    .unwrap();

    // Make second commit in submodule
    let sub_c2 = crate::testing::commit_files(
        &sub_repo,
        "sub commit 2",
        &[("sub_file.txt", "hello sub v2\n")],
    )
    .unwrap();

    // Bump parent submodule pointer
    crate::testing::commit_tree_entries(
        &parent_repo,
        "bump submodule",
        &[("my_sub", gix::object::tree::EntryKind::Commit, sub_c2)],
    )
    .unwrap();

    let repo = Repository::open(parent.path()).unwrap();
    let target = ReviewTarget {
        repo: Repo::from_root(parent.path()),
        range: Some(crate::types::RevisionRange(
            "HEAD~1".to_string(),
            "HEAD".to_string(),
        )),
        git_revs: vec!["HEAD~1..HEAD".to_string()],
        pathspecs: vec![],
        staged: false,
        rev_key: None,
    };

    let res =
        crate::diff::diff_repository(&repo, &target, 3, crate::diff::WhitespaceMode::TrimEquality)
            .unwrap();
    assert_eq!(res.files.len(), 1);
    let sub_file = &res.files[0];
    assert_eq!(sub_file.path, "my_sub");
    assert!(!sub_file.hunks.is_empty());
    assert!(!sub_file.raw_hunks.is_empty());
    assert!(res.diff.contains("Subproject commit"));
}

#[test]
fn test_trim_equality_preserves_indentation() {
    let old = "\
fn outer() {
    fn inner() {
        // level 2
    }
}
fn other() {
    let x = 1;
}
";
    let new = "\
fn outer() {
    fn inner() {
        // level 2 modified
    }
}
fn other() {
    let x = 2;
}
";
    let (hunks, patch) =
        crate::diff::diff_text(old, new, 3, crate::diff::WhitespaceMode::TrimEquality);
    assert!(!hunks.is_empty());
    // Verify that the indentation of closing braces at different indentation levels
    // is preserved accurately without interning bleed. In a unified diff, context
    // lines are prefixed with ' ', so 4 leading spaces becomes 5 leading spaces.
    assert!(patch.contains("     }\n }\n"));
    for hunk in &hunks {
        for row in &hunk.rows {
            if row.text().trim() == "}" {
                // Must be either "    }" (4 spaces) or "}" (0 spaces), not corrupted
                assert!(row.text() == "    }" || row.text() == "}");
            }
        }
    }
}

#[test]
fn test_mutation_apply_hunk_forward_and_reverse() {
    let old = "fn main() {\n    let a = 1;\n    let b = 2;\n}\n";
    let new = "fn main() {\n    let a = 10;\n    let b = 2;\n}\n";
    let (hunks, _) = crate::diff::diff_text(old, new, 3, crate::diff::WhitespaceMode::Exact);
    assert_eq!(hunks.len(), 1);

    // Forward application to old buffer produces new buffer:
    let applied = crate::mutation::apply_hunk_to_buffer(old.as_bytes(), &hunks[0], false).unwrap();
    assert_eq!(String::from_utf8(applied).unwrap(), new);

    // Reverse application to new buffer restores old buffer:
    let reverted = crate::mutation::apply_hunk_to_buffer(new.as_bytes(), &hunks[0], true).unwrap();
    assert_eq!(String::from_utf8(reverted).unwrap(), old);
}

#[test]
fn test_mutation_stage_and_unstage_flow() {
    let dir = tempfile::tempdir().unwrap();
    let repo = crate::testing::init_repo(dir.path()).unwrap();
    let base_code = "\
fn one() {}
// padding 1
// padding 2
// padding 3
// padding 4
// padding 5
// padding 6
// padding 7
// padding 8
// padding 9
// padding 10
fn two() {}
";
    crate::testing::commit_files(&repo, "init", &[("code.rs", base_code)]).unwrap();

    // Modify code.rs: two separate functions modified
    let mod_code = "\
fn one() {
    println!(\"one\");
}
// padding 1
// padding 2
// padding 3
// padding 4
// padding 5
// padding 6
// padding 7
// padding 8
// padding 9
// padding 10
fn two() {
    println!(\"two\");
}
";
    std::fs::write(dir.path().join("code.rs"), mod_code).unwrap();

    let repo = Repository::open(dir.path()).unwrap();

    // Generate diff between index and worktree
    let target = ReviewTarget {
        repo: Repo::from_root(dir.path()),
        range: None,
        git_revs: vec![],
        pathspecs: vec![],
        staged: false,
        rev_key: None,
    };
    let res = crate::diff::diff_repository(&repo, &target, 0, crate::diff::WhitespaceMode::Exact)
        .unwrap();
    assert_eq!(res.files.len(), 1);
    let f = &res.files[0];
    assert_eq!(f.hunks.len(), 2);

    // 1. Stage only hunk 0
    crate::mutation::stage_hunk(&repo, "code.rs", &f.hunks[0]).unwrap();

    // Verify index now contains hunk 0 applied, but NOT hunk 1
    let index_blob = repo.load_index_blob("code.rs").unwrap();
    let index_text = String::from_utf8(index_blob.data).unwrap();
    assert!(index_text.contains("println!(\"one\");"));
    assert!(!index_text.contains("println!(\"two\");"));

    // 2. Unstage hunk 0 back
    crate::mutation::unstage_hunk(&repo, "code.rs", &f.hunks[0]).unwrap();
    let index_blob = repo.load_index_blob("code.rs").unwrap();
    let index_text = String::from_utf8(index_blob.data).unwrap();
    assert_eq!(index_text, base_code);

    // 3. Stage whole file
    crate::mutation::stage_file(&repo, "code.rs").unwrap();
    let index_blob = repo.load_index_blob("code.rs").unwrap();
    let index_text = String::from_utf8(index_blob.data).unwrap();
    assert!(index_text.contains("println!(\"one\");"));
    assert!(index_text.contains("println!(\"two\");"));

    // 4. Unstage whole file
    crate::mutation::unstage_file(&repo, "code.rs").unwrap();
    let index_blob = repo.load_index_blob("code.rs").unwrap();
    let index_text = String::from_utf8(index_blob.data).unwrap();
    assert_eq!(index_text, base_code);

    // 5. Discard hunk 0 from worktree
    let stash = crate::mutation::discard_hunk(&repo, "code.rs", &f.hunks[0]).unwrap();
    assert!(stash.is_some());
    let worktree_text = std::fs::read_to_string(dir.path().join("code.rs")).unwrap();
    assert!(!worktree_text.contains("println!(\"one\");"));
    assert!(worktree_text.contains("println!(\"two\");"));

    // 6. Discard whole file
    let stash2 = crate::mutation::discard_file(&repo, "code.rs").unwrap();
    assert!(stash2.is_some());
    let worktree_text = std::fs::read_to_string(dir.path().join("code.rs")).unwrap();
    assert_eq!(worktree_text, base_code);
}

#[test]
fn test_testing_fixtures_builder() {
    use crate::testing::{commit_files, create_branch, create_tag, delete_files, init_repo};

    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo(tmp.path()).expect("init_repo succeeds");

    // Commit 1: add two files
    let c1 = commit_files(
        &repo,
        "initial commit",
        &[("a.txt", "hello\n"), ("sub/b.txt", "world\n")],
    )
    .expect("commit 1 succeeds");

    assert_eq!(repo.head_commit_hash().unwrap(), c1.to_string());
    assert_eq!(repo.load_blob("HEAD", "a.txt").unwrap().data, b"hello\n");
    assert_eq!(
        repo.load_blob("HEAD", "sub/b.txt").unwrap().data,
        b"world\n"
    );

    // Create branch & tag at c1
    create_branch(&repo, "feature", c1).expect("create_branch succeeds");
    create_tag(&repo, "v1.0", c1).expect("create_tag succeeds");

    // Commit 2: update a.txt and add c.txt
    let c2 = commit_files(
        &repo,
        "second commit",
        &[("a.txt", "hello modified\n"), ("c.txt", "new file\n")],
    )
    .expect("commit 2 succeeds");

    assert_eq!(repo.head_commit_hash().unwrap(), c2.to_string());
    assert_eq!(repo.rev_to_full_hash("HEAD~1").unwrap(), c1.to_string());

    // Commit 3: delete sub/b.txt
    let c3 = delete_files(&repo, "delete b", &["sub/b.txt"]).expect("delete succeeds");
    assert_eq!(repo.head_commit_hash().unwrap(), c3.to_string());
    assert!(repo.load_blob("HEAD", "sub/b.txt").is_none());

    // Diff commit 1 to commit 2 natively
    let target = crate::types::ReviewTarget::from_repo_args(
        crate::types::Repo::from_root(tmp.path()),
        vec![format!("{c1}..{c2}")],
        false,
    );
    let res = crate::diff::diff_repository(&repo, &target, 3, crate::diff::WhitespaceMode::Exact)
        .expect("diff succeeds");
    assert_eq!(res.files.len(), 2);
    let a_file = res.files.iter().find(|f| f.path == "a.txt").unwrap();
    assert_eq!(a_file.status, diffuse_parse::FileStatus::Modified);
}

#[test]
fn test_is_single_commit_range() {
    use crate::testing::{commit_files, commit_index_with_parents, create_tag, init_repo};

    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo(tmp.path()).expect("init_repo succeeds");

    let c1 = commit_files(&repo, "c1", &[("a.txt", "1\n")]).expect("c1");
    create_tag(&repo, "v1.0", c1).expect("tag v1.0");
    let c2 = commit_files(&repo, "c2", &[("a.txt", "2\n")]).expect("c2");
    let c3 = commit_files(&repo, "c3", &[("a.txt", "3\n")]).expect("c3");

    // Exactly one commit
    assert!(repo.is_single_commit_range(&c1.to_string(), &c2.to_string()));
    assert!(repo.is_single_commit_range(&c2.to_string(), &c3.to_string()));
    assert!(repo.is_single_commit_range("HEAD~1", "HEAD"));
    assert!(repo.is_single_commit_range("HEAD~2", "HEAD~1"));
    assert!(repo.is_single_commit_range("v1.0", "HEAD~1"));
    assert!(repo.is_single_commit_range("HEAD~1", ""));

    // Multi-commit ranges (>= 2 commits)
    assert!(!repo.is_single_commit_range(&c1.to_string(), &c3.to_string()));
    assert!(!repo.is_single_commit_range("HEAD~2", "HEAD"));

    // Zero-commit ranges
    assert!(!repo.is_single_commit_range("HEAD", "HEAD"));
    assert!(!repo.is_single_commit_range(&c2.to_string(), &c2.to_string()));
    assert!(!repo.is_single_commit_range("", "HEAD"));

    // Root commit (c1 has 0 parents)
    assert!(!repo.is_single_commit_range("HEAD", &c1.to_string()));

    // Merge commit (has multiple parents)
    let merge_commit =
        commit_index_with_parents(&repo, "merge", &[c1, c2]).expect("create merge commit");
    assert!(!repo.is_single_commit_range(&c1.to_string(), &merge_commit.to_string()));
    assert!(!repo.is_single_commit_range(&c2.to_string(), &merge_commit.to_string()));

    // Invalid / unresolvable
    assert!(!repo.is_single_commit_range("no-such-ref", "HEAD"));
    assert!(!repo.is_single_commit_range("HEAD", "no-such-ref"));

    // Non-commit objects (e.g. blob)
    let blob = repo.load_blob("HEAD", "a.txt").expect("blob");
    assert!(!repo.is_single_commit_range(&blob.id, "HEAD"));
    assert!(!repo.is_single_commit_range("HEAD", &blob.id));

    // Test resolve_commit_range for single commit vs merge commit
    let s1 = &c1.to_string()[..7];
    let s2 = &c2.to_string()[..7];
    let s3 = &c3.to_string()[..7];
    let s_merge = &merge_commit.to_string()[..7];

    // Single commit range: c1..c2 displays c2^!
    assert_eq!(
        repo.resolve_commit_range(&[format!("{c1}..{c2}")]),
        Some(format!("{s2}^!"))
    );
    assert_eq!(
        repo.resolve_commit_range(&[c1.to_string(), c2.to_string()]),
        Some(format!("{s2}^!"))
    );

    // Multi-commit range: c1..c3 remains c1..c3
    assert_eq!(
        repo.resolve_commit_range(&[format!("{c1}..{c3}")]),
        Some(format!("{s1}..{s3}"))
    );

    // Merge commit ranges: c1..merge and c2..merge have > 1 parent so they stay
    // start..end
    assert_eq!(
        repo.resolve_commit_range(&[format!("{c1}..{merge_commit}")]),
        Some(format!("{s1}..{s_merge}"))
    );
    assert_eq!(
        repo.resolve_commit_range(&[format!("{c2}..{merge_commit}")]),
        Some(format!("{s2}..{s_merge}"))
    );

    // Explicit caret-exclamation on merge commit
    assert_eq!(
        repo.resolve_commit_range(&[format!("{merge_commit}^!")]),
        Some(format!("{s_merge}^!"))
    );
}

#[test]
fn test_notes_scope_resolution() {
    use crate::testing::{commit_files, init_repo};

    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo(tmp.path()).expect("init_repo succeeds");

    let c1 = commit_files(&repo, "c1", &[("a.txt", "1\n")]).expect("c1");
    let c2 = commit_files(&repo, "c2", &[("a.txt", "2\n")]).expect("c2");
    let c3 = commit_files(&repo, "c3", &[("a.txt", "3\n")]).expect("c3");

    // 1. Working tree: resolves to HEAD (c3)
    assert_eq!(repo.notes_commit(&[]), Some(c3.to_string()));

    // 2. Single-commit range c1..c2 (1 commit): resolves to c1 (parent of c2)
    assert_eq!(
        repo.notes_commit(&[format!("{c1}..{c2}")]),
        Some(c1.to_string())
    );

    // 3. Single-commit notation c2^!: resolves to c1 (parent of c2)
    assert_eq!(
        repo.notes_commit(&[format!("{c2}^!")]),
        Some(c1.to_string())
    );

    // 4. Multi-commit range c1..c3 (c2, c3): base c1, parent of tip c3 is c2 ->
    //    "c1..c2"
    assert_eq!(
        repo.notes_commit(&[format!("{c1}..{c3}")]),
        Some(format!("{c1}..{c2}"))
    );

    // 5. Single commit ref c1: resolves to c1
    assert_eq!(repo.notes_commit(&[c1.to_string()]), Some(c1.to_string()));

    // 6. Range c1.. (equivalent to c1..HEAD): base c1, parent of tip c3 (HEAD) is
    //    c2 -> "c1..c2"
    assert_eq!(
        repo.notes_commit(&[format!("{c1}..")]),
        Some(format!("{c1}..{c2}"))
    );
}

#[test]
fn test_repository_create_tag() {
    use crate::testing::{commit_files, init_repo};

    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo(tmp.path()).expect("init_repo succeeds");
    let c1 = commit_files(&repo, "c1", &[("a.txt", "1\n")]).expect("c1");
    let c2 = commit_files(&repo, "c2", &[("a.txt", "2\n")]).expect("c2");

    // Tag commit 1
    repo.create_tag("tag-c1", &c1.to_string())
        .expect("create tag-c1");
    assert_eq!(repo.rev_to_full_hash("tag-c1").unwrap(), c1.to_string());

    // Tag commit 2 (default HEAD)
    repo.create_tag("tag-head", "").expect("create tag-head");
    assert_eq!(repo.rev_to_full_hash("tag-head").unwrap(), c2.to_string());

    // Diff using tag^!
    let target = crate::types::ReviewTarget::from_repo_args(
        crate::types::Repo::from_root(tmp.path()),
        vec!["tag-head^!".to_string()],
        false,
    );
    let res = crate::diff::diff_repository(&repo, &target, 3, crate::diff::WhitespaceMode::Exact)
        .expect("diff with tag^!");
    assert_eq!(res.files.len(), 1);
    assert_eq!(res.files[0].path, "a.txt");

    // Invalid target fails
    assert!(repo.create_tag("invalid", "no-such-ref").is_err());
}

#[test]
fn test_pathspecs_match_with_excludes() {
    use crate::status::pathspecs_match;

    // Empty pathspecs matches everything
    assert!(pathspecs_match("src/lib.rs", &[]));

    // Plain inclusion pattern
    let inc = vec!["src/".to_string()];
    assert!(pathspecs_match("src/lib.rs", &inc));
    assert!(pathspecs_match("src/sub/mod.rs", &inc));
    assert!(!pathspecs_match("tests/main.rs", &inc));

    // Plain exclusion pattern with :(exclude)
    let exc1 = vec![":(exclude)tests/golden".to_string()];
    assert!(pathspecs_match("src/lib.rs", &exc1));
    assert!(pathspecs_match("tests/unit.rs", &exc1));
    assert!(!pathspecs_match("tests/golden/patch.diff", &exc1));
    assert!(!pathspecs_match("tests/golden", &exc1));

    // Exclusion pattern with :!
    let exc2 = vec![":!tests/golden/".to_string()];
    assert!(pathspecs_match("src/lib.rs", &exc2));
    assert!(!pathspecs_match("tests/golden/patch.diff", &exc2));

    // Exclusion pattern with ^
    let exc3 = vec!["^tests/golden".to_string()];
    assert!(pathspecs_match("src/lib.rs", &exc3));
    assert!(!pathspecs_match("tests/golden/patch.diff", &exc3));

    // Combined include and exclude
    let combo = vec![
        "crates/".to_string(),
        ":(exclude)crates/ignored".to_string(),
    ];
    assert!(pathspecs_match("crates/diffuse/src/main.rs", &combo));
    assert!(!pathspecs_match("crates/ignored/file.rs", &combo));
    assert!(!pathspecs_match("README.md", &combo));
}

#[test]
fn test_resolve_commit_display_name() {
    use crate::testing::{commit_files, create_tag, init_repo};

    let tmp = tempfile::tempdir().unwrap();
    let repo = init_repo(tmp.path()).expect("init_repo succeeds");

    let c1 = commit_files(&repo, "c1", &[("a.txt", "1\n")]).expect("c1");
    create_tag(&repo, "v1.0", c1).expect("tag v1.0");

    let c2 = commit_files(&repo, "c2", &[("a.txt", "2\n")]).expect("c2");

    repo.raw()
        .reference(
            "refs/heads/scrollbar-grab-drag",
            c2,
            gix::refs::transaction::PreviousValue::Any,
            "create branch scrollbar-grab-drag",
        )
        .expect("create branch");

    let _c3 = commit_files(&repo, "c3", &[("a.txt", "3\n")]).expect("c3");

    // 1. Exact branch name
    assert_eq!(
        repo.resolve_commit_display_name("scrollbar-grab-drag"),
        Some("scrollbar-grab-drag".to_string())
    );

    // 2. Exact tag name
    assert_eq!(
        repo.resolve_commit_display_name("v1.0"),
        Some("v1.0".to_string())
    );

    // 3. HEAD (case-insensitive)
    assert_eq!(
        repo.resolve_commit_display_name("HEAD"),
        Some("HEAD".to_string())
    );
    assert_eq!(
        repo.resolve_commit_display_name("head"),
        Some("HEAD".to_string())
    );

    // 4. Commit hash falls back to 7-character short hash
    let s1 = &c1.to_string()[..7];
    let s2 = &c2.to_string()[..7];
    assert_eq!(
        repo.resolve_commit_display_name(&c1.to_string()),
        Some(s1.to_string())
    );
    assert_eq!(
        repo.resolve_commit_display_name(&c2.to_string()),
        Some(s2.to_string())
    );

    // 5. Test resolve_commit_range preserving symbolic branch/tag/HEAD names
    assert_eq!(
        repo.resolve_commit_range(&["scrollbar-grab-drag^!".to_string()]),
        Some("scrollbar-grab-drag^!".to_string())
    );
    assert_eq!(
        repo.resolve_commit_range(&["v1.0^!".to_string()]),
        Some("v1.0^!".to_string())
    );
    assert_eq!(
        repo.resolve_commit_range(&["HEAD^!".to_string()]),
        Some("HEAD^!".to_string())
    );
    assert_eq!(
        repo.resolve_commit_range(&["head^!".to_string()]),
        Some("HEAD^!".to_string())
    );

    // Single-commit range c1..scrollbar-grab-drag resolves to scrollbar-grab-drag^!
    assert_eq!(
        repo.resolve_commit_range(&[format!("{c1}..scrollbar-grab-drag")]),
        Some("scrollbar-grab-drag^!".to_string())
    );
    assert_eq!(
        repo.resolve_commit_range(&[c1.to_string(), "scrollbar-grab-drag".to_string()]),
        Some("scrollbar-grab-drag^!".to_string())
    );

    // Single-commit range HEAD~1..HEAD resolves to HEAD^!
    assert_eq!(
        repo.resolve_commit_range(&["HEAD~1..HEAD".to_string()]),
        Some("HEAD^!".to_string())
    );

    // Single-commit range c1..c2 with commit hashes displays short hash ^!
    assert_eq!(
        repo.resolve_commit_range(&[format!("{c1}..{c2}")]),
        Some(format!("{s2}^!"))
    );
}
