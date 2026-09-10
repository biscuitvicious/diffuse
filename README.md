# diffuse

[![Crates.io](https://img.shields.io/badge/crates.io-v0.1.0-orange.svg)](https://crates.io)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build & Test](https://img.shields.io/badge/checks-passing-brightgreen.svg)](scripts/check.sh)

A high-performance **diff engine**, **ANSI terminal renderer**, and **interactive code review TUI** written natively in Rust.

---

## Overview

Traditional diff tools output raw line additions and deletions. **diffuse** goes deeper:
- **Intra-Line Word Alignments**: Identifies exact character- and word-level modifications within rewritten lines.
- **Relocation & Move Detection**: Distinguishes moved code blocks from actual content deletions.
- **Syntax-Aware Token Streams**: Classifies common, struck, and filled token runs enriched with language-specific syntax highlighting.
- **Interactive Review Interface**: A fast, keyboard-driven terminal UI for reviewing changes, staging/unstaging hunks, expanding hidden context, taking persistent review notes, and navigating diffs.
- **Renderer-Agnostic Engine**: Emits a structured token-stream JSON wire format, making it easy to embed into custom TUIs, IDEs, web inspectors, or automated review bots.

---

## Table of Contents

- [Command-Line Interface (CLI)](#command-line-interface-cli)
  - [Installation](#installation)
  - [CLI Usage](#cli-usage)
  - [Interactive TUI Reviewer](#interactive-tui-reviewer)
  - [TUI Keybindings](#tui-keybindings)
- [Rust Library](#rust-library)
  - [Adding diffuse to your project](#adding-diffuse-to-your-project)
  - [Unified Architecture](#unified-architecture)
  - [Quick Start: Running the Diff Pipeline](#quick-start-running-the-diff-pipeline)
  - [Terminal ANSI Rendering](#terminal-ansi-rendering)
  - [Live Streaming Pipeline](#live-streaming-pipeline)
  - [Embedding the Interactive TUI](#embedding-the-interactive-tui)
- [How the Engine Works](#how-the-engine-works)
- [Workspace Crate Topology](#workspace-crate-topology)
- [Development & Verification](#development--verification)

---

## Command-Line Interface (CLI)

### Installation

Build and install the `diffuse` binary directly via Cargo:

```bash
cargo install --path crates/diffuse
```

Or run directly from source:

```bash
cargo run -p diffuse -- [options]
```

### CLI Usage

```bash
# Launch interactive TUI review of working tree changes
diffuse

# Review staged changes (--cached / --staged / -s)
diffuse --cached

# Review changes across specific commits or branches
diffuse main..feature
diffuse HEAD~3..HEAD

# Target a specific repository or subdirectory (-C)
diffuse -C /path/to/repo

# Filter diff to specific files or directories
diffuse src/lib.rs tests/

# Pretty-print formatted ANSI diff directly to terminal/stdout (-p / --print)
diffuse --print

# Emit structured token-stream JSON to stdout (-j / --json)
diffuse --json
diffuse -j > diff_stream.json
```

### CLI Options

| Flag / Option | Description |
|---|---|
| `-C <PATH>` | Run as if diffuse was started in `<PATH>` instead of the current working directory |
| `-p`, `--print` | Print rendered ANSI diff directly to terminal and exit |
| `-j`, `--json` | Output structured token-stream JSON to stdout and exit |
| `-s`, `--staged`, `--cached` | View staged changes (`git diff --cached`) |
| `--theme <NAME>` | Color theme (`default`, `dark`, `black`) |
| `--expand <LINES>` | Number of hidden context lines revealed per expand step (default: `10`) |
| `--context <LINES>` | Surrounding context lines preserved on hunk reset (default: `3`) |
| `-h`, `--help` | Print command-line help |
| `-V`, `--version` | Print version information |

---

### Interactive TUI Reviewer

Launching `diffuse` without `--print` or `--json` starts the full-screen terminal reviewer.

#### TUI Keybindings

| Category | Key | Action |
|---|---|---|
| **Navigation** | `j` / `k` or `↓` / `↑` | Move cursor down / up by row |
| | `J` / `K` or `}` / `{` | Jump to next / previous hunk |
| | `n` / `p` or `]` / `[` | Jump to next / previous file |
| | `g` / `G` or `Home` / `End` | Jump to top / bottom of diff |
| | `Space` / `Ctrl+f` / `PgDn` | Page down |
| | `Ctrl+b` / `PgUp` | Page up |
| | `h` / `l` or `←` / `→` | Horizontal pan left / right |
| **Search & Finder** | `@` | Open fuzzy file search modal |
| | `Esc` | Close active modal or cancel selection |
| **Context & Expansion** | `a` / `+` | Expand **all** hidden context lines across all hunks |
| | `c` | Reset active hunk context back to default (3 lines) |
| | `C` | Reset all hunks across all files to default context |
| | `Click ▼▼ / ▲▲` | Expand context above or below hunk divider |
| **Visual Modes** | `w` | Toggle inline subword diff styling on / off |
| | `y` | Toggle syntax token highlighting on / off |
| | `R` | Toggle raw unified diff view vs. aligned mod view |
| | `Click ▶ / ▼` | Collapse / expand individual file diffs |
| **Review Notes** | `m` | Open review note draft editor on active line / selection |
| | `Ctrl+Enter` | Save and commit note (persisted to `.diffuse/notes.json`) |
| | `d` | Delete note attached to current line or selection |
| **Git Operations** | `s` | Stage current hunk (`git apply --cached`) |
| | `u` | Unstage current hunk |
| | `x` | Discard current hunk modifications |
| **System** | `e` / `Enter` | Open active file in `$EDITOR` at current line |
| | `?` | Toggle keybinding cheat sheet modal |
| | `q` | Quit diffuse |

---

## Rust Library

`diffuse` can be embedded directly into Rust applications as a high-performance diff calculation and rendering engine.

### Adding diffuse to your project

In your `Cargo.toml`:

```toml
[dependencies]
diffuse = "0.1"
```

---

### Unified Architecture

The root `diffuse` crate re-exports the modular engine subcrates under a unified, ergonomic namespace:

- [`diffuse::core`](crates/diffuse-core): Parsing, dynamic-programming line clustering, sequence alignment, token classification, and JSON wire types.
- [`diffuse::render`](crates/diffuse-render): ANSI terminal styling, color themes, display text formatting, and line slicing.
- [`diffuse::syntax`](crates/diffuse-syntax): Language profiles, syntax token classifiers, and Markdown code fence detection.
- [`diffuse::tui`](crates/diffuse-tui): Terminal UI state machine, command dispatch, event handling, and viewport management.
- [`diffuse::prelude`](crates/diffuse/src/lib.rs): High-frequency engine and rendering primitives ready for import.

---

### Quick Start: Running the Diff Pipeline

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

    // Run the pipeline (diff parsing + move detection + token alignment)
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

---

### Terminal ANSI Rendering

Format diff outputs into colored, terminal-ready ANSI strings with customizable themes:

```rust
use diffuse::prelude::*;

fn render_my_diff(diff_str: &str) -> String {
    let output = run_pipeline(diff_str, None);
    let theme = default_theme();
    
    // Render full diff with styled prefix gutters and word-level highlights
    render_pipeline_output(&output, &theme, true)
}
```

---

### Live Streaming Pipeline

For large diffs spanning hundreds of files, stream diff ingestion across background worker threads without UI freezes:

```rust
use diffuse::core::{PipelineOptions, streaming::stream_diff_pipeline};

fn process_streaming(git_diff: &str) {
    let consumer = stream_diff_pipeline(
        git_diff,
        None, // optional new_content map
        None, // optional old_content map
        PipelineOptions::default(),
        Some(4), // worker thread count
    );

    // Consume processed files as soon as workers finish them
    while let Ok(file) = consumer.rx.recv() {
        println!("Streamed file processed: {}", file.path);
    }
}
```

---

### Embedding the Interactive TUI

Embed the full interactive reviewer inside your own terminal workflow:

```rust
use std::path::PathBuf;
use diffuse::prelude::*;
use diffuse::tui::run_streaming_interactive_tui;

fn launch_reviewer(diff_text: &str, repo_root: PathBuf) -> std::io::Result<()> {
    let output = run_pipeline(diff_text, None);
    let notes_path = repo_root.join(".diffuse/notes.json");

    run_streaming_interactive_tui(
        output,
        None,
        None,
        "my_repo".to_string(),
        notes_path,
        repo_root,
        vec![],
        10, // context expand lines
    )
}
```

---

## How the Engine Works

```
git diff + file contents
        │
        ▼
  1. parse        Split git unified diff into files, hunks, and row additions/deletions.
  2. moves        Detect relocated blocks (identical content elsewhere) with block-move rules.
  3. cluster      Pair deletions with additions into "mod" rows using Dynamic Programming:
                  - Stretch: one deletion across multiple additions.
                  - Reverse-Stretch: multiple deletions into one addition.
                  - Reflow: merging re-wrapped formatting changes.
                  - Anchor Pairing: brace and function delimiter matching.
  4. merge & LCS  Generate token stream: common characters, struck deletions, filled additions.
  5. syntax       Overlay syntax highlighting profiles (Rust, TS/JS, Python, Go, C/C++, Markdown).
        │
        ▼
  Structured Token-Stream JSON (Renderer-Agnostic Contract)
```

---

## Workspace Crate Topology

```
crates/
├── diffuse/          Top-level facade library and CLI binaries (`diffuse`, `diffuse-harvest`)
├── diffuse-core/     Core diff parsing, DP clustering, sequence alignment, and wire formats
├── diffuse-render/   ANSI terminal stylization, diff color themes, and row rendering
├── diffuse-syntax/   Language profiles, syntax token classification, and markdown fences
└── diffuse-tui/      Interactive terminal review application and state machine
```

---

## Development & Verification

Diffuse enforces strict test-first discipline and latency budgets:

```bash
# Run the complete verification gate (fmt, clippy, cargo tests, doctests, and extension tests)
./scripts/check.sh

# Run only cargo tests across workspace
cargo test

# Run doc-tests for the unified diffuse facade
cargo test -p diffuse --doc
```

### Golden Acceptance Suite

The golden suite in `tests/golden/` acts as the acceptance ground truth over captured real-world diffs. Run acceptance tests via:

```bash
cargo test --test golden
```

---

## License

MIT © Daniel
