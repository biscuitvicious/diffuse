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
pub use terminal::TerminalGuard;
pub use types::{
    CursorSnapshot, FlatRow, FlatRowKind, HunkMeta, HunkStart, Selection, StickyHeaderInfo,
    StickyHunkInfo,
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

fn trigger_streaming_reload(
    state: &mut AppState,
    streaming_rx: &mut Option<Receiver<OutputFile>>,
    meta_rx: &mut Option<Receiver<Result<GitRepoMeta, String>>>,
    streaming_active: &mut bool,
    reload_snapshot: &mut Option<CursorSnapshot>,
    reload_seen_paths: &mut Option<HashSet<String>>,
    dirty: &mut bool,
) {
    let repo_root = state.repo_root.clone();
    let git_args = state.git_args.clone();
    let staged = state.target.is_staged();
    let pipeline_opts = diffuse_core::PipelineOptions::default()
        .with_notes(Some(state.notes.clone()), Some(state.notes_key.clone()))
        .with_expand_lines(state.expand_lines)
        .with_lazy_collapsed(true)
        .with_show_resolved(true);

    let (consumer, new_meta_rx) = diffuse_core::streaming::spawn_git_stream_pipeline(
        repo_root,
        git_args,
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
    state.set_status_message("↻ reloading diff...".into(), true);
    *dirty = true;
}

/// Launch and run the interactive TUI application with live streaming file
/// ingestion.
#[allow(clippy::too_many_arguments)]
pub fn run_tui(
    initial_output: PipelineOutput,
    streaming_rx: Option<Receiver<OutputFile>>,
    meta_rx: Option<Receiver<Result<GitRepoMeta, String>>>,
    repo: String,
    notes_path: PathBuf,
    repo_root: PathBuf,
    git_args: Vec<String>,
    staged: bool,
    expand_lines: usize,
    theme: crate::render::DiffuseTheme,
    show_resolved: bool,
    show_source: bool,
    watch: bool,
) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let target = diffuse_core::ReviewTarget::from_repo_args(
        diffuse_core::Repo::from_root(repo_root.clone()).with_name(repo.clone()),
        git_args.clone(),
        staged,
    );
    let mut state = AppState::new(initial_output, repo, notes_path, theme)
        .with_target(target)
        .with_repo_context(repo_root.clone(), git_args, staged)
        .with_expand_lines(expand_lines)
        .with_show_resolved_notes(show_resolved)
        .with_show_source_lines(show_source);
    state.enable_async_on_demand();

    let (fs_tx, fs_rx) = mpsc::channel();
    let _watcher = if watch && !state.target.is_range() && !repo_root.as_os_str().is_empty() {
        crate::watcher::RepoWatcher::spawn(repo_root, fs_tx).ok()
    } else {
        None
    };
    let mut fs_collector = crate::watcher::FsChangeCollector::new();

    let mut streaming_rx = streaming_rx;
    let mut meta_rx = meta_rx;
    let mut reload_snapshot: Option<CursorSnapshot> = None;
    let mut reload_seen_paths: Option<HashSet<String>> = None;
    if streaming_rx.is_some() {
        state.is_streaming = true;
    }
    let mut streaming_active = streaming_rx.is_some();
    let mut dirty = true;
    let mut last_size = guard.size();
    let mut last_dot = state.streaming_dot();

    while !state.should_quit {
        if state.poll_on_demand() {
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
                        crate::watcher::FsEvent::Global => {
                            fs_collector.pending_global = true;
                            fs_collector.last_activity = Some(Instant::now() - FS_DEBOUNCE_WINDOW);
                        }
                        crate::watcher::FsEvent::Paths(paths) => {
                            for p in paths {
                                fs_collector.pending_files.insert(p);
                            }
                            fs_collector.last_activity = Some(Instant::now() - FS_DEBOUNCE_WINDOW);
                        }
                    }
                } else {
                    match event {
                        crate::watcher::FsEvent::Paths(paths) => {
                            let rr = state.repo_root.clone();
                            let ga = state.git_args.clone();
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
                        crate::watcher::FsEvent::Global => {
                            trigger_streaming_reload(
                                &mut state,
                                &mut streaming_rx,
                                &mut meta_rx,
                                &mut streaming_active,
                                &mut reload_snapshot,
                                &mut reload_seen_paths,
                                &mut dirty,
                            );
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
                &mut dirty,
            );
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
                                state.should_quit = true;
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

        let size = guard.size();
        if size != last_size {
            dirty = true;
            last_size = size;
        }

        if state.poll_status_expiry(std::time::Instant::now()) {
            dirty = true;
        }

        if dirty {
            guard.draw(|f| state.render_ratatui_frame(f))?;
            dirty = false;
        }

        let poll_duration = if is_any_streaming {
            Duration::from_millis(20)
        } else {
            Duration::from_millis(100)
        };

        if let Some(event) = guard.poll_event(poll_duration)? {
            handle_event(&mut state, event);
            while let Ok(Some(next_event)) = guard.poll_event(Duration::ZERO) {
                handle_event(&mut state, next_event);
            }
            dirty = true;
        }

        if let Some(req) = state.pending_editor.take() {
            let repo_root = state.repo_root.clone();
            let git_args = state.git_args.clone();
            let _ = guard.suspend();
            let _ = editor::open_editor(&req.file, req.line, req.range, &repo_root);
            let _ = guard.resume();
            let _ = actions::reload_diff(&mut state, &repo_root, &git_args);
            dirty = true;
        }
    }

    Ok(())
}

/// Launch and run the interactive TUI application with static output.
pub fn run_interactive_tui(
    output: PipelineOutput,
    repo: String,
    notes_path: PathBuf,
    repo_root: PathBuf,
    git_args: Vec<String>,
    staged: bool,
    theme: crate::render::DiffuseTheme,
) -> io::Result<()> {
    run_tui(
        output,
        None,
        None,
        repo,
        notes_path,
        repo_root,
        git_args,
        staged,
        DEFAULT_EXPAND_LINES,
        theme,
        false,
        false,
        false,
    )
}
