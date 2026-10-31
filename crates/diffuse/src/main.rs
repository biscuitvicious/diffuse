//! diffuse (bin): read a git diff + the per-file contents (the fixture
//! shape), run the engine pipeline, emit the token-stream JSON.
//!
//! The bin is BOTH the CLI and the TS integration contract: the pi
//! extension spawns it with the review input and renders the returned
//! rows. The output schema is frozen in PLAN.md §2 (the wire contract).

use std::{env, io, process};

use clap::Parser;

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
pub use cli::{args::SubCommand, dispatch::route_mode, theme::resolve_theme};

fn main() -> io::Result<()> {
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
