# Automatic Filesystem Watching & Live Working Tree Reload

- [x] 1. [Phase 1 — Filesystem Watcher Engine & Path Filtering](#1-phase-1--filesystem-watcher-engine--path-filtering)
- [x] 2. [Phase 2 — Debounced Event Dispatch & Channel Plumbing](#2-phase-2--debounced-event-dispatch--channel-plumbing)
- [x] 3. [Phase 3 — TUI Event Loop Integration & Interaction Guarding](#3-phase-3--tui-event-loop-integration--interaction-guarding)
- [x] 4. [Phase 4 — CLI Flag & Review Target Scoping](#4-phase-4--cli-flag--review-target-scoping)
- [x] 5. [Phase 5 — Status Feedback & Transient Notifications](#5-phase-5--status-feedback--transient-notifications)
- [x] 6. [Phase 6 — Automated Integration Test Suite & Verification Gate](#6-phase-6--automated-integration-test-suite--verification-gate)

---

## Executive Summary

When reviewing working tree changes (unstaged or staged), external processes—such as code editors, build tools, formatting scripts, or background `git add` / `git commit` commands—frequently mutate files on disk while *diffuse* is running. Currently, *diffuse* requires either launching an external editor from within the TUI (`e`), manually toggling staged mode (`s`), or restarting the session to reflect updated content.

This plan specifies the design and implementation of an asynchronous filesystem watcher for *diffuse* that continuously monitors working tree files, the git staging index (`.git/index`), and active branch references (`.git/HEAD`, `.git/refs/heads/*`). When changes are detected, the engine debounces high-frequency event bursts, checks interaction safety invariants (suspending reload while the user edits note drafts or interacts with modal dialogs), and seamlessly refreshes the diff pipeline while preserving the active cursor row, scroll position, and expanded hunk state.

### Architectural Invariants

1. **Non-Blocking Asynchronous Monitoring**: The watcher runs on a dedicated background thread or task. Filesystem I/O, directory walking, and kernel event notifications (via `inotify`, `FSEvents`, or `ReadDirectoryChangesW`) never block the TUI event loop, terminal frame drawing, or keyboard input handling.
2. **Selective Path Filtering**: Events are strictly filtered to discard noisy build artifacts (`target/`, `node_modules/`, `.git/objects/`, `.git/logs/`). Only relevant source paths and canonical repository state files (`.git/index`, `.git/HEAD`, refs) trigger pipeline reloads.
3. **Temporal Debouncing**: Burst events from atomic saves (temporary file creation $\to$ atomic rename $\to$ chmod) or multi-file build steps are coalesced over a configurable quiet window ($\Delta t \approx 150\,\text{ms}$) before computing diff transformations.
4. **Interaction Safety Invariant**: A reload must never clobber active user input. If the user has an open modal (file finder, help overlay) or an active review note draft in progress, the reload event is deferred and automatically flushed once the modal or note input concludes.
5. **Cursor & Viewport Continuity**: Reloading invokes the proven reconciliation engine in [`AppState::reload`](crates/diffuse-tui/src/app/sync.rs), which re-anchors the active cursor row and viewport offset to the corresponding post-reload visual lines.
6. **Granular Single-File Reload vs Global Refresh**: When the watcher detects modifications to specific source files in the working tree, only those specific files are re-diffed, re-aligned, and updated in `AppState.output.files`, leaving all other files untouched. Full repository reloads are reserved for repository-level transitions (`.git/index` staging changes, `.git/HEAD` branch switches, or file additions/deletions).

---

## Table of Contents

1. [Phase 1 — Filesystem Watcher Engine & Path Filtering](#1-phase-1--filesystem-watcher-engine--path-filtering)
2. [Phase 2 — Debounced Event Dispatch & Channel Plumbing](#2-phase-2--debounced-event-dispatch--channel-plumbing)
3. [Phase 3 — TUI Event Loop Integration & Interaction Guarding](#3-phase-3--tui-event-loop-integration--interaction-guarding)
4. [Phase 4 — CLI Flag & Review Target Scoping](#4-phase-4--cli-flag--review-target-scoping)
5. [Phase 5 — Status Feedback & Transient Notifications](#5-phase-5--status-feedback--transient-notifications)
6. [Phase 6 — Automated Integration Test Suite & Verification Gate](#6-phase-6--automated-integration-test-suite--verification-gate)

---

## 1. Phase 1 — Filesystem Watcher Engine & Path Filtering

### Objective
Implement a lightweight, decoupled filesystem watcher module in `diffuse-tui` that monitors the repository root recursively while selectively ignoring build directories and internal git pack/object stores.

### File Modifications
- [`crates/diffuse-tui/Cargo.toml`](crates/diffuse-tui/Cargo.toml): Add `notify = { version = "8", default-features = false, features = ["macos_kqueue"] }` or standard cross-platform backend.
- [`crates/diffuse-tui/src/watcher.rs`](crates/diffuse-tui/src/watcher.rs): New module containing `RepoWatcher` and `WatchFilter`.

### Key Design & Algorithms
1. **Path Relevance Predicate**:
   ```rust
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
               return matches!(components[1].as_ref(), "index" | "HEAD" | "MERGE_HEAD");
           }
           if components.len() >= 3 && components[1] == "refs" {
               // Branch / tag updates
               return true;
           }
           // Discard packfiles, loose objects, commit-graph, hooks, logs
           return false;
       }

       // Discard common build & artifact directories:
       if matches!(components[0].as_ref(), "target" | "node_modules" | ".next" | "build" | "dist" | ".cache") {
           return false;
       }

       // Accept all other source tree paths
       true
   }
   ```
2. **Watcher Lifecycle**:
   - `RepoWatcher::spawn(repo_root: PathBuf, tx: mpsc::Sender<FsEvent>) -> Result<RepoWatcher, WatcherError>`.
   - Spawns background worker thread listening on the native notify event stream.
   - When dropped, cleanly unregisters kernel watch descriptors.

### Verification Gate
- Unit tests in `crates/diffuse-tui/src/watcher/tests.rs`:
  - `test_relevant_path_accepts_working_tree_files`: Verifies source files are marked relevant.
  - `test_relevant_path_accepts_git_index_and_head`: Verifies `.git/index` and `.git/HEAD` pass.
  - `test_relevant_path_rejects_git_objects_and_pack`: Verifies `.git/objects/...` is filtered.
  - `test_relevant_path_rejects_target_and_build`: Verifies `target/...` is filtered.

---

## 2. Phase 2 — Debounced Event Dispatch & Granular Change Tracking

### Objective
Buffer and coalesce rapid bursts of filesystem change notifications into consolidated reload pulses, distinguishing specific changed working tree files from global repository state changes.

### File Modifications
- [`crates/diffuse-tui/src/watcher.rs`](crates/diffuse-tui/src/watcher.rs): Debouncer queue, path collection, and event categorization.

### Key Design & Algorithms
1. **Event Classification**:
   ```rust
   #[derive(Debug, Clone, PartialEq, Eq)]
   pub enum FsEvent {
       /// Changes localized to specific working tree source paths.
       Paths(HashSet<PathBuf>),
       /// Repository-level metadata change requiring global reload (.git/index, .git/HEAD, refs).
       Global,
   }
   ```
2. **Debounce Dynamics**:
   - Editors often generate 3–10 filesystem events per save (write temp $\to$ rename $\to$ update metadata $\to$ remove backup).
   - The watcher channel sends classified `FsEvent` items.
   - If any event in the debounce window touches `.git/index`, `.git/HEAD`, or branch refs, the entire batch upgrades to `FsEvent::Global`.
   - Otherwise, the paths are accumulated in a `HashSet<PathBuf>`.
   - A reload is dispatched only when `last_fs_event.elapsed() >= Duration::from_millis(150)`.
3. **Backpressure & Drain**:
   - The event channel is non-blocking (`mpsc::channel`).
   - While the pipeline is recomputing, incoming events accumulate in the pending set rather than spawning redundant diff jobs.

### Verification Gate
- Unit tests in `crates/diffuse-tui/src/watcher/tests.rs`:
  - `test_debounced_event_batching`: Simulates 20 rapid file events within 10ms; asserts consolidated paths emit after the quiet window.
  - `test_git_index_mutation_upgrades_to_global_reload`: Verifies that a change to `.git/index` promotes the pending batch to `FsEvent::Global`.

---

## 3. Phase 3 — Granular File Diffing & TUI Event Loop Integration

### Objective
Implement targeted single-file reloading in `actions.rs` / `AppState` and integrate the watcher receiver into the main interactive event loop in [`crates/diffuse-tui/src/lib.rs`](crates/diffuse-tui/src/lib.rs).

### File Modifications
- [`crates/diffuse-tui/src/actions.rs`](crates/diffuse-tui/src/actions.rs): Add `reload_specific_files` running `load_git_repo_with_notes` with targeted pathspecs.
- [`crates/diffuse-tui/src/app/sync.rs`](crates/diffuse-tui/src/app/sync.rs): Add `reload_file` / update in-place logic for single files in `AppState.output.files`.
- [`crates/diffuse-tui/src/lib.rs`](crates/diffuse-tui/src/lib.rs): Event loop poll, granular reload trigger, and interaction guards.

### Key Design & Algorithms
1. **Targeted Diff Calculation**:
   - When `FsEvent::Paths(paths)` is ready:
     - Convert relative paths to repo pathspecs.
     - Call `load_git_repo_with_notes` with `paths: &[String]`. Gitoxide diffs only those tree entries, avoiding tree-walks over the rest of the repository.
     - Run `run_pipeline_full` on the localized diff slice.
     - Update or remove the corresponding file entries in `state.output.files`:
       - If the file still has diff hunks, replace its `OutputFile` in-place, rebuild its `hunk_meta`, and bump content layout.
       - If the file has no remaining changes (e.g. reverted), remove it from `state.output.files`.
       - If a new file appeared, insert it in sorted path order.
   - When `FsEvent::Global` is received (e.g. branch switch, `git add` index update):
     - Execute full `actions::reload_diff(&mut state, &repo_root, &git_args)`.
2. **Interaction Safety Invariant**:
   - Reloading is deferred while `state.is_reload_deferred()` is true (`note_draft.is_some()`, `file_finder.is_some()`, `show_help`, or mouse drag active).
   - Once the user closes the modal or submits the note, the pending change is executed immediately.

### Verification Gate
- Unit tests in `crates/diffuse-tui/src/actions/tests.rs`:
  - `test_reload_specific_file_leaves_unaffected_files_intact`: Asserts only the modified file is updated in `output.files`.
  - `test_reload_specific_file_removes_clean_file`: Asserts reverting changes in a file removes it cleanly.
         if state.is_reload_deferred() {
             // Defer execution until active modal/draft closes
         } else {
             pending_fs_reload = false;
             let _ = actions::reload_diff(&mut state, &repo_root, &git_args);
             dirty = true;
         }
     }
     ```
3. **Flushing Deferred Reloads**:
   - In `AppState::close_modal()`, `AppState::cancel_note()`, and `AppState::submit_note()`:
     If `pending_fs_reload` is set, execute the reload immediately once the interaction ends.

### Verification Gate
- Unit tests in `crates/diffuse-tui/src/app/tests.rs`:
  - `test_reload_deferred_during_note_draft`: Asserts reload is delayed while `state.note_draft.is_some()`.
  - `test_reload_deferred_during_file_finder`: Asserts reload is delayed while `state.file_finder.is_some()`.
  - `test_deferred_reload_executes_on_modal_close`: Verifies deferred reload fires immediately once the modal is closed.

---

## 4. Phase 4 — CLI Flag & Review Target Scoping

### Objective
Expose the `--no-watch` flag in the CLI, automatically activating FS listening only when viewing working tree or staged changes.

### File Modifications
- [`crates/diffuse/src/main.rs`](crates/diffuse/src/main.rs): Add `--no-watch` CLI option and pass watch configuration to TUI runner.
- [`crates/diffuse-tui/src/lib.rs`](crates/diffuse-tui/src/lib.rs): Accept `watch: bool` in `run_interactive_tui`.

### Key Design & Algorithms
1. **CLI Flag Declaration**:
   ```rust
   /// Disable automatic filesystem watching and live working tree reloads.
   #[arg(long, help = "Disable automatic filesystem watching for live working tree changes")]
   no_watch: bool,
   ```
2. **Automatic Target Scoping**:
   - Watching is active only when all conditions hold:
     1. TUI mode is active (`!no_tui` and stdout is a terminal).
     2. `--no-watch` was not passed.
     3. Not reading unified diff from `--diff <path>` or stdin (`pipe`).
     4. Review target represents live repository changes (worktree or staged), *not* an immutable commit range (`!target.is_range()`).
   - If user reviews `diffuse main..feat` or `diffuse HEAD~1`, watching defaults to inactive because historical commits are immutable.

### Verification Gate
- CLI argument tests in `crates/diffuse/src/main_tests.rs`:
  - `test_no_watch_flag_parsed`: Asserts `--no-watch` parses correctly.
  - `test_watch_disabled_for_commit_ranges`: Asserts watcher is not spawned when `is_range()` is true.

---

## 5. Phase 5 — Status Feedback & Transient Notifications

### Objective
Provide clean, non-intrusive visual feedback in the status bar when an automatic reload occurs, keeping the user informed of external changes.

### File Modifications
- [`crates/diffuse-tui/src/app.rs`](crates/diffuse-tui/src/app.rs): Transient message emission.
- [`crates/diffuse-tui/src/view/header.rs`](crates/diffuse-tui/src/view/header.rs): Status message styling.

### Key Design & Algorithms
1. **Transient Notification**:
   - On successful automatic reload:
     `state.set_transient_status(format!("↻ live reload ({} files)", state.output.files.len()), true, Instant::now());`
   - Uses existing `poll_status_expiry` with standard 2.5-second timeout.
   - If the reload results in zero files (e.g. all changes were discarded or committed), display:
     `"↻ live reload: no remaining changes"` while retaining clean empty-state rendering.

### Verification Gate
- Unit tests in `crates/diffuse-tui/src/view/header/tests.rs`:
  - `test_status_bar_displays_reload_notice`: Asserts transient status bar formatting.

---

## 6. Phase 6 — Automated Integration Test Suite & Verification Gate

### Objective
Author an end-to-end integration test exercising actual disk file modifications and git index updates, validating that *diffuse* detects the changes and reloads seamlessly without regression.

### File Modifications
- [`crates/diffuse-tui/tests/watcher_integration.rs`](crates/diffuse-tui/tests/watcher_integration.rs): New integration test suite.

### Key Design & Algorithms
1. **Integration Test Scenario**:
   - Create temporary git repository with initial commit.
   - Spawn `AppState` on unstaged changes (`file.txt` with change 1).
   - Verify initial diff shows change 1.
   - Modify `file.txt` on disk (change 2).
   - Simulate watcher pulse / debounce trigger.
   - Assert `AppState` reloads and now presents change 2.
   - Stage the file (`git add file.txt`) and trigger staged mode; verify staged diff updates.
2. **Full Verification Gate**:
   - Run `scripts/check.sh` ensuring nightly format, zero clippy warnings, sub-second test execution budgets, and clean golden regression baselines.

---

## Quality Invariants & Verification Checklist

- [x] All unit tests execute in $< 0.5\,\text{s}$ per test.
- [x] Non-blocking invariant: FS watcher thread does not introduce latency or dropped frames to TUI event loop.
- [x] Interaction safety: active note modal or file finder is never closed or cleared by an incoming reload.
- [x] 100% branch coverage on all new watcher filter and debouncer paths.
- [x] Full `./scripts/check.sh` verification gate passes with zero warnings.
