//! Reusable Gitoxide repository test fixtures and helpers.
//!
//! Provides pure in-process git repository initialization, commit creation,
//! branching, and tree modification for unit and integration tests across
//! all workspace crates without external git subprocesses.

use std::{fs, path::Path};

use gix::{bstr::ByteSlice, prelude::Write, refs::transaction::PreviousValue};

use crate::{
    GitError, Repository,
    mutation::{index_mode_to_entry_kind, stage_file},
};

/// Initialize a new git repository at `path` with canonical test configuration.
pub fn init_repo(path: &Path) -> Result<Repository, GitError> {
    gix::init(path)?;

    // Write minimal portable git configuration for testing
    let config_dir = path.join(".git");
    let config_path = config_dir.join("config");
    let initial_config = "\
[core]
\trepositoryformatversion = 0
\tfilemode = true
\tbare = false
\tlogallrefupdates = true
[user]
\tname = diffuse
\temail = diffuse@example.com
[commit]
\tgpgsign = false
[init]
\tdefaultBranch = main
";
    fs::write(config_path, initial_config)
        .map_err(|e| GitError::OperationFailed(format!("failed to write git config: {e}")))?;

    Repository::open(path)
}

/// Create a commit in `repo` with the given `files` added or updated.
///
/// Writes files to the worktree, stages them in the index, writes the resulting
/// tree to the object database, and advances the current branch or HEAD.
pub fn commit_files(
    repo: &Repository,
    message: &str,
    files: &[(&str, &str)],
) -> Result<gix::ObjectId, GitError> {
    let work_dir = repo
        .work_dir()
        .ok_or_else(|| GitError::OperationFailed("cannot commit in bare repository".into()))?;

    for (rel_path, content) in files {
        let abs_path = work_dir.join(rel_path);
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| GitError::OperationFailed(format!("failed to create dir: {e}")))?;
        }
        fs::write(&abs_path, content)
            .map_err(|e| GitError::OperationFailed(format!("failed to write {rel_path}: {e}")))?;
        stage_file(repo, rel_path).map_err(GitError::OperationFailed)?;
    }

    commit_index(repo, message)
}

/// Delete the specified files from `repo` and record a new commit.
pub fn delete_files(
    repo: &Repository,
    message: &str,
    paths: &[&str],
) -> Result<gix::ObjectId, GitError> {
    let work_dir = repo
        .work_dir()
        .ok_or_else(|| GitError::OperationFailed("cannot commit in bare repository".into()))?;

    for rel_path in paths {
        let abs_path = work_dir.join(rel_path);
        if abs_path.exists() {
            let _ = fs::remove_file(&abs_path);
        }
        stage_file(repo, rel_path).map_err(GitError::OperationFailed)?;
    }

    commit_index(repo, message)
}

/// Write the current index as a tree and create a commit on the current branch.
pub fn commit_index(repo: &Repository, message: &str) -> Result<gix::ObjectId, GitError> {
    let raw = repo.raw();
    let head_commit_opt = raw.head_commit().ok();
    let parent_ids: Vec<gix::hash::ObjectId> = match head_commit_opt {
        Some(ref head) => vec![head.id().detach()],
        None => Vec::new(),
    };
    commit_index_with_parents(repo, message, &parent_ids)
}

/// Write the current index as a tree and create a commit with specific parents.
pub fn commit_index_with_parents(
    repo: &Repository,
    message: &str,
    parent_ids: &[gix::ObjectId],
) -> Result<gix::ObjectId, GitError> {
    let raw = repo.raw();
    let mut editor = raw.edit_tree(gix::hash::ObjectId::empty_tree(raw.object_hash()))?;

    let index = crate::mutation::open_or_create_index(raw).map_err(GitError::OperationFailed)?;
    for entry in index.entries() {
        let path_str = entry
            .path(&index)
            .to_str()
            .map_err(|e| GitError::OperationFailed(format!("invalid utf-8 path in index: {e}")))?;
        let kind = index_mode_to_entry_kind(entry.mode);
        editor.upsert(path_str, kind, entry.id)?;
    }

    let tree_id = editor.write()?;

    let sig = gix::actor::Signature {
        name: "diffuse".into(),
        email: "diffuse@example.com".into(),
        time: gix::date::Time::now_utc(),
    };

    let commit = gix::objs::Commit {
        tree: tree_id.detach(),
        parents: parent_ids.to_vec().into(),
        author: sig.clone(),
        committer: sig,
        encoding: None,
        message: message.into(),
        extra_headers: Vec::new(),
    };

    let commit_id = raw.write(&commit)?;

    // Update branch or HEAD reference
    let head = raw.head()?;
    if let Some(branch_name) = head.referent_name() {
        raw.reference(
            branch_name,
            commit_id,
            PreviousValue::Any,
            format!("commit: {message}"),
        )?;
    } else {
        raw.reference(
            "HEAD",
            commit_id,
            PreviousValue::Any,
            format!("commit (detached): {message}"),
        )?;
    }

    Ok(commit_id)
}

/// Create a new branch pointing to `target` commit.
pub fn create_branch(
    repo: &Repository,
    branch_name: &str,
    target: gix::ObjectId,
) -> Result<(), GitError> {
    let full_name = format!("refs/heads/{branch_name}");
    repo.raw().reference(
        full_name,
        target,
        PreviousValue::Any,
        format!("branch: created {branch_name}"),
    )?;
    Ok(())
}

/// Create a lightweight tag pointing to `target` commit.
pub fn create_tag(
    repo: &Repository,
    tag_name: &str,
    target: gix::ObjectId,
) -> Result<(), GitError> {
    let full_name = format!("refs/tags/{tag_name}");
    repo.raw().reference(
        full_name,
        target,
        PreviousValue::Any,
        format!("tag: created {tag_name}"),
    )?;
    Ok(())
}

/// Create a commit with explicit tree entries (e.g. blobs,
/// submodules/gitlinks).
pub fn commit_tree_entries(
    repo: &Repository,
    message: &str,
    entries: &[(&str, gix::object::tree::EntryKind, gix::ObjectId)],
) -> Result<gix::ObjectId, GitError> {
    let raw = repo.raw();
    let head_commit_opt = raw.head_commit().ok();
    let parent_ids: Vec<gix::hash::ObjectId> = match head_commit_opt {
        Some(ref head) => vec![head.id().detach()],
        None => Vec::new(),
    };

    let head_tree_opt = head_commit_opt.as_ref().and_then(|h| h.tree().ok());
    let mut editor = match head_tree_opt {
        Some(ref t) => t.edit()?,
        None => raw.edit_tree(gix::hash::ObjectId::empty_tree(raw.object_hash()))?,
    };

    for (path, kind, id) in entries {
        editor.upsert(*path, *kind, *id)?;
    }

    let tree_id = editor.write()?;

    let sig = gix::actor::Signature {
        name: "diffuse".into(),
        email: "diffuse@example.com".into(),
        time: gix::date::Time::now_utc(),
    };

    let commit = gix::objs::Commit {
        tree: tree_id.detach(),
        parents: parent_ids.into(),
        author: sig.clone(),
        committer: sig,
        encoding: None,
        message: message.into(),
        extra_headers: Vec::new(),
    };

    let commit_id = raw.write(&commit)?;

    let head = raw.head()?;
    if let Some(branch_name) = head.referent_name() {
        raw.reference(
            branch_name,
            commit_id,
            PreviousValue::Any,
            format!("commit: {message}"),
        )?;
    } else {
        raw.reference(
            "HEAD",
            commit_id,
            PreviousValue::Any,
            format!("commit (detached): {message}"),
        )?;
    }

    Ok(commit_id)
}
