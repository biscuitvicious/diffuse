# diffuse — TODO & Future Roadmap

## 1. Rust Binary & Standalone TUI
- [x] **Standalone Rust TUI**: Full terminal user interface in the `diffuse` binary equivalent to the Pi extension's interactive review UI.
- [x] **Pager mode**: Non-interactive pretty-printed pager mode in the Rust binary (`diffuse` as a drop-in `git diff` pager).
- [x] ~~**Migrate TUI layout rendering to ratatui**~~: Migrate custom TUI layout, buffer rendering, and terminal primitives to `ratatui`. (See [Ratatui TUI Layout & Rendering Migration](plan/Ratatui%20Migration.md))
- [ ] **TUI configuration**: Configurable theme, colors, and keybindings loaded from `~/.config/diffuse/config.toml`.
- [ ] **Theme styling (deeper black & muted red/green backgrounds)**: Tune default theme palette to make the background more black and mute the red (deletion) and green (addition) background shades for enhanced contrast and reduced eye fatigue.
- [x] **CLI UX & subcommands**: Explicit subcommands, CLI arguments, and flags (e.g. `diffuse review [commit-range]`, `diffuse parse`, `diffuse -p`/`--pager`, `-U`/`--context`).
- [x] ~~**Direct git access via git crate**~~: Migrate all git tool callouts into direct git access using a native git crate (e.g. `gix` or `git2`). (See [Native Git Engine & Asynchronous Repository Loader](plan/done/Native%20Git%20Engine%20&%20Async%20Loader.md))

## 2. Streaming Pipeline & Incremental Loading (Pi Extension & Engine)
- [x] ~~**Incremental file streaming**~~: Append and render each file as it arrives from the backend rather than waiting for the entire multi-file diff to finish processing. (See [Incremental Streaming Pipeline & Worker Pool](plan/done/Incremental%20Streaming%20Pipeline%20&%20Worker%20Pool.md))
- [x] ~~**Incremental diff line streaming (JSONL)**~~: Stream and render individual tokenized rows/lines progressively over a JSON Lines protocol. (See [Incremental Streaming Pipeline & Worker Pool](plan/done/Incremental%20Streaming%20Pipeline%20&%20Worker%20Pool.md))

## 3. UI, Navigation & Display Features
- [ ] **Selection round: Shift-Up/Down selection**: Add Shift-Up/Down to start a selection, ends when shift is released. Pressing `v` while selecting clears the selection.
- [ ] **Hunk dividers as selectable rows**: Make hunk dividers "normal" rows that the cursor can select and receive key shortcuts (e.g. `Enter`, `m`, `s`) like file headers and diff lines, instead of click-only expansion targets.
- [ ] **Subline selection with automatic clipboard copy**: Support character-level / subline selection start and end coordinates with automatic copying of selected text to the system clipboard upon selection confirmation / mouse release.
- [ ] **Command palette**: Searchable menu of all available commands and their key shortcuts. Divided into 1) contextual commands applicable to the current row (file header, hunk divider, or diff line/selection), and 2) global commands.
- [x] **Expand / collapse entire files**: Toggle file visibility on/off by pressing Enter on the file header row.
- [x] **Non-wrapping mode with horizontal scrolling**: Option to disable line wrapping and horizontally scroll wide rows (`h`/`l`, `^`/`$`).
- [x] **Scrollbar(s)**: Visual scrollbar indicators for both file navigation and viewport position.
- [x] **Untracked files support**: Detect and display untracked files in the repository (collapsed by default).
- [ ] **Switch to Markdown mode in TUI**: Add a TUI key shortcut for switching to Markdown mode.

## 4. Notes & Review State
- [ ] **Selectable notes, note editing, formatting viewer & targeted deletion**: Make note callouts selectable rows so the cursor can navigate directly onto individual notes (`j`/`k`). Enables 1) editing an existing note in the note modal by pressing `m`/`Enter` when focused on that note, 2) key binding to view a note in a popup modal with its original multiline formatting preserved, and 3) disambiguated note deletion (`d`) when multiple notes exist on the same line or range.
- [ ] **File-level notes on file header**: Make it possible to add/delete/etc notes to a file using the same key shortcuts (`m`, `d`, etc.) but on the file header.
- [ ] **Mark notes as done / resolved**: Ability to mark review notes as done and display them collapsed.
- [ ] **Notes / diff lifecycle, stale notes reconciliation & auto-sync to underlying files**: When underlying files are edited or git state advances, notes anchored to absolute line numbers can become detached or orphaned. Implement: 1) content-aware anchor tracking (contextual hashing / snippet anchoring rather than pure line numbers), 2) auto-reanchoring and line offset adjustment across diff reloads, 3) visual indicators and tray for orphaned / detached notes that no longer match the active diff hunks, and 4) file-watcher / auto-sync lifecycle integration.
- [ ] **Project-local notes storage (git metadata)**: Store review notes directly in project git metadata / refs by default rather than global database storage.

## 5. Golden Acceptance & Engine Roadmap
- [x] ~~**Multi-line expansion subline highlighting flag**~~: When a line is expanded into a multi-line change with a `mod` (`~`) row followed by expanded `add`/`del` rows, support a flag that enables subline-highlighting for continuation lines that belong to a preceding `mod` line (light green added spans, light red deleted spans with strikethroughs). Only lines that *belong* to the preceding `mod` receive subline donor highlights; independent `add`/`del` lines following a `mod` that do not belong together must not be subline-highlighted (rendered as standard normal `add`/`del` lines with dark backgrounds). This should apply universally to *all file types* when a line is expanded across multiple lines with additions/deletions after a `mod` line. (See [Split AppState](plan/done/Split%20AppState.md))
- [ ] **Visually signal both the source del and the target add of a moved line**: Currently `mark_moves` pairs identical lines across hunks as `Mov` and renders only the target (the add side), dropping the source del. This breaks diff reversibility: you cannot reconstruct the original file from the displayed diff alone because the source location is invisible. Fix: render moved lines with *both* a struck-out ghost at the source hunk (del side, muted/strikethrough style) and a highlighted target at the destination hunk (add side), so the viewer sees where the content came from *and* where it went. The wire contract must carry the source del row for moved lines so renderers can display both sides.
- [ ] **Widen golden corpus**: Expand captured fixtures to 50–100+ realistic code diffs across diverse programming languages.
- [ ] **Multi-language diversity**: Rust, C/C++, Go, Zig, TypeScript, JavaScript, Python, JSON, YAML, Markdown, SQL, Shell.
- [ ] **TOML syntax highlighting for `Cargo.lock` & configuration files**: Support TOML syntax highlighting for `Cargo.lock` and other well-established TOML configuration filenames / locations.
- [x] ~~**Per-golden dedicated directories & plain file storage**~~: Extract file contents out of golden JSON into dedicated per-corpus directories (`tests/golden/<corpus>/`) storing uncompressed plain files (`diff.patch`, `meta.json`, `files/<stem>.{old,new}.<ext>`). (See [Inline Test Decoupling & Test Suite Performance](plan/Inline%20Test%20Decoupling%20&%20Test%20Suite%20Performance.md))
- [x] ~~**JSON Lines golden fixtures (`<corpus>.wire.jsonl.gz`)**~~: Store and validate jsonl golden fixtures `<corpus>.wire.jsonl.gz`.

## 6. Performance Engineering & Algorithm Modernization
- [x] **Production Performance Optimizations**: Top 5 hot-path optimizations across tokenizer, TUI flat row indexing, precomputed similarity tokens, DP scratch arena, zero-allocation pipeline, and UTF-8 string slicing. (See [Production Performance Optimizations](plan/done/Production%20Performance%20Optimizations.md))
- [x] **Modular Top-Tier Diffing & LCS Algorithms**: 100% modular, generic sequence alignment engines including Myers' 64-bit Bit-Parallel LCS, Myers' $O(ND)$ diffing, Hunt-Szymanski / Robinson-Schensted patience LIS, Hirschberg linear-space solver, and Aho-Corasick prefix automata. (See [Modular Top-Tier Diffing & LCS Algorithms](plan/done/Modular%20Top-Tier%20Diffing%20&%20LCS%20Algorithms.md))

## 7. Execution Context & CLI
- [x] **Support `-C` (change directory) option**: Conceptually `chdir` to the given directory before scanning for repositories, loading files, and resolving all relative paths. Must be honoured everywhere (git discovery, file I/O, config, golden/corpus paths, etc.). Supports single or multiple sequential `-C` flags (e.g. `diffuse -C <parent> -C <child>`).
- [x] **Raw mode CLI argument (`--raw`)**: Add `--raw` argument to CLI, rendering the diff in exactly the same way as the "raw mode" of the TUI (`R` keybinding).

## 8. Codebase Architecture & Modularization
- [x] ~~**Split `diffuse-tui/src/app.rs` into focused modules**~~: Decompose the monolithic ~5,200-line `app.rs` into logically distinct submodules with single responsibilities (e.g. `state.rs`, `navigation.rs`, `layout.rs`, `expansion.rs`, `render.rs`, `events.rs`, etc.). (See [Split AppState](plan/done/Split%20AppState.md))
- [x] ~~**Unified crate namespace refactoring (`diffuse::core`, `diffuse::tui`, etc.)**~~: Restructure workspace crate layout and facades so external and internal usage references modules via `diffuse::core::*`, `diffuse::tui::*`, `diffuse::render::*`, and `diffuse::syntax::*`. (See [Unified Crate Namespace](plan/done/Unified%20Crate%20Namespace.md))
- [x] ~~**Migrate all inline tests to submodule pattern**~~: Move inline `mod tests { ... }` blocks inside `x.rs` into dedicated submodules (`x/tests.rs` or `x/mod.rs` + `x/tests.rs`) across all workspace crates to keep component source files focused and clean. (See [Inline Test Decoupling & Test Suite Performance](plan/Inline%20Test%20Decoupling%20&%20Test%20Suite%20Performance.md))
- [ ] **Centralized ANSI module & theming engine refactor**: Capture all styling and colorization strings & related logic in an ANSI module. Replace all usages of direct ANSI escapes and related logic with the ANSI module. Update GUIDELINES with this rule (no direct ANSI escapes). Make every single color the responsibility of the theming engine. Default to auto-generation of style colors using coloring logic, while allowing for targeted overrides through configuration (theme base + overrides, or completely new themes through config). Remove the old-old theme configuration if it's still there.

## 9. Observability, Tracing & Logging
- [x] **Tokio tracing & structured logging**: Add tokio tracing logging (copy setup from `mirage/logging.rs` et al). Instrument key methods with tokio method call tracing (`#[instrument]`). Log method call parameters and return value/error, skipping parameters which are big (e.g. large diff strings, file contents, whole AST vectors). (See [Tokio Tracing & Structured Logging](plan/done/Tokio%20Tracing%20&%20Structured%20Logging.md))

