//! Native Diffuse Interactive TUI Engine.

pub mod app;
pub mod event;
pub mod terminal;

pub use app::AppState;
pub use event::handle_event;
pub use terminal::TerminalGuard;

use crate::bin_pipeline::PipelineOutput;
use std::{io, path::PathBuf, time::Duration};

/// Launch and run the interactive TUI application.
pub fn run_interactive_tui(
    output: PipelineOutput,
    repo: String,
    notes_path: PathBuf,
) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let mut state = AppState::new(output, repo, notes_path);

    while !state.should_quit {
        let (width, height) = guard.size();
        let frame = state.render_frame(width as usize, height as usize);
        guard.write_frame(&frame)?;

        if let Some(event) = guard.poll_event(Duration::from_millis(50))? {
            handle_event(&mut state, event);
        }
    }

    Ok(())
}
