use std::fs;

use diffuse_parse::Hunk;
use gix::bstr::BStr;

use super::{
    apply::apply_hunk_to_buffer,
    checkpoint::{create_stash_checkpoint, open_or_create_index},
};
use crate::Repository;

/// Discard changes in a whole file by restoring it from the index (`git
/// checkout -- <path>`). Creates a safety stash checkpoint before discarding.
pub fn discard_file(repo: &Repository, file_path: &str) -> Result<Option<String>, String> {
    let stash = create_stash_checkpoint(repo);

    let work_dir = repo
        .work_dir()
        .ok_or_else(|| "cannot discard in bare repository".to_string())?;
    let full_path = work_dir.join(file_path);
    let bpath = BStr::new(file_path.as_bytes());

    let index = open_or_create_index(repo.raw())?;

    if let Ok(entry_idx) = index.entry_index_by_path(bpath) {
        let entry = &index.entries()[entry_idx];
        let blob = repo
            .raw()
            .find_blob(entry.id)
            .map_err(|e| format!("failed to find blob in repo: {e}"))?;
        if let Some(parent) = full_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        fs::write(&full_path, &blob.data)
            .map_err(|e| format!("failed to write file '{file_path}': {e}"))?;

        #[cfg(unix)]
        if entry.mode == gix::index::entry::Mode::FILE_EXECUTABLE {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&full_path, fs::Permissions::from_mode(0o755));
        }
    } else if full_path.exists() {
        let _ = fs::remove_file(&full_path);
    }

    Ok(stash)
}

/// Discard a single hunk from the working tree.
/// Creates a safety stash checkpoint before discarding.
pub fn discard_hunk(
    repo: &Repository,
    file_path: &str,
    hunk: &Hunk,
) -> Result<Option<String>, String> {
    let stash = create_stash_checkpoint(repo);

    let work_dir = repo
        .work_dir()
        .ok_or_else(|| "cannot discard in bare repository".to_string())?;
    let full_path = work_dir.join(file_path);

    let worktree_bytes =
        fs::read(&full_path).map_err(|e| format!("failed to read file '{file_path}': {e}"))?;

    let reverted_bytes = apply_hunk_to_buffer(&worktree_bytes, hunk, true)?;

    fs::write(&full_path, &reverted_bytes)
        .map_err(|e| format!("failed to write file '{file_path}': {e}"))?;

    Ok(stash)
}
