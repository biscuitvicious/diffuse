//! Repository, revision range, and review target identity.
//!
//! Re-exports canonical types from `diffuse-git` and provides review notes
//! key matching integration.

pub use diffuse_git::{Repo, ReviewTarget, RevisionRange, parse_git_rev_and_pathspecs};

use crate::notes::NotesDB;

/// Extension trait providing review notes key matching on [`ReviewTarget`].
pub trait ReviewTargetNotesExt {
    /// Identify database keys in `db` matching this review target.
    fn matched_notes_db_keys<'a>(&self, db: &'a NotesDB) -> Vec<&'a str>;
}

impl ReviewTargetNotesExt for ReviewTarget {
    fn matched_notes_db_keys<'a>(&self, db: &'a NotesDB) -> Vec<&'a str> {
        let r = self.repo_name();
        let mut matched = Vec::new();

        if let Some(ref target_commit) = self.notes_commit() {
            let prefix = format!("{r}@");
            for k in db.keys() {
                if let Some(hash) = k.strip_prefix(&prefix)
                    && (target_commit.starts_with(hash) || hash.starts_with(target_commit))
                {
                    matched.push(k.as_str());
                }
            }
        } else if let Some((k, _)) = db.get_key_value(r) {
            matched.push(k.as_str());
        }

        matched.sort_unstable();
        matched.dedup();
        matched
    }
}

#[cfg(test)]
mod tests;
