//! Note storage, persistence, and line anchoring for review notes.

pub mod anchor;
pub mod note;
pub mod path;
pub mod store;

#[cfg(test)]
mod tests;

pub use anchor::{DiffCoord, LineHash, note_anchors_in_context_lines, note_anchors_in_rows};
pub use note::{
    LineNum, NoteLocation, NoteStatus, ReviewNote, normalize_note_text, note_line_span,
    reply_to_note_text,
};
#[cfg(test)]
pub(crate) use path::state_dir_from;
pub use path::{
    default_notes_path, default_state_dir, default_state_path, generate_note_id, head_commit,
    notes_commit, now_millis,
};
#[cfg(test)]
pub(crate) use std::{borrow::Cow, num::NonZeroU32};
pub(crate) use store::commit_spec_matches;
pub use store::{
    NotesDB, add_note, all_notes, delete_note_in_repo, find_note_by_id, find_note_in_repo,
    load_notes, note_files_for_repo, notes_for_file, open_note_by_id, query_notes_for_file,
    remove_file_notes, remove_note_by_id, remove_notes_in_range, reopen_note_by_id,
    reply_to_note_by_id, resolve_note_by_id, save_notes, set_note_status_by_id,
    set_note_status_in_repo, update_note_in_repo, update_note_text, without_resolved,
};

pub use crate::pipeline::note_runs;
