//! diffuse (bin): read a git diff + the per-file contents (the fixture
//! shape), run the engine pipeline, emit the token-stream JSON.
//!
//! The bin is BOTH the CLI and the TS integration contract: the pi
//! extension spawns it with the review input and renders the returned
//! rows. The output schema is frozen in PLAN.md §2 (the wire contract).

use std::{env, io, process};

use clap::{CommandFactory as _, Parser};

use diffuse::init_logging;

mod cli;
mod notes_cli;

#[path = "main_tests.rs"]
#[cfg(test)]
mod tests;

pub use cli::{
    args::{Args, NotesAction, NotesArgs, RevisionArgs},
    theme::init_color_mode,
};

#[cfg(test)]
pub use cli::{
    args::SubCommand,
    dispatch::{is_terminal_interactive, route_mode},
    theme::{ThemeTarget, resolve_theme},
};

fn main() -> io::Result<()> {
    clap_complete::CompleteEnv::with_factory(Args::command).complete();

    let _ = init_logging();
    let args = Args::parse();

    for dir in &args.directory {
        if let Err(e) = env::set_current_dir(dir) {
            eprintln!(
                "diffuse: cannot change directory to '{}': {e}",
                dir.display()
            );
            process::exit(1);
        }
    }

    cli::dispatch::dispatch(&args)
}
