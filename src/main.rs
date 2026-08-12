//! diffuse (bin): read a git diff + the per-file contents (the fixture
//! shape), run the engine pipeline, emit the token-stream JSON.
//!
//! The bin is BOTH the CLI and the TS integration contract: the pi
//! extension spawns it with the review input and renders the returned
//! rows. The output schema is frozen in PLAN.md §2 (the wire contract).

use clap::Parser;
use diffuse::bin_pipeline::run_pipeline;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Read};

/// The diffuse diff-engine CLI.
#[derive(Parser, Debug)]
#[command(name = "diffuse", version, about = "diff-engine token-stream backend")]
struct Args {
    /// Read the diff from this file instead of stdin.
    #[arg(long)]
    diff: Option<String>,
}

#[derive(Deserialize)]
struct Input {
    diff: String,
    contents: HashMap<String, String>,
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let raw = match &args.diff {
        Some(path) => std::fs::read_to_string(path)?,
        None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s)?;
            s
        }
    };
    // The input is the fixture shape: {"diff": ..., "contents": {...}}.
    let input: Input = serde_json::from_str(&raw).unwrap_or_else(|_| Input {
        diff: raw,
        contents: HashMap::new(),
    });
    let output = run_pipeline(&input.diff, &input.contents);
    println!("{}", serde_json::to_string(&output).expect("serialize"));
    Ok(())
}
