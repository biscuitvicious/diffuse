# diffuse

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE.md)
[![Build & Test](https://img.shields.io/badge/checks-passing-brightgreen.svg)](scripts/check.sh)

*diffuse* is a diff analysis engine, ANSI terminal renderer, and interactive
code review interface, implemented natively in Rust.

Conventional diff tools report only inserted and deleted lines. *diffuse*
reconstructs the finer-grained structure of a change: which removed line
became which added line, which blocks merely relocated, and what changed
within rewritten lines, down to the individual token.

---

## Table of Contents

- [1. Capabilities](#1-capabilities)
- [2. Installation](#2-installation)
- [3. Command-Line Usage](#3-command-line-usage)
  - [3.1. Interactive review](#31-interactive-review)
  - [3.2. Selecting what to review](#32-selecting-what-to-review)
  - [3.3. Non-interactive output](#33-non-interactive-output)
  - [3.4. Options reference](#34-options-reference)
- [4. The Interactive Reviewer](#4-the-interactive-reviewer)
  - [4.1. Screen layout](#41-screen-layout)
  - [4.2. Keybindings](#42-keybindings)
  - [4.3. Review notes](#43-review-notes)
- [5. Embedding *diffuse* in a Rust application](#5-embedding-diffuse-in-a-rust-application)
- [6. Key Entry Points](#6-key-entry-points)
- [7. Contributing](#7-contributing)
- [8. License](#8-license)

---

## 1. Capabilities

Given a unified diff, optionally accompanied by the pre- and post-image
file contents, *diffuse* performs the following analyses:

- **Line correspondence.** Deleted and added lines are paired into
  modifications wherever the evidence supports it. This includes
  asymmetric cases: one deletion stretched across several additions,
  several deletions merged into one addition, and re-wrapped text
  reflowed as a unit.
- **Relocation detection.** Blocks that reappear elsewhere are classified
  as moves and distinguished from genuine deletions and insertions.
- **Sub-line alignment.** Within paired lines, the engine computes
  token-level correspondence, separating unchanged characters from struck
  (removed) and filled (added) runs.
- **Syntax classification.** Tokens are annotated with language-specific
  syntactic roles, such as keywords, strings, and comments, for all
  supported language profiles.
- **Renderer-agnostic output.** The result is a structured token stream
  per changed row, serializable as JSON. The terminal interface is one
  consumer of this stream; the engine itself performs no terminal I/O.

## 2. Installation

A Rust toolchain is required. The canonical install builds the release
binary into the Cargo binary directory:

```bash
scripts/install.sh
```

This places the `diffuse` executable on the path (under `~/.cargo/bin`
in the standard configuration). Alternatively, install directly with
Cargo, or run from a source checkout without installing:

```bash
cargo install --path crates/diffuse
cargo run -p diffuse -- [options]
```

Use a single gate for development and verification. It checks formatting,
lints, and the full test suite:

```bash
scripts/check.sh
```

## 3. Command-Line Usage

### 3.1. Interactive review

Run `diffuse` inside a repository with no output flags to open the
full-screen interactive reviewer over the working-tree diff:

```bash
diffuse
```

The equivalent explicit form is `diffuse review`. The reviewer starts
only when both standard input and standard output are terminals.
Otherwise, *diffuse* falls back to non-interactive output.

### 3.2. Selecting what to review

Trailing arguments are passed through to `git diff`. Use any revision
specification git understands to select the change under review:

```bash
diffuse --cached          # staged changes
diffuse main..feature     # changes on a branch
diffuse HEAD~3..HEAD      # a range of recent commits
diffuse -C /path/to/repo  # operate in another directory
diffuse src/lib.rs tests/ # restrict to paths
git diff | diffuse        # read a diff from standard input
```

### 3.3. Non-interactive output

The following output modes cover scripting and integration use:

```bash
diffuse --pager           # print the rendered ANSI diff and exit
diffuse --raw             # flat red/green diff without sub-line detail
diffuse --json            # token-stream JSON document on stdout
diffuse --jsonl           # token-stream JSON Lines on stdout
diffuse --markdown        # GitHub-Flavored Markdown rendering
git diff | diffuse parse --json       # parse an explicit diff payload
git diff | diffuse parse --markdown   # Markdown rendering of a payload
```

The `parse` subcommand reads a diff from standard input, or from
`--diff <file>`, and emits the requested representation. The `--json`
and `--jsonl` forms expose the engine's token stream directly. They
constitute the machine interface for external tooling.

### 3.4. Options reference

| Flag | Description |
|---|---|
| `-C <DIR>` | Run as if started in `<DIR>` (repeatable, as with `git -C`) |
| `--diff <FILE>` | Read the diff from a file instead of standard input |
| `--notes-file <FILE>` | Path to the review-notes database |
| `--repo <NAME>` | Repository key used for notes lookup (defaults to directory name) |
| `-U, --context <LINES>` | Unified-diff context lines (default: `4`) |
| `--expand-lines <LINES>` | Hidden context lines revealed per expansion (default: `6`) |
| `--token-product-cap <N>` | Quadratic cap for sub-line alignment (default: `250000`) |
| `-j, --jobs <N>` | Worker thread count (default: half the CPUs) |
| `--json` | Emit the token stream as a single JSON document |
| `--jsonl` | Emit the token stream as JSON Lines |
| `-m, --markdown` | Render GitHub-Flavored Markdown |
| `--color <WHEN>` | ANSI colorization: `always`, `never`, or `auto` (default) |
| `--no-color` | Disable ANSI colorization |
| `--theme <NAME>` | Color theme (`default`, `dark`, `black`) |
| `-p, --pager`, `--no-tui` | Print the rendered diff and exit |
| `--raw` | Flat red/green rendering without sub-line detail |
| `-h, --help` | Print help |
| `-V, --version` | Print version information |

## 4. The Interactive Reviewer

### 4.1. Screen layout

The reviewer presents two header bars above the diff viewport.

The top bar identifies the repository, the branch or commit range under
review, aggregate change counts, and the shared directory prefix. The
status bar identifies the current file with its per-file counts, together
with the file and hunk position and the current hunk's counts:

```text
diffuse ⎇ main −8588 +9374 ~343 ≈2 💬 2
file 8/11 · src/view/diff_grid.rs (−15 +1 ~2) · hunk 1/2 (−3 ~1)
```

Counts use a compact notation throughout: `−` deletions, `+` additions,
`~` modifications, `≈` relocations, and `💬` review notes. Zero-valued
categories are omitted.

### 4.2. Keybindings

Cursor motion wraps around the edges of the content in both directions.
File (`n`/`p`) and hunk (`J`/`K`) jumps wrap as well.

| Key | Action |
|---|---|
| `j` / `k`, `↓` / `↑` | Move cursor down / up |
| `J` / `K`, `}` / `{` | Jump to next / previous hunk |
| `n` / `p`, `]` / `[`, `Tab` / `Shift-Tab` | Jump to next / previous file |
| `g` / `G`, `Home` / `End` | Jump to top / bottom of diff |
| `<digits>` then `g` / `G` | Jump to the given line number |
| `Space`, `Ctrl-f`, `Ctrl-d`, `PgDn` | Page down |
| `Ctrl-b`, `Ctrl-u`, `PgUp` | Page up |
| `h` / `l`, `←` / `→` | Pan horizontally; `^` resets, `$` pans right |
| `Enter` | Collapse / expand the current file |
| `w` | Toggle sub-word diff styling |
| `y` | Toggle syntax highlighting |
| `R` | Toggle raw unified view versus aligned view |
| `a`, `A`, `+`, `E` | Expand all hidden context |
| `c`, `z` | Reset current hunk to default context |
| `C`, `Z` | Reset all hunks to default context |
| `@` | Fuzzy file finder |
| `v` | Begin range selection |
| `m` | Draft a review note on the cursor or selection |
| `d` | Delete the note at the cursor or selection |
| `o` | Open the current file in `$EDITOR` |
| `s` / `S` | Stage current hunk / current file |
| `u` / `U` | Unstage current hunk / current file |
| `x` / `X` | Discard current hunk / current file |
| `?` | Keybinding reference |
| `q`, `Ctrl-c` | Quit; `Esc` dismisses dialogs and selections |

### 4.3. Review notes

Notes drafted with `m` are persisted to a JSON notes database and
reattached to their file locations on subsequent invocations. By default
the database lives under `$XDG_STATE_HOME/diffuse/notes.json`; override
it with `--notes-file`.

Note totals appear alongside the change counts in both header bars.
Per-hunk note counts appear in the status bar.

## 5. Embedding *diffuse* in a Rust application

The `diffuse` crate is both the command-line binary and a library
facade. It re-exports the engine (`diffuse::core`), the renderer
(`diffuse::render`), the language profiles (`diffuse::syntax`), and the
terminal interface (`diffuse::tui`), with the most frequently used items
collected in `diffuse::prelude`. Add the dependency in `Cargo.toml`:

```toml
[dependencies]
diffuse = "0.1"
```

Running the analysis pipeline over a unified diff yields a
`PipelineOutput`. This is the same structured token-stream payload that
the JSON wire format serializes:

```rust
use diffuse::prelude::*;

fn main() {
    let diff = r#"diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn main() {
-    println!("Hello, World!");
+    println!("Hello, diffuse!");
 }
"#;

    // Parse, detect moves, pair modifications, align tokens.
    let output: PipelineOutput = run_pipeline(diff, None);

    for file in &output.files {
        println!("File: {} ({})", file.path, file.status);
        for hunk in &file.hunks {
            for row in &hunk.rows {
                println!("  [{:?}] {}", row.kind, row.text);
                for run in &row.runs {
                    println!("    run: {:?} '{}' ({:?})", run.kind, run.text, run.color);
                }
            }
        }
    }
}
```

- Each `row` carries a `RowKind`: addition, deletion, context,
  modification, or relocation endpoint.
- Each `run` carries a common, struck, or filled classification with its
  syntax color.
- Supply the pre- and post-image file contents as
  `Option<HashMap<String, String>>` arguments to enable full-fidelity
  analysis. Pass `None` to restrict the engine to the diff text alone.
- `run_pipeline_full` exposes pipeline options: visual context,
  expansion width, worker configuration, and attached notes.
- `diffuse::render` provides ANSI rendering (`render_pipeline_output`,
  `render_file_diff`).
- `diffuse::tui` exposes the interactive reviewer (`run_tui`,
  `run_interactive_tui`) for embedding the full interface in another
  terminal application.

---

## 6. Key Entry Points

The table below lists the primary modules and entry points for library
integrators embedding *diffuse* and contributors navigating the codebase:

| Subsystem | Primary Crate | Key Entry Points & Primitives | Primary Role |
|---|---|---|---|
| **High-level Facade** | `diffuse` | `diffuse::prelude::*`, `run_pipeline` | Convenient re-exports of common pipeline and rendering primitives |
| **Analysis Pipeline** | `diffuse-core` | `run_pipeline`, `run_pipeline_full`, `PipelineOutput` | Full diff parsing, DP clustering, move detection, and token alignment |
| **Streaming Pipeline** | `diffuse-core` | `stream_diff_pipeline`, `StreamingConsumer` | Multi-threaded background diff ingestion without UI blocking |
| **Wire Contract** | `diffuse-core` | `OutputFile`, `OutputHunk`, `OutputRow`, `OutputRun` | Canonical JSON / JSONL serialization schema for tools and frontends |
| **Diff Algorithms** | `diffuse-core::algo` | `myers_diff`, `align_affine`, `mark_moves`, `hunt_szymanski` | Generic, reusable sequence alignment and bipartite matching engines |
| **Syntax Highlighting** | `diffuse-syntax` | `LangProfile`, `detect_fences`, `classify_tokens` | Multi-language syntax classification and Markdown code-fence detection |
| **Terminal Rendering** | `diffuse-render` | `render_pipeline_output`, `render_file_diff`, `DiffuseTheme` | High-level ANSI styling, color themes, and diff-line slicing |
| **Interactive TUI** | `diffuse-tui` | `run_tui`, `AppState`, `header_bar_line_themed` | Terminal UI event loop, modal dialogs, and viewport state machine |

---

## 7. Contributing

Review the contributor guidelines and project standards before submitting
pull requests:

- [Code Style & Conventions](CODE_STYLE.md) — Nightly formatting rules, import grouping, and top-down component layout.
- [Architecture & Implementation Guidelines](GUIDELINES.md) — Subsystem boundaries, fail-fast invariants, and algorithmic decoupling rules.
- [Agent & Contributor Guidelines](AGENTS.md) — Standard development workflows, test-first discipline, and engineering plan conventions.
- [Golden Acceptance Test Guidelines](tests/AGENTS.md) — Review discipline, golden corpus validation gates, and fixture regeneration mechanics.

All pull requests must follow the test-first discipline with exhaustive
branch coverage and pass `scripts/check.sh` cleanly with zero warnings.

---

## 8. License

This project is licensed under the MIT License. See [LICENSE.md](LICENSE.md) for the full license text.
