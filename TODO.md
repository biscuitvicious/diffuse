# diffuse — TODO & Future Roadmap

## 1. Rust Binary & Standalone TUI
- [x] **Standalone Rust TUI**: Full terminal user interface in the `diffuse` binary equivalent to the Pi extension's interactive review UI.
- [x] **Pager mode**: Non-interactive pretty-printed pager mode in the Rust binary (`diffuse` as a drop-in `git diff` pager).
- [ ] **TUI configuration**: Configurable theme, colors, and keybindings loaded from `~/.config/diffuse/config.toml`.
- [ ] **CLI UX & subcommands**: Remove raw `JSON | stdin` default mode in favor of explicit subcommands, CLI arguments, and flags (e.g. `diffuse review [commit-range]`, `diffuse parse`, `diffuse --pager`).
- [ ] ~~**Direct git access via git crate**: Migrate all git tool callouts into direct git access using a native git crate (e.g. `gix` or `git2`).~~ (See [Native Git Engine & Asynchronous Repository Loader](plan/done/Native%20Git%20Engine%20&%20Async%20Loader.md))

## 2. Streaming Pipeline & Incremental Loading (Pi Extension & Engine)
- [ ] ~~**Incremental file streaming**: Append and render each file as it arrives from the backend rather than waiting for the entire multi-file diff to finish processing.~~ (See [Incremental Streaming Pipeline & Worker Pool](plan/done/Incremental%20Streaming%20Pipeline%20&%20Worker%20Pool.md))
- [ ] ~~**Incremental diff line streaming (JSONL)**: Stream and render individual tokenized rows/lines progressively over a JSON Lines protocol.~~ (See [Incremental Streaming Pipeline & Worker Pool](plan/done/Incremental%20Streaming%20Pipeline%20&%20Worker%20Pool.md))

## 3. UI, Navigation & Display Features
- [x] **Expand / collapse entire files**: Toggle file visibility on/off by pressing Enter on the file header row.
- [x] **Non-wrapping mode with horizontal scrolling**: Option to disable line wrapping and horizontally scroll wide rows (`h`/`l`, `^`/`$`).
- [x] **Scrollbar(s)**: Visual scrollbar indicators for both file navigation and viewport position.
- [x] **Untracked files support**: Detect and display untracked files in the repository (collapsed by default).

## 4. Notes & Review State
- [ ] **Mark notes as done / resolved**: Ability to mark review notes as done and display them collapsed.
- [ ] **Project-local notes storage (git metadata)**: Store review notes directly in project git metadata / refs by default rather than global database storage.

## 5. Golden Acceptance & Engine Roadmap
- [ ] **Multi-line expansion subline highlighting flag**: When a line is expanded into a multi-line change with a `mod` (`~`) row followed by expanded `add`/`del` rows, support a flag that enables subline-highlighting for continuation lines that belong to a preceding `mod` line (light green added spans, light red deleted spans with strikethroughs). Only lines that *belong* to the preceding `mod` receive subline donor highlights; independent `add`/`del` lines following a `mod` that do not belong together must not be subline-highlighted (rendered as standard normal `add`/`del` lines with dark backgrounds). This should apply universally to *all file types* when a line is expanded across multiple lines with additions/deletions after a `mod` line.
- [ ] **Widen golden corpus**: Expand captured fixtures to 50–100+ realistic code diffs across diverse programming languages.
- [ ] **Multi-language diversity**: Rust, C/C++, Go, Zig, TypeScript, JavaScript, Python, JSON, YAML, Markdown, SQL, Shell.
- [ ] **Automated fixture harvesting tool**: CLI tool to harvest diffs + old/new file snapshots from repository histories.

## 6. Performance Engineering & Algorithm Modernization
- [ ] **Production Performance Optimizations**: Top 5 hot-path optimizations across tokenizer, TUI flat row indexing, precomputed similarity tokens, DP scratch arena, and UTF-8 string slicing. (See [Production Performance Optimizations](plan/Production%20Performance%20Optimizations.md))
- [ ] **Modular Top-Tier Diffing & LCS Algorithms**: 100% modular, generic sequence alignment engines including Myers' 64-bit Bit-Parallel LCS, Myers' $O(ND)$ diffing, Hunt-Szymanski / Robinson-Schensted patience LIS, Hirschberg linear-space solver, and Aho-Corasick prefix automata. (See [Modular Top-Tier Diffing & LCS Algorithms](plan/Modular%20Top-Tier%20Diffing%20&%20LCS%20Algorithms.md))

## 7. Execution Context & CLI
- [ ] **Support `-C` (change directory) option**: Conceptually `chdir` to the given directory before scanning for repositories, loading files, and resolving all relative paths. Must be honoured everywhere (git discovery, file I/O, config, golden/corpus paths, etc.). If many call sites are affected, introduce an `ExecutionContext` (or `AppContext`/`ResolveContext`) struct that tracks the effective cwd and exposes `resolve(path) -> PathBuf` / `join` helpers, threaded through the pipeline instead of ad-hoc `Path::new(".")` / `env::current_dir()` uses.
