//! diffuse (bin): read a git diff + the per-file contents (the fixture
//! shape), run the engine pipeline, emit the token-stream JSON.
//!
//! The bin is BOTH the CLI and the TS integration contract: the pi
//! extension spawns it with the review input and renders the returned
//! rows. The output schema is frozen in PLAN.md §2 (the wire contract).

use clap::Parser;
use diffuse::bin_pipeline::run_pipeline_with_notes;
use diffuse::git::load_git_repo;
use diffuse::notes::{NotesDB, load_notes};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{self, Read};
use std::path::Path;

/// The diffuse diff-engine CLI.
#[derive(Parser, Debug)]
#[command(name = "diffuse", version, about = "diff-engine token-stream backend")]
struct Args {
    /// Read the diff from this file instead of stdin.
    #[arg(long)]
    diff: Option<String>,
    /// Path to notes database file (e.g. ~/.pi/review/notes.json).
    #[arg(long)]
    notes_file: Option<String>,
    /// Repository name key for notes lookup.
    #[arg(long)]
    repo: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    #[serde(default)]
    diff: String,
    #[serde(default)]
    contents: HashMap<String, String>,
    #[serde(default)]
    repo_root: Option<String>,
    #[serde(default)]
    base_args: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    cached: bool,
    #[serde(default)]
    context: Option<usize>,
    #[serde(default)]
    notes: Option<NotesDB>,
    #[serde(default)]
    notes_file: Option<String>,
    #[serde(default)]
    repo: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum InputPayload {
    Batch(Vec<Input>),
    Single(Box<Input>),
}

fn process_input(input: Input, args: &Args) -> diffuse::bin_pipeline::PipelineOutput {
    let notes_file_path = args.notes_file.clone().or(input.notes_file);
    let loaded_db = notes_file_path.as_ref().map(|p| load_notes(Path::new(p)));
    let db_ref = loaded_db.as_ref().or(input.notes.as_ref());
    let repo_ref = args.repo.as_deref().or(input.repo.as_deref());

    if let Some(ref repo_root) = input.repo_root {
        let context = input.context.unwrap_or(6);
        if let Ok(git_res) = load_git_repo(
            Path::new(repo_root),
            &input.base_args,
            &input.paths,
            input.cached,
            context,
        ) {
            let mut out =
                run_pipeline_with_notes(&git_res.diff_text, &git_res.contents, db_ref, repo_ref);
            out.branch = git_res.branch;
            out.untracked_skipped = git_res.untracked_skipped;
            return out;
        }
    }

    run_pipeline_with_notes(&input.diff, &input.contents, db_ref, repo_ref)
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

    let payload: InputPayload = serde_json::from_str(&raw).unwrap_or_else(|_| {
        InputPayload::Single(Box::new(Input {
            diff: raw,
            contents: HashMap::new(),
            repo_root: None,
            base_args: Vec::new(),
            paths: Vec::new(),
            cached: false,
            context: None,
            notes: None,
            notes_file: None,
            repo: None,
        }))
    });

    match payload {
        InputPayload::Batch(inputs) => {
            let outputs: Vec<_> = inputs
                .into_iter()
                .map(|i| process_input(i, &args))
                .collect();
            println!("{}", serde_json::to_string(&outputs).expect("serialize"));
        }
        InputPayload::Single(input) => {
            let output = process_input(*input, &args);
            println!("{}", serde_json::to_string(&output).expect("serialize"));
        }
    }
    Ok(())
}
