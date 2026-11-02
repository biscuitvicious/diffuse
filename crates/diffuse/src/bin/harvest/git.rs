//! Git revision resolution, commit pinning, and endpoint extraction.

use std::path::Path;

use diffuse_git::Repository;

/// Resolves a git revision or range expression to absolute short commit ids or
/// `<ref>^!` so the emitted fixture pins the exact harvested history.
///
/// Single-commit ranges (e.g. `HEAD~1..HEAD`, `c1..c2` where `c1 == c2^`, or
/// `c^!`) are formatted as `<short>^!`. Multi-commit ranges stick to a plain
/// `<short_left>..<short_right>` (or `...` for three-dot). Single revisions
/// resolve to their short sha1 hash.
pub fn resolve_revision(repo_root: &Path, revision: &str) -> Result<String, String> {
    if revision == "WORKTREE" {
        return Ok(revision.to_string());
    }
    let repo = Repository::open(repo_root).map_err(|e| format!("cannot open repository: {e}"))?;
    let resolve_side = |side: &str| -> Result<String, String> {
        let side = if side.is_empty() { "HEAD" } else { side };
        repo.rev_to_short_hash(side)
            .ok_or_else(|| format!("cannot resolve revision '{side}'"))
    };

    if let Some(c) = revision.strip_suffix("^!") {
        let short = resolve_side(c)?;
        return Ok(format!("{short}^!"));
    }

    let (pos, sep_len) = match (revision.find("..."), revision.find("..")) {
        (Some(i), _) => (i, 3),
        (_, Some(i)) => (i, 2),
        _ => (revision.len(), 0),
    };

    if sep_len == 2 {
        let left_spec = &revision[..pos];
        let right_spec = &revision[pos + 2..];
        if repo.is_single_commit_range(left_spec, right_spec) {
            let short_right = resolve_side(right_spec)?;
            Ok(format!("{short_right}^!"))
        } else {
            let left = resolve_side(left_spec)?;
            let right = resolve_side(right_spec)?;
            Ok(format!("{left}..{right}"))
        }
    } else if sep_len == 3 {
        let left = resolve_side(&revision[..pos])?;
        let right = resolve_side(&revision[pos + 3..])?;
        Ok(format!("{left}...{right}"))
    } else {
        resolve_side(revision)
    }
}

/// Extracts the target commit endpoint (new/head side) of a revision or range
/// specifier.
pub fn target_revision_endpoint(revision: &str) -> &str {
    if revision == "WORKTREE" {
        "HEAD"
    } else if let Some(c) = revision.strip_suffix("^!") {
        if c.is_empty() { "HEAD" } else { c }
    } else if let Some(pos) = revision.find("...") {
        let right = &revision[pos + 3..];
        if right.is_empty() { "HEAD" } else { right }
    } else if let Some(pos) = revision.find("..") {
        let right = &revision[pos + 2..];
        if right.is_empty() { "HEAD" } else { right }
    } else if revision.is_empty() {
        "HEAD"
    } else {
        revision
    }
}
