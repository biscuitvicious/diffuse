use std::{collections::HashSet, fs, path::Path, sync::Arc};

use super::{
    blob::load_git_blob, get_current_branch, is_inside_work_tree, resolve_refs,
    synthetic_unmodified_diff,
};

///// Progressive streaming producer for git diff and blob retrieval.
/// Directly emits `FileJob`s to `StreamingProducer` as each file's diff chunk
/// is encountered.
#[expect(clippy::too_many_arguments)]
#[tracing::instrument(
    level = "trace",
    skip(paths, producer, meta_tx, notes_db),
    fields(
        repo_root = %repo_root.display(),
        base_args = ?base_args,
        path_count = paths.len(),
        staged,
        context
    )
)]
pub fn stream_git_repo_to_producer(
    repo_root: &Path,
    base_args: &[String],
    paths: &[String],
    staged: bool,
    context: usize,
    producer: crate::streaming::StreamingProducer,
    meta_tx: &std::sync::mpsc::Sender<Result<crate::streaming::GitRepoMeta, String>>,
    notes_db: Option<&crate::notes::NotesDB>,
    repo_key: Option<&str>,
    rev_key: Option<&str>,
) {
    if !is_inside_work_tree(repo_root) {
        let _ = meta_tx.send(Err(format!(
            "not a git repository (or any of the parent directories): {}",
            repo_root.display()
        )));
        producer.finish();
        return;
    }

    let branch = get_current_branch(repo_root);
    tracing::info!(branch = %branch, "Streaming git repository diff");

    let mut seen_paths: HashSet<String> = HashSet::new();
    let refs = resolve_refs(repo_root, base_args, staged);

    let git_repo = match diffuse_git::Repository::open(repo_root)
        .or_else(|_| diffuse_git::Repository::discover(repo_root))
    {
        Ok(r) => r,
        Err(e) => {
            let _ = meta_tx.send(Err(format!("cannot open repository: {e}")));
            producer.finish();
            return;
        }
    };

    let target = diffuse_git::ReviewTarget::new(
        diffuse_git::Repo::from_root(repo_root),
        refs.revision_range(),
        base_args.to_vec(),
        paths.to_vec(),
        staged,
    );
    let repo_diff = match diffuse_git::diff_repository(
        &git_repo,
        &target,
        context as u32,
        diffuse_git::WhitespaceMode::Exact,
    ) {
        Ok(rd) => rd,
        Err(e) => {
            let _ = meta_tx.send(Err(format!("diff repository failed: {e}")));
            producer.finish();
            return;
        }
    };

    let mut file_idx = 0;
    for f in repo_diff.files {
        seen_paths.insert(f.path.clone());
        let new_c = repo_diff.new_content.get(&f.path).cloned();
        let old_c = repo_diff
            .old_content
            .get(f.old_path.as_deref().unwrap_or(&f.path))
            .cloned();
        producer.push(crate::streaming::FileJob {
            idx: file_idx,
            file: f,
            new_content: new_c.map(Into::into),
            old_content: old_c.map(Into::into),
            diff_fallback: None,
        });
        file_idx += 1;
    }

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

                if !has_anchor {
                    // No anchoring notes: show the file header and any
                    // unanchored notes, but do not load the file contents as
                    // context.
                    let mut f = crate::parse::DiffFile::new(&file_path);
                    f.status = crate::parse::FileStatus::Unmodified;
                    f.has_file_headers = true;
                    producer.push(crate::streaming::FileJob {
                        idx: file_idx,
                        file: f,
                        new_content: None,
                        old_content: None,
                        diff_fallback: Some(Arc::new(synthetic_unmodified_diff(&file_path))),
                    });
                    seen_paths.insert(file_path);
                    file_idx += 1;
                    continue;
                }

                let mut f = crate::parse::DiffFile::new(&file_path);
                f.status = crate::parse::FileStatus::Unmodified;
                f.has_file_headers = true;
                f.new_bytes = Some(c.len());
                f.old_bytes = Some(c.len());
                let synthetic_diff = synthetic_unmodified_diff(&file_path);
                let arc_c = Arc::new(c);
                producer.push(crate::streaming::FileJob {
                    idx: file_idx,
                    file: f,
                    new_content: Some(Arc::clone(&arc_c)),
                    old_content: Some(arc_c),
                    diff_fallback: Some(Arc::new(synthetic_diff)),
                });
                seen_paths.insert(file_path);
                file_idx += 1;
            }
        }
    }

    let commit_range = git_repo.resolve_commit_range(base_args);
    let _ = meta_tx.send(Ok(crate::streaming::GitRepoMeta {
        branch,
        untracked_skipped: repo_diff.untracked_skipped,
        commit_range,
    }));
    producer.finish();
}
