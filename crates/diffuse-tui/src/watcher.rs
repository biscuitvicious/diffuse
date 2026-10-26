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

/// Check whether `path` modified on disk points to the review notes database
/// file.
#[must_use]
pub fn is_notes_path(path: &Path, notes_path: &Path) -> bool {
    if path == notes_path {
        return true;
    }
    if let (Ok(c1), Ok(c2)) = (path.canonicalize(), notes_path.canonicalize())
        && c1 == c2
    {
        return true;
    }
    if let (Some(n1), Some(n2)) = (path.file_name(), notes_path.file_name())
        && n1 == n2
        && let (Some(p1), Some(p2)) = (path.parent(), notes_path.parent())
    {
        if p1 == p2 {
            return true;
        }
        if let (Ok(cp1), Ok(cp2)) = (p1.canonicalize(), p2.canonicalize())
            && cp1 == cp2
        {
            return true;
        }
    }
    false
}

/// Classification of a single raw filesystem mutation event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsChange {
    /// Working tree file mutation (path relative to repo root).
    File(String),
    /// Repository-level metadata change (.git/index, .git/HEAD, refs).
    Global,
    /// Review notes database file mutation.
    Notes,
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

/// Sub-classification of diff-related reloads in a consolidated event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsDiffEvent {
    /// Changes localized to specific working tree source paths (relative to
    /// repo root).
    Paths(Vec<String>),
    /// Repository-level metadata change requiring global reload (.git/index,
    /// .git/HEAD, refs).
    Global,
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
    /// Review notes database file modified on disk.
    Notes,
    /// Both diff files and review notes changed within the quiet window.
    DiffAndNotes { diff: FsDiffEvent },
}

/// Collector that aggregates and debounces incoming `FsChange` events over a
/// quiet window.
#[derive(Debug, Default)]
pub struct FsChangeCollector {
    pub pending_files: HashSet<String>,
    pub pending_global: bool,
    pub pending_notes: bool,
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
            FsChange::Notes => {
                self.pending_notes = true;
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
        let notes = std::mem::take(&mut self.pending_notes);

        let diff = if self.pending_global {
            self.pending_global = false;
            self.pending_files.clear();
            Some(FsDiffEvent::Global)
        } else if !self.pending_files.is_empty() {
            let files = std::mem::take(&mut self.pending_files);
            Some(FsDiffEvent::Paths(files.into_iter().collect()))
        } else {
            None
        };

        match (diff, notes) {
            (Some(diff), true) => Some(FsEvent::DiffAndNotes { diff }),
            (Some(FsDiffEvent::Global), false) => Some(FsEvent::Global),
            (Some(FsDiffEvent::Paths(paths)), false) => Some(FsEvent::Paths(paths)),
            (None, true) => Some(FsEvent::Notes),
            (None, false) => None,
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
        Self::spawn_with_notes(Some(repo_root), None, tx)
    }

    /// Spawn a background filesystem watcher on optional `repo_root` and
    /// optional `notes_path` that sends classified changes to `tx`.
    pub fn spawn_with_notes(
        repo_root: Option<PathBuf>,
        notes_path: Option<PathBuf>,
        tx: mpsc::Sender<FsChange>,
    ) -> Result<Self, notify::Error> {
        let root_for_filter = repo_root.clone();
        let notes_for_filter = notes_path.clone();

        let event_handler = move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if !is_mutation_event(&event.kind) {
                    return;
                }
                for p in &event.paths {
                    if let Some(ref np) = notes_for_filter
                        && is_notes_path(p, np)
                    {
                        let _ = tx.send(FsChange::Notes);
                        continue;
                    }
                    if let Some(ref root) = root_for_filter
                        && let Some(change) = classify_repo_path(p, root)
                    {
                        let _ = tx.send(change);
                    }
                }
            }
        };

        let mut watcher = RecommendedWatcher::new(event_handler, Config::default())?;

        if let Some(ref root) = repo_root
            && root.exists()
        {
            watcher.watch(root, RecursiveMode::Recursive)?;
        }

        if let Some(ref np) = notes_path {
            if let Some(parent) = np.parent() {
                let _ = std::fs::create_dir_all(parent);
                if parent.exists() {
                    let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
                } else if np.exists() {
                    let _ = watcher.watch(np, RecursiveMode::NonRecursive);
                }
            } else if np.exists() {
                let _ = watcher.watch(np, RecursiveMode::NonRecursive);
            }
        }

        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests;
