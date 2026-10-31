//! Notes database persistence, query engine, and in-memory mutation operations.

use std::{collections::HashMap, fs, io, path::Path};

use super::note::{NoteLocation, NoteStatus, ReviewNote, reply_to_note_text};

/// The notes database: repo_key -> rev_key -> file_path -> list of notes.
pub type NotesDB = HashMap<String, HashMap<String, HashMap<String, Vec<ReviewNote>>>>;

/// Load notes from disk. Returns an empty DB if the file does not exist or
/// fails to parse.
pub fn load_notes(path: &Path) -> NotesDB {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Save notes to disk.
pub fn save_notes(path: &Path, db: &NotesDB) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(db)?;
    fs::write(path, raw)
}

/// Check if a commit or range specification matches a candidate hash/range from
/// the database.
///
/// Supports prefix matching for single commits as well as dual-endpoint prefix
/// matching for commit ranges (`base..tip`). Single-commit targets never match
/// range candidates, and range targets never match single-commit candidates.
#[inline]
pub(crate) fn commit_spec_matches(target_spec: &str, candidate_hash: &str) -> bool {
    if target_spec.is_empty() || candidate_hash.is_empty() {
        return target_spec == candidate_hash;
    }
    if let Some((target_base, target_tip)) = target_spec.split_once("..") {
        if let Some((c_base, c_tip)) = candidate_hash.split_once("..") {
            (target_base.starts_with(c_base) || c_base.starts_with(target_base))
                && (target_tip.starts_with(c_tip) || c_tip.starts_with(target_tip))
        } else {
            false
        }
    } else if candidate_hash.contains("..") {
        false
    } else {
        target_spec.starts_with(candidate_hash) || candidate_hash.starts_with(target_spec)
    }
}

/// Get all notes for a specific repo, rev, and file.
pub fn notes_for_file<'a>(
    db: &'a NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
) -> impl Iterator<Item = &'a ReviewNote> {
    db.get(repo_key)
        .and_then(|rev_map| rev_map.get(rev_key))
        .and_then(|file_map| file_map.get(file))
        .into_iter()
        .flatten()
}

/// Query notes for a specific file path within a repository.
///
/// If `repo_key` is None, returns empty (notes disabled outside a repository).
/// When `rev_key` is specified, matches exact rev_key first, then compatible
/// commit/range prefixes via [`commit_spec_matches`].
/// When `rev_key` is None, queries notes across all revisions for that
/// repo_key.
pub fn query_notes_for_file<'a>(
    db: &'a NotesDB,
    repo_key: Option<&str>,
    rev_key: Option<&str>,
    file: &str,
) -> Vec<&'a ReviewNote> {
    let Some(repo_k) = repo_key else {
        return Vec::new();
    };
    let Some(rev_map) = db.get(repo_k) else {
        return Vec::new();
    };

    if let Some(target_rev) = rev_key {
        if let Some(file_map) = rev_map.get(target_rev)
            && let Some(notes) = file_map.get(file)
            && !notes.is_empty()
        {
            return notes.iter().collect();
        }
        return rev_map
            .iter()
            .filter(|(k, _)| commit_spec_matches(target_rev, k))
            .filter_map(|(_, file_map)| file_map.get(file))
            .flatten()
            .collect();
    }

    if let Some(file_map) = rev_map.get("")
        && let Some(notes) = file_map.get(file)
        && !notes.is_empty()
    {
        return notes.iter().collect();
    }

    rev_map
        .values()
        .filter_map(|file_map| file_map.get(file))
        .flatten()
        .collect()
}

/// Query all distinct file paths that have notes for the given repo (and
/// optional rev).
pub fn note_files_for_repo(
    db: &NotesDB,
    repo_key: Option<&str>,
    rev_key: Option<&str>,
) -> Vec<String> {
    let Some(repo_k) = repo_key else {
        return Vec::new();
    };
    let Some(rev_map) = db.get(repo_k) else {
        return Vec::new();
    };

    let mut files = std::collections::HashSet::new();
    if let Some(target_rev) = rev_key {
        if let Some(file_map) = rev_map.get(target_rev) {
            for (f, notes) in file_map {
                if !notes.is_empty() {
                    files.insert(f.clone());
                }
            }
        }
        for (cand_rev, file_map) in rev_map {
            if cand_rev != target_rev && commit_spec_matches(target_rev, cand_rev) {
                for (f, notes) in file_map {
                    if !notes.is_empty() {
                        files.insert(f.clone());
                    }
                }
            }
        }
    } else {
        for file_map in rev_map.values() {
            for (f, notes) in file_map {
                if !notes.is_empty() {
                    files.insert(f.clone());
                }
            }
        }
    }

    let mut sorted: Vec<String> = files.into_iter().collect();
    sorted.sort();
    sorted
}

/// Get all notes for a specific repository.
pub fn all_notes<'a>(db: &'a NotesDB, repo_key: &str) -> impl Iterator<Item = &'a ReviewNote> {
    db.get(repo_key)
        .into_iter()
        .flat_map(|rev_map| rev_map.values())
        .flat_map(|file_map| file_map.values())
        .flatten()
}

/// Return a copy of the database restricted to non-resolved notes.
///
/// Callers that load content only for notes the user can currently see (for
/// example, synthesizing context around an anchoring note) use this to derive
/// a content-scoped view; the authoritative source database is never modified.
/// Empty per-file, per-revision, and per-repo maps are pruned after removal.
#[must_use]
pub fn without_resolved(db: &NotesDB) -> NotesDB {
    let mut out = db.clone();
    for rev_map in out.values_mut() {
        for file_map in rev_map.values_mut() {
            for notes in file_map.values_mut() {
                notes.retain(|n| !n.status.is_resolved());
            }
            file_map.retain(|_, notes| !notes.is_empty());
        }
        rev_map.retain(|_, file_map| !file_map.is_empty());
    }
    out.retain(|_, rev_map| !rev_map.is_empty());
    out
}

/// Add a note to the database for a given repository and revision.
pub fn add_note(db: &mut NotesDB, repo_key: &str, rev_key: &str, note: ReviewNote) {
    db.entry(repo_key.to_string())
        .or_default()
        .entry(rev_key.to_string())
        .or_default()
        .entry(note.file.clone())
        .or_default()
        .push(note);
}

/// Remove line notes overlapping the line range `[start, end]` on either
/// diff side. Returns the number of removed notes.
pub fn remove_notes_in_range(
    db: &mut NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
    start: u32,
    end: u32,
) -> usize {
    let Some(rev_map) = db.get_mut(repo_key) else {
        return 0;
    };
    let Some(file_map) = rev_map.get_mut(rev_key) else {
        return 0;
    };
    let Some(list) = file_map.get_mut(file) else {
        return 0;
    };
    let before = list.len();
    list.retain(|n| {
        let note_range = match &n.location {
            NoteLocation::File => return true,
            NoteLocation::Line { coord } => {
                let num = coord.new_line_num.or(coord.old_line_num);
                match num {
                    Some(n) => (n.get(), n.get()),
                    None => return true,
                }
            }
            NoteLocation::Range { start: s, end: e } => {
                let s_num = s.new_line_num.or(s.old_line_num);
                let e_num = e.new_line_num.or(e.old_line_num);
                match (s_num, e_num) {
                    (Some(sn), Some(en)) => (sn.get(), en.get()),
                    _ => return true,
                }
            }
        };
        let (n_start, n_end) = note_range;
        n_end < start || n_start > end
    });
    let removed = before - list.len();
    if list.is_empty() {
        file_map.remove(file);
        if file_map.is_empty() {
            rev_map.remove(rev_key);
        }
    }
    removed
}

/// Remove all file-level notes for a file (notes whose location is
/// [`NoteLocation::File`]). Returns the number of removed notes.
pub fn remove_file_notes(db: &mut NotesDB, repo_key: &str, rev_key: &str, file: &str) -> usize {
    let Some(rev_map) = db.get_mut(repo_key) else {
        return 0;
    };
    let Some(file_map) = rev_map.get_mut(rev_key) else {
        return 0;
    };
    let Some(list) = file_map.get_mut(file) else {
        return 0;
    };
    let before = list.len();
    list.retain(|n| !n.location.is_file());
    let removed = before - list.len();
    if list.is_empty() {
        file_map.remove(file);
        if file_map.is_empty() {
            rev_map.remove(rev_key);
        }
    }
    removed
}

/// Remove a specific note matching the repo, rev, file, and unique `note_id`.
/// Returns `true` if a note was removed, `false` otherwise.
pub fn remove_note_by_id(
    db: &mut NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
    note_id: &str,
) -> bool {
    let Some(rev_map) = db.get_mut(repo_key) else {
        return false;
    };
    let Some(file_map) = rev_map.get_mut(rev_key) else {
        return false;
    };
    let Some(list) = file_map.get_mut(file) else {
        return false;
    };
    let before = list.len();
    list.retain(|n| n.id != note_id);
    let removed = list.len() < before;
    if list.is_empty() {
        file_map.remove(file);
        if file_map.is_empty() {
            rev_map.remove(rev_key);
        }
    }
    removed
}

/// Update the text of a specific note matching the repo, rev, file, and unique
/// `note_id`. Returns `true` if the note was found and updated, `false`
/// otherwise.
pub fn update_note_text(
    db: &mut NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
    note_id: &str,
    new_text: String,
) -> bool {
    let Some(rev_map) = db.get_mut(repo_key) else {
        return false;
    };
    let Some(file_map) = rev_map.get_mut(rev_key) else {
        return false;
    };
    let Some(list) = file_map.get_mut(file) else {
        return false;
    };
    if let Some(note) = list.iter_mut().find(|n| n.id == note_id) {
        note.text = new_text;
        true
    } else {
        false
    }
}

/// Set the lifecycle status of a specific note matching repo, rev, file, and
/// `note_id`.
pub fn set_note_status_by_id(
    db: &mut NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
    note_id: &str,
    status: NoteStatus,
) -> bool {
    let Some(rev_map) = db.get_mut(repo_key) else {
        return false;
    };
    let Some(file_map) = rev_map.get_mut(rev_key) else {
        return false;
    };
    let Some(list) = file_map.get_mut(file) else {
        return false;
    };
    if let Some(note) = list.iter_mut().find(|n| n.id == note_id) {
        note.status = status;
        true
    } else {
        false
    }
}

/// Mark a specific note as resolved (`NoteStatus::Resolved`).
#[inline]
pub fn resolve_note_by_id(
    db: &mut NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
    note_id: &str,
) -> bool {
    set_note_status_by_id(db, repo_key, rev_key, file, note_id, NoteStatus::Resolved)
}

/// Reopen a specific note back to open status (`NoteStatus::Open`).
#[inline]
pub fn reopen_note_by_id(
    db: &mut NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
    note_id: &str,
) -> bool {
    set_note_status_by_id(db, repo_key, rev_key, file, note_id, NoteStatus::Open)
}

/// Alias for [`reopen_note_by_id`]: reset note back to open.
#[inline]
pub fn open_note_by_id(
    db: &mut NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
    note_id: &str,
) -> bool {
    reopen_note_by_id(db, repo_key, rev_key, file, note_id)
}

/// Reply to a specific note by prepending reply text and blockquoting original
/// text. Optionally updates the note's status.
pub fn reply_to_note_by_id(
    db: &mut NotesDB,
    repo_key: &str,
    rev_key: &str,
    file: &str,
    note_id: &str,
    reply: &str,
    status: Option<NoteStatus>,
) -> bool {
    let Some(rev_map) = db.get_mut(repo_key) else {
        return false;
    };
    let Some(file_map) = rev_map.get_mut(rev_key) else {
        return false;
    };
    let Some(list) = file_map.get_mut(file) else {
        return false;
    };
    if let Some(note) = list.iter_mut().find(|n| n.id == note_id) {
        note.text = reply_to_note_text(&note.text, reply);
        if let Some(s) = status {
            note.status = s;
        }
        true
    } else {
        false
    }
}

/// Find a note and its rev and file path by note ID within `repo_key`.
pub fn find_note_by_id<'a>(
    db: &'a NotesDB,
    repo_key: &str,
    note_id: &str,
) -> Option<(&'a str, &'a str, &'a ReviewNote)> {
    let rev_map = db.get(repo_key)?;
    for (rev, file_map) in rev_map {
        for (file, notes) in file_map {
            if let Some(note) = notes.iter().find(|n| n.id == note_id) {
                return Some((rev.as_str(), file.as_str(), note));
            }
        }
    }
    None
}

/// Set a note's lifecycle status searching across all revisions and files in
/// `repo_key`.
pub fn set_note_status_in_repo(
    db: &mut NotesDB,
    repo_key: &str,
    note_id: &str,
    status: NoteStatus,
) -> bool {
    let Some(rev_map) = db.get_mut(repo_key) else {
        return false;
    };
    for file_map in rev_map.values_mut() {
        for notes in file_map.values_mut() {
            if let Some(note) = notes.iter_mut().find(|n| n.id == note_id) {
                note.status = status;
                return true;
            }
        }
    }
    false
}

/// Find a note by ID (or ID prefix) strictly within `repo_key`.
///
/// Returns `Some((rev_key, file_path, note))` if found.
/// Matches exact note ID first; if none match, matches note ID prefix.
pub fn find_note_in_repo<'a>(
    db: &'a NotesDB,
    repo_key: &str,
    id_or_prefix: &str,
) -> Option<(&'a str, &'a str, &'a ReviewNote)> {
    let rev_map = db.get(repo_key)?;

    // 1. Exact ID match pass
    for (rev, file_map) in rev_map {
        for (file, notes) in file_map {
            if let Some(note) = notes.iter().find(|n| n.id == id_or_prefix) {
                return Some((rev.as_str(), file.as_str(), note));
            }
        }
    }

    // 2. Prefix match pass
    for (rev, file_map) in rev_map {
        for (file, notes) in file_map {
            if let Some(note) = notes.iter().find(|n| n.id.starts_with(id_or_prefix)) {
                return Some((rev.as_str(), file.as_str(), note));
            }
        }
    }

    None
}

/// Mutate a note found by ID (or prefix) strictly within `repo_key`.
pub fn update_note_in_repo<F>(
    db: &mut NotesDB,
    repo_key: &str,
    id_or_prefix: &str,
    mut f: F,
) -> bool
where
    F: FnMut(&mut ReviewNote),
{
    let found = find_note_in_repo(db, repo_key, id_or_prefix)
        .map(|(rev, file, note)| (rev.to_string(), file.to_string(), note.id.clone()));

    if let Some((rev, file, note_id)) = found
        && let Some(rev_map) = db.get_mut(repo_key)
        && let Some(file_map) = rev_map.get_mut(&rev)
        && let Some(notes) = file_map.get_mut(&file)
        && let Some(note) = notes.iter_mut().find(|n| n.id == note_id)
    {
        f(note);
        return true;
    }
    false
}

/// Delete a note by ID (or prefix) strictly within `repo_key`.
pub fn delete_note_in_repo(db: &mut NotesDB, repo_key: &str, id_or_prefix: &str) -> bool {
    let found = find_note_in_repo(db, repo_key, id_or_prefix)
        .map(|(rev, file, note)| (rev.to_string(), file.to_string(), note.id.clone()));

    if let Some((rev, file, note_id)) = found
        && let Some(rev_map) = db.get_mut(repo_key)
        && let Some(file_map) = rev_map.get_mut(&rev)
        && let Some(notes) = file_map.get_mut(&file)
    {
        let before = notes.len();
        notes.retain(|n| n.id != note_id);
        let removed = notes.len() < before;
        if notes.is_empty() {
            file_map.remove(&file);
            if file_map.is_empty() {
                rev_map.remove(&rev);
            }
        }
        return removed;
    }
    false
}
