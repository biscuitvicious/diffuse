//! Dynamic shell completion for git revisions, branches, and tags.
//!
//! Provides intelligent tab-completion for git revisions and commit ranges
//! (`HEAD`, branches, tags, `main..feat`, `HEAD~1`), searching parent
//! directories upward to find the repository root.

use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use clap_complete::engine::CompletionCandidate;
use diffuse::git::{Repository, RevisionCandidate, RevisionKind};

/// Locate the git repository root by walking parent directories upward from
/// `start`.
#[must_use]
pub fn find_git_repository(start: &Path) -> Option<Repository> {
    for dir in start.ancestors() {
        let dot_git = dir.join(".git");
        if dot_git.exists()
            && let Ok(repo) = Repository::open(dir)
        {
            return Some(repo);
        }
    }
    Repository::discover(start).ok()
}

/// Dynamic completer function for git revision arguments.
///
/// Handles:
/// - Direct branch, tag, and HEAD completion (e.g. `mai` -> `main`)
/// - Two-dot commit ranges (e.g. `main..fe` -> `main..feat`)
/// - Three-dot commit ranges (e.g. `main...fe` -> `main...feat`)
/// - Relative revisions (e.g. `HEAD~` -> `HEAD~1`)
#[must_use]
pub fn complete_git_revision(current: &OsStr) -> Vec<CompletionCandidate> {
    let current_str = current.to_string_lossy();
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let Some(repo) = find_git_repository(&cwd) else {
        return Vec::new();
    };

    let raw_candidates = repo.revision_candidates();

    // Handle range syntax: `lhs..rhs` or `lhs...rhs`
    if let Some((lhs, rhs)) = current_str.split_once("...") {
        let prefix = format!("{lhs}...");
        return filter_and_format_candidates(&raw_candidates, rhs, Some(&prefix));
    }
    if let Some((lhs, rhs)) = current_str.split_once("..") {
        let prefix = format!("{lhs}..");
        return filter_and_format_candidates(&raw_candidates, rhs, Some(&prefix));
    }

    // Relative parent notation helpers: `HEAD~` or `HEAD^`
    if current_str.starts_with("HEAD~") {
        return (1..=5)
            .map(|n| {
                let val = format!("HEAD~{n}");
                CompletionCandidate::new(val).help(Some(RevisionKind::Special.as_str().into()))
            })
            .collect();
    }
    if current_str.starts_with("HEAD^") {
        return (1..=3)
            .map(|n| {
                let val = format!("HEAD^{n}");
                CompletionCandidate::new(val).help(Some(RevisionKind::Special.as_str().into()))
            })
            .collect();
    }

    filter_and_format_candidates(&raw_candidates, &current_str, None)
}

fn filter_and_format_candidates(
    candidates: &[RevisionCandidate],
    query: &str,
    range_prefix: Option<&str>,
) -> Vec<CompletionCandidate> {
    candidates
        .iter()
        .filter(|c| c.name.starts_with(query))
        .map(|c| {
            let full_val = match range_prefix {
                Some(p) => format!("{p}{}", c.name),
                None => c.name.clone(),
            };
            CompletionCandidate::new(full_val).help(Some(c.kind.as_str().into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_find_git_repository_parent_walk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let deep_sub = root.join("a/b/c/d");
        fs::create_dir_all(&deep_sub).unwrap();

        // Not a repo yet
        assert!(find_git_repository(&deep_sub).is_none());

        // Create git repo at root
        let _ = diffuse_git::testing::init_repo(root).unwrap();

        // Discovers repo from deep child
        let repo = find_git_repository(&deep_sub).expect("must discover repo from deep subpath");
        assert_eq!(repo.work_dir(), Some(root));
    }

    #[test]
    fn test_complete_git_revision_ranges_and_relative() {
        let candidates = vec![
            RevisionCandidate {
                name: "main".to_string(),
                kind: RevisionKind::LocalBranch,
            },
            RevisionCandidate {
                name: "feature-1".to_string(),
                kind: RevisionKind::LocalBranch,
            },
            RevisionCandidate {
                name: "origin/main".to_string(),
                kind: RevisionKind::RemoteBranch,
            },
            RevisionCandidate {
                name: "v1.0.0".to_string(),
                kind: RevisionKind::Tag,
            },
        ];

        // Direct matching
        let res = filter_and_format_candidates(&candidates, "fea", None);
        assert_eq!(res.len(), 1);

        // Two-dot range
        let res_range = filter_and_format_candidates(&candidates, "fea", Some("main.."));
        assert_eq!(res_range.len(), 1);

        // Three-dot range
        let res_three = filter_and_format_candidates(&candidates, "v", Some("origin/main..."));
        assert_eq!(res_three.len(), 1);
    }
}
