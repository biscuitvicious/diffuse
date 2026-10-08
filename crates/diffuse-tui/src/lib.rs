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

pub use actions::{
    discard_action, reload_diff, stage_action, toggle_staged_action, unstage_action,
};
pub use app::AppState;
pub use command::TuiCommand;
pub use editor::{Editor, open_editor, resolve_editor_args};
pub use event::handle_event;
pub use finder::FileFinderState;
pub use terminal::TerminalGuard;
pub use types::{FlatRow, HunkMeta, HunkStart, Selection};

use std::{
    io,
    path::PathBuf,
    sync::mpsc::{Receiver, TryRecvError},
    time::Duration,
};

use crate::core::{
    DEFAULT_EXPAND_LINES,
    streaming::GitRepoMeta,
    wire::{OutputFile, PipelineOutput},
};

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
) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let target = diffuse_core::ReviewTarget::from_repo_args(
        diffuse_core::Repo::from_root(repo_root).with_name(repo.clone()),
        git_args,
        staged,
    );
    let mut state = AppState::new(initial_output, repo, notes_path, theme)
        .with_target(target)
        .with_expand_lines(expand_lines)
        .with_show_resolved_notes(show_resolved)
        .with_show_source_lines(show_source);
    state.enable_async_on_demand();
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
                    state.status_message = Some((err, false));
                    dirty = true;
                }
                Err(_) => {}
            }
        }

        if streaming_active && let Some(ref rx) = streaming_rx {
            loop {
                match rx.try_recv() {
                    Ok(file) => {
                        state.append_file(file);
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
    )
}
