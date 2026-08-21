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

use std::{io, path::PathBuf, time::Duration};

use diffuse_core::wire::PipelineOutput;

/// Launch and run the interactive TUI application.
pub fn run_interactive_tui(
    output: PipelineOutput,
    repo: String,
    notes_path: PathBuf,
    repo_root: PathBuf,
    git_args: Vec<String>,
) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let mut state = AppState::new(output, repo, notes_path).with_repo_context(repo_root, git_args);
    let mut dirty = true;
    let mut last_size = guard.size();

    while !state.should_quit {
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

        if let Some(event) = guard.poll_event(Duration::from_millis(100))? {
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
