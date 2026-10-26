//! Filesystem watching and event filtering for live repository reload.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use notify::{
    Config, Event, RecommendedWatcher, RecursiveMode, Watcher,
    event::{EventKind, MetadataKind, ModifyKind},
};

/// Check whether a filesystem notification event represents an actual data/file
/// mutation (and not a read or access-time update).
#[must_use]
pub fn is_mutation_event(kind: &EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Any => true,
        EventKind::Modify(modify_kind) => {
            !matches!(modify_kind, ModifyKind::Metadata(MetadataKind::AccessTime))
        }
        EventKind::Access(_) | EventKind::Other => false,
    }
}

/// Check whether a path modified on disk is relevant to working tree diffs.
#[must_use]
pub fn is_relevant_repo_path(path: &Path, repo_root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(repo_root) else {
        return false;
    };
    let components: Vec<_> = rel.iter().map(|c| c.to_string_lossy()).collect();
    if components.is_empty() {
        return false;
    }

    // Git internal metadata handling:
    if components[0] == ".git" {
        if components.len() == 2 {
            // Top-level git state files
            return matches!(
                components[1].as_ref(),
                "index" | "HEAD" | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD"
            );
        }
        if components.len() >= 3 && components[1] == "refs" {
            // Branch / tag updates
            return true;
        }
        // Discard packfiles, loose objects, commit-graph, hooks, logs
        return false;
    }

    // Discard common build, temporary, & artifact directories:
    if matches!(
        components[0].as_ref(),
        "target" | "node_modules" | ".next" | "build" | "dist" | ".cache"
    ) {
        return false;
    }

    // Accept all other source tree paths
    true
}

/// Classification of a single raw filesystem mutation event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    /// Working tree file mutation (path relative to repo root).
    File(String),
    /// Repository-level metadata change (.git/index, .git/HEAD, refs).
    Global,
}

/// Classify a path modification into a working tree file change or global git
/// metadata change.
#[must_use]
pub fn classify_repo_path(path: &Path, repo_root: &Path) -> Option<FsChange> {
    if !is_relevant_repo_path(path, repo_root) {
        return None;
    }
    let rel = path.strip_prefix(repo_root).ok()?;
    let rel_str = rel.to_string_lossy();
    if rel_str.starts_with(".git") {
        Some(FsChange::Global)
    } else {
        Some(FsChange::File(rel_str.to_string()))
    }
}

/// Consolidated, debounced event ready for execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEvent {
    /// Changes localized to specific working tree source paths (relative to
    /// repo root).
    Paths(Vec<String>),
    /// Repository-level metadata change requiring global reload (.git/index,
    /// .git/HEAD, refs).
    Global,
}

/// Collector that aggregates and debounces incoming `FsChange` events over a
/// quiet window.
#[derive(Debug, Default)]
pub struct FsChangeCollector {
    pub pending_files: HashSet<String>,
    pub pending_global: bool,
    pub last_activity: Option<Instant>,
}

impl FsChangeCollector {
    /// Create a new, empty change collector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an incoming filesystem change and refresh the activity timestamp.
    pub fn record(&mut self, change: FsChange, now: Instant) {
        self.last_activity = Some(now);
        match change {
            FsChange::Global => self.pending_global = true,
            FsChange::File(path) => {
                self.pending_files.insert(path);
            }
        }
    }

    /// Check if the quiet window has elapsed and return the consolidated event
    /// to execute.
    pub fn take_debounced(&mut self, now: Instant, quiet_window: Duration) -> Option<FsEvent> {
        let last = self.last_activity?;
        if now.duration_since(last) < quiet_window {
            return None;
        }

        self.last_activity = None;
        if self.pending_global {
            self.pending_global = false;
            self.pending_files.clear();
            Some(FsEvent::Global)
        } else if !self.pending_files.is_empty() {
            let files = std::mem::take(&mut self.pending_files);
            Some(FsEvent::Paths(files.into_iter().collect()))
        } else {
            None
        }
    }
}

/// Filesystem repository watcher that triggers notifications on relevant file
/// mutations.
pub struct RepoWatcher {
    _watcher: RecommendedWatcher,
}

impl RepoWatcher {
    /// Spawn a background filesystem watcher on `repo_root` that sends
    /// classified changes to `tx`.
    pub fn spawn(repo_root: PathBuf, tx: mpsc::Sender<FsChange>) -> Result<Self, notify::Error> {
        let root_for_filter = repo_root.clone();
        let event_handler = move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if !is_mutation_event(&event.kind) {
                    return;
                }
                for p in &event.paths {
                    if let Some(change) = classify_repo_path(p, &root_for_filter) {
                        let _ = tx.send(change);
                    }
                }
            }
        };

        let mut watcher = RecommendedWatcher::new(event_handler, Config::default())?;
        watcher.watch(&repo_root, RecursiveMode::Recursive)?;

        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests;
