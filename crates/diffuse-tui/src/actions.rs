//! Interactive Git Actions for Diffuse TUI (stage, unstage, discard, reload).

use std::{io::Write, path::Path, process::Command};

use crate::core::{
    ENGINE_CONTEXT, PipelineOptions, git::load_git_repo, pipeline::run_pipeline_full,
};

use super::app::AppState;

/// Build a unified patch for the current hunk or untracked file.
pub fn hunk_patch(state: &AppState, repo_root: &Path, base_args: &[String]) -> Option<String> {
    let f = state.current_file()?;
    let hi = state.current_hunk()?;
    let h = f
        .hunks
        .get(hi)
        .expect("current hunk index out of bounds in f.hunks");

    if f.status == "untracked" {
        let body: String = h
            .rows
            .iter()
            .map(|r| format!("+{}", r.text()))
            .collect::<Vec<_>>()
            .join("\n");
        return Some(format!(
            "diff --git a/{path} b/{path}\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{count} @@\n{body}\n",
            path = f.path,
            count = h.new_count
        ));
    }

    let mut cmd = Command::new("git");
    cmd.arg("diff")
        .arg("-M")
        .arg("--no-color")
        .arg(format!("-U{}", ENGINE_CONTEXT));
    if state.target.is_staged() {
        cmd.arg("--staged");
    }
    cmd.args(base_args)
        .arg("--")
        .arg(&f.path)
        .current_dir(repo_root);

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.split('\n').collect();

    let mut start_idx = None;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("@@ ") {
            // Parse @@ -old,count +new,count @@
            if let Some(caps) = parse_hunk_header(line)
                && caps.0 == h.old_start
                && caps.1 == h.new_start
            {
                start_idx = Some(i);
                break;
            }
        }
    }

    let start = start_idx?;
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.starts_with("@@ ") {
            end = i;
            break;
        }
    }

    let block = lines[start..end].join("\n");
    let a_path = format!("a/{}", f.path);
    let b_path = format!("b/{}", f.path);
    let old_header = if f.status == "added" {
        "--- /dev/null".to_string()
    } else {
        format!("--- a/{}", f.path)
    };
    let new_header = if f.status == "deleted" {
        "+++ /dev/null".to_string()
    } else {
        format!("+++ b/{}", f.path)
    };

    Some(format!(
        "diff --git {a_path} {b_path}\n{old_header}\n{new_header}\n{block}\n"
    ))
}

fn parse_hunk_header(line: &str) -> Option<(u32, u32)> {
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

/// Apply a patch string via `git apply`.
pub fn apply_patch(repo_root: &Path, patch: &str, flags: &[&str]) -> Result<(), String> {
    let mut tmp = tempfile::Builder::new()
        .prefix("diffuse-patch-")
        .suffix(".patch")
        .tempfile()
        .map_err(|e| format!("failed to create tmp patch: {e}"))?;

    tmp.write_all(patch.as_bytes())
        .map_err(|e| format!("failed to write tmp patch: {e}"))?;
    tmp.flush()
        .map_err(|e| format!("failed to flush tmp patch: {e}"))?;

    let check = Command::new("git")
        .arg("apply")
        .arg("--check")
        .args(flags)
        .arg(tmp.path())
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("git apply --check failed: {e}"))?;

    if !check.status.success() {
        let err = String::from_utf8_lossy(&check.stderr);
        return Err(format!("patch does not apply: {}", err.trim()));
    }

    let res = Command::new("git")
        .arg("apply")
        .args(flags)
        .arg(tmp.path())
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("git apply failed: {e}"))?;

    if !res.status.success() {
        let err = String::from_utf8_lossy(&res.stderr);
        return Err(format!("git apply error: {}", err.trim()));
    }

    Ok(())
}

/// Stage the current hunk or whole file (`s`/`S`).
pub fn stage_action(
    state: &mut AppState,
    repo_root: &Path,
    base_args: &[String],
    whole_file: bool,
) -> Result<(), String> {
    let f = match state.current_file() {
        Some(file) => file,
        None => return Err("no active file".into()),
    };

    if whole_file {
        let res = Command::new("git")
            .arg("add")
            .arg("--")
            .arg(&f.path)
            .current_dir(repo_root)
            .output()
            .map_err(|e| format!("git add failed: {e}"))?;

        if !res.status.success() {
            let err = String::from_utf8_lossy(&res.stderr);
            return Err(format!("git add error: {}", err.trim()));
        }
    } else {
        let patch = hunk_patch(state, repo_root, base_args)
            .ok_or_else(|| "cannot build hunk patch for staging".to_string())?;
        apply_patch(repo_root, &patch, &["--cached"])?;
    }

    Ok(())
}

/// Unstage the current hunk or whole file (`u`/`U`).
pub fn unstage_action(
    state: &mut AppState,
    repo_root: &Path,
    base_args: &[String],
    whole_file: bool,
) -> Result<(), String> {
    let f = match state.current_file() {
        Some(file) => file,
        None => return Err("no active file".into()),
    };

    if whole_file {
        let res = Command::new("git")
            .arg("reset")
            .arg("-q")
            .arg("--")
            .arg(&f.path)
            .current_dir(repo_root)
            .output()
            .map_err(|e| format!("git reset failed: {e}"))?;

        if !res.status.success() {
            let err = String::from_utf8_lossy(&res.stderr);
            return Err(format!("git reset error: {}", err.trim()));
        }
    } else {
        let patch = hunk_patch(state, repo_root, base_args)
            .ok_or_else(|| "cannot build hunk patch for unstaging".to_string())?;
        apply_patch(repo_root, &patch, &["--cached", "-R"])?;
    }

    Ok(())
}

/// Discard changes in the current hunk or whole file (`x`/`X`).
/// Creates a safety stash checkpoint before discarding.
pub fn discard_action(
    state: &mut AppState,
    repo_root: &Path,
    base_args: &[String],
    whole_file: bool,
) -> Result<Option<String>, String> {
    let f = match state.current_file() {
        Some(file) => file,
        None => return Err("no active file".into()),
    };

    // Safety stash checkpoint
    let stash_ref = create_stash_checkpoint(repo_root);

    if whole_file {
        let res = Command::new("git")
            .arg("checkout")
            .arg("--")
            .arg(&f.path)
            .current_dir(repo_root)
            .output()
            .map_err(|e| format!("git checkout failed: {e}"))?;

        if !res.status.success() {
            let err = String::from_utf8_lossy(&res.stderr);
            return Err(format!("git checkout error: {}", err.trim()));
        }
    } else {
        let patch = hunk_patch(state, repo_root, base_args)
            .ok_or_else(|| "cannot build hunk patch for discard".to_string())?;
        apply_patch(repo_root, &patch, &["-R"])?;
    }

    Ok(stash_ref)
}

fn create_stash_checkpoint(repo_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("stash")
        .arg("create")
        .arg("review-checkpoint")
        .current_dir(repo_root)
        .output()
        .ok()?;

    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    None
}

/// Reload the git repository diff and re-anchor state onto the active line.
pub fn reload_diff(
    state: &mut AppState,
    repo_root: &Path,
    git_args: &[String],
) -> Result<(), String> {
    let staged = state.target.is_staged();
    let git_res = load_git_repo(repo_root, git_args, &[], staged, ENGINE_CONTEXT)
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

/// Toggle between viewing staged changes and unstaged working tree changes.
/// Returns an error if viewing a commit range, or if the target view has no
/// changes.
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

    let git_res = load_git_repo(repo_root, git_args, &[], target_staged, ENGINE_CONTEXT)
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

    if new_out.files.is_empty() {
        return Err(if target_staged {
            "no staged changes".into()
        } else {
            "no unstaged changes".into()
        });
    }

    state.target.staged = target_staged;
    state.commit_range = git_res.commit_range;
    state.reload(new_out);

    Ok(())
}

#[cfg(test)]
mod tests;
