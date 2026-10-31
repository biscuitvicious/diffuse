//! Command-line argument definitions and Clap schema.

use clap::{Parser, Subcommand};
use diffuse::prelude::*;
use std::path::PathBuf;

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
pub struct Args {
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
    pub directory: Vec<PathBuf>,

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
    pub diff: Option<String>,

    /// Root directory of old pre-image files (for diff hydration).
    #[arg(
        long,
        value_name = "DIR",
        value_hint = clap::ValueHint::DirPath,
        global = true,
        help_heading = "Repository & Input",
        help = "Root directory of old pre-image files for context hydration",
        long_help = "Root directory containing original pre-image files used to \
hydrate context, token classification, and function boundary pairing."
    )]
    pub old_dir: Option<PathBuf>,

    /// Root directory of new post-image files (for diff hydration).
    #[arg(
        long,
        value_name = "DIR",
        value_hint = clap::ValueHint::DirPath,
        global = true,
        help_heading = "Repository & Input",
        help = "Root directory of new post-image files for context hydration",
        long_help = "Root directory containing modified post-image files used to \
hydrate context, token classification, and function boundary pairing."
    )]
    pub new_dir: Option<PathBuf>,

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
    pub notes_file: Option<String>,

    /// Repository review key for notes lookup and scoping.
    #[arg(
        long,
        value_name = "KEY",
        global = true,
        help_heading = "Review Notes",
        help = "Repository review key for notes storage and lookup",
        long_help = "Repository key used to store and look up notes in the notes database. \
Defaults to the canonical root directory path of the detected git repository."
    )]
    pub repo_key: Option<String>,

    /// Revision review key for notes lookup and scoping.
    #[arg(
        long,
        value_name = "KEY",
        global = true,
        help_heading = "Review Notes",
        help = "Revision review key for notes storage and lookup",
        long_help = "Revision key used to store and look up notes in the notes database. \
Defaults to the commit hash or revision range of the review target."
    )]
    pub rev_key: Option<String>,

    /// Include resolved notes in diff, JSON, or Markdown output.
    #[arg(
        long,
        global = true,
        help_heading = "Review Notes",
        help = "Include resolved notes in output (hidden by default)",
        long_help = "Include resolved review notes (marked with ✅) in diff, JSON, or \
Markdown output. By default, only active open (💬) and in-progress (🔄) notes are displayed."
    )]
    pub resolved: bool,

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
    pub all: bool,

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
    pub show_source: bool,

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
    pub context: usize,

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
    pub expand_lines: usize,

    /// Maximum quadratic token-product cap for subline diffing (default:
    /// 250000).
    #[arg(
        long,
        hide = true,
        default_value_t = DEFAULT_TOKEN_PRODUCT_CAP,
        global = true
    )]
    pub token_product_cap: usize,

    /// Maximum cell budget for cluster DP alignment (default: 10000000).
    #[arg(long, hide = true, default_value_t = DEFAULT_CLUSTER_BUDGET, global = true)]
    pub cluster_budget: usize,

    /// Minimum corridor half-bandwidth for cluster DP alignment (default: 64).
    #[arg(
        long,
        hide = true,
        default_value_t = DEFAULT_CLUSTER_BANDWIDTH_MIN,
        global = true
    )]
    pub cluster_bandwidth_min: usize,

    /// Bandwidth slack added to cluster length difference (default: 32).
    #[arg(
        long,
        hide = true,
        default_value_t = DEFAULT_CLUSTER_BANDWIDTH_SLACK,
        global = true
    )]
    pub cluster_bandwidth_slack: usize,

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
    pub jobs: Option<usize>,

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
    pub pager: bool,

    /// Disable interactive TUI mode (alias for --pager).
    #[arg(
        long,
        global = true,
        help_heading = "Execution Modes",
        help = "Disable interactive TUI mode (alias for --pager)",
        long_help = "Disable interactive TUI mode and run as a non-interactive CLI pager."
    )]
    pub no_tui: bool,

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
    pub raw: bool,

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
    pub staged: bool,

    /// Disable automatic filesystem watching for live working tree changes.
    #[arg(
        long,
        global = true,
        help_heading = "Execution Modes",
        help = "Disable automatic filesystem watching for live working tree changes",
        long_help = "Disable the background filesystem watcher. By default, diffuse continuously \
monitors the repository for changes and refreshes the display automatically."
    )]
    pub no_watch: bool,

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
    pub json: bool,

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
    pub jsonl: bool,

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
    pub markdown: bool,

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
    pub color: String,

    /// Disable ANSI colorization (equivalent to --color=never).
    #[arg(
        long,
        global = true,
        help_heading = "Theme & Styling",
        help = "Disable ANSI colorization (equivalent to --color=never)",
        long_help = "Disable all ANSI escape sequences, emitting plain text output."
    )]
    pub no_color: bool,

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
    pub theme: Option<String>,

    #[command(subcommand)]
    pub command: Option<SubCommand>,

    #[command(flatten)]
    pub revision: RevisionArgs,
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
pub enum SubCommand {
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
