use std::path::Path;

use gix::{bstr::ByteSlice, objs::Commit, prelude::Write};

use crate::Repository;

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
pub(crate) fn entry_kind_to_index_mode(
    kind: gix::object::tree::EntryKind,
) -> gix::index::entry::Mode {
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
pub(crate) fn file_stat(path: &Path) -> gix::index::entry::Stat {
    gix::index::fs::Metadata::from_path_no_follow(path)
        .ok()
        .and_then(|m| gix::index::entry::Stat::from_fs(&m).ok())
        .unwrap_or_default()
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
