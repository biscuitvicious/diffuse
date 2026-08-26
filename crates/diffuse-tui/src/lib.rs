//! Native Diffuse Interactive TUI Engine.

pub mod actions;
pub mod app;
pub mod command;
pub mod editor;
pub mod event;
pub mod finder;
pub mod terminal;
pub mod types;
pub mod view;

pub use actions::{discard_action, reload_diff, stage_action, unstage_action};
pub use app::AppState;
pub use command::TuiCommand;
pub use editor::{open_editor, resolve_editor_args};
pub use event::handle_event;
pub use finder::FileFinderState;
pub use terminal::TerminalGuard;
pub use types::{FlatRow, HunkMeta, HunkStart, Selection};

use std::{io, path::PathBuf, sync::mpsc::Receiver, time::Duration};

use diffuse_core::wire::{OutputFile, PipelineOutput};

/// Launch and run the interactive TUI application with live streaming file
/// ingestion.
#[allow(clippy::too_many_arguments)]
pub fn run_streaming_interactive_tui(
    initial_output: PipelineOutput,
    streaming_rx: Option<Receiver<OutputFile>>,
    meta_rx: Option<Receiver<Result<diffuse_core::streaming::GitRepoMeta, String>>>,
    repo: String,
    notes_path: PathBuf,
    repo_root: PathBuf,
    git_args: Vec<String>,
    expand_lines: usize,
) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let mut state = AppState::new(initial_output, repo, notes_path)
        .with_repo_context(repo_root, git_args)
        .with_expand_lines(expand_lines);
    if streaming_rx.is_some() {
        state.is_streaming = true;
    }
    let mut streaming_active = streaming_rx.is_some();
    let mut dirty = true;
    let mut last_size = guard.size();

    while !state.should_quit {
        if let Some(ref mrx) = meta_rx {
            match mrx.try_recv() {
                Ok(Ok(meta)) => {
                    state.output.branch = meta.branch;
                    state.output.untracked_skipped = meta.untracked_skipped;
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
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        state.is_streaming = false;
                        streaming_active = false;
                        dirty = true;
                        break;
                    }
                }
            }
        }

        let size = guard.size();
        if size != last_size {
            dirty = true;
            last_size = size;
        }

        if dirty {
            let (width, height) = size;
            let frame = state.render_frame(width as usize, height as usize);
            guard.write_frame(&frame)?;
            dirty = false;
        }

        let poll_duration = if streaming_active {
            Duration::from_millis(20)
        } else {
            Duration::from_millis(100)
        };

        if let Some(event) = guard.poll_event(poll_duration)? {
            handle_event(&mut state, event);
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
) -> io::Result<()> {
    run_streaming_interactive_tui(
        output,
        None,
        None,
        repo,
        notes_path,
        repo_root,
        git_args,
        diffuse_core::DEFAULT_EXPAND_LINES,
    )
}
