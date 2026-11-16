//! Native Diffuse Interactive TUI Engine.

pub use diffuse_core as core;
pub use diffuse_render as render;
pub use diffuse_render::ColorExt;
pub use diffuse_syntax as syntax;

pub mod actions;
pub mod app;
pub mod command;
pub mod editor;
pub mod event;
pub mod finder;
pub mod terminal;
pub mod types;
pub mod view;
pub mod watcher;

pub use actions::{
    discard_action, reload_diff, stage_action, toggle_staged_action, unstage_action,
};
pub use app::AppState;
pub use command::TuiCommand;
pub use editor::{Editor, open_editor, resolve_editor_args};
pub use event::handle_event;
pub use finder::FileFinderState;
pub use terminal::{PollOutcome, TerminalGuard};
pub use types::{
    CursorSnapshot, FlatRow, FlatRowKind, HunkExpandIcon, HunkMeta, HunkStart, LineNumTracking,
    ModRowKey, Selection, StickyHeaderInfo, StickyHunkInfo,
};

use std::{
    collections::HashSet,
    io,
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};

use crate::core::{
    DEFAULT_EXPAND_LINES,
    streaming::GitRepoMeta,
    wire::{OutputFile, PipelineOutput},
};

/// Collect the paths that must be re-streamed after a notes change: every file
/// that currently carries notes (visible or not), plus `previously` noted paths
/// so note removal can drop a synthetic file from the view.
fn affected_notes_files(state: &AppState, previously: &[String]) -> Vec<String> {
    let mut affected: HashSet<String> = diffuse_core::notes::note_files_for_repo(
        &state.notes,
        state.repo_key.as_deref(),
        state.rev_key.as_deref(),
    )
    .into_iter()
    .collect();
    for p in previously {
        affected.insert(p.clone());
    }
    for f in &state.output.files {
        if f.counts.notes > 0 || f.counts.resolved_notes > 0 || f.counts.in_progress_notes > 0 {
            affected.insert(f.path.clone());
        }
    }
    affected.into_iter().collect()
}

/// Re-stream exactly the files affected by a notes change and reflect the
/// outcome in the status bar. Assumes the notes database in `state` already
/// contains the post-change state. Returns whether the view changed.
fn apply_targeted_notes_reload(state: &mut AppState, previously: &[String]) -> bool {
    let paths = affected_notes_files(state, previously);
    if paths.is_empty() {
        return false;
    }
    let repo_root = state.repo_root.clone();
    let git_revs = state.git_revs.clone();
    match crate::actions::reload_specific_files(state, &repo_root, &git_revs, &paths) {
        Ok(n) => {
            if n > 0 {
                state.set_status_message(
                    format!(
                        "↻ notes updated ({} file{})",
                        n,
                        if n == 1 { "" } else { "s" }
                    ),
                    true,
                );
            }
            true
        }
        Err(e) => {
            state.set_status_message(e, false);
            true
        }
    }
}

fn trigger_streaming_reload(
    state: &mut AppState,
    streaming_rx: &mut Option<Receiver<OutputFile>>,
    meta_rx: &mut Option<Receiver<Result<GitRepoMeta, String>>>,
    streaming_active: &mut bool,
    reload_snapshot: &mut Option<CursorSnapshot>,
    reload_seen_paths: &mut Option<HashSet<String>>,
    announce: bool,
) {
    let repo_root = state.repo_root.clone();
    let git_revs = state.git_revs.clone();
    let staged = state.target.is_staged();
    let _ = state.reload_notes_from_disk();
    let notes_db = state.visible_notes_db();
    let pipeline_opts = diffuse_core::PipelineOptions::default()
        .with_notes(
            Some(notes_db),
            state.repo_key.clone(),
            state.rev_key.clone(),
        )
        .with_expand_lines(state.expand_lines)
        .with_lazy_collapsed(true)
        .with_show_resolved(true);

    let (consumer, new_meta_rx) = diffuse_core::streaming::spawn_git_stream_pipeline(
        repo_root,
        git_revs,
        Vec::new(),
        staged,
        diffuse_core::git::ENGINE_CONTEXT,
        pipeline_opts,
        None,
    );

    *streaming_rx = Some(consumer.into_receiver());
    *meta_rx = Some(new_meta_rx);
    *streaming_active = true;

    // Snapshot cursor anchor and screen position to avoid jumping or blanking
    *reload_snapshot = Some(state.capture_cursor_snapshot());
    *reload_seen_paths = Some(HashSet::new());

    // Light up streaming indicator without blanking screen content
    state.is_streaming = true;
    state.record_stream_activity();
    if announce {
        state.set_status_message("↻ reloading diff...".into(), true);
    }
}

/// The terminal surface the interactive run loop drives.
///
/// [`TerminalGuard`] is the production implementation. Tests substitute a
/// synthetic session to drive [`run_session`] without a real terminal.
pub trait Session {
    /// Poll the input source, waiting at most `timeout` for an event.
    fn poll_outcome(&self, timeout: Duration) -> PollOutcome;
    /// Current terminal size as `(columns, rows)`.
    fn size(&self) -> (u16, u16);
    /// Render one frame of `state`.
    fn draw_frame(&mut self, state: &mut AppState) -> io::Result<()>;
    /// Release the terminal before running an external program.
    fn suspend(&mut self) -> io::Result<()>;
    /// Reclaim the terminal after an external program exits.
    fn resume(&mut self) -> io::Result<()>;
}

impl Session for TerminalGuard {
    fn poll_outcome(&self, timeout: Duration) -> PollOutcome {
        TerminalGuard::poll_outcome(self, timeout)
    }

    fn size(&self) -> (u16, u16) {
        TerminalGuard::size(self)
    }

    fn draw_frame(&mut self, state: &mut AppState) -> io::Result<()> {
        self.draw(|frame| state.render_ratatui_frame(frame))
    }

    fn suspend(&mut self) -> io::Result<()> {
        TerminalGuard::suspend(self)
    }

    fn resume(&mut self) -> io::Result<()> {
        TerminalGuard::resume(self)
    }
}

/// Launch and run the interactive TUI application with live streaming file
/// ingestion.
#[allow(clippy::too_many_arguments)]
pub fn run_tui(
    initial_output: PipelineOutput,
    streaming_rx: Option<Receiver<OutputFile>>,
    meta_rx: Option<Receiver<Result<GitRepoMeta, String>>>,
    target: diffuse_core::ReviewTarget,
    notes_path: PathBuf,
    expand_lines: usize,
    theme: crate::render::DiffuseTheme,
    show_resolved: bool,
    show_source: bool,
    watch: bool,
    expand_mods: Option<bool>,
    line_num_width: Option<usize>,
    side_by_side: Option<bool>,
) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    run_session(
        &mut guard,
        initial_output,
        streaming_rx,
        meta_rx,
        target,
        notes_path,
        expand_lines,
        theme,
        show_resolved,
        show_source,
        watch,
        expand_mods,
        line_num_width,
        side_by_side,
    )
}

/// Drive the interactive loop until the user quits or the session reports that
/// its terminal is gone.
#[allow(clippy::too_many_arguments)]
pub fn run_session<S: Session + ?Sized>(
    session: &mut S,
    initial_output: PipelineOutput,
    streaming_rx: Option<Receiver<OutputFile>>,
    meta_rx: Option<Receiver<Result<GitRepoMeta, String>>>,
    target: diffuse_core::ReviewTarget,
    notes_path: PathBuf,
    expand_lines: usize,
    theme: crate::render::DiffuseTheme,
    show_resolved: bool,
    show_source: bool,
    watch: bool,
    expand_mods: Option<bool>,
    line_num_width: Option<usize>,
    side_by_side: Option<bool>,
) -> io::Result<()> {
    let (term_w, _) = session.size();
    let mut state = AppState::new(
        initial_output,
        target.display_name().to_string(),
        notes_path,
        theme,
    )
    .with_target(target);
    state = state
        .with_expand_lines(expand_lines)
        .with_show_resolved_notes(show_resolved)
        .with_show_note_source(show_source)
        .with_expand_mods(expand_mods)
        .with_line_num_width(line_num_width);
    let initial_sbs =
        side_by_side.unwrap_or_else(|| state.is_wide_enough_for_side_by_side(term_w as usize));
    state = state
        .with_side_by_side(initial_sbs)
        .with_explicit_side_by_side(side_by_side);
    state.enable_async_on_demand();

    let (fs_tx, fs_rx) = mpsc::channel();
    let mut fs_collector = crate::watcher::FsChangeCollector::new();

    let mut streaming_rx = streaming_rx;
    let mut meta_rx = meta_rx;
    let mut reload_snapshot: Option<CursorSnapshot> = None;
    let mut reload_seen_paths: Option<HashSet<String>> = None;
    if streaming_rx.is_some() {
        state.is_streaming = true;
    }
    let mut streaming_active = streaming_rx.is_some();
    let mut dirty = false;
    let mut last_size = session.size();
    let mut last_dot = state.streaming_dot();

    // Present the first frame before any work whose cost scales with the
    // repository: ingesting a streamed file and registering the filesystem
    // watcher each walk content of unbounded size, and either can hold the
    // first paint back by hundreds of milliseconds in a large tree.
    let cur_size = session.size();
    state.adapt_side_by_side_to_width(cur_size.0 as usize);
    session.draw_frame(&mut state)?;

    // Registering the watch walks every directory under the repository, which
    // costs hundreds of milliseconds in a large tree. It runs on a helper
    // thread while the review is already on screen, and the loop adopts the
    // watcher as soon as it is ready.
    let (watcher_tx, watcher_rx) = mpsc::channel();
    let repo_root = state.repo_root.clone();
    let watch_repo = !state.target.is_range() && !repo_root.as_os_str().is_empty();
    let watch_notes = !state.notes_path.as_os_str().is_empty();
    let mut watcher_rx = (watch && (watch_repo || watch_notes)).then_some(watcher_rx);
    let mut _watcher: Option<crate::watcher::RepoWatcher> = None;
    if watcher_rx.is_some() {
        let repo_root = watch_repo.then_some(repo_root);
        let notes_path = watch_notes.then(|| state.notes_path.clone());
        std::thread::spawn(move || {
            let registered =
                crate::watcher::RepoWatcher::spawn_with_notes(repo_root, notes_path, fs_tx);
            let _ = watcher_tx.send(registered.ok());
        });
    }

    while !state.should_quit {
        if state.poll_on_demand() {
            dirty = true;
        }

        if _watcher.is_none()
            && let Some(rx) = watcher_rx.as_ref()
            && let Ok(adopted) = rx.try_recv()
        {
            _watcher = adopted;
            watcher_rx = None;
            // The registration walk descends the tree one directory at a time.
            // A change made in a directory that was not yet watched produced no
            // event, and the diff that started before the walk cannot contain
            // it. Rebuilding the diff now that every directory is watched
            // closes that gap; anything later is delivered as an event. The
            // rebuild is silent, because nothing the user did caused it.
            trigger_streaming_reload(
                &mut state,
                &mut streaming_rx,
                &mut meta_rx,
                &mut streaming_active,
                &mut reload_snapshot,
                &mut reload_seen_paths,
                false,
            );
            dirty = true;
        }

        if _watcher.is_some() {
            while let Ok(change) = fs_rx.try_recv() {
                fs_collector.record(change, Instant::now());
            }

            const FS_DEBOUNCE_WINDOW: Duration = Duration::from_millis(150);
            if let Some(event) = fs_collector.take_debounced(Instant::now(), FS_DEBOUNCE_WINDOW) {
                if state.is_reload_deferred() {
                    match event {
                        crate::watcher::FsEvent::Notes => {
                            fs_collector.pending_notes = true;
                        }
                        crate::watcher::FsEvent::Global => {
                            fs_collector.pending_global = true;
                        }
                        crate::watcher::FsEvent::Paths(paths) => {
                            for p in paths {
                                fs_collector.pending_files.insert(p);
                            }
                        }
                        crate::watcher::FsEvent::DiffAndNotes { diff } => {
                            fs_collector.pending_notes = true;
                            match diff {
                                crate::watcher::FsDiffEvent::Global => {
                                    fs_collector.pending_global = true;
                                }
                                crate::watcher::FsDiffEvent::Paths(paths) => {
                                    for p in paths {
                                        fs_collector.pending_files.insert(p);
                                    }
                                }
                            }
                        }
                    }
                    fs_collector.last_activity = Some(Instant::now() - FS_DEBOUNCE_WINDOW);
                } else {
                    let mut reload_notes = false;
                    let mut diff_action = None;
                    match event {
                        crate::watcher::FsEvent::Notes => {
                            reload_notes = true;
                        }
                        crate::watcher::FsEvent::Paths(paths) => {
                            diff_action = Some(crate::watcher::FsDiffEvent::Paths(paths));
                        }
                        crate::watcher::FsEvent::Global => {
                            diff_action = Some(crate::watcher::FsDiffEvent::Global);
                        }
                        crate::watcher::FsEvent::DiffAndNotes { diff } => {
                            reload_notes = true;
                            diff_action = Some(diff);
                        }
                    }

                    let old_note_files: Vec<String> = if reload_notes {
                        diffuse_core::notes::note_files_for_repo(
                            &state.notes,
                            state.repo_key.as_deref(),
                            state.rev_key.as_deref(),
                        )
                    } else {
                        Vec::new()
                    };
                    let notes_reloaded = if reload_notes {
                        state.reload_notes_from_disk()
                    } else {
                        None
                    };
                    if notes_reloaded.is_some() {
                        dirty = true;
                        // Re-stream only the files touched by added/removed
                        // notes (including unchanged files that gained or lost
                        // a note) instead of reloading the whole repository.
                        let _ = apply_targeted_notes_reload(&mut state, &old_note_files);
                    }

                    if let Some(diff) = diff_action {
                        match diff {
                            crate::watcher::FsDiffEvent::Paths(paths) => {
                                let rr = state.repo_root.clone();
                                let ga = state.git_revs.clone();
                                match actions::reload_specific_files(&mut state, &rr, &ga, &paths) {
                                    Ok(n) => {
                                        let msg = format!(
                                            "↻ live reload ({} file{})",
                                            n,
                                            if n == 1 { "" } else { "s" }
                                        );
                                        state.set_status_message(msg, true);
                                        dirty = true;
                                    }
                                    Err(e) => {
                                        state.set_status_message(e, false);
                                        dirty = true;
                                    }
                                }
                            }
                            crate::watcher::FsDiffEvent::Global => {
                                trigger_streaming_reload(
                                    &mut state,
                                    &mut streaming_rx,
                                    &mut meta_rx,
                                    &mut streaming_active,
                                    &mut reload_snapshot,
                                    &mut reload_seen_paths,
                                    true,
                                );
                                dirty = true;
                            }
                        }
                    }
                }
            }
        }

        if state.pending_force_reload {
            state.pending_force_reload = false;
            trigger_streaming_reload(
                &mut state,
                &mut streaming_rx,
                &mut meta_rx,
                &mut streaming_active,
                &mut reload_snapshot,
                &mut reload_seen_paths,
                true,
            );
            dirty = true;
        }

        if let Some(previously) = state.pending_notes_reload.take()
            && apply_targeted_notes_reload(&mut state, &previously)
        {
            dirty = true;
        }

        if let Some(ref mrx) = meta_rx {
            match mrx.try_recv() {
                Ok(Ok(meta)) => {
                    state.output.branch = meta.branch;
                    state.output.untracked_skipped = meta.untracked_skipped;
                    if meta.commit_range.is_some() {
                        state.commit_range = meta.commit_range;
                    }
                    dirty = true;
                }
                Ok(Err(err)) => {
                    state.set_status_message(err, false);
                    dirty = true;
                }
                Err(_) => {}
            }
        }

        if streaming_active && let Some(ref rx) = streaming_rx {
            loop {
                match rx.try_recv() {
                    Ok(file) => {
                        if let (Some(snapshot), Some(seen)) =
                            (&reload_snapshot, &mut reload_seen_paths)
                        {
                            seen.insert(file.path.clone());
                            state.replace_or_insert_file(file, snapshot);
                        } else {
                            state.append_file(file);
                        }
                        dirty = true;
                    }
                    Err(TryRecvError::Empty) => {
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        streaming_active = false;
                        if state.loading_files.is_empty() {
                            state.is_streaming = false;
                        }
                        if let (Some(snapshot), Some(seen)) =
                            (reload_snapshot.take(), reload_seen_paths.take())
                        {
                            state.output.files.retain(|f| seen.contains(&f.path));
                            if state.output.files.is_empty() {
                                let empty_out = diffuse_core::wire::PipelineOutput {
                                    files: Vec::new(),
                                    branch: state.output.branch.clone(),
                                    untracked_skipped: state.output.untracked_skipped.clone(),
                                };
                                state.reload(empty_out);
                            } else {
                                state.rebuild_flat_preserving_snapshot(&snapshot);
                            }
                        }
                        if state
                            .status_message
                            .as_ref()
                            .is_some_and(|(msg, _, _)| msg.starts_with("↻ reloading diff"))
                        {
                            state.status_message = None;
                        }
                        dirty = true;
                        break;
                    }
                }
            }
        }

        let is_any_streaming = streaming_active || !state.loading_files.is_empty();
        if is_any_streaming {
            let cur_dot = state.streaming_dot();
            if cur_dot != last_dot {
                dirty = true;
                last_dot = cur_dot;
            }
        }

        let size = session.size();
        if size != last_size {
            dirty = true;
            last_size = size;
            state.screen_height = size.1 as usize;
            state.terminal_width = size.0 as usize;
            state.adapt_side_by_side_to_width(size.0 as usize);
        }

        if state.poll_status_expiry(std::time::Instant::now()) {
            dirty = true;
        }

        if dirty {
            let cur_size = session.size();
            state.adapt_side_by_side_to_width(cur_size.0 as usize);
            session.draw_frame(&mut state)?;
            dirty = false;
        }

        let poll_duration = if is_any_streaming {
            Duration::from_millis(20)
        } else {
            Duration::from_millis(100)
        };

        match session.poll_outcome(poll_duration) {
            PollOutcome::Event(event) => {
                handle_event(&mut state, event);
                while let PollOutcome::Event(next_event) = session.poll_outcome(Duration::ZERO) {
                    handle_event(&mut state, next_event);
                }
                dirty = true;
            }
            PollOutcome::Idle => {}
            PollOutcome::Disconnected => {
                // The session's terminal is gone (closed pty, hung-up
                // multiplexer pane). Leaving the loop restores the terminal
                // and exits instead of spinning on a dead descriptor.
                state.should_quit = true;
            }
        }

        if let Some(req) = state.pending_editor.take() {
            let repo_root = state.repo_root.clone();
            let git_revs = state.git_revs.clone();
            let _ = session.suspend();
            let _ = editor::open_editor(&req.file, req.line, req.range, &repo_root);
            let _ = session.resume();
            let _ = actions::reload_diff(&mut state, &repo_root, &git_revs);
            dirty = true;
        }
    }

    Ok(())
}

/// Launch and run the interactive TUI application with static output.
pub fn run_interactive_tui(
    output: PipelineOutput,
    target: diffuse_core::ReviewTarget,
    notes_path: PathBuf,
    theme: crate::render::DiffuseTheme,
) -> io::Result<()> {
    run_tui(
        output,
        None,
        None,
        target,
        notes_path,
        DEFAULT_EXPAND_LINES,
        theme,
        false,
        false,
        false,
        Some(false),
        None,
        None,
    )
}
