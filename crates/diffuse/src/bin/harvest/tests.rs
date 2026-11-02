//! Acceptance and regression unit tests for the golden harvest tool.

use std::path::Path;

use clap::CommandFactory;
use diffuse_git::testing::{commit_files, init_repo};

use super::{
    args::{Args, exclude_pathspecs},
    fixture::{is_diffuse_repo, resolve_golden_name, snapshot_filename},
    git::{resolve_revision, target_revision_endpoint},
};

#[test]
fn resolves_revision_ranges_to_fixed_commit_ids() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = init_repo(dir.path()).expect("init repo");
    let c1 = commit_files(&repo, "one", &[("a.txt", "one")])
        .expect("commit 1")
        .to_string();
    let c2 = commit_files(&repo, "two", &[("a.txt", "two")])
        .expect("commit 2")
        .to_string();
    let c3 = commit_files(&repo, "three", &[("a.txt", "three")])
        .expect("commit 3")
        .to_string();

    let s1 = &c1[..7];
    let s2 = &c2[..7];
    let s3 = &c3[..7];

    // 1. Single commit range (HEAD~1..HEAD or c2..c3 or c1..c2) resolves to
    //    <short>^!
    assert_eq!(
        resolve_revision(dir.path(), "HEAD~1..HEAD").expect("resolve 1-commit range"),
        format!("{s3}^!"),
        "single commit relative range resolves to <short_hash>^!"
    );
    assert_eq!(
        resolve_revision(dir.path(), &format!("{c1}..{c2}")).expect("resolve 1-commit range"),
        format!("{s2}^!"),
        "single commit absolute range resolves to <short_hash>^!"
    );
    assert_eq!(
        resolve_revision(dir.path(), "HEAD^!").expect("resolve ^!"),
        format!("{s3}^!"),
        "explicit ^! resolves to <short_hash>^!"
    );

    // 2. Multi-commit range (HEAD~2..HEAD or c1..c3) resolves to
    //    <short_left>..<short_right>
    assert_eq!(
        resolve_revision(dir.path(), "HEAD~2..HEAD").expect("resolve multi-commit range"),
        format!("{s1}..{s3}"),
        "multi-commit relative range resolves to short sha range"
    );
    assert_eq!(
        resolve_revision(dir.path(), &format!("{c1}..{c3}")).expect("resolve multi-commit range"),
        format!("{s1}..{s3}"),
        "multi-commit absolute range resolves to short sha range"
    );

    // 3. Single revision (HEAD or commit sha) resolves to its short hash
    assert_eq!(
        resolve_revision(dir.path(), "HEAD").expect("resolve single"),
        s3,
        "single revision resolves to its short commit id"
    );

    // 4. Three-dot range is preserved with short hashes
    assert_eq!(
        resolve_revision(dir.path(), "HEAD~2...HEAD").expect("resolve merge-base range"),
        format!("{s1}...{s3}"),
        "three-dot separator is preserved with short hashes"
    );

    // 5. Open-left range (..HEAD means HEAD..HEAD, which is 0 commits)
    assert_eq!(
        resolve_revision(dir.path(), "..HEAD").expect("resolve open-left range"),
        format!("{s3}..{s3}"),
        "empty range side means HEAD"
    );

    // 6. WORKTREE mode
    assert_eq!(
        resolve_revision(dir.path(), "WORKTREE").expect("worktree passthrough"),
        "WORKTREE",
        "WORKTREE is a special mode, not a revision"
    );

    // 7. Unresolvable revisions
    assert!(
        resolve_revision(dir.path(), "no-such-rev").is_err(),
        "unresolvable revisions fail loudly"
    );
}

#[test]
fn exclude_patterns_map_to_git_exclude_pathspecs() {
    assert!(exclude_pathspecs(&[]).is_empty(), "no patterns, no specs");
    assert_eq!(
        exclude_pathspecs(&["tests/golden".to_string(), "dist".to_string()]),
        vec![":(exclude)tests/golden", ":(exclude)dist"],
        "each pattern becomes a git exclude pathspec"
    );
}

#[test]
fn snapshot_names_slug_paths_and_cap_length() {
    assert_eq!(
        snapshot_filename("src/collapse/mod.rs", "old"),
        "src-collapse-mod.old.rs",
        "path folds to a side-tagged snapshot name"
    );
    assert_eq!(
        snapshot_filename("Makefile", "new"),
        "Makefile.new",
        "extension-less paths get a plain side suffix"
    );

    // Overflow keeps the TAIL of the slug — the file's own name — with a
    // stable hash of the full path in front; side tag and extension still
    // trail.
    let deep = "very-long-directory-name/".repeat(12).to_string() + "mod.rs";
    let deep_name = snapshot_filename(&deep, "old");
    assert!(deep_name.len() <= 255, "fits the filesystem limit");
    assert!(deep_name.ends_with(".old.rs"), "side and extension survive");
    let core = deep_name
        .strip_suffix(".old.rs")
        .expect("side and extension");
    let (tail, hash) = core.rsplit_once('-').expect("hash separator");
    assert_eq!(
        hash.len(),
        16,
        "stable FNV-1a hash marks the dropped prefix"
    );
    assert!(tail.ends_with("mod"), "the file's own name is kept");
    assert_eq!(deep_name, snapshot_filename(&deep, "old"), "stable");

    // A filename longer than the whole budget keeps its own tail.
    let long_a = format!("deep/{}", "x".repeat(200));
    let long_b = format!("deep/{}", "y".repeat(200));
    for (name, fill) in [
        (snapshot_filename(&long_a, "old"), 'x'),
        (snapshot_filename(&long_b, "old"), 'y'),
    ] {
        assert!(name.len() <= 255, "fits the filesystem limit");
        assert!(name.ends_with(".old"), "side tag survives");
        let core = name.strip_suffix(".old").expect("side tag");
        let (tail, hash) = core.rsplit_once('-').expect("hash separator");
        assert_eq!(hash.len(), 16, "stable FNV-1a hash suffix");
        assert!(
            tail.chars().all(|c| c == fill),
            "the filename's tail survives: {tail}"
        );
    }
    assert_ne!(
        snapshot_filename(&long_a, "old"),
        snapshot_filename(&long_b, "old"),
        "distinct long paths hash to distinct names"
    );

    // A long path with a compound extension keeps the extension.
    let tgz = format!("deep/{}.tar.gz", "z".repeat(200));
    assert!(
        snapshot_filename(&tgz, "new").ends_with(".new.gz"),
        "compound extension survives truncation"
    );
}

#[test]
fn test_harvest_clap_debug_assert_and_help() {
    Args::command().debug_assert();
    let mut cmd = Args::command();
    let help = cmd.render_help().to_string();
    assert!(help.contains("Repository & Input:"));
    assert!(help.contains("Output:"));
    assert!(help.contains("Diff Options:"));
    assert!(help.contains("TYPICAL USAGE & EXAMPLES:"));
}

#[test]
fn test_resolve_golden_name() {
    // No tag and not diffuse
    assert_eq!(resolve_golden_name(None, None, false).unwrap(), None);

    // Explicit tag name
    assert_eq!(
        resolve_golden_name(Some("my-tag"), None, false).unwrap(),
        Some("my-tag".to_string())
    );

    // In diffuse repo, automatically derives from output path
    let out = Path::new("tests/golden/scrollbar-grab-drag");
    assert_eq!(
        resolve_golden_name(None, Some(out), true).unwrap(),
        Some("scrollbar-grab-drag".to_string())
    );

    // Not in diffuse repo without explicit tag
    assert_eq!(resolve_golden_name(None, Some(out), false).unwrap(), None);

    // Explicit empty tag flag (--tag without value)
    assert_eq!(
        resolve_golden_name(Some(""), Some(out), false).unwrap(),
        Some("scrollbar-grab-drag".to_string())
    );

    // Explicit empty tag flag without output path returns error
    assert!(resolve_golden_name(Some(""), None, false).is_err());
}

#[test]
fn test_is_diffuse_repo() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.parent().unwrap().parent().unwrap();
    assert!(is_diffuse_repo(root));

    let tmp = tempfile::tempdir().unwrap();
    assert!(!is_diffuse_repo(tmp.path()));
}

#[test]
fn test_target_revision_endpoint() {
    assert_eq!(target_revision_endpoint("WORKTREE"), "HEAD");
    assert_eq!(target_revision_endpoint("HEAD~1..HEAD"), "HEAD");
    assert_eq!(target_revision_endpoint("c1..c2"), "c2");
    assert_eq!(target_revision_endpoint("c2^!"), "c2");
    assert_eq!(target_revision_endpoint("HEAD^!"), "HEAD");
    assert_eq!(target_revision_endpoint("HEAD~2...HEAD"), "HEAD");
    assert_eq!(target_revision_endpoint("..HEAD"), "HEAD");
    assert_eq!(target_revision_endpoint("HEAD.."), "HEAD");
    assert_eq!(target_revision_endpoint("c1"), "c1");
    assert_eq!(target_revision_endpoint(""), "HEAD");
}
