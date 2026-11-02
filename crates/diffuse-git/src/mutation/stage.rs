use std::{fs, path::Path};

use diffuse_parse::Hunk;
use gix::bstr::BStr;

use super::{
    apply::apply_hunk_to_buffer,
    checkpoint::{entry_kind_to_index_mode, file_stat, open_or_create_index},
};
use crate::Repository;

/// Stage a whole file from the working tree into the Git index (`git add --
/// <path>`).
pub fn stage_file(repo: &Repository, file_path: &str) -> Result<(), String> {
    let work_dir = repo
        .work_dir()
        .ok_or_else(|| "cannot stage in bare repository".to_string())?;
    let full_path = work_dir.join(file_path);
    let bpath = BStr::new(file_path.as_bytes());

    let mut index = open_or_create_index(repo.raw())?;

    if full_path.exists() {
        let bytes =
            fs::read(&full_path).map_err(|e| format!("failed to read file '{file_path}': {e}"))?;
        let blob_id = repo
            .raw()
            .write_blob(&bytes)
            .map_err(|e| format!("failed to write blob: {e}"))?;

        let metadata = fs::symlink_metadata(&full_path).ok();
        #[cfg(unix)]
        let mode = if let Some(ref m) = metadata {
            use std::os::unix::fs::PermissionsExt;
            if m.permissions().mode() & 0o111 != 0 {
                gix::index::entry::Mode::FILE_EXECUTABLE
            } else {
                gix::index::entry::Mode::FILE
            }
        } else {
            gix::index::entry::Mode::FILE
        };
        #[cfg(not(unix))]
        let mode = gix::index::entry::Mode::FILE;

        let stat = file_stat(&full_path);

        if let Ok(entry_idx) = index.entry_index_by_path(bpath) {
            let entry = &mut index.entries_mut()[entry_idx];
            entry.id = blob_id.detach();
            entry.mode = mode;
            entry.stat = stat;
        } else {
            index.dangerously_push_entry(
                stat,
                blob_id.detach(),
                gix::index::entry::Flags::empty(),
                mode,
                bpath,
            );
            index.sort_entries();
        }
    } else if let Ok(entry_idx) = index.entry_index_by_path(bpath) {
        index.remove_entry_at_index(entry_idx);
    }

    index.remove_tree();
    index
        .write(Default::default())
        .map_err(|e| format!("failed to write git index: {e}"))?;

    Ok(())
}

/// Unstage a whole file from the Git index back to HEAD (`git reset -q --
/// <path>`).
pub fn unstage_file(repo: &Repository, file_path: &str) -> Result<(), String> {
    let bpath = BStr::new(file_path.as_bytes());

    let mut index = open_or_create_index(repo.raw())?;

    let head_entry = {
        let head_commit = repo.raw().head_commit().ok();
        let head_tree = head_commit.as_ref().and_then(|c| c.tree().ok());
        head_tree.as_ref().and_then(|t| {
            t.lookup_entry_by_path(Path::new(file_path))
                .ok()
                .flatten()
                .map(|e| {
                    let mode = entry_kind_to_index_mode(e.mode().into());
                    let id = e.id().detach();
                    (mode, id)
                })
        })
    };

    if let Some((mode, id)) = head_entry {
        if let Ok(entry_idx) = index.entry_index_by_path(bpath) {
            let entry = &mut index.entries_mut()[entry_idx];
            entry.id = id;
            entry.mode = mode;
            entry.stat = gix::index::entry::Stat::default();
        } else {
            index.dangerously_push_entry(
                gix::index::entry::Stat::default(),
                id,
                gix::index::entry::Flags::empty(),
                mode,
                bpath,
            );
            index.sort_entries();
        }
    } else if let Ok(entry_idx) = index.entry_index_by_path(bpath) {
        index.remove_entry_at_index(entry_idx);
    }

    index.remove_tree();
    index
        .write(Default::default())
        .map_err(|e| format!("failed to write git index: {e}"))?;

    Ok(())
}

/// Stage a single hunk from unstaged changes into the Git index.
pub fn stage_hunk(repo: &Repository, file_path: &str, hunk: &Hunk) -> Result<(), String> {
    let base_bytes = repo
        .load_index_blob(file_path)
        .map(|b| b.data)
        .unwrap_or_default();

    let new_bytes = apply_hunk_to_buffer(&base_bytes, hunk, false)?;

    let blob_id = repo
        .raw()
        .write_blob(&new_bytes)
        .map_err(|e| format!("failed to write blob: {e}"))?;

    let bpath = BStr::new(file_path.as_bytes());
    let mut index = open_or_create_index(repo.raw())?;

    let work_dir = repo.work_dir();
    let full_path = work_dir.map(|w| w.join(file_path));
    let metadata = full_path
        .as_ref()
        .and_then(|p| fs::symlink_metadata(p).ok());
    let stat = full_path.as_deref().map(file_stat).unwrap_or_default();

    if let Ok(entry_idx) = index.entry_index_by_path(bpath) {
        let entry = &mut index.entries_mut()[entry_idx];
        entry.id = blob_id.detach();
        entry.stat = stat;
    } else {
        #[cfg(unix)]
        let mode = if let Some(ref m) = metadata {
            use std::os::unix::fs::PermissionsExt;
            if m.permissions().mode() & 0o111 != 0 {
                gix::index::entry::Mode::FILE_EXECUTABLE
            } else {
                gix::index::entry::Mode::FILE
            }
        } else {
            gix::index::entry::Mode::FILE
        };
        #[cfg(not(unix))]
        let mode = gix::index::entry::Mode::FILE;

        index.dangerously_push_entry(
            stat,
            blob_id.detach(),
            gix::index::entry::Flags::empty(),
            mode,
            bpath,
        );
        index.sort_entries();
    }

    index.remove_tree();
    index
        .write(Default::default())
        .map_err(|e| format!("failed to write git index: {e}"))?;

    Ok(())
}

/// Unstage a single hunk from the Git index back to HEAD.
pub fn unstage_hunk(repo: &Repository, file_path: &str, hunk: &Hunk) -> Result<(), String> {
    let index_bytes = repo
        .load_index_blob(file_path)
        .map(|b| b.data)
        .ok_or_else(|| format!("file '{file_path}' is not in index"))?;

    let reverted_bytes = apply_hunk_to_buffer(&index_bytes, hunk, true)?;

    let head_blob = {
        let head_commit = repo.raw().head_commit().ok();
        let head_tree = head_commit.as_ref().and_then(|c| c.tree().ok());
        head_tree.as_ref().and_then(|t| {
            t.lookup_entry_by_path(Path::new(file_path))
                .ok()
                .flatten()
                .and_then(|e| {
                    let id = e.id().detach();
                    repo.raw().find_blob(id).ok().map(|b| (id, b.data.to_vec()))
                })
        })
    };

    let bpath = BStr::new(file_path.as_bytes());
    let mut index = open_or_create_index(repo.raw())?;

    let entry_idx = index
        .entry_index_by_path(bpath)
        .map_err(|_| format!("entry '{file_path}' not found in index"))?;

    if let Some((head_id, ref head_data)) = head_blob {
        if head_data == &reverted_bytes {
            let entry = &mut index.entries_mut()[entry_idx];
            entry.id = head_id;
        } else {
            let blob_id = repo
                .raw()
                .write_blob(&reverted_bytes)
                .map_err(|e| format!("failed to write blob: {e}"))?;
            let entry = &mut index.entries_mut()[entry_idx];
            entry.id = blob_id.detach();
        }
    } else if reverted_bytes.is_empty() {
        index.remove_entry_at_index(entry_idx);
    } else {
        let blob_id = repo
            .raw()
            .write_blob(&reverted_bytes)
            .map_err(|e| format!("failed to write blob: {e}"))?;
        let entry = &mut index.entries_mut()[entry_idx];
        entry.id = blob_id.detach();
    }

    index.remove_tree();
    index
        .write(Default::default())
        .map_err(|e| format!("failed to write git index: {e}"))?;

    Ok(())
}
