//! In-process Git index mutation, hunk staging/unstaging/discard, and safety
//! stash checkpoints.

use std::{fs, path::Path};

use diffuse_parse::Hunk;
use gix::{
    bstr::{BStr, ByteSlice},
    objs::Commit,
    prelude::Write,
};

use crate::{Repository, diff::split_lines};

/// Canonical default Git actor signature for diffuse.
pub fn default_signature() -> gix::actor::Signature {
    gix::actor::Signature {
        name: "diffuse".into(),
        email: "diffuse@local".into(),
        time: gix::date::Time::now_local_or_utc(),
    }
}

/// Convert a Git index entry mode to a tree entry kind.
pub(crate) fn index_mode_to_entry_kind(
    mode: gix::index::entry::Mode,
) -> gix::object::tree::EntryKind {
    use gix::{index::entry::Mode, object::tree::EntryKind};
    if mode == Mode::FILE_EXECUTABLE {
        EntryKind::BlobExecutable
    } else if mode == Mode::SYMLINK {
        EntryKind::Link
    } else if mode == Mode::COMMIT {
        EntryKind::Commit
    } else if mode == Mode::DIR {
        EntryKind::Tree
    } else {
        EntryKind::Blob
    }
}

/// Convert a tree entry kind to an index entry mode.
fn entry_kind_to_index_mode(kind: gix::object::tree::EntryKind) -> gix::index::entry::Mode {
    use gix::{index::entry::Mode, object::tree::EntryKind};
    match kind {
        EntryKind::BlobExecutable => Mode::FILE_EXECUTABLE,
        EntryKind::Link => Mode::SYMLINK,
        EntryKind::Commit => Mode::COMMIT,
        EntryKind::Tree => Mode::DIR,
        EntryKind::Blob => Mode::FILE,
    }
}

/// Compute index entry Stat from a filesystem path.
fn file_stat(path: &Path) -> gix::index::entry::Stat {
    gix::index::fs::Metadata::from_path_no_follow(path)
        .ok()
        .and_then(|m| gix::index::entry::Stat::from_fs(&m).ok())
        .unwrap_or_default()
}

/// Apply a hunk to an in-memory buffer, either forward (`reverse = false`)
/// or in reverse (`reverse = true`).
pub fn apply_hunk_to_buffer(buffer: &[u8], hunk: &Hunk, reverse: bool) -> Result<Vec<u8>, String> {
    let buffer_lines = split_lines(buffer);

    // Detect line endings from the original buffer (default to "\n").
    let eol = if buffer.windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let (nominal_start, expected_texts, replacement_lines) = if !reverse {
        let nominal = hunk.old_start.saturating_sub(1) as usize;
        let mut exp = Vec::new();
        let mut rep = Vec::new();
        for row in &hunk.rows {
            if let Some(t) = row.old_text() {
                exp.push(t);
            }
            if let Some(t) = row.new_text() {
                rep.push(format!("{}{eol}", t));
            }
        }
        (nominal, exp, rep)
    } else {
        let nominal = hunk.new_start.saturating_sub(1) as usize;
        let mut exp = Vec::new();
        let mut rep = Vec::new();
        for row in &hunk.rows {
            if let Some(t) = row.new_text() {
                exp.push(t);
            }
            if let Some(t) = row.old_text() {
                rep.push(format!("{}{eol}", t));
            }
        }
        (nominal, exp, rep)
    };

    // If buffer is empty and we expect nothing, replacement lines form the entire
    // output.
    if buffer_lines.is_empty() && expected_texts.is_empty() {
        let mut out = Vec::new();
        for r in &replacement_lines {
            out.extend_from_slice(r.as_bytes());
        }
        return Ok(out);
    }

    let matched_start = find_hunk_match(&buffer_lines, nominal_start, &expected_texts)
        .ok_or_else(|| "hunk does not apply: context mismatch".to_string())?;

    let mut new_bytes = Vec::new();
    for &l in &buffer_lines[..matched_start] {
        new_bytes.extend_from_slice(l);
    }
    for r in &replacement_lines {
        new_bytes.extend_from_slice(r.as_bytes());
    }
    for &l in &buffer_lines[matched_start + expected_texts.len()..] {
        new_bytes.extend_from_slice(l);
    }

    Ok(new_bytes)
}

fn find_hunk_match(
    buffer_lines: &[&[u8]],
    nominal_start: usize,
    expected: &[&str],
) -> Option<usize> {
    if expected.is_empty() {
        return Some(nominal_start.min(buffer_lines.len()));
    }

    let matches_at = |start: usize| -> bool {
        if start + expected.len() > buffer_lines.len() {
            return false;
        }
        for (i, &exp) in expected.iter().enumerate() {
            let buf_line = buffer_lines[start + i].trim_end();
            if buf_line != exp.as_bytes().trim_end() {
                return false;
            }
        }
        true
    };

    if matches_at(nominal_start) {
        return Some(nominal_start);
    }

    let max_offset = buffer_lines.len();
    for offset in 1..=max_offset {
        if nominal_start >= offset && matches_at(nominal_start - offset) {
            return Some(nominal_start - offset);
        }
        if nominal_start + offset < buffer_lines.len() && matches_at(nominal_start + offset) {
            return Some(nominal_start + offset);
        }
    }

    None
}

/// Open the Git index, or initialize a new empty index state if none exists
/// yet.
pub(crate) fn open_or_create_index(repo: &gix::Repository) -> Result<gix::index::File, String> {
    match repo.open_index() {
        Ok(idx) => Ok(idx),
        Err(_) => {
            let index_path = repo.index_path();
            Ok(gix::index::File::from_state(
                gix::index::State::new(repo.object_hash()),
                index_path,
            ))
        }
    }
}

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

/// Create an in-memory safety stash checkpoint commit and return its hex hash.
pub fn create_stash_checkpoint(repo: &Repository) -> Option<String> {
    let head = repo.raw().head_commit().ok()?;
    let head_tree = head.tree().ok()?;
    let mut editor = head_tree.edit().ok()?;

    let index = open_or_create_index(repo.raw()).ok()?;
    for entry in index.entries() {
        let path_str = entry.path(&index).to_str().ok()?;
        let kind = index_mode_to_entry_kind(entry.mode);
        let _ = editor.upsert(path_str, kind, entry.id);
    }

    let tree_id = editor.write().ok()?;

    let sig = repo
        .raw()
        .committer()
        .and_then(|res| res.ok())
        .and_then(|s| s.to_owned().ok())
        .unwrap_or_else(default_signature);

    let commit = Commit {
        tree: tree_id.detach(),
        parents: vec![head.id().detach()].into(),
        author: sig.clone(),
        committer: sig,
        encoding: None,
        message: "review-checkpoint".into(),
        extra_headers: Vec::new(),
    };

    let commit_id = repo.raw().write(&commit).ok()?;
    Some(commit_id.to_string())
}
