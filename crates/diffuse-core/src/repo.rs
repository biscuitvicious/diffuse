//! Repository, revision range, and review target identity.
//!
//! Re-exports canonical types from `diffuse-git` and provides review notes
//! key matching integration.

pub use diffuse_git::{Repo, ReviewTarget, RevisionRange, parse_git_rev_and_pathspecs};

use crate::notes::NotesDB;

/// Extension trait providing review notes key matching on [`ReviewTarget`].
pub trait ReviewTargetNotesExt {
    /// Identify revision keys in `db` matching this review target under its
    /// repository key.
    fn matched_rev_keys<'a>(&self, db: &'a NotesDB) -> Vec<&'a str>;
}

impl ReviewTargetNotesExt for ReviewTarget {
    fn matched_rev_keys<'a>(&self, db: &'a NotesDB) -> Vec<&'a str> {
        let Some(repo_key) = self.repo_key() else {
            return Vec::new();
        };
        let Some(rev_map) = db.get(repo_key) else {
            return Vec::new();
        };

        let mut matched = Vec::new();
        if let Some(target_rev) = self.rev_key() {
            for cand_rev in rev_map.keys() {
                if cand_rev == target_rev || crate::notes::commit_spec_matches(target_rev, cand_rev)
                {
                    matched.push(cand_rev.as_str());
                }
            }
        } else {
            for cand_rev in rev_map.keys() {
                matched.push(cand_rev.as_str());
            }
        }

        matched.sort_unstable();
        matched.dedup();
        matched
    }
}

#[cfg(test)]
mod tests;
