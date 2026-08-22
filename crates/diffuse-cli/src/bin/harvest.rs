//! diffuse-harvest: Automated fixture generator and dataset extractor.
//!
//! Harvests multi-file git diffs with old and new side snapshots from any
//! repository and emits standard `.json` fixtures conforming to
//! tests/AGENTS.md.

use std::{collections::HashMap, env, fs, path::PathBuf, process};

use clap::Parser;
use diffuse_core::git::load_git_repo;
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(
    name = "diffuse-harvest",
    about = "Harvest git diffs into golden fixtures"
)]
struct Args {
    /// Git revision or range (e.g. HEAD~1..HEAD, main..feature, HEAD)
    #[arg(default_value = "HEAD~1..HEAD")]
    revision: String,

    /// Target repository path (defaults to current directory)
    #[arg(short, long)]
    repo: Option<PathBuf>,

    /// Output fixture path (defaults to stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Lines of context around changes
    #[arg(short, long, default_value_t = 6)]
    context: usize,
}

#[derive(Serialize)]
struct FixtureOutput {
    diff: String,
    contents: HashMap<String, String>,
    old_contents: HashMap<String, String>,
}

fn main() {
    let args = Args::parse();
    let repo_root = args
        .repo
        .unwrap_or_else(|| env::current_dir().expect("valid current dir"));

    let base_args = vec![args.revision.clone()];
    let git_res = match load_git_repo(&repo_root, &base_args, &[], false, args.context) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("diffuse-harvest: {e}");
            process::exit(1);
        }
    };

    let fixture = FixtureOutput {
        diff: git_res.diff_text,
        contents: git_res.contents,
        old_contents: git_res.old_contents,
    };

    let json = serde_json::to_string_pretty(&fixture).expect("serialize fixture");

    if let Some(out_path) = args.output {
        fs::write(&out_path, json).expect("write output file");
        eprintln!(
            "Harvested {} files to {}",
            fixture.contents.len(),
            out_path.display()
        );
    } else {
        println!("{json}");
    }
}
