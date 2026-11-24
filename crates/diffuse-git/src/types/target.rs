//! Review target specification and argument partitioning.

use std::path::{Path, PathBuf};

use super::{
    range::{RefPair, RevisionRange},
    repo::Repo,
};

fn resolve_rev_key(repo: &Repo, git_revs: &[String]) -> Option<String> {
    let root = repo.root()?;
    crate::repository::Repository::open(root)
        .ok()?
        .notes_commit(git_revs)
}

/// The subject of a diffuse review: a repository and its revision target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewTarget {
    pub repo: Repo,
    pub range: Option<RevisionRange>,
    pub git_revs: Vec<String>,
    pub pathspecs: Vec<String>,
    pub staged: bool,
    pub rev_key: Option<String>,
}

impl ReviewTarget {
    /// Create a review target from a repo, optional range, preserved git args,
    /// and staged flag.
    pub fn new(
        repo: Repo,
        range: Option<RevisionRange>,
        git_revs: Vec<String>,
        pathspecs: Vec<String>,
        staged: bool,
    ) -> Self {
        let rev_key = resolve_rev_key(&repo, &git_revs);
        Self {
            repo,
            range,
            git_revs,
            pathspecs,
            staged,
            rev_key,
        }
    }

    /// Create a review target from a repo, git arguments, and staged flag.
    pub fn from_repo_args(repo: Repo, git_revs: Vec<String>, staged: bool) -> Self {
        let (revs, paths) = parse_git_rev_and_pathspecs(repo.root(), &git_revs);
        let range = repo
            .root()
            .and_then(|root| RevisionRange::resolve(root, &revs));
        let rev_key = resolve_rev_key(&repo, &revs);
        Self {
            repo,
            range,
            git_revs: revs,
            pathspecs: paths,
            staged,
            rev_key,
        }
    }

    /// Create a review target from explicit revision arguments and pathspecs.
    pub fn from_parts(
        repo: Repo,
        git_revs: Vec<String>,
        pathspecs: Vec<String>,
        staged: bool,
    ) -> Self {
        let range = repo
            .root()
            .and_then(|root| RevisionRange::resolve(root, &git_revs));
        let rev_key = resolve_rev_key(&repo, &git_revs);
        Self {
            repo,
            range,
            git_revs,
            pathspecs,
            staged,
            rev_key,
        }
    }

    /// Create a default review target for a repo with empty git arguments.
    pub fn from_repo(repo: Repo) -> Self {
        let rev_key = resolve_rev_key(&repo, &[]);
        Self {
            repo,
            range: None,
            git_revs: Vec::new(),
            pathspecs: Vec::new(),
            staged: false,
            rev_key,
        }
    }

    /// Create a review target rooted at a directory with empty git arguments.
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self::from_repo(Repo::from_root(root))
    }

    /// Override the revision key on this review target.
    pub fn with_rev_key(mut self, rev_key: impl Into<String>) -> Self {
        self.rev_key = Some(rev_key.into());
        self
    }

    /// Override the revision key on this review target if Some.
    pub fn with_rev_key_opt(mut self, rev_key: Option<String>) -> Self {
        if let Some(rk) = rev_key {
            self.rev_key = Some(rk);
        }
        self
    }

    /// Override the repository key on this review target if Some.
    pub fn with_repo_key_opt(mut self, repo_key: Option<String>) -> Self {
        self.repo = self.repo.with_key_opt(repo_key);
        self
    }

    /// Whether this target is a commit-to-commit range rather than a working
    /// tree review.
    pub fn is_range(&self) -> bool {
        self.range.is_some()
    }

    /// The revision range, if this target compares two commits.
    pub fn range(&self) -> Option<&RevisionRange> {
        self.range.as_ref()
    }

    /// The repository key used for notes lookup (e.g. canonical root path or
    /// explicit override).
    pub fn repo_key(&self) -> Option<&str> {
        self.repo.key()
    }

    /// The revision key used for notes lookup (e.g. commit hash, range, or
    /// explicit override).
    pub fn rev_key(&self) -> Option<&str> {
        self.rev_key.as_deref()
    }

    /// The human-readable display name of the repository.
    pub fn display_name(&self) -> &str {
        self.repo.display_name()
    }

    /// The filesystem root of the repository, if rooted.
    pub fn repo_root(&self) -> Option<&Path> {
        self.repo.root()
    }

    /// Formatted commit range string (e.g. `c1..c2` or `c2^!`), or `None` if
    /// reviewing working tree.
    pub fn commit_range(&self) -> Option<String> {
        if let Some(root) = self.repo.root()
            && let Ok(repo) = crate::repository::Repository::open(root)
            && let Some(r) = repo.resolve_commit_range(&self.git_revs)
        {
            return Some(r);
        }
        if let Some(ref r) = self.range
            && let Some(root) = self.repo.root()
            && let Ok(repo) = crate::repository::Repository::open(root)
            && repo.is_single_commit_range(&r.0, &r.1)
        {
            let name = repo
                .resolve_commit_display_name(&r.1)
                .or_else(|| repo.rev_to_short_hash(&r.1))
                .unwrap_or_else(|| r.1.clone());
            return Some(format!("{name}^!"));
        }
        self.range.as_ref().map(ToString::to_string)
    }

    /// Resolves the single commit hash if `git_revs` targets a single commit
    /// (e.g. `c^!` or `p..c` where `p` is `c`'s parent).
    pub fn single_commit(&self) -> Option<String> {
        let root = self.repo.root()?;
        let repo = crate::repository::Repository::open(root).ok()?;
        let ref_pair = RefPair::resolve(&self.git_revs, self.staged);
        if !ref_pair.worktree_new
            && let (Some(old), Some(new)) = (&ref_pair.old_ref, &ref_pair.new_ref)
            && repo.is_single_commit_range(old, new)
        {
            return repo.rev_to_full_hash(new);
        }
        None
    }

    /// Check whether a file path matches this target's pathspecs.
    pub fn matches_path(&self, file_path: &str, current_dir: &Path) -> bool {
        if self.pathspecs.is_empty() {
            return true;
        }

        let file_clean = file_path.trim_start_matches("./");

        self.pathspecs.iter().any(|spec| {
            let spec_clean = spec.trim_start_matches("./").trim_end_matches('/');

            if file_clean == spec_clean {
                return true;
            }

            if file_clean.starts_with(&format!("{spec_clean}/")) {
                return true;
            }

            if let Some(root) = self.repo.root()
                && let Ok(rel) = current_dir.join(spec).strip_prefix(root)
            {
                let rel_str = rel.to_string_lossy();
                let rel_clean = rel_str.trim_start_matches("./").trim_end_matches('/');
                if file_clean == rel_clean || file_clean.starts_with(&format!("{rel_clean}/")) {
                    return true;
                }
            }

            file_clean.ends_with(&format!("/{spec_clean}"))
        })
    }

    /// Active branch name.
    pub fn branch(&self) -> String {
        self.repo.branch()
    }

    /// Whether the repository is inside a git work tree.
    pub fn is_work_tree(&self) -> bool {
        self.repo.is_work_tree()
    }

    /// Whether the review target diffs against the staged index.
    pub fn is_staged(&self) -> bool {
        self.staged
    }

    /// Alias for [`ReviewTarget::is_staged`].
    pub fn is_cached(&self) -> bool {
        self.is_staged()
    }
}

/// Separate git CLI arguments into revisions and pathspecs.
pub fn parse_git_rev_and_pathspecs(
    repo_root: Option<&Path>,
    git_args: &[String],
) -> (Vec<String>, Vec<String>) {
    if let Some(pos) = git_args.iter().position(|a| a == "--") {
        let revs = git_args[..pos]
            .iter()
            .filter(|a| !a.starts_with('-'))
            .cloned()
            .collect();
        let paths = git_args[pos + 1..].to_vec();
        return (revs, paths);
    }

    let mut revs = Vec::new();
    let mut paths = Vec::new();

    let repo = repo_root.and_then(|r| crate::repository::Repository::open(r).ok());

    for arg in git_args {
        if arg.starts_with('-') {
            continue;
        }
        if arg.contains("..") || arg.contains("...") || arg.ends_with("^!") {
            revs.push(arg.clone());
            continue;
        }
        if let Some(ref r) = repo {
            let is_commit = r.is_commit(arg);
            if is_commit && revs.len() < 2 && !r.work_dir().is_some_and(|w| w.join(arg).exists()) {
                revs.push(arg.clone());
            } else {
                paths.push(arg.clone());
            }
        } else {
            paths.push(arg.clone());
        }
    }

    (revs, paths)
}
