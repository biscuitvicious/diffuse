//! Interactive Git Actions for Diffuse TUI (stage, unstage, discard, reload).

use std::path::Path;

use diffuse_git::mutation;

use crate::core::{
    ENGINE_CONTEXT, PipelineOptions, git::load_git_repo_with_notes, pipeline::run_pipeline_full,
};

use super::app::AppState;

/// Parse old and new starting line numbers from a unified diff hunk header.
pub fn parse_hunk_header(line: &str) -> Option<(u32, u32)> {
    // @@ -old,count +new,count @@
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 || parts[0] != "@@" {
        return None;
    }
    let old_part = parts[1].strip_prefix('-')?;
    let old_start = old_part.split(',').next()?.parse::<u32>().ok()?;

    let new_part = parts[2].strip_prefix('+')?;
    let new_start = new_part.split(',').next()?.parse::<u32>().ok()?;

    Some((old_start, new_start))
}

/// Stage the current hunk or whole file (`s`/`S`).
pub fn stage_action(
    state: &mut AppState,
    repo_root: &Path,
    whole_file: bool,
) -> Result<(), String> {
    let f = match state.current_file() {
        Some(file) => file,
        None => return Err("no active file".into()),
    };
    let repo = diffuse_git::Repository::open(repo_root)
        .map_err(|e| format!("failed to open git repo: {e}"))?;

    if whole_file {
        mutation::stage_file(&repo, &f.path)
    } else {
        let hi = state
            .current_hunk()
            .ok_or_else(|| "cannot find active hunk for staging".to_string())?;
        let hunk = f
            .hunks
            .get(hi)
            .ok_or_else(|| "current hunk index out of bounds in f.hunks".to_string())?
            .reconstruct();
        mutation::stage_hunk(&repo, &f.path, &hunk)
    }
}

/// Unstage the current hunk or whole file (`u`/`U`).
pub fn unstage_action(
    state: &mut AppState,
    repo_root: &Path,
    whole_file: bool,
) -> Result<(), String> {
    let f = match state.current_file() {
        Some(file) => file,
        None => return Err("no active file".into()),
    };
    let repo = diffuse_git::Repository::open(repo_root)
        .map_err(|e| format!("failed to open git repo: {e}"))?;

    if whole_file {
        mutation::unstage_file(&repo, &f.path)
    } else {
        let hi = state
            .current_hunk()
            .ok_or_else(|| "cannot find active hunk for unstaging".to_string())?;
        let hunk = f
            .hunks
            .get(hi)
            .ok_or_else(|| "current hunk index out of bounds in f.hunks".to_string())?
            .reconstruct();
        mutation::unstage_hunk(&repo, &f.path, &hunk)
    }
}

/// Discard changes in the current hunk or whole file (`x`/`X`).
/// Creates a safety stash checkpoint before discarding.
pub fn discard_action(
    state: &mut AppState,
    repo_root: &Path,
    whole_file: bool,
) -> Result<Option<String>, String> {
    let f = match state.current_file() {
        Some(file) => file,
        None => return Err("no active file".into()),
    };
    let repo = diffuse_git::Repository::open(repo_root)
        .map_err(|e| format!("failed to open git repo: {e}"))?;

    if whole_file {
        mutation::discard_file(&repo, &f.path)
    } else {
        let hi = state
            .current_hunk()
            .ok_or_else(|| "cannot find active hunk for discard".to_string())?;
        let hunk = f
            .hunks
            .get(hi)
            .ok_or_else(|| "current hunk index out of bounds in f.hunks".to_string())?
            .reconstruct();
        mutation::discard_hunk(&repo, &f.path, &hunk)
    }
}

/// Reload the git repository diff and re-anchor state onto the active line.
pub fn reload_diff(
    state: &mut AppState,
    repo_root: &Path,
    git_args: &[String],
) -> Result<(), String> {
    let staged = state.target.is_staged();
    let notes_db = state.visible_notes_db();

    let git_res = load_git_repo_with_notes(
        repo_root,
        git_args,
        &[],
        staged,
        ENGINE_CONTEXT,
        Some(&notes_db),
        Some(&state.notes_key),
    )
    .map_err(|e| format!("failed to reload git repo: {e}"))?;

    let pipeline_opts = PipelineOptions::default()
        .with_notes(Some(state.notes.clone()), Some(state.notes_key.clone()))
        .with_expand_lines(state.expand_lines);
    let mut new_out = run_pipeline_full(
        &git_res.diff,
        Some(git_res.new_content),
        Some(git_res.old_content),
        pipeline_opts,
    );
    new_out.branch = git_res.branch;
    new_out.untracked_skipped = git_res.untracked_skipped;
    state.commit_range = git_res.commit_range;

    state.reload(new_out);
    Ok(())
}

/// Reload a specific subset of files in the git repository diff and re-anchor
/// state. Returns the number of files checked/reloaded.
pub fn reload_specific_files(
    state: &mut AppState,
    repo_root: &Path,
    git_args: &[String],
    paths: &[String],
) -> Result<usize, String> {
    if paths.is_empty() {
        return Ok(0);
    }
    let staged = state.target.is_staged();
    let notes_db = state.visible_notes_db();

    let git_res = load_git_repo_with_notes(
        repo_root,
        git_args,
        paths,
        staged,
        ENGINE_CONTEXT,
        Some(&notes_db),
        Some(&state.notes_key),
    )
    .map_err(|e| format!("failed to reload git repo: {e}"))?;

    let pipeline_opts = PipelineOptions::default()
        .with_notes(Some(state.notes.clone()), Some(state.notes_key.clone()))
        .with_expand_lines(state.expand_lines);
    let mut new_out = run_pipeline_full(
        &git_res.diff,
        Some(git_res.new_content),
        Some(git_res.old_content),
        pipeline_opts,
    );
    new_out.branch = git_res.branch;
    new_out.untracked_skipped = git_res.untracked_skipped;
    state.commit_range = git_res.commit_range;

    let checked_count = paths.len();
    state.reload_files(new_out.files, paths);
    Ok(checked_count)
}

/// Toggle between viewing staged changes and unstaged working tree changes.
/// Always switches the view, even when the target side has no changes (the
/// empty view then follows the filesystem watcher). Returns an error only when
/// viewing a commit range, or if the git repository cannot be loaded.
pub fn toggle_staged_action(
    state: &mut AppState,
    repo_root: &Path,
    git_args: &[String],
) -> Result<(), String> {
    if state.commit_range.is_some()
        || state.target.is_range()
        || diffuse_core::repo::RevisionRange::resolve(repo_root, git_args).is_some()
    {
        return Err("cannot switch between staged and unstaged when viewing a commit range".into());
    }

    let target_staged = !state.target.is_staged();

    let notes_db = state.visible_notes_db();

    let git_res = load_git_repo_with_notes(
        repo_root,
        git_args,
        &[],
        target_staged,
        ENGINE_CONTEXT,
        Some(&notes_db),
        Some(&state.notes_key),
    )
    .map_err(|e| format!("failed to reload git repo: {e}"))?;

    let pipeline_opts = PipelineOptions::default()
        .with_notes(Some(state.notes.clone()), Some(state.notes_key.clone()))
        .with_expand_lines(state.expand_lines);
    let mut new_out = run_pipeline_full(
        &git_res.diff,
        Some(git_res.new_content),
        Some(git_res.old_content),
        pipeline_opts,
    );
    new_out.branch = git_res.branch;
    new_out.untracked_skipped = git_res.untracked_skipped;

    state.target.staged = target_staged;
    state.commit_range = git_res.commit_range;
    state.reload(new_out);

    Ok(())
}

#[cfg(test)]
mod tests;
