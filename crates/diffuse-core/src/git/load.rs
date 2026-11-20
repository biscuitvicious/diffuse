use std::{collections::HashSet, fs, path::Path, sync::Arc};

use super::{
    GitFileItem, LoadGitResult, blob::load_git_blob, get_current_branch, is_inside_work_tree,
    resolve_refs, synthetic_unmodified_diff,
};

#[tracing::instrument(
    level = "trace",
    ret,
    err,
    skip(paths, notes_db),
    fields(
        repo_root = %repo_root.display(),
        base_args = ?base_args,
        path_count = paths.len(),
        staged,
        context
    )
)]
#[allow(clippy::too_many_arguments)]
pub fn load_git_repo_with_notes(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    staged: bool,
    context: usize,
    notes_db: Option<&crate::notes::NotesDB>,
    repo_key: Option<&str>,
    rev_key: Option<&str>,
) -> Result<LoadGitResult, String> {
    if !is_inside_work_tree(repo_root) {
        return Err(format!(
            "not a git repository (or any of the parent directories): {}",
            repo_root.display()
        ));
    }

    let branch = get_current_branch(repo_root);

    let refs = resolve_refs(repo_root, base_args, staged);
    let git_repo = diffuse_git::Repository::open(repo_root)
        .or_else(|_| diffuse_git::Repository::discover(repo_root))
        .map_err(|e| format!("cannot open repository: {e}"))?;

    let target = diffuse_git::ReviewTarget::new(
        diffuse_git::Repo::from_root(repo_root),
        refs.revision_range(),
        base_args.to_vec(),
        paths.to_vec(),
        staged,
    );
    let repo_diff = diffuse_git::diff_repository(
        &git_repo,
        &target,
        context as u32,
        diffuse_git::WhitespaceMode::Exact,
    )
    .map_err(|e| format!("diff repository failed: {e}"))?;

    let mut diff = repo_diff.diff;
    let mut files: Vec<GitFileItem> = Vec::with_capacity(repo_diff.files.len());
    for f in &repo_diff.files {
        let status_str = match f.status {
            diffuse_parse::FileStatus::Added => "added",
            diffuse_parse::FileStatus::Deleted => "deleted",
            diffuse_parse::FileStatus::Renamed => "renamed",
            diffuse_parse::FileStatus::Untracked => "untracked",
            diffuse_parse::FileStatus::ModeChanged => "mode_changed",
            diffuse_parse::FileStatus::Unmodified => "unmodified",
            _ => "modified",
        };
        files.push(GitFileItem {
            path: f.path.clone(),
            status: status_str.to_string(),
            old_path: f.old_path.clone(),
            new_content: repo_diff.new_content.get(&f.path).cloned(),
            old_content: repo_diff
                .old_content
                .get(f.old_path.as_deref().unwrap_or(&f.path))
                .cloned(),
            no_content: false,
        });
    }

    let mut seen_paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    let mut new_content = repo_diff.new_content;
    let mut old_content = repo_diff.old_content;

    // Unmodified files carrying review notes.
    if let Some(db) = notes_db {
        let effective_repo_key = repo_key.or_else(|| target.repo_key());
        let effective_rev_key = rev_key.or_else(|| target.rev_key());
        let note_files =
            crate::notes::note_files_for_repo(db, effective_repo_key, effective_rev_key);
        for file_path in note_files {
            if seen_paths.contains(&file_path) {
                continue;
            }
            if !paths.is_empty()
                && !paths.iter().any(|p| {
                    let clean = p.strip_suffix('/').unwrap_or(p);
                    file_path == clean || file_path.starts_with(&format!("{clean}/"))
                })
            {
                continue;
            }

            let content = if refs.worktree_new {
                let abs = repo_root.join(&file_path);
                fs::read_to_string(&abs).ok()
            } else if let Some(ref new_ref) = refs.new_ref {
                load_git_blob(repo_root, new_ref, &file_path).0
            } else {
                let old_tree_ref = refs.old_ref.as_deref().unwrap_or("HEAD");
                load_git_blob(repo_root, old_tree_ref, &file_path).0
            };

            if let Some(c) = content {
                let file_lines = crate::pipeline::split_content_lines(&c);
                let file_notes =
                    crate::notes::query_notes_for_file(db, repo_key, rev_key, &file_path);
                if file_notes.is_empty() {
                    continue;
                }
                let has_anchor = file_notes
                    .iter()
                    .any(|n| crate::notes::note_anchors_in_context_lines(n, &file_lines));
                if !diff.is_empty() && !diff.ends_with('\n') {
                    diff.push('\n');
                }
                diff.push_str(&synthetic_unmodified_diff(&file_path));
                if has_anchor {
                    new_content.insert(file_path.clone(), c.clone());
                    old_content.insert(file_path.clone(), c);
                }
                seen_paths.insert(file_path);
            }
        }
    }

    let commit_range = git_repo.resolve_commit_range(base_args);
    Ok(LoadGitResult {
        diff,
        new_content,
        old_content,
        branch,
        untracked_skipped: repo_diff.untracked_skipped,
        commit_range,
    })
}

#[inline]
pub fn load_git_repo(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    staged: bool,
    context: usize,
) -> Result<LoadGitResult, String> {
    load_git_repo_with_notes(
        repo_root, base_args, paths, staged, context, None, None, None,
    )
}

/// Load and process the diff for a single file on demand from a git repository.
#[tracing::instrument(
    level = "debug",
    skip(repo_root, base_args, opts),
    fields(path, status)
)]
pub fn load_single_file_diff(
    repo_root: &Path,
    base_args: &[String],
    path: &str,
    old_path: Option<&str>,
    status: &str,
    context: usize,
    opts: &crate::pipeline::PipelineOptions,
) -> Option<crate::wire::OutputFile> {
    if status == "untracked" {
        let abs = repo_root.join(path);
        let content = fs::read_to_string(&abs).ok()?;
        let line_count = content.lines().count();
        let mut synthetic_diff = format!(
            "diff --git a/{path} b/{path}\nuntracked file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{line_count} @@\n"
        );
        for line in content.lines() {
            synthetic_diff.push('+');
            synthetic_diff.push_str(line);
            synthetic_diff.push('\n');
        }
        let mut parsed = crate::parse::parse_diff(&synthetic_diff)
            .expect("synthetic untracked diff is hunk-count exact by construction");
        let mut f = parsed.pop()?;
        f.status = crate::parse::FileStatus::Untracked;
        f.new_bytes = Some(content.len());
        let mut rendered = crate::pipeline::process_single_diff_file(
            f,
            Some(&content),
            None,
            Some(Arc::new(synthetic_diff)),
            opts,
        );
        rendered.raw_source = Some(crate::wire::RawFileSource::Git {
            path: path.to_string(),
            old_path: old_path.map(String::from),
            status: status.to_string(),
        });
        return Some(rendered);
    }

    let refs = resolve_refs(repo_root, base_args, false);
    let repo = diffuse_git::Repository::open(repo_root)
        .or_else(|_| diffuse_git::Repository::discover(repo_root))
        .ok()?;

    let (old_ref, new_ref, worktree_new) = if let Some(ref r) = refs.revision_range() {
        (Some(r.0.clone()), Some(r.1.clone()), false)
    } else {
        (None, None, true)
    };
    let old_lookup = old_path.unwrap_or(path);
    let old_bytes = if status == "added" || status == "untracked" {
        None
    } else if let Some(ref r) = old_ref {
        repo.load_blob(r, old_lookup).map(|b| b.data)
    } else {
        let old_tree_ref = refs.old_ref.as_deref().unwrap_or("HEAD");
        repo.load_blob(old_tree_ref, old_lookup).map(|b| b.data)
    };
    let new_bytes = if status == "deleted" {
        None
    } else if worktree_new {
        if let Ok(b) = fs::read(repo_root.join(path)) {
            Some(b)
        } else if let Ok(sub) = gix::open(repo_root.join(path))
            && let Ok(head) = sub.head_id()
        {
            Some(format!("Subproject commit {head}\n").into_bytes())
        } else {
            None
        }
    } else if let Some(ref r) = new_ref {
        repo.load_blob(r, path).map(|b| b.data)
    } else {
        fs::read(repo_root.join(path)).ok()
    };
    let active_bytes = new_bytes.as_deref().or(old_bytes.as_deref()).unwrap_or(b"");
    let is_binary = repo.is_path_binary(path, active_bytes)
        || old_bytes.as_ref().is_some_and(|b| b.contains(&0))
        || new_bytes.as_ref().is_some_and(|b| b.contains(&0));

    let st = match status {
        "renamed" => crate::parse::FileStatus::Renamed,
        "mode changed" => crate::parse::FileStatus::ModeChanged,
        "deleted" => crate::parse::FileStatus::Deleted,
        "added" => crate::parse::FileStatus::Added,
        "untracked" => crate::parse::FileStatus::Untracked,
        "unmodified" => crate::parse::FileStatus::Unmodified,
        _ => crate::parse::FileStatus::Modified,
    };

    let res = diffuse_git::generate_file_diff(diffuse_git::FileDiffParams {
        path,
        old_path,
        old_bytes: old_bytes.as_deref(),
        new_bytes: new_bytes.as_deref(),
        status: st,
        old_mode: None,
        new_mode: None,
        is_binary,
        context: context as u32,
        mode: diffuse_git::WhitespaceMode::Exact,
    });

    let new_content_str = new_bytes.as_ref().and_then(|b| std::str::from_utf8(b).ok());
    let old_content_str = old_bytes.as_ref().and_then(|b| std::str::from_utf8(b).ok());

    let mut rendered = crate::pipeline::process_single_diff_file(
        res.file,
        new_content_str,
        old_content_str,
        Some(Arc::new(res.patch)),
        opts,
    );
    rendered.raw_source = Some(crate::wire::RawFileSource::Git {
        path: path.to_string(),
        old_path: old_path.map(String::from),
        status: status.to_string(),
    });
    Some(rendered)
}
