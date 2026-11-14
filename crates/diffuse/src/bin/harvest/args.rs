//! Command-line argument parsing and exclusion pathspec mapping for harvest.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "harvest",
    version,
    about = "Harvest git diffs into golden fixtures conforming to tests/AGENTS.md",
    long_about = "harvest captures multi-file git diffs with old- and new-side snapshots from any \
git repository into frozen golden corpus directories conforming to tests/AGENTS.md.\n\n\
The recorded revision is resolved and pinned to short commit IDs or `<ref>^!`, ensuring \
deterministic, byte-identical acceptance testing across engine changes.",
    after_help = "TYPICAL USAGE & EXAMPLES:
    harvest HEAD~1..HEAD -o tests/golden/my-fixture
    harvest WORKTREE -o tests/golden/my-worktree
    harvest --exclude tests/golden -o tests/golden/my-fixture main..feature"
)]
pub struct Args {
    /// Git revision or range (e.g. HEAD~1..HEAD, main..feature, HEAD).
    /// Use the literal `WORKTREE` to diff the working tree against HEAD
    /// (for uncommitted-change fixtures).
    #[arg(
        value_name = "REVISION",
        default_value = "HEAD~1..HEAD",
        help = "Git revision or range (e.g. HEAD~1..HEAD, main..feature, WORKTREE)"
    )]
    pub revision: String,

    /// Change to DIR before doing anything (equivalent to git -C <DIR>).
    #[arg(
        short = 'C',
        value_name = "DIR",
        value_hint = clap::ValueHint::DirPath,
        action = clap::ArgAction::Append,
        help_heading = "Repository & Input",
        help = "Change to DIR before doing anything (equivalent to git -C <DIR>)"
    )]
    pub directory: Vec<PathBuf>,

    /// Target repository path (defaults to current directory)
    #[arg(
        short,
        long,
        value_name = "PATH",
        value_hint = clap::ValueHint::DirPath,
        help_heading = "Repository & Input",
        help = "Target repository path (defaults to current directory)"
    )]
    pub repo: Option<PathBuf>,

    /// Output fixture path (defaults to stdout)
    #[arg(
        short,
        long,
        value_name = "PATH",
        value_hint = clap::ValueHint::DirPath,
        help_heading = "Output",
        help = "Output fixture directory path (e.g. tests/golden/<name>)"
    )]
    pub output: Option<PathBuf>,

    /// Create a git tag in the source repository at the harvested commit.
    /// If passed without a value (e.g. `--tag`), uses the corpus directory
    /// name from `--output`.
    #[arg(
        long,
        value_name = "TAG",
        num_args = 0..=1,
        default_missing_value = "",
        help_heading = "Output",
        help = "Create a git tag in the source repository at the harvested commit (defaults to corpus name if --output is specified)"
    )]
    pub tag: Option<String>,

    /// Glob pathspecs to exclude from the harvested diff (git pathspec
    /// syntax, e.g. `tests/golden` — passed as `:(exclude)<pattern>` so a
    /// corpus never diffs the golden tree itself).
    #[arg(
        long,
        value_name = "PATTERN",
        help_heading = "Diff Options",
        help = "Pathspec patterns to exclude from harvested diff"
    )]
    pub exclude: Vec<String>,

    /// Path to review-notes database (defaults to
    /// ~/.local/state/diffuse/notes.json)
    #[arg(
        long,
        value_name = "PATH",
        value_hint = clap::ValueHint::FilePath,
        help_heading = "Notes",
        help = "Path to the review-notes database (defaults to ~/.local/state/diffuse/notes.json)"
    )]
    pub notes_file: Option<PathBuf>,
}

/// Maps exclusion patterns to git exclude-pathspecs.
pub fn exclude_pathspecs(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .map(|pattern| format!(":(exclude){pattern}"))
        .collect()
}
