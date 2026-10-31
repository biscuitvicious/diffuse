use std::{fs, slice, sync::mpsc};

use diffuse_git::testing::{commit_files, init_repo};

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

    let repo = init_repo(dir).unwrap();
    commit_files(&repo, "commit1", &[("a.rs", "fn a() {}\n")]).unwrap();
    commit_files(&repo, "commit2", &[("a.rs", "fn a() { 1 }\n")]).unwrap();

    // Untracked file — must NOT appear when diffing a fixed range
    fs::write(dir.join("bogus_untracked.txt"), "I should not appear\n").unwrap();

    let range = "HEAD~1..HEAD".to_string();
    let res = load_git_repo(dir, slice::from_ref(&range), &[], false, 4).unwrap();
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
    let (meta_tx, _meta_rx) = mpsc::channel();
    stream_git_repo_to_producer(
        dir,
        &[range],
        &[],
        false,
        4,
        producer,
        &meta_tx,
        None,
        None,
        None,
    );
    let files: Vec<_> = consumer.collect();
    assert!(
        files.iter().all(|f| f.path != "bogus_untracked.txt"),
        "streaming: fixed commit range must not include untracked files"
    );

    // Clean up
    fs::remove_file(dir.join("bogus_untracked.txt")).unwrap();
}

#[test]
fn test_load_git_repo_includes_untracked_files_with_synthetic_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = init_repo(dir).unwrap();
    commit_files(&repo, "init", &[("tracked.rs", "fn tracked() {}\n")]).unwrap();

    // Create untracked file
    fs::write(dir.join("untracked.rs"), "fn untracked() {}\n").unwrap();

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
    assert_eq!(untracked_file.hunks[0].rows[0].text(), "fn untracked() {}");
}

#[test]
fn test_load_git_blobs_batch() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = init_repo(dir).unwrap();
    commit_files(
        &repo,
        "commit1",
        &[("f1.txt", "content 1\n"), ("f2.txt", "content 2\n")],
    )
    .unwrap();
    commit_files(&repo, "commit2", &[("f1.txt", "content 1 mod\n")]).unwrap();

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

    let repo = init_repo(dir).unwrap();
    commit_files(
        &repo,
        "native1",
        &[("f1.txt", "hello native\n"), ("f2.txt", "world native\n")],
    )
    .unwrap();
    commit_files(&repo, "native2", &[("f1.txt", "hello native mod\n")]).unwrap();

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

    let repo = init_repo(dir).unwrap();
    let initial_content = "# Initial Title\nLine 1\nLine 2\n";
    commit_files(&repo, "commit1", &[("doc.md", initial_content)]).unwrap();

    // Append a line: only additions (+1), 0 deletions (-0)
    let appended_content = "# Initial Title\nLine 1\nLine 2\nLine 3 added\n";
    fs::write(dir.join("doc.md"), appended_content).unwrap();

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
    let (meta_tx, _meta_rx) = mpsc::channel();
    stream_git_repo_to_producer(
        dir,
        &[],
        &[],
        false,
        4,
        producer,
        &meta_tx,
        None,
        None,
        None,
    );
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

    let repo = init_repo(dir).unwrap();
    commit_files(&repo, "c1", &[("a.rs", "fn a() {}\n")]).unwrap();
    let c1_hash = repo.rev_to_short_hash("HEAD").unwrap();

    commit_files(&repo, "c2", &[("a.rs", "fn a() { 1 }\n")]).unwrap();
    let c2_hash = repo.rev_to_short_hash("HEAD").unwrap();

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
    assert_eq!(resolve_commit_range(dir, &["--staged".into()]), None);
    assert_eq!(resolve_commit_range(dir, &["--cached".into()]), None);
    assert_eq!(resolve_commit_range(dir, &["HEAD".into()]), None);
    assert_eq!(resolve_commit_range(dir, &["a.rs".into()]), None);

    // Also test that load_git_repo populates commit_range
    let res = load_git_repo(dir, &["HEAD~1..HEAD".into()], &[], false, 4).unwrap();
    assert_eq!(res.commit_range, Some(format!("{c1_hash}..{c2_hash}")));
}

#[test]
fn test_load_git_repo_staged_excludes_untracked_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = init_repo(dir).unwrap();
    commit_files(&repo, "init", &[("tracked.rs", "fn tracked() {}\n")]).unwrap();

    // Create untracked file and stage a change to tracked.rs
    fs::write(dir.join("untracked.rs"), "fn untracked() {}\n").unwrap();
    fs::write(dir.join("tracked.rs"), "fn tracked() { 42 }\n").unwrap();
    diffuse_git::stage_file(&repo, "tracked.rs").unwrap();

    // 1. With staged param true: excludes untracked files
    let res_staged = load_git_repo(dir, &[], &[], true, 6).unwrap();
    assert!(
        !res_staged.diff.contains("untracked.rs"),
        "staged diff must NOT contain untracked.rs"
    );
    assert!(
        !res_staged.new_content.contains_key("untracked.rs"),
        "staged new_content must NOT contain untracked.rs"
    );
    assert!(
        res_staged.diff.contains("tracked.rs"),
        "staged diff must contain staged tracked.rs"
    );

    // 2. With staged param false: includes untracked files
    let res_unstaged = load_git_repo(dir, &[], &[], false, 6).unwrap();
    assert!(
        res_unstaged.diff.contains("untracked.rs"),
        "unstaged diff must contain untracked.rs"
    );
    assert!(
        res_unstaged.new_content.contains_key("untracked.rs"),
        "unstaged new_content must contain untracked.rs"
    );
}

#[test]
fn test_load_git_repo_unmodified_files_with_notes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = init_repo(dir).unwrap();
    commit_files(
        &repo,
        "init",
        &[
            ("modified.rs", "fn modified() { 1 }\n"),
            (
                "untouched.rs",
                "fn untouched() {\n    println!(\"hello\");\n}\n",
            ),
        ],
    )
    .unwrap();

    // Modify one file
    fs::write(dir.join("modified.rs"), "fn modified() { 2 }\n").unwrap();

    // Prepare NotesDB with a note on untouched.rs
    let mut db = crate::notes::NotesDB::new();
    let head = crate::notes::head_commit(dir).unwrap();
    let note = crate::notes::ReviewNote {
        id: "n1".into(),
        file: "untouched.rs".into(),
        location: crate::notes::NoteLocation::line(crate::notes::DiffCoord::new_side(2)),
        text: "note on untouched file".into(),
        created_at: 1000,
        content_hash: None,
        ..Default::default()
    };
    crate::notes::add_note(&mut db, "myrepo", &head, note);

    let res = load_git_repo_with_notes(
        dir,
        &[],
        &[],
        false,
        4,
        Some(&db),
        Some("myrepo"),
        Some(&head),
    )
    .unwrap();
    assert!(
        res.diff.contains("untouched.rs"),
        "diff must contain unmodified untouched.rs"
    );
    assert!(
        !res.new_content.contains_key("untouched.rs"),
        "unanchored note must not load untouched.rs content"
    );

    let out = crate::pipeline::run_pipeline_full(
        &res.diff,
        Some(res.new_content),
        Some(res.old_content),
        crate::pipeline::PipelineOptions::default().with_notes(
            Some(db.clone()),
            Some("myrepo".to_string()),
            Some(head.clone()),
        ),
    );
    assert_eq!(out.files.len(), 2, "expected modified.rs and untouched.rs");
    let untouched = out
        .files
        .iter()
        .find(|f| f.path == "untouched.rs")
        .expect("untouched.rs present");
    assert_eq!(untouched.status, "unmodified");
    assert_eq!(untouched.counts.adds, 0);
    assert_eq!(untouched.counts.dels, 0);
    assert_eq!(untouched.counts.mods, 0);
    assert_eq!(untouched.counts.notes, 1);
    assert!(
        untouched.hunks.is_empty(),
        "unanchored note must not generate a synthetic context hunk"
    );
}

#[test]
fn ws_only_changed_file_stays_visible_and_paired() {
    // A file whose only change is a reindent is invisible to a
    // whitespace-insensitive diff. The loader must keep it visible, and the
    // pipeline must pair the reindent with its true source (as a Mod
    // carrying only indent spans, or as a Mov — representation is engine
    // policy).
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = init_repo(dir).unwrap();
    commit_files(&repo, "commit1", &[("f.rs", "fn a() {\n    x();\n}\n")]).unwrap();

    // Whitespace-only change: the line reindents.
    fs::write(dir.join("f.rs"), "fn a() {\n\tx();\n}\n").unwrap();

    let res = load_git_repo(dir, &[], &[], false, 6).unwrap();
    assert!(
        res.diff.contains("+++ b/f.rs"),
        "a whitespace-only change must stay visible in the loaded diff"
    );
    assert!(res.old_content.contains_key("f.rs"), "old snapshot loaded");
    assert!(res.new_content.contains_key("f.rs"), "new snapshot loaded");

    let out = crate::pipeline::run_pipeline_full(
        res.diff,
        Some(res.new_content),
        Some(res.old_content),
        crate::pipeline::PipelineOptions::default(),
    );
    let rows: Vec<&crate::wire::OutputRow> = out
        .files
        .iter()
        .filter(|f| f.path == "f.rs")
        .flat_map(|f| f.hunks.iter())
        .flat_map(|h| h.rows.iter())
        .collect();
    assert!(
        git_rows_pair(&rows, 2, 2),
        "the reindented line must pair with its source, got {rows:#?}"
    );
}

#[test]
fn ws_only_changed_file_pairs_through_single_file_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    let repo = init_repo(dir).unwrap();
    commit_files(&repo, "commit1", &[("f.rs", "fn a() {\n    x();\n}\n")]).unwrap();

    fs::write(dir.join("f.rs"), "fn a() {\n\tx();\n}\n").unwrap();

    let rendered = load_single_file_diff(
        dir,
        &[],
        "f.rs",
        None,
        "modified",
        6,
        &crate::pipeline::PipelineOptions::default(),
    )
    .expect("single-file render");
    let rows: Vec<&crate::wire::OutputRow> =
        rendered.hunks.iter().flat_map(|h| h.rows.iter()).collect();
    assert!(
        git_rows_pair(&rows, 2, 2),
        "the reindented line must pair with its source, got {rows:#?}"
    );
}

/// Whether rendered rows link old line `old` with new line `new` — as a Mod,
/// or as an adjacent MovSrc/MovTgt pair (representation is engine policy;
/// the pairing is the contract).
fn git_rows_pair(rows: &[&crate::wire::OutputRow], old: u32, new: u32) -> bool {
    crate::wire::test_rows_pair_with(rows, old, new)
}
