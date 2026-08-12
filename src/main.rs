//! diffuse (bin): read a git diff + the per-file contents, run the engine
//! pipeline, emit the token streams as JSON.
//!
//! The bin is BOTH the CLI and the TS integration contract: the pi
//! extension spawns it with the review input and renders the returned rows.
//! The output schema is frozen in PLAN.md (the wire contract) — Phase 3
//! integration and the Rust TUI (later) both consume it.

use clap::Parser;
use std::io::{self, Read};

/// The diffuse diff-engine CLI.
#[derive(Parser, Debug)]
#[command(name = "diffuse", version, about = "diff-engine token-stream backend")]
struct Args {
    /// Read the diff from this file instead of stdin.
    #[arg(long)]
    diff: Option<String>,

    /// Emit the golden-walk format (acceptance tests) instead of the
    /// token-stream JSON.
    #[arg(long)]
    golden: bool,
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let input = match &args.diff {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            s
        }
    };
    let _ = input;
    // Phase 1: the engine is not yet ported — the pipeline is a stub.
    eprintln!("diffuse: the engine port is Phase 1 of PLAN.md — nothing to run yet");
    Ok(())
}
