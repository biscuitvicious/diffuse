//! diffuse (bin): read a git diff + the per-file contents (the fixture
//! shape), run the engine pipeline, emit the token-stream JSON.
//!
//! The bin is BOTH the CLI and the TS integration contract: the pi
//! extension spawns it with the review input and renders the returned
//! rows. The output schema is frozen in PLAN.md §2 (the wire contract).

use std::{
    collections::HashMap,
    env, fs,
    io::{self, IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process,
};

use clap::{Parser, Subcommand};
use serde::Deserialize;

use diffuse::{
    git::{is_inside_work_tree, load_git_repo_with_notes},
    init_logging,
    notes::{
        remove_notes_in_range, reply_to_note_text, set_note_status_in_repo, update_note_in_db,
    },
    prelude::*,
    streaming::{self, spawn_git_stream_pipeline, stream_diff_pipeline},
    wire::emit_file_jsonl,
};

mod notes_cli;

/// The diffuse diff-engine CLI.
#[derive(Parser, Debug)]
#[command(
    name = "diffuse",
    version,
    about = "Structured diff analysis engine, interactive reviewer & review notes tracker",
    long_about = "diffuse is a structured diff analysis engine and terminal review tool.\n\n\
By default, running `diffuse` in a git repository launches the interactive terminal \
reviewer (TUI). When piping input (`git diff | diffuse`) or specifying output flags \
(`--json`, `--jsonl`, `-m`, `-p`), diffuse processes the diff non-interactively.",
    after_help = "TYPICAL USAGE & EXAMPLES:
    diffuse                         Launch interactive TUI on uncommitted changes
    diffuse -s / --staged           Review staged index changes against HEAD
    diffuse HEAD~1                  Review the previous commit
    diffuse main..feature           Review commit range
    diffuse path/to/file.rs         Review specific file or pathspec
    diffuse -p src/                 Print colorized diff to terminal (pager mode)
    diffuse -m > review.md          Export diff and notes as GitHub-Flavored Markdown
    diffuse --json                  Emit structured token-stream wire JSON
    git diff | diffuse              Process arbitrary diff piped into stdin
    diffuse notes                   List active review notes for current repo

ENVIRONMENT VARIABLES:
    DIFFUSE_THEME    Default color theme: auto (default), dark/default, light
    NO_COLOR         Disable ANSI colorization (equivalent to --no-color)
    COLORTERM        Terminal 24-bit truecolor support flag"
)]
struct Args {
    // Repository & Input
    /// Change to DIR before doing anything (equivalent to git -C <DIR>).
    #[arg(
        short = 'C',
        value_name = "DIR",
        value_hint = clap::ValueHint::DirPath,
        action = clap::ArgAction::Append,
        global = true,
        help_heading = "Repository & Input",
        help = "Change working directory before executing (equivalent to git -C <DIR>)",
        long_help = "Change working directory to DIR before locating git repositories, \
reading diff inputs, or loading source files. Can be specified multiple times."
    )]
    directory: Vec<PathBuf>,

    /// Read the diff from this file instead of stdin.
    #[arg(
        long,
        value_name = "FILE",
        value_hint = clap::ValueHint::FilePath,
        global = true,
        help_heading = "Repository & Input",
        help = "Read unified diff from file instead of stdin",
        long_help = "Read the unified diff from the specified file path instead of \
spawning git or reading standard input."
    )]
    diff: Option<String>,

    // Review Notes
    /// Path to notes database file (default:
    /// ~/.local/state/diffuse/notes.json).
    #[arg(
        long,
        value_name = "PATH",
        value_hint = clap::ValueHint::FilePath,
        global = true,
        help_heading = "Review Notes",
        help = "Path to notes database file (default: ~/.local/state/diffuse/notes.json)",
        long_help = "Path to the JSON review notes database file. \
Defaults to $XDG_STATE_HOME/diffuse/notes.json (or ~/.local/state/diffuse/notes.json)."
    )]
    notes_file: Option<String>,

    /// Repository name key for notes lookup.
    #[arg(
        long,
        value_name = "NAME",
        global = true,
        help_heading = "Review Notes",
        help = "Repository name key for notes lookup",
        long_help = "Repository key used to store and look up notes in the notes database. \
Defaults to the root directory name of the detected git repository."
    )]
    repo: Option<String>,

    /// Include resolved notes in diff, JSON, or Markdown output.
    #[arg(
        long,
        global = true,
        help_heading = "Review Notes",
        help = "Include resolved notes in output (hidden by default)",
        long_help = "Include resolved review notes (marked with ✅) in diff, JSON, or \
Markdown output. By default, only active open (💬) and in-progress (🔄) notes are displayed."
    )]
    resolved: bool,

    /// Include all notes (including resolved notes) in diff, JSON, or Markdown
    /// output.
    #[arg(
        short = 'a',
        long = "all",
        alias = "all-notes",
        global = true,
        help_heading = "Review Notes",
        help = "Include all notes (open, in-progress, resolved)",
        long_help = "Include all review notes regardless of status in diff, JSON, or \
Markdown output."
    )]
    all: bool,

    /// Show original source rows captured when creating notes in diff, JSON,
    /// or Markdown output.
    #[arg(
        long,
        global = true,
        help_heading = "Review Notes",
        help = "Show each note's original source rows alongside notes",
        long_help = "Display the original source rows snapshot captured when each review \
note was created, preserving original context across code refactors."
    )]
    show_source: bool,

    // Diff & Performance Tuning
    /// Unified diff context lines (default: 4).
    #[arg(
        short = 'U',
        long,
        value_name = "LINES",
        default_value_t = DEFAULT_CONTEXT_LINES,
        global = true,
        help_heading = "Diff & Performance Tuning",
        help = "Unified diff context lines around changes (default: 4)",
        long_help = "Number of leading and trailing context lines surrounding each changed \
hunk in the diff."
    )]
    context: usize,

    /// Number of context lines to expand when activating a hidden gap separator
    /// (default: 6).
    #[arg(
        long,
        value_name = "LINES",
        default_value_t = DEFAULT_EXPAND_LINES,
        global = true,
        help_heading = "Diff & Performance Tuning",
        help = "Context lines to reveal when expanding a hidden gap (default: 6)",
        long_help = "Number of additional context lines to expand upward or downward \
when activating a hidden gap separator in the TUI reviewer."
    )]
    expand_lines: usize,

    /// Maximum quadratic token-product cap for subline diffing (default:
    /// 250000).
    #[arg(
        long,
        hide = true,
        default_value_t = DEFAULT_TOKEN_PRODUCT_CAP,
        global = true
    )]
    token_product_cap: usize,

    /// Maximum cell budget for cluster DP alignment (default: 10000000).
    #[arg(long, hide = true, default_value_t = DEFAULT_CLUSTER_BUDGET, global = true)]
    cluster_budget: usize,

    /// Minimum corridor half-bandwidth for cluster DP alignment (default: 64).
    #[arg(
        long,
        hide = true,
        default_value_t = DEFAULT_CLUSTER_BANDWIDTH_MIN,
        global = true
    )]
    cluster_bandwidth_min: usize,

    /// Bandwidth slack added to cluster length difference (default: 32).
    #[arg(
        long,
        hide = true,
        default_value_t = DEFAULT_CLUSTER_BANDWIDTH_SLACK,
        global = true
    )]
    cluster_bandwidth_slack: usize,

    /// Number of worker threads (default: max(1, #CPUs / 2)).
    #[arg(
        short = 'j',
        long = "jobs",
        alias = "threads",
        alias = "workers",
        value_name = "COUNT",
        global = true,
        help_heading = "Diff & Performance Tuning",
        help = "Number of worker threads (default: max(1, #CPUs / 2))",
        long_help = "Thread pool size for parallel file processing and subline LCS alignment."
    )]
    jobs: Option<usize>,

    // Execution Modes
    /// Run in non-interactive pager mode (print colored diff to terminal).
    #[arg(
        short = 'p',
        long,
        global = true,
        help_heading = "Execution Modes",
        help = "Run in non-interactive pager mode (print colored diff to terminal)",
        long_help = "Run in non-interactive pager mode, printing the colorized diff \
directly to stdout with full token alignments and review notes."
    )]
    pager: bool,

    /// Disable interactive TUI mode (alias for --pager).
    #[arg(
        long,
        global = true,
        help_heading = "Execution Modes",
        help = "Disable interactive TUI mode (alias for --pager)",
        long_help = "Disable interactive TUI mode and run as a non-interactive CLI pager."
    )]
    no_tui: bool,

    /// Emit raw colorized diff (flat green/red, no subword highlighting).
    /// Equivalent to the TUI's R (raw diff) toggle.
    #[arg(
        long,
        global = true,
        help_heading = "Execution Modes",
        help = "Emit raw colorized diff without subword token alignment",
        long_help = "Emit raw colorized diff (flat red deletions, green additions) without \
subword token alignment, relocation detection, or phrase anchoring."
    )]
    raw: bool,

    /// View staged (cached) index changes against HEAD.
    #[arg(
        short = 's',
        long,
        alias = "cached",
        global = true,
        help_heading = "Execution Modes",
        help = "View staged (cached) index changes against HEAD",
        long_help = "Compare the git index (staged changes) against HEAD, instead of \
unstaged working tree changes."
    )]
    staged: bool,

    /// Disable automatic filesystem watching for live working tree changes.
    #[arg(
        long,
        global = true,
        help_heading = "Execution Modes",
        help = "Disable automatic filesystem watching for live working tree changes",
        long_help = "Disable the background filesystem watcher. By default, diffuse continuously \
monitors the repository for changes and refreshes the display automatically."
    )]
    no_watch: bool,

    // Output Formats
    /// Emit raw token-stream JSON (for machine/extension integration).
    #[arg(
        long,
        global = true,
        help_heading = "Output Formats",
        help = "Emit raw token-stream JSON document",
        long_help = "Emit the full diff analysis as a single structured token-stream JSON \
document conforming to the wire contract in diffuse-core::wire."
    )]
    json: bool,

    /// Emit raw token-stream JSON Lines (one JSON object per file as it is
    /// processed).
    #[arg(
        long,
        global = true,
        help_heading = "Output Formats",
        help = "Emit streaming token-stream JSON Lines (one file per line)",
        long_help = "Emit streaming JSON Lines (JSONL), serializing each file's structured \
token stream as soon as analysis completes."
    )]
    jsonl: bool,

    /// Emit diff in GitHub-Flavored Markdown (GFM) format.
    #[arg(
        short = 'm',
        long,
        global = true,
        help_heading = "Output Formats",
        help = "Emit diff in GitHub-Flavored Markdown (GFM) format",
        long_help = "Emit the diff and review notes as formatted GitHub-Flavored Markdown, \
featuring summary count badges, syntax-highlighted code blocks, and review notes callouts."
    )]
    markdown: bool,

    // Theme & Styling
    /// Control ANSI colorization ('always', 'never', or 'auto').
    #[arg(
        long,
        value_name = "WHEN",
        default_value = "auto",
        global = true,
        help_heading = "Theme & Styling",
        help = "Control ANSI colorization: 'always', 'never', or 'auto'",
        long_help = "Control whether ANSI escape sequences are emitted for terminal colors. \
'auto' (default) enables colorization when stdout is a terminal; 'always' forces color; \
'never' strips all escape codes."
    )]
    color: String,

    /// Disable ANSI colorization (equivalent to --color=never).
    #[arg(
        long,
        global = true,
        help_heading = "Theme & Styling",
        help = "Disable ANSI colorization (equivalent to --color=never)",
        long_help = "Disable all ANSI escape sequences, emitting plain text output."
    )]
    no_color: bool,

    /// Color theme ('auto', 'default'/'dark', 'light'). Defaults to 'auto',
    /// selecting based on terminal background mode. Overrides `DIFFUSE_THEME`.
    #[arg(
        long,
        value_name = "NAME",
        global = true,
        help_heading = "Theme & Styling",
        help = "Color theme: 'auto', 'default'/'dark', 'light'",
        long_help = "Color palette theme for terminal rendering. \
'auto' (default) detects the background via COLORFGBG / environment, and via \
an OSC 11 terminal query in interactive TUI mode only (one-shot pager/pipe \
modes never query, so nothing can leak into the shell); \
'default'/'dark' applies deep dark theme; 'light' applies high-contrast light theme. \
Overrides the DIFFUSE_THEME environment variable."
    )]
    theme: Option<String>,

    #[command(subcommand)]
    command: Option<SubCommand>,

    #[command(flatten)]
    revision: RevisionArgs,
}

/// Reusable git diff arguments (revisions, commit ranges, or file pathspecs).
#[derive(clap::Args, Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionArgs {
    /// Git revision arguments (revisions or commit ranges)
    #[arg(
        value_name = "GIT_REV",
        help_heading = "Revision & Path Controls",
        help = "Git revision arguments (revisions or commit ranges)",
        long_help = "Arguments specifying commits (e.g. HEAD~1) or ranges (e.g. main..feat)."
    )]
    pub git_revs: Vec<String>,

    /// File pathspecs scoping the diff (after `--`)
    #[arg(
        value_name = "PATHSPEC",
        last = true,
        help_heading = "Revision & Path Controls",
        help = "File pathspecs scoping the diff (after `--`)",
        long_help = "File pathspecs (e.g. src/lib.rs) scoping the review. Everything after `--` \
is treated as a pathspec, mirroring `git diff [revs] -- [paths]`."
    )]
    pub pathspecs: Vec<String>,
}

impl RevisionArgs {
    pub fn new(git_revs: Vec<String>) -> Self {
        Self {
            git_revs,
            pathspecs: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.git_revs.is_empty() && self.pathspecs.is_empty()
    }

    pub fn extend(&mut self, other: &RevisionArgs) {
        self.git_revs.extend(other.git_revs.iter().cloned());
        self.pathspecs.extend(other.pathspecs.iter().cloned());
    }

    pub fn combined(&self, other: &RevisionArgs) -> Self {
        let mut res = self.clone();
        res.extend(other);
        res
    }
}

#[derive(Subcommand, Debug)]
enum SubCommand {
    /// Interactive diff reviewer (default when invoked in a repository).
    #[command(
        about = "Interactive terminal diff reviewer (default in git repository)",
        long_about = "Launch the interactive Ratatui-based terminal diff reviewer. Features include:\n\
- Unified diff view with token-level syntax alignments\n\
- Vim-style motion navigation (j/k, Ctrl+d/u, gg/G, w/b)\n\
- Inline review note drafting ('n') and viewing\n\
- Note status cycling: 'x' (Open 💬 -> InProgress 🔄 -> Resolved ✅)\n\
- Resolved notes toggle: 'z' (show/hide resolved notes)\n\
- Original source lines toggle: 'S'\n\
- File tree navigator and fuzzy jump ('o')\n\
- Raw diff mode toggle ('R') and line wrapping toggle ('W')",
        after_help = "TYPICAL USAGE:
    diffuse review                  Review unstaged working tree changes
    diffuse review --staged         Review staged index changes
    diffuse review HEAD~1           Review the previous commit
    diffuse review main..feat       Review branch differences
    diffuse review src/crates/      Scope review to specific path"
    )]
    Review {
        #[command(flatten)]
        revision: RevisionArgs,
    },

    /// Parse a diff and emit structured token-stream JSON, JSON Lines, or
    /// Markdown.
    #[command(
        about = "Parse diff and emit structured token-stream JSON, JSONL, or Markdown",
        long_about = "Parses a unified diff and executes the full diffuse analysis pipeline \
(hunk pairing, shift minimization, relocation detection, token alignment, and \
review notes integration). Emits the structured result as JSON, JSON Lines, or Markdown.",
        after_help = "TYPICAL USAGE & PIPELINES:
    git diff | diffuse parse --json
    git diff | diffuse parse --jsonl
    git diff | diffuse parse -m > diff_summary.md
    diffuse parse --diff changes.patch --show-source -m"
    )]
    Parse {
        /// Read raw diff or JSON payload from file instead of stdin.
        #[arg(
            long,
            value_name = "FILE",
            value_hint = clap::ValueHint::FilePath,
            help_heading = "Input",
            help = "Read raw diff from file instead of stdin",
            long_help = "Read the raw unified diff from the specified file path instead of standard input."
        )]
        diff: Option<String>,

        /// Emit single JSON document (default).
        #[arg(
            long,
            help_heading = "Format Options",
            help = "Emit single JSON document adhering to the wire contract",
            long_help = "Emit the full diff analysis as a single structured token-stream JSON document."
        )]
        json: bool,

        /// Emit JSON Lines.
        #[arg(
            long,
            help_heading = "Format Options",
            help = "Emit streaming JSON Lines (one JSON object per file)",
            long_help = "Emit streaming JSON Lines, outputting one JSON object per analyzed file."
        )]
        jsonl: bool,

        /// Emit GitHub-Flavored Markdown.
        #[arg(
            short = 'm',
            long,
            help_heading = "Format Options",
            help = "Emit formatted GitHub-Flavored Markdown",
            long_help = "Emit the diff analysis and review notes formatted as GitHub-Flavored Markdown."
        )]
        markdown: bool,

        /// Emit raw colorized diff (flat green/red, no subword highlighting).
        #[arg(
            long,
            help_heading = "Format Options",
            help = "Emit raw colorized diff without subword token alignments",
            long_help = "Emit raw colorized diff without subword token alignment or relocation detection."
        )]
        raw: bool,

        /// Include resolved notes in output.
        #[arg(
            long,
            help_heading = "Review Notes",
            help = "Include resolved notes in output (hidden by default)",
            long_help = "Include resolved review notes (marked with ✅) in output."
        )]
        resolved: bool,

        /// Include all notes (including resolved notes) in output.
        #[arg(
            short = 'a',
            long,
            help_heading = "Review Notes",
            help = "Include all notes regardless of status",
            long_help = "Include all notes (open, in-progress, and resolved) in output."
        )]
        all: bool,

        /// Show original source rows captured when creating notes.
        #[arg(
            long,
            help_heading = "Review Notes",
            help = "Display original source rows snapshot alongside notes",
            long_help = "Display the original source rows captured when each review note was created."
        )]
        show_source: bool,
    },

    /// Manage, view, reply to, and resolve code review notes.
    #[command(
        about = "Manage, view, reply to, and resolve code review notes",
        long_about = "Manage code review notes attached to diffs and snapshots. \
Notes support a 3-state task lifecycle:
  💬 Open        Standard pending review feedback
  🔄 InProgress  Feedback currently being addressed (counts as active)
  ✅ Resolved    Completed feedback (hidden by default)

Notes also capture original source row snapshots, support threaded \
quoted replies, and can be inspected in terminal, JSON, or Markdown formats.",
        after_help = "TYPICAL USAGE & EXAMPLES:
    diffuse notes                          List active (open & in-progress) notes
    diffuse notes --all                    List all notes including resolved
    diffuse notes show <id>                Show details and source snapshot for a note
    diffuse notes add src/lib.rs:42 \"text\" Create note anchored to line 42
    diffuse notes reply <id> \"Done\" --resolve Reply and mark resolved
    diffuse notes progress <id>            Mark note as in-progress (🔄)
    diffuse notes resolve <id>             Mark note as resolved (✅)
    diffuse notes reopen <id>              Reopen note back to open status (💬)
    diffuse notes status <id> <status>     Set status (open, in-progress, resolved)"
    )]
    Notes(Box<NotesArgs>),
}

#[derive(clap::Args, Debug, Clone)]
#[command(
    about = "Manage, view, reply to, and track code review notes",
    long_about = "Manage code review notes attached to diffs and snapshots. \
Notes support a 3-state task lifecycle:
  💬 Open        Standard pending review feedback
  🔄 InProgress  Feedback currently being addressed (counts as active)
  ✅ Resolved    Completed feedback (hidden by default)

Notes also capture original source row snapshots, support threaded \
quoted replies, and can be inspected in terminal, JSON, or Markdown formats.",
    after_help = "TYPICAL USAGE & EXAMPLES:
    diffuse notes                          List active (open & in-progress) notes
    diffuse notes --all                    List all notes including resolved
    diffuse notes show <id>                Show details and source snapshot for a note
    diffuse notes add src/lib.rs:42 \"text\" Create note anchored to line 42
    diffuse notes reply <id> \"Done\" --resolve Reply and mark resolved
    diffuse notes progress <id>            Mark note as in-progress (🔄)
    diffuse notes resolve <id>             Mark note as resolved (✅)
    diffuse notes reopen <id>              Reopen note back to open status (💬)
    diffuse notes status <id> <status>     Set status (open, in-progress, resolved)"
)]
pub struct NotesArgs {
    #[command(subcommand)]
    pub action: Option<NotesAction>,

    #[command(flatten)]
    pub revision: RevisionArgs,

    /// Show resolved notes only (shorthand for `list --resolved`)
    #[arg(
        long,
        help_heading = "Filters",
        help = "Show resolved notes only (shorthand for 'list --resolved')",
        long_help = "Filter output to display only resolved review notes (marked with ✅)."
    )]
    pub resolved: bool,

    /// Show in-progress notes only (shorthand for `list --in-progress`)
    #[arg(
        long,
        help_heading = "Filters",
        help = "Show in-progress notes only (shorthand for 'list --in-progress')",
        long_help = "Filter output to display only in-progress review notes (marked with 🔄)."
    )]
    pub in_progress: bool,

    /// Show all notes (open, in-progress, resolved) (shorthand for `list
    /// --all`)
    #[arg(
        short = 'a',
        long,
        help_heading = "Filters",
        help = "Show all notes (open, in-progress, resolved)",
        long_help = "Display all review notes across all status categories."
    )]
    pub all: bool,

    /// Show original source lines captured when creating the notes
    #[arg(
        long,
        help_heading = "Display Options",
        help = "Show original source lines captured when creating the notes",
        long_help = "Print the exact original source code snippet captured when each review note was created."
    )]
    pub show_source: bool,

    /// Emit notes as JSON
    #[arg(
        long,
        help_heading = "Display Options",
        help = "Emit notes as structured JSON",
        long_help = "Emit the review notes collection as a structured JSON array."
    )]
    pub json: bool,

    /// Emit notes as GitHub-Flavored Markdown
    #[arg(
        short = 'm',
        long,
        help_heading = "Display Options",
        help = "Emit notes as GitHub-Flavored Markdown",
        long_help = "Render the review notes as GitHub-Flavored Markdown callout blocks."
    )]
    pub markdown: bool,

    /// Target revision or commit range (e.g. `HEAD~1`, `c1..c2`)
    #[arg(
        short = 'r',
        long = "rev",
        alias = "revision",
        value_name = "REV",
        help_heading = "Filters",
        help = "Target revision or commit range (e.g. 'HEAD~1', 'c1..c2')",
        long_help = "Filter notes matching a specific revision or commit range."
    )]
    pub rev: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum NotesAction {
    /// List review notes (default action)
    #[command(
        about = "List review notes (default action)",
        long_about = "List review notes for the current repository. By default, displays active \
notes (💬 open and 🔄 in-progress) with a summary count footer of resolved notes. \
Can filter by status, revision, or pathspecs, or export to JSON and Markdown.",
        after_help = "EXAMPLES:
    diffuse notes list
    diffuse notes list HEAD~1
    diffuse notes list --staged
    diffuse notes list -- src/lib.rs
    diffuse notes list HEAD~1 -- src/lib.rs
    diffuse notes list --all
    diffuse notes list --resolved
    diffuse notes list --in-progress
    diffuse notes list --show-source
    diffuse notes list --json
    diffuse notes list -m"
    )]
    List {
        #[command(flatten)]
        revision: RevisionArgs,

        /// Show resolved notes only
        #[arg(
            long,
            help_heading = "Filters",
            help = "Show resolved notes only",
            long_help = "Filter output to display only resolved review notes (marked with ✅)."
        )]
        resolved: bool,

        /// Show in-progress notes only
        #[arg(
            long,
            help_heading = "Filters",
            help = "Show in-progress notes only",
            long_help = "Filter output to display only in-progress review notes (marked with 🔄)."
        )]
        in_progress: bool,

        /// Show all notes (open, in-progress, resolved)
        #[arg(
            short = 'a',
            long,
            help_heading = "Filters",
            help = "Show all notes (open, in-progress, resolved)",
            long_help = "Display all review notes across all status categories."
        )]
        all: bool,

        /// Show original source lines captured when creating the notes
        #[arg(
            long,
            help_heading = "Display Options",
            help = "Show the original source rows captured when creating the notes",
            long_help = "Print the exact original source rows captured when each review note was created, rendered with standard diff coloring."
        )]
        show_source: bool,

        /// Emit notes as JSON
        #[arg(
            long,
            help_heading = "Display Options",
            help = "Emit notes as structured JSON",
            long_help = "Emit the review notes collection as a structured JSON array."
        )]
        json: bool,

        /// Emit notes as Markdown
        #[arg(
            short = 'm',
            long,
            help_heading = "Display Options",
            help = "Emit notes as GitHub-Flavored Markdown",
            long_help = "Render the review notes as GitHub-Flavored Markdown callout blocks."
        )]
        markdown: bool,

        /// Target revision or commit range (e.g. `HEAD~1`, `c1..c2`)
        #[arg(
            short = 'r',
            long = "rev",
            alias = "revision",
            value_name = "REV",
            help_heading = "Filters",
            help = "Target revision or commit range (e.g. 'HEAD~1', 'c1..c2')",
            long_help = "Filter notes matching a specific revision or commit range."
        )]
        rev: Option<String>,
    },

    /// Create a new review note
    #[command(
        alias = "create",
        alias = "new",
        about = "Create a new review note",
        long_about = "Create a new review note anchored to diff coordinates on a file. \
Coordinates may be given as a target (`file.rs:17` on the new side), via `--new` \
(post-image line or range; alias `--line`), or via `--old` (pre-image line or \
range). When both `--old` and `--new` are given, they must locate the same diff \
row(s).\n\n\
The note stores the exact diff rows it was created against, so it re-anchors to \
those rows and never drifts when the diff changes.",
        after_help = "TYPICAL USAGE & EXAMPLES:
    diffuse notes create src/lib.rs:42 \"Fix typo in doc comment\"
    diffuse notes create src/lib.rs:10-25 \"Refactor into helper method\"
    diffuse notes create src/lib.rs --new 12 \"New-side line 12\"
    diffuse notes create src/lib.rs --line 12 \"--line is an alias for --new\"
    diffuse notes create src/lib.rs --old 15 \"Pre-image line 15 (deletion)\"
    diffuse notes create src/lib.rs --old 14 --new 15 \"Both sides of a modification\"
    diffuse notes create src/lib.rs --new 10-25 \"Range on the new side\"
    diffuse notes create src/lib.rs:42 --status in-progress -m \"Start as in-progress\"
    echo \"Detailed comment\" | diffuse notes create src/lib.rs:42"
    )]
    Add {
        /// Target file path, optionally with new-side coordinates (e.g.
        /// `src/lib.rs`, `src/lib.rs:42`, `src/lib.rs:10-25`)
        #[arg(
            value_name = "TARGET",
            value_hint = clap::ValueHint::AnyPath,
            help_heading = "Target & Coordinates",
            help = "Target file path or new-side coordinate (e.g. 'src/lib.rs:42', 'src/lib.rs:10-25')"
        )]
        target: Option<String>,

        /// Pre-image (old-side) line or range: `N` or `N-M`
        #[arg(
            long,
            value_name = "OLD",
            help_heading = "Target & Coordinates",
            help = "Pre-image (old-side) line or range: 'N' or 'N-M'",
            long_help = "Pre-image (old-side) coordinates the note anchors to. Either a single line \
number or an inclusive range 'N-M'. May be combined with --new only when both locate the same diff rows."
        )]
        old: Option<String>,

        /// Post-image (new-side) line or range: `N` or `N-M`
        #[arg(
            long,
            alias = "line",
            value_name = "NEW",
            help_heading = "Target & Coordinates",
            help = "Post-image (new-side) line or range: 'N' or 'N-M' (alias: --line)",
            long_help = "Post-image (new-side) coordinates the note anchors to. Either a single line \
number or an inclusive range 'N-M'. Alias: --line. May be combined with --old only when both locate \
the same diff rows."
        )]
        new: Option<String>,

        /// Note text message (if omitted, reads from stdin)
        #[arg(
            short = 'm',
            long = "message",
            value_name = "MESSAGE",
            help_heading = "Message Content",
            help = "Note text message (if omitted, reads from stdin)",
            long_help = "The review note comment text. If omitted from flags and positional arguments, read from stdin."
        )]
        message: Option<String>,

        /// Trailing note text message argument
        #[arg(
            value_name = "TEXT",
            help_heading = "Message Content",
            help = "Trailing note text message argument"
        )]
        text: Option<String>,

        /// Initial status of the note: 'open' (default), 'in-progress', or
        /// 'resolved'
        #[arg(
            long,
            value_name = "STATUS",
            default_value = "open",
            help_heading = "Status & Output",
            help = "Initial note status: 'open' (default), 'in-progress', or 'resolved'",
            long_help = "Initial lifecycle status for the note: 'open' (💬), 'in-progress' (🔄), or 'resolved' (✅)."
        )]
        status: String,

        /// Emit created note as JSON
        #[arg(
            long,
            help_heading = "Status & Output",
            help = "Emit created note as JSON",
            long_help = "Print the newly created review note serialized as JSON to stdout."
        )]
        json: bool,

        /// Target revision or commit range (e.g. `HEAD~1`, `c1..c2`)
        #[arg(
            short = 'r',
            long = "rev",
            alias = "revision",
            value_name = "REV",
            help_heading = "Target & Coordinates",
            help = "Target revision or commit range (e.g. 'HEAD~1', 'c1..c2')",
            long_help = "Target revision or commit range to attach the note to. Can be a single commit \
or a commit range. Overrides any top-level revision."
        )]
        rev: Option<String>,
    },

    /// Show details for a specific note
    #[command(
        about = "Show details and source snapshot for a specific note",
        long_about = "Displays full metadata and content for a review note, including ID, \
target file, line range, side, creation timestamp, status glyph, message text, and \
the original captured source lines snapshot.",
        after_help = "EXAMPLES:
    diffuse notes show 1728400000-1234-1
    diffuse notes show abc-123 --show-source
    diffuse notes show abc-123 --json
    diffuse notes show abc-123 -m"
    )]
    Show {
        /// Note ID or unique ID prefix
        #[arg(value_name = "ID", help = "Note ID or unique ID prefix")]
        id: String,

        /// Show original source lines
        #[arg(
            long,
            help_heading = "Display Options",
            help = "Show original source lines snapshot",
            long_help = "Display the original source lines captured when the note was created."
        )]
        show_source: bool,

        /// Emit as JSON
        #[arg(
            long,
            help_heading = "Display Options",
            help = "Emit note as JSON",
            long_help = "Print the note serialized as JSON to stdout."
        )]
        json: bool,

        /// Emit as Markdown
        #[arg(
            short = 'm',
            long,
            help_heading = "Display Options",
            help = "Emit note as GitHub-Flavored Markdown",
            long_help = "Render the note as a GitHub-Flavored Markdown callout block."
        )]
        markdown: bool,
    },

    /// Reply to a note, prepending reply text and quoting previous content
    #[command(
        about = "Reply to a note, prepending reply text and quoting previous content",
        long_about = "Prepends your reply text to the review note and blockquotes the previous \
note content, creating a clean inline conversation thread. Can simultaneously update \
the note's lifecycle status.",
        after_help = "EXAMPLES:
    diffuse notes reply abc-123 \"Addressed in commit 9f1b2\" --resolve
    diffuse notes reply abc-123 -m \"Working on this\" --status in-progress
    echo \"Detailed reply\" | diffuse notes reply abc-123"
    )]
    Reply {
        /// Note ID or unique ID prefix
        #[arg(value_name = "ID", help = "Note ID or unique ID prefix")]
        id: String,

        /// Reply message text (if omitted or '-', reads from stdin)
        #[arg(
            short = 'm',
            long = "message",
            value_name = "MESSAGE",
            help_heading = "Reply Content",
            help = "Reply message text (if omitted or '-', reads from stdin)",
            long_help = "Reply message text. If omitted or '-', reads message text from stdin."
        )]
        message: Option<String>,

        /// Optional reply message text as trailing positional argument
        #[arg(
            value_name = "TEXT",
            help_heading = "Reply Content",
            help = "Optional reply message text as trailing positional argument"
        )]
        text: Option<String>,

        /// Transition note status on reply: open, in-progress, resolved
        #[arg(
            long,
            value_name = "STATUS",
            help_heading = "Status Transitions",
            help = "Transition note status on reply: open, in-progress, resolved",
            long_help = "Update note lifecycle status upon replying: 'open' (💬), 'in-progress' (🔄), or 'resolved' (✅)."
        )]
        status: Option<String>,

        /// Mark the note as resolved after replying (shorthand for --status
        /// resolved)
        #[arg(
            long,
            help_heading = "Status Transitions",
            help = "Mark the note as resolved after replying (shorthand for '--status resolved')",
            long_help = "Mark the note as resolved (✅) immediately after saving the reply."
        )]
        resolve: bool,
    },

    /// Mark a note as in-progress (🔄)
    #[command(
        about = "Mark a note as in-progress (🔄)",
        long_about = "Updates the status of a review note to InProgress (🔄). \
In-progress notes represent work currently underway, displaying alongside open notes \
while distinctively marked with the 🔄 glyph.",
        after_help = "EXAMPLES:
    diffuse notes progress abc-123"
    )]
    Progress {
        /// Note ID or unique ID prefix
        #[arg(value_name = "ID", help = "Note ID or unique ID prefix")]
        id: String,
    },

    /// Mark a note as resolved (✅)
    #[command(
        about = "Mark a note as resolved (✅)",
        long_about = "Updates the status of a review note to Resolved (✅). \
Resolved notes are hidden from active views by default and summarized in footer count badges.",
        after_help = "EXAMPLES:
    diffuse notes resolve abc-123"
    )]
    Resolve {
        /// Note ID or unique ID prefix
        #[arg(value_name = "ID", help = "Note ID or unique ID prefix")]
        id: String,
    },

    /// Reopen a note back to open status (💬)
    #[command(
        about = "Reopen a note back to open status (💬)",
        long_about = "Resets an in-progress (🔄) or resolved (✅) review note back to Open (💬).",
        after_help = "EXAMPLES:
    diffuse notes open abc-123"
    )]
    Open {
        /// Note ID or unique ID prefix
        #[arg(value_name = "ID", help = "Note ID or unique ID prefix")]
        id: String,
    },

    /// Alias for `open`: reopen a note back to open status (💬)
    #[command(
        about = "Alias for 'open': reopen a note back to open status (💬)",
        long_about = "Alias for 'open'. Resets an in-progress (🔄) or resolved (✅) review note back to Open (💬).",
        after_help = "EXAMPLES:
    diffuse notes reopen abc-123"
    )]
    Reopen {
        /// Note ID or unique ID prefix
        #[arg(value_name = "ID", help = "Note ID or unique ID prefix")]
        id: String,
    },

    /// Explicitly set the status of a note (open, in-progress, resolved)
    #[command(
        about = "Explicitly set note status (open, in-progress, resolved)",
        long_about = "Explicitly transitions the review note to a designated lifecycle status:
  - 'open' (💬)
  - 'in-progress' / 'progress' (🔄)
  - 'resolved' (✅)",
        after_help = "EXAMPLES:
    diffuse notes status abc-123 open
    diffuse notes status abc-123 in-progress
    diffuse notes status abc-123 resolved"
    )]
    Status {
        /// Note ID or unique ID prefix
        #[arg(value_name = "ID", help = "Note ID or unique ID prefix")]
        id: String,

        /// New status: open, in-progress, resolved
        #[arg(
            value_name = "STATUS",
            help = "New status: open, in-progress, resolved",
            long_help = "Target status: 'open' (💬), 'in-progress' / 'progress' (🔄), or 'resolved' (✅)."
        )]
        status: String,
    },
}

#[derive(Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct Input {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    note: Option<ReviewNote>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    start: Option<u32>,
    #[serde(default)]
    end: Option<u32>,
    #[serde(default)]
    diff: String,
    #[serde(default)]
    new_content: Option<HashMap<String, String>>,
    #[serde(default)]
    old_content: Option<HashMap<String, String>>,
    #[serde(default)]
    repo_root: Option<String>,
    #[serde(default)]
    base_args: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default, alias = "cached")]
    staged: bool,
    #[serde(default)]
    context: Option<usize>,
    #[allow(dead_code)]
    #[serde(default)]
    notes: Option<NotesDB>,
    #[serde(default)]
    notes_file: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    token_product_cap: Option<usize>,
    #[serde(default)]
    cluster_budget: Option<usize>,
    #[serde(default)]
    cluster_bandwidth_min: Option<usize>,
    #[serde(default)]
    cluster_bandwidth_slack: Option<usize>,
    #[serde(default)]
    note_id: Option<String>,
    #[serde(default)]
    status: Option<NoteStatus>,
    #[serde(default)]
    reply_text: Option<String>,
    #[serde(default)]
    show_resolved: Option<bool>,
    #[allow(dead_code)]
    #[serde(default)]
    show_source: Option<bool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum InputPayload {
    Batch(Vec<Input>),
    Single(Box<Input>),
}

/// Notes-DB key for a JSON-protocol input: commit-scoped when the input
/// carries a repo root, bare repo name otherwise.
fn notes_key_for_input(input: &Input, repo: &str) -> String {
    let base_repo = match input.repo_root.as_deref() {
        Some(root) => Repo::from_root(Path::new(root)).with_name(repo),
        None => Repo::from_name(repo),
    };
    ReviewTarget::from_repo_args(base_repo, input.base_args.clone(), input.staged).notes_key()
}

fn process_input(input: Input, args: &Args) -> PipelineOutput {
    let explicit_notes_path = args.notes_file.as_ref().or(input.notes_file.as_ref());
    let notes_path = match (explicit_notes_path, input.repo_root.as_deref()) {
        (Some(p), _) => Some(PathBuf::from(p)),
        (None, Some(_)) => Some(default_notes_path()),
        (None, None) => None,
    };

    if let Some(ref path) = notes_path {
        if let Some("addNote") = input.action.as_deref()
            && let (Some(n), Some(r)) = (input.note.clone(), input.repo.as_deref())
        {
            let mut db = load_notes(path);
            add_note(&mut db, &notes_key_for_input(&input, r), n);
            let _ = save_notes(path, &db);
        }

        if let Some("removeNotes") = input.action.as_deref()
            && let (Some(r), Some(f), Some(st), Some(en)) = (
                input.repo.as_deref(),
                input.file.as_deref(),
                input.start,
                input.end,
            )
        {
            let mut db = load_notes(path);
            remove_notes_in_range(&mut db, &notes_key_for_input(&input, r), f, st, en);
            let _ = save_notes(path, &db);
        }

        if let Some("setNoteStatus") = input.action.as_deref()
            && let (Some(r), Some(id), Some(st)) = (
                input.repo.as_deref(),
                input.note_id.as_deref(),
                input.status,
            )
        {
            let mut db = load_notes(path);
            let key = notes_key_for_input(&input, r);
            let _ = set_note_status_in_repo(&mut db, &key, id, st);
            let _ = save_notes(path, &db);
        }

        if let Some("resolveNote") = input.action.as_deref()
            && let (Some(r), Some(id)) = (input.repo.as_deref(), input.note_id.as_deref())
        {
            let mut db = load_notes(path);
            let key = notes_key_for_input(&input, r);
            let _ = set_note_status_in_repo(&mut db, &key, id, NoteStatus::Resolved);
            let _ = save_notes(path, &db);
        }

        if let Some("reopenNote") = input.action.as_deref()
            && let (Some(r), Some(id)) = (input.repo.as_deref(), input.note_id.as_deref())
        {
            let mut db = load_notes(path);
            let key = notes_key_for_input(&input, r);
            let _ = set_note_status_in_repo(&mut db, &key, id, NoteStatus::Open);
            let _ = save_notes(path, &db);
        }

        if let Some("replyNote") = input.action.as_deref()
            && let (Some(r), Some(id), Some(reply)) = (
                input.repo.as_deref(),
                input.note_id.as_deref(),
                input.reply_text.as_deref(),
            )
        {
            let mut db = load_notes(path);
            let key = notes_key_for_input(&input, r);
            let _ = update_note_in_db(&mut db, Some(&key), id, |n| {
                n.text = reply_to_note_text(&n.text, reply);
                if let Some(s) = input.status {
                    n.status = s;
                }
            });
            let _ = save_notes(path, &db);
        }
    }

    let loaded_db = notes_path.as_ref().map(|p| load_notes(p));
    let repo_str = if input.repo_root.is_some() {
        args.repo
            .clone()
            .or_else(|| input.repo.clone())
            .map(|r| notes_key_for_input(&input, &r))
    } else {
        args.repo.clone().or_else(|| input.repo.clone())
    };
    let visual = input.context.unwrap_or(args.context);
    let cap = input.token_product_cap.unwrap_or(args.token_product_cap);
    let budget = input.cluster_budget.unwrap_or(args.cluster_budget);
    let bw_min = input
        .cluster_bandwidth_min
        .unwrap_or(args.cluster_bandwidth_min);
    let bw_slack = input
        .cluster_bandwidth_slack
        .unwrap_or(args.cluster_bandwidth_slack);
    let show_resolved = input.show_resolved.unwrap_or(args.resolved || args.all);
    let opts = PipelineOptions::default()
        .with_notes(loaded_db.clone(), repo_str.clone())
        .with_show_resolved(show_resolved)
        .with_visual_context(visual)
        .with_expand_lines(args.expand_lines)
        .with_token_product_cap(cap)
        .with_cluster_product_cap(budget)
        .with_cluster_bandwidth_min(bw_min)
        .with_cluster_bandwidth_slack(bw_slack);

    if let Some(ref repo_root) = input.repo_root
        && let Ok(git_res) = load_git_repo_with_notes(
            Path::new(repo_root),
            &input.base_args,
            &input.paths,
            input.staged,
            ENGINE_CONTEXT,
            loaded_db.as_ref(),
            repo_str.as_deref(),
        )
    {
        let mut out = run_pipeline_full(
            &git_res.diff,
            Some(git_res.new_content),
            Some(git_res.old_content),
            opts,
        );
        out.branch = git_res.branch;
        out.untracked_skipped = git_res.untracked_skipped;
        return out;
    }

    run_pipeline_full(&input.diff, input.new_content, input.old_content, opts)
}

/// Decide between the machine/extension JSON contract (`true`) and the
/// standalone reviewer producing terminal output (`false`).
///
/// The extension pipes a JSON payload on stdin (a non-terminal stdin with
/// content). A standalone run has git arguments, an interactive stdin, or
/// an empty piped stdin (e.g. `diffuse < /dev/null | cat`). Routing on the
/// *content* of a piped stdin keeps the extension contract intact while
/// letting fully-piped invocations fall through to the full render — and
/// the TUI is only ever entered when both ends are real terminals (see
/// `main`).
fn route_mode(
    json: bool,
    jsonl: bool,
    diff: Option<&str>,
    git_revs: &[String],
    stdin_tty: bool,
    piped_stdin: &str,
) -> bool {
    json || jsonl
        || diff.is_some()
        || (git_revs.is_empty() && !stdin_tty && !piped_stdin.trim().is_empty())
}

/// Resolve the active [`DiffuseTheme`] from the `--theme` CLI flag,
/// falling back to the `DIFFUSE_THEME` environment variable and then the
/// default theme. An unknown `--theme` name warns on stderr and falls back.
///
/// `interactive` must be true only when the process owns the terminal for
/// its whole lifetime (the fullscreen TUI): only then may `auto` issue the
/// OSC 11 background query. One-shot modes (`-p`, pipes, redirects) always
/// resolve without touching the terminal, so no late query response can
/// leak into the shell as typed input.
pub fn resolve_theme(cli_theme: Option<&str>, interactive: bool) -> DiffuseTheme {
    if let Some(name) = cli_theme {
        if name.trim().eq_ignore_ascii_case("auto") {
            return if interactive {
                DiffuseTheme::auto_detect_interactive()
            } else {
                DiffuseTheme::auto_detect()
            };
        }
        if let Some(theme) = DiffuseTheme::from_name(name) {
            return theme;
        }
        eprintln!("diffuse: unknown theme '{name}', falling back to default theme");
    }
    if interactive {
        DiffuseTheme::from_env_interactive()
    } else {
        DiffuseTheme::from_env()
    }
}

/// Configure global ANSI styling condition via `yansi` from CLI flags and
/// environment.
pub fn init_color_mode(color: &str, no_color: bool) -> bool {
    let no_color_env = env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    if no_color || color == "never" || (color == "auto" && no_color_env) {
        yansi::disable();
        false
    } else {
        yansi::enable();
        true
    }
}

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

    if let Some(SubCommand::Notes(ref notes_args)) = args.command {
        return notes_cli::run_notes_command(notes_args, &args);
    }

    let stdin_tty = io::stdin().is_terminal();
    let stdout_tty = io::stdout().is_terminal();
    let colorize = init_color_mode(&args.color, args.no_color);

    let workers = args
        .jobs
        .unwrap_or_else(streaming::default_worker_count)
        .max(1);
    streaming::init_thread_pool(workers);

    let (explicit_cmd, git_revs, pathspecs, is_json, is_jsonl, is_markdown, is_raw, diff_opt) =
        match &args.command {
            Some(SubCommand::Review {
                revision: sub_revision,
            }) => {
                let mut ga = args.revision.git_revs.clone();
                ga.extend(sub_revision.git_revs.iter().cloned());
                let mut ps = args.revision.pathspecs.clone();
                ps.extend(sub_revision.pathspecs.iter().cloned());
                (
                    Some("review"),
                    ga,
                    ps,
                    args.json,
                    args.jsonl,
                    args.markdown,
                    args.raw,
                    args.diff.clone(),
                )
            }
            Some(SubCommand::Parse {
                diff,
                json,
                jsonl,
                markdown,
                raw,
                resolved: _,
                all: _,
                show_source: _,
            }) => {
                let md = *markdown || args.markdown;
                let rw = *raw || args.raw;
                let j = if !*json && !*jsonl && !md && !rw {
                    true
                } else {
                    *json || args.json
                };
                let jl = *jsonl || args.jsonl;
                (
                    Some("parse"),
                    Vec::new(),
                    Vec::new(),
                    j,
                    jl,
                    md,
                    rw,
                    diff.clone().or_else(|| args.diff.clone()),
                )
            }
            None => (
                None,
                args.revision.git_revs.clone(),
                args.revision.pathspecs.clone(),
                args.json,
                args.jsonl,
                args.markdown,
                args.raw,
                args.diff.clone(),
            ),
            Some(SubCommand::Notes(_)) => unreachable!(),
        };

    let show_resolved = match &args.command {
        Some(SubCommand::Parse { resolved, all, .. }) => {
            *resolved || *all || args.resolved || args.all
        }
        _ => args.resolved || args.all,
    };
    let show_source = match &args.command {
        Some(SubCommand::Parse { show_source, .. }) => *show_source || args.show_source,
        _ => args.show_source,
    };

    let no_tui = args.no_tui || args.pager || is_markdown || is_raw;

    // Peek a piped stdin only when the routing decision depends on its
    // content (no git args, no explicit contract flags, no explicit
    // subcommand).
    let mut piped_stdin = String::new();
    if explicit_cmd.is_none()
        && !stdin_tty
        && git_revs.is_empty()
        && !is_json
        && !is_jsonl
        && diff_opt.is_none()
    {
        io::stdin().read_to_string(&mut piped_stdin)?;
    }

    let is_json_contract = match explicit_cmd {
        Some("parse") => true,
        Some("review") => is_json || is_jsonl,
        _ => route_mode(
            is_json,
            is_jsonl,
            diff_opt.as_deref(),
            &git_revs,
            stdin_tty,
            &piped_stdin,
        ),
    };

    // Standalone reviewer: `diffuse [git-args...]` or `diffuse review
    // [git-args...]` When either end is non-interactive or
    // --pager/--no-tui/--markdown is requested, render the full ANSI diff or
    // GFM Markdown instead of the TUI.
    if !is_json_contract {
        let repo_root = env::current_dir()?;
        if !is_inside_work_tree(&repo_root) {
            eprintln!(
                "diffuse: not a git repository (or any of the parent directories): {}",
                repo_root.display()
            );
            process::exit(2);
        }
        let notes_path = args
            .notes_file
            .as_ref()
            .map(Path::new)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_notes_path);
        let loaded_db = load_notes(&notes_path);

        let target = ReviewTarget::from_parts(
            Repo::from_root(&repo_root).with_name_opt(args.repo.clone()),
            git_revs.clone(),
            pathspecs.clone(),
            args.staged,
        );

        let pipeline_opts = PipelineOptions::default()
            .with_notes(Some(loaded_db.clone()), Some(target.notes_key()))
            .with_show_resolved(show_resolved)
            .with_visual_context(args.context)
            .with_expand_lines(args.expand_lines)
            .with_token_product_cap(args.token_product_cap)
            .with_cluster_product_cap(args.cluster_budget)
            .with_cluster_bandwidth_min(args.cluster_bandwidth_min)
            .with_cluster_bandwidth_slack(args.cluster_bandwidth_slack);

        let staged = args.staged;

        // Interactive TUI only when both ends are real terminals and not in
        // pager or markdown mode; any piped or redirected end falls
        // back to the full ANSI or Markdown render.
        if !no_tui && stdin_tty && stdout_tty {
            // Theme resolves here — TUI takeover is now certain, so `auto`
            // may issue the OSC 11 background query: the app owns the
            // terminal (raw mode, full stdin consumption) for its whole
            // lifetime, and any late response lands in its own event loop,
            // never in the shell. Every other branch below resolves
            // query-free.
            let theme = resolve_theme(args.theme.as_deref(), true);
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_revs.clone(),
                pathspecs.clone(),
                staged,
                ENGINE_CONTEXT,
                pipeline_opts
                    .with_lazy_collapsed(true)
                    .with_show_resolved(true),
                args.jobs,
            );
            let initial_out = PipelineOutput {
                files: Vec::new(),
                branch: String::new(),
                untracked_skipped: Vec::new(),
            };
            run_tui(
                initial_out,
                Some(consumer.into_receiver()),
                Some(meta_rx),
                target.repo_name().to_string(),
                notes_path,
                repo_root.clone(),
                git_revs.clone(),
                args.staged,
                args.expand_lines,
                theme,
                show_resolved,
                show_source,
                !args.no_watch,
            )?;
        } else if is_markdown {
            // One-shot render: resolve query-free so no terminal response
            // can outrun process exit and leak into the shell.
            let theme = resolve_theme(args.theme.as_deref(), false);
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_revs.clone(),
                Vec::new(),
                staged,
                ENGINE_CONTEXT,
                pipeline_opts,
                args.jobs,
            );
            if let Ok(Err(e)) = meta_rx.recv() {
                eprintln!("diffuse: {e}");
                process::exit(2);
            }
            let files: Vec<_> = consumer.collect();
            let out = PipelineOutput {
                files,
                branch: String::new(),
                untracked_skipped: Vec::new(),
            };
            let md_colorize = if colorize {
                if args.color == "always" {
                    true
                } else {
                    stdout_tty
                }
            } else {
                false
            };
            let md_opts = MarkdownOptions {
                colorize: md_colorize,
                theme,
                show_resolved_notes: show_resolved,
                show_source,
                ..MarkdownOptions::default()
            };
            let md = render_markdown_output(&out, &md_opts);
            let mut stdout = io::stdout().lock();
            if let Err(e) = stdout.write_all(md.as_bytes()) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    return Ok(());
                }
                return Err(e);
            }
            let _ = stdout.flush();
        } else {
            // One-shot render: resolve query-free (see markdown branch).
            let theme = resolve_theme(args.theme.as_deref(), false);
            let (consumer, meta_rx) = spawn_git_stream_pipeline(
                repo_root.clone(),
                git_revs.clone(),
                Vec::new(),
                staged,
                ENGINE_CONTEXT,
                pipeline_opts,
                args.jobs,
            );
            if let Ok(Err(e)) = meta_rx.recv() {
                eprintln!("diffuse: {e}");
                process::exit(2);
            }
            let mut stdout = io::stdout().lock();
            let term_width = detect_terminal_width();
            if is_raw {
                for file in consumer {
                    let mut rendered =
                        render_raw_file_diff_with_width(&file, None, &theme, term_width);
                    if !colorize {
                        rendered = strip_ansi(&rendered).to_string();
                    }
                    if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
            } else {
                for file in consumer {
                    let mut rendered =
                        render_file_diff_with_width(&file, None, &theme, true, term_width);
                    if !colorize {
                        rendered = strip_ansi(&rendered).to_string();
                    }
                    if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
            }
            if colorize {
                let _ = stdout.write_all(SGR_RESET.as_bytes());
            }
            let _ = stdout.flush();
        }
        return Ok(());
    }

    if diff_opt.is_none() && stdin_tty && piped_stdin.is_empty() {
        let repo_root = env::current_dir()?;
        if !is_inside_work_tree(&repo_root) {
            eprintln!(
                "diffuse: not a git repository (or any of the parent directories): {}",
                repo_root.display()
            );
            process::exit(2);
        }
        let notes_path = args
            .notes_file
            .as_ref()
            .map(Path::new)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(default_notes_path);
        let loaded_db = load_notes(&notes_path);
        let target = ReviewTarget::from_repo_args(
            Repo::from_root(&repo_root).with_name_opt(args.repo.clone()),
            git_revs.clone(),
            args.staged,
        );
        let pipeline_opts = PipelineOptions::default()
            .with_notes(Some(loaded_db), Some(target.notes_key()))
            .with_show_resolved(show_resolved)
            .with_visual_context(args.context)
            .with_expand_lines(args.expand_lines)
            .with_token_product_cap(args.token_product_cap)
            .with_cluster_product_cap(args.cluster_budget)
            .with_cluster_bandwidth_min(args.cluster_bandwidth_min)
            .with_cluster_bandwidth_slack(args.cluster_bandwidth_slack);

        // One-shot path (raw/jsonl/json render, no TUI): resolve query-free
        // so no terminal response can outrun process exit.
        let theme = resolve_theme(args.theme.as_deref(), false);
        let (consumer, meta_rx) = spawn_git_stream_pipeline(
            repo_root,
            git_revs,
            Vec::new(),
            args.staged,
            ENGINE_CONTEXT,
            pipeline_opts,
            args.jobs,
        );
        if let Ok(Err(e)) = meta_rx.recv() {
            eprintln!("diffuse: {e}");
            process::exit(2);
        }
        if is_raw {
            let mut stdout = io::stdout().lock();
            let term_width = detect_terminal_width();
            for file in consumer {
                let rendered = render_raw_file_diff_with_width(&file, None, &theme, term_width);
                if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
            }
            let _ = stdout.write_all(SGR_RESET.as_bytes());
            let _ = stdout.flush();
        } else if is_jsonl {
            let mut stdout = io::stdout().lock();
            for file in consumer {
                if let Err(e) = emit_file_jsonl(&file, &mut stdout) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
            }
            let _ = stdout.flush();
        } else {
            let files: Vec<_> = consumer.collect();
            let out = PipelineOutput {
                files,
                branch: String::new(),
                untracked_skipped: Vec::new(),
            };
            println!("{}", serde_json::to_string(&out).expect("serialize"));
        }
        return Ok(());
    }

    let raw = match &diff_opt {
        Some(path) => fs::read_to_string(path)?,
        None => {
            if piped_stdin.is_empty() {
                let mut s = String::new();
                io::stdin().read_to_string(&mut s)?;
                s
            } else {
                piped_stdin
            }
        }
    };

    // JSON-contract tail path: one-shot by construction, resolve query-free.
    let theme = resolve_theme(args.theme.as_deref(), false);

    let payload: InputPayload = serde_json::from_str(&raw).unwrap_or_else(|_| {
        InputPayload::Single(Box::new(Input {
            diff: raw,
            ..Default::default()
        }))
    });

    let md_colorize = if colorize {
        if args.color == "always" {
            true
        } else {
            stdout_tty
        }
    } else {
        false
    };
    let md_opts = MarkdownOptions {
        colorize: md_colorize,
        theme,
        show_resolved_notes: show_resolved,
        show_source,
        ..MarkdownOptions::default()
    };

    match payload {
        InputPayload::Batch(inputs) => {
            let outputs: Vec<_> = inputs
                .into_iter()
                .map(|i| process_input(i, &args))
                .collect();
            if is_markdown {
                let mut stdout = io::stdout().lock();
                for out in &outputs {
                    let md = render_markdown_output(out, &md_opts);
                    if let Err(e) = stdout.write_all(md.as_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
                let _ = stdout.flush();
            } else if is_raw {
                let mut stdout = io::stdout().lock();
                let term_width = detect_terminal_width();
                for out in &outputs {
                    let mut rendered =
                        render_raw_pipeline_output_with_width(out, &theme, term_width);
                    if !colorize {
                        rendered = strip_ansi(&rendered).to_string();
                    }
                    if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
                let _ = stdout.flush();
            } else if is_jsonl {
                let mut stdout = io::stdout().lock();
                for out in &outputs {
                    for file in &out.files {
                        if let Err(e) = emit_file_jsonl(file, &mut stdout) {
                            if e.kind() == io::ErrorKind::BrokenPipe {
                                return Ok(());
                            }
                            return Err(e);
                        }
                    }
                }
                let _ = stdout.flush();
            } else {
                println!("{}", serde_json::to_string(&outputs).expect("serialize"));
            }
        }
        InputPayload::Single(input) => {
            if is_markdown {
                let output = process_input(*input, &args);
                let md = render_markdown_output(&output, &md_opts);
                let mut stdout = io::stdout().lock();
                if let Err(e) = stdout.write_all(md.as_bytes()) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
                let _ = stdout.flush();
            } else if is_raw {
                let output = process_input(*input, &args);
                let term_width = detect_terminal_width();
                let mut rendered =
                    render_raw_pipeline_output_with_width(&output, &theme, term_width);
                if !colorize {
                    rendered = strip_ansi(&rendered).to_string();
                }
                let mut stdout = io::stdout().lock();
                if let Err(e) = stdout.write_all(rendered.as_bytes()) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        return Ok(());
                    }
                    return Err(e);
                }
                let _ = stdout.flush();
            } else if is_jsonl {
                let explicit_notes_path = args.notes_file.as_ref().or(input.notes_file.as_ref());
                let notes_path = match (explicit_notes_path, input.repo_root.as_deref()) {
                    (Some(p), _) => Some(PathBuf::from(p)),
                    (None, Some(_)) => Some(default_notes_path()),
                    (None, None) => None,
                };
                let loaded_db = notes_path.as_ref().map(|p| load_notes(p));
                let repo_str = if input.repo_root.is_some() {
                    args.repo
                        .clone()
                        .or_else(|| input.repo.clone())
                        .map(|r| notes_key_for_input(&input, &r))
                } else {
                    args.repo.clone().or_else(|| input.repo.clone())
                };
                let cap = input.token_product_cap.unwrap_or(args.token_product_cap);
                let budget = input.cluster_budget.unwrap_or(args.cluster_budget);
                let bw_min = input
                    .cluster_bandwidth_min
                    .unwrap_or(args.cluster_bandwidth_min);
                let bw_slack = input
                    .cluster_bandwidth_slack
                    .unwrap_or(args.cluster_bandwidth_slack);
                let opts = PipelineOptions::default()
                    .with_notes(loaded_db, repo_str)
                    .with_show_resolved(show_resolved)
                    .with_visual_context(input.context.unwrap_or(args.context))
                    .with_expand_lines(args.expand_lines)
                    .with_token_product_cap(cap)
                    .with_cluster_product_cap(budget)
                    .with_cluster_bandwidth_min(bw_min)
                    .with_cluster_bandwidth_slack(bw_slack);
                let consumer = stream_diff_pipeline(
                    &input.diff,
                    input.new_content,
                    input.old_content,
                    opts,
                    args.jobs,
                );
                let mut stdout = io::stdout().lock();
                for file in consumer {
                    if let Err(e) = emit_file_jsonl(&file, &mut stdout) {
                        if e.kind() == io::ErrorKind::BrokenPipe {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
                let _ = stdout.flush();
            } else {
                let output = process_input(*input, &args);
                println!("{}", serde_json::to_string(&output).expect("serialize"));
            }
        }
    }
    Ok(())
}

#[path = "main_tests.rs"]
#[cfg(test)]
mod tests;
