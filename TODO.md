# diffuse — TODO and future roadmap

Outstanding work items, grouped by subsystem. Checked items are complete.

## 1. Rust binary and standalone TUI

- [x] **Standalone Rust TUI**: Full terminal user interface in the `diffuse` binary, equivalent to the Pi extension's interactive review UI.
- [x] **Pager mode**: Non-interactive pretty-printed pager mode in the Rust binary (`diffuse` as a drop-in `git diff` pager).
- [x] ~~**Migrate TUI layout rendering to ratatui**~~: Migrate custom TUI layout, buffer rendering, and terminal primitives to `ratatui`. (See [Ratatui TUI Layout & Rendering Migration](plan/Ratatui%20Migration.md))
- [ ] **TUI configuration**: Configurable theme, colors, and keybindings loaded from `~/.config/diffuse/config.toml`.
- [x] **Theme styling (deeper black & muted red/green backgrounds)**: Tune the default theme palette to make the background more black, and mute the red (deletion) and green (addition) background shades, for enhanced contrast and reduced eye fatigue.
- [x] **CLI UX & subcommands**: Explicit subcommands, CLI arguments, and flags (for example, `diffuse review [commit-range]`, `diffuse parse`, `diffuse -p`/`--pager`, `-U`/`--context`).
- [x] **Enforce pure-Rust `gix` independence across all git operations**: Apply the guideline to use `gix` for all access that would require `git` to be available. Extract a dedicated `diffuse-git` workspace crate with zero internal dependencies on diffuse engine crates. Give `gix` the full package with pure-Rust implementations across all operations, profile thoroughly against git subprocess under heavy stress tests, and avoid admitting any subprocess code unless profiling proves a specific operation is radically faster. (See [Native Gix Migration & Diffuse-Git Extraction](plan/done/Native%20Gix%20Migration%20&%20Diffuse-Git%20Extraction.md))
- [x] ~~**Direct git access via git crate**~~: Migrate all git tool callouts into direct git access using a native git crate (for example, `gix` or `git2`). (See [Native Git Engine & Asynchronous Repository Loader](plan/done/Native%20Git%20Engine%20&%20Async%20Loader.md))

## 2. Streaming pipeline and incremental loading (Pi extension and engine)

- [x] ~~**Incremental file streaming**~~: Append and render each file as it arrives from the backend, rather than waiting for the entire multi-file diff to finish processing. (See [Incremental Streaming Pipeline & Worker Pool](plan/done/Incremental%20Streaming%20Pipeline%20&%20Worker%20Pool.md))
- [x] ~~**Incremental diff line streaming (JSONL)**~~: Stream and render individual tokenized rows and lines progressively over a JSON Lines protocol. (See [Incremental Streaming Pipeline & Worker Pool](plan/done/Incremental%20Streaming%20Pipeline%20&%20Worker%20Pool.md))
- [ ] **On-demand chunked hunk pagination (>1000 lines OOM failsafe)**: Collapse any hunk longer than 1,000 lines from line 1,000 onward into a hidden lines bar (similar to, but visually distinct from, the hidden context lines bar). Clicking the bar loads another 1,000 lines on demand. This also applies to single-hunk files (untracked, renamed, mode changes, deleted, and so on). Load only the first 1,000 lines into memory and fetch additional lines on demand. This is the primary failsafe against out-of-memory failures on massive diffs, scaling to 1M+ line files with multiple inline-span changes per line.

## 3. UI, navigation and display features

- [ ] **Collapsible file picker pane**: Add a collapsible file picker and list pane to the left of the diff display for fast file browsing and jump navigation.
- [ ] **Collapsible old-line number gutter**: Add a collapsible old-line number gutter to the left of the new-line number gutter, toggleable to maximize horizontal code viewing space.
- [ ] **Collapsible side-by-side diff**: Add a split side-by-side dual-pane diff layout that can be toggled on and off.
- [ ] **Auto-width-adjusting number gutters**: Dynamic line number gutters start at 3 columns and expand adaptively (4+ columns) as line numbers greater than 999 or 9999 are encountered during streaming.
- [ ] **Selection round: Shift-Up/Down selection**: Add Shift-Up/Down to start a selection, ending when shift is released. Pressing `v` while selecting clears the selection.
- [ ] **Hunk dividers as selectable rows**: Make hunk dividers normal rows that the cursor can select and that receive key shortcuts (for example, `Enter`, `m`, `s`) like file headers and diff lines, instead of click-only expansion targets.
- [ ] **Subline selection with automatic clipboard copy**: Support character-level / subline selection start and end coordinates, automatically copying the selected text to the system clipboard on selection confirmation or mouse release.
- [ ] **Command palette**: Add a searchable menu of all available commands and their key shortcuts, divided into 1) contextual commands applicable to the current row (file header, hunk divider, or diff line/selection), and 2) global commands.
- [x] **Expand / collapse entire files**: Toggle file visibility on and off by pressing Enter on the file header row.
- [x] **Non-wrapping mode with horizontal scrolling**: Option to disable line wrapping and horizontally scroll wide rows (`h`/`l`, `^`/`$`).
- [x] **Scrollbar(s)**: Visual scrollbar indicators for both file navigation and viewport position.
- [x] **Untracked files support**: Detect and display untracked files in the repository (collapsed by default).
- [ ] **Switch to Markdown mode in TUI**: Add a TUI key shortcut for switching to Markdown mode.
- [ ] **Context gutter display for whitespace-only mod rows**: For lines with whitespace-only changes, display an empty context gutter prefix (` `) instead of a mod (`~`) row icon. Keep the subline highlighting that isolates the changed whitespace, and keep treating the row as a mod row under the hood. Only the icon changes, to reduce the cognitive load of scanning for what changed.
- [ ] **Undo mechanism for destructive changes (`x`, notes deletion)**: Add an undo mechanism to reverse destructive operations: recover discarded hunks and files (restoring changes from the safety stash commit created on `x`/`X`), and restore deleted review notes.

## 4. Notes and review state

- [ ] **Selectable notes, note editing, formatting viewer & targeted deletion**: Make note callouts selectable rows so the cursor can navigate directly onto individual notes (`j`/`k`). This enables 1) editing an existing note in the note modal by pressing `m`/`Enter` when focused on that note, 2) a key binding to view a note in a popup modal with its original multiline formatting preserved, and 3) disambiguated note deletion (`d`) when multiple notes exist on the same line or range.
- [ ] **File-level notes on file banner (`m`, `d`)**: Support pressing `m` or `d` on the file banner, behaving the same as on a file row (using the same logic when several notes exist). This makes it possible to add, delete, and manage notes on a file using the same key shortcuts on the file header.
- [ ] **Mark notes as done / resolved**: Add the ability to mark review notes as done and display them collapsed.
- [ ] **Notes / diff lifecycle, stale notes reconciliation & auto-sync to underlying files**: Notes anchored to absolute line numbers can become detached or orphaned when underlying files are edited or git state advances. Implement 1) content-aware anchor tracking (contextual hashing / snippet anchoring rather than pure line numbers), 2) auto-reanchoring and line offset adjustment across diff reloads, 3) visual indicators and a tray for orphaned / detached notes that no longer match the active diff hunks, and 4) file-watcher / auto-sync lifecycle integration.
- [ ] **Project-local notes storage (git metadata)**: Store review notes directly in project git metadata and refs by default, rather than in global database storage.
- [ ] **Reload file state & version-tracked patches**: Add a key shortcut to reload the current file's state. Track file changes as patches on top of the previous file state. Tie notes to file versions using a hash of the row selection the notes are attached to, so notes survive file reloads and remain anchored to their original content regardless of subsequent edits.
- [x] **Incremental Markdown rendering in the note creation modal**: Render the text typed into the note creation modal incrementally as source-accurate Markdown — the source stays exactly as typed with all markers visible, and only styling is applied on top. Unlike the note callout rendering mode (`plan/Note Markdown Rendering.md`), no source characters are stripped or replaced.

## 5. Golden acceptance and engine roadmap

- [x] ~~**Multi-line expansion subline highlighting flag**~~: When a line expands into a multi-line change (a `mod` (`~`) row followed by expanded `add`/`del` rows), support a flag that enables subline highlighting for continuation lines belonging to the preceding `mod` line (light green added spans; light red deleted spans with strikethroughs). Only lines that *belong to* the preceding `mod` receive subline donor highlights; independent `add`/`del` lines following a `mod` that do not belong together must not be subline-highlighted (rendered as standard normal `add`/`del` lines with dark backgrounds). Apply this universally to *all file types* whenever a line expands across multiple lines with additions/deletions after a `mod` line. (See [Split AppState](plan/done/Split%20AppState.md))
- [x] ~~**Visually signal both the source del and the target add of a moved line**~~: Render moved lines with both a struck-out ghost at the source hunk (del side, `≈-` with strikethrough) and a highlighted target at the destination hunk (`≈+`), split into `Row::MovSrc` and `Row::MovTgt` with wire serialization.
- [x] ~~**Split wholesale replacement mod rows into separate del + add rows**~~: Lines consisting entirely of struck-out spans followed by entirely added spans (with zero common anchor or context tokens) should not render as a single `mod` (`~`) row. Keep them as separate `del` (`-`) + `add` (`+`) rows, or decompose them into granular subline-highlighted spans that isolate the exact token changes, rather than presenting an opaque whole-line strike/fill block.
- [ ] **Widen golden corpus**: Expand captured fixtures to 50–100+ realistic code diffs across diverse programming languages.
- [ ] **Multi-language diversity**: Rust, C/C++, Go, Zig, TypeScript, JavaScript, Python, JSON, YAML, Markdown, SQL, Shell.
- [ ] **TOML syntax highlighting for `Cargo.lock` & configuration files**: Support TOML syntax highlighting for `Cargo.lock` and other well-established TOML configuration filenames and locations.
- [x] ~~**Per-golden dedicated directories & plain file storage**~~: Extract file contents out of golden JSON into dedicated per-corpus directories (`tests/golden/<corpus>/`) storing uncompressed plain files (`diff.patch`, `meta.json`, `files/<stem>.{old,new}.<ext>`). (See [Inline Test Decoupling & Test Suite Performance](plan/Inline%20Test%20Decoupling%20&%20Test%20Suite%20Performance.md))
- [x] ~~**JSON Lines golden fixtures (`<corpus>.wire.jsonl.gz`)**~~: Store and validate JSON Lines golden fixtures `<corpus>.wire.jsonl.gz`.

## 6. Performance engineering and algorithm modernization

- [ ] **Contiguous prefix/suffix salient token weighting in DP pairing**: Reward and promote salient token sequences that exactly match a contiguous (trimmed) prefix or suffix between candidate lines in the DP cluster pairing solver. Contiguous token alignment at sequence boundaries exhibits strong structural correspondence (e.g., `Subproject commit <hash>` or call-site modifications with large trailing arguments), making boundary-anchored tokens substantially more salient than scattered, coincidental matches of identical character length, and encouraging in-place subline `Row::Mod` formation over fragmented `Del` + `Add` decompositions.
- [x] **Production Performance Optimizations**: Top 5 hot-path optimizations across tokenizer, TUI flat row indexing, precomputed similarity tokens, DP scratch arena, zero-allocation pipeline, and UTF-8 string slicing. (See [Production Performance Optimizations](plan/done/Production%20Performance%20Optimizations.md))
- [x] **Modular Top-Tier Diffing & LCS Algorithms**: 100% modular, generic sequence alignment engines, including Myers' 64-bit Bit-Parallel LCS, Myers' O(ND) diffing, Hunt-Szymanski / Robinson-Schensted patience LIS, the Hirschberg linear-space solver, and Aho-Corasick prefix automata. (See [Modular Top-Tier Diffing & LCS Algorithms](plan/done/Modular%20Top-Tier%20Diffing%20&%20LCS%20Algorithms.md))
- [ ] **High-performance memory allocator for musl target (`mimalloc` / `jemalloc`)**: Introduce a fast Rust memory allocator crate (for example, `mimalloc` or `tikv-jemallocator`) when compiling for `*-unknown-linux-musl`. This prevents the standard musl `malloc` from degrading performance during multi-threaded, allocation-heavy diff workloads.

## 7. Execution context and CLI

- [x] **Support `-C` (change directory) option**: Conceptually `chdir` to the given directory before scanning for repositories, loading files, and resolving all relative paths. Honor it everywhere (git discovery, file I/O, config, golden/corpus paths, and so on). Support single or multiple sequential `-C` flags (for example, `diffuse -C <parent> -C <child>`).
- [x] **Raw mode CLI argument (`--raw`)**: Add a `--raw` argument to the CLI, rendering the diff in exactly the same way as the raw mode of the TUI (`R` keybinding).
- [ ] **Use ariadne for CLI error reporting**: Replace ad-hoc error formatting with the `ariadne` crate for rich, colorful, structured CLI error reporting with source-code snippets and labels.

## 8. Codebase architecture and modularization

- [x] ~~**Split `diffuse-tui/src/app.rs` into focused modules**~~: Decompose the monolithic ~5,200-line `app.rs` into logically distinct submodules with single responsibilities (for example, `state.rs`, `navigation.rs`, `layout.rs`, `expansion.rs`, `render.rs`, `events.rs`, and so on). (See [Split AppState](plan/done/Split%20AppState.md))
- [x] ~~**Unified crate namespace refactoring (`diffuse::core`, `diffuse::tui`, etc.)**~~: Restructure workspace crate layout and facades so external and internal usage references modules via `diffuse::core::*`, `diffuse::tui::*`, `diffuse::render::*`, and `diffuse::syntax::*`. (See [Unified Crate Namespace](plan/done/Unified%20Crate%20Namespace.md))
- [x] ~~**Migrate all inline tests to submodule pattern**~~: Move inline `mod tests { ... }` blocks inside `x.rs` into dedicated submodules (`x/tests.rs` or `x/mod.rs` + `x/tests.rs`) across all workspace crates, keeping component source files focused and clean. (See [Inline Test Decoupling & Test Suite Performance](plan/Inline%20Test%20Decoupling%20&%20Test%20Suite%20Performance.md))
- [x] ~~**Centralized ANSI module & theming engine refactor**~~: Capture all styling and colorization strings and related logic in an ANSI module, and replace all direct ANSI escapes with it. Update GUIDELINES with the no-direct-ANSI-escapes rule. Make every color the responsibility of the theming engine. Default to auto-generating style colors with coloring logic, while allowing targeted overrides through configuration (theme base + overrides, or completely new themes through config). Remove the old-old theme configuration if it still exists.
- [x] ~~**Switch to yansi for terminal styling**~~: Replace the centralized ANSI module with the `yansi` crate for terminal styling. Leverage yansi's `Colorize` trait, `Paint` type, and runtime style customization instead of the custom ANSI module. Update GUIDELINES to reflect `yansi` as the styling source. (See [Yansi Terminal Styling Engine Migration](plan/Yansi%20Terminal%20Styling%20Engine%20Migration.md))
- [x] **Extract standalone `diffuse-git` workspace crate**: Decouple all git interactions from `diffuse-core` and `diffuse-tui` into an autonomous `diffuse-git` crate with zero internal dependencies on diffuse engine crates. Own domain types (`Repo`, `ReviewTarget`, `RevisionRange`, `GitFileItem`) and implement 100% pure-gix primitives with stress-tested empirical profiling before admitting any subprocess usage. (See [Native Gix Migration & Diffuse-Git Extraction](plan/done/Native%20Gix%20Migration%20&%20Diffuse-Git%20Extraction.md))
- [ ] **Analyze and refactor method names across entire codebase**: Analyze method and function names across the entire codebase and ensure they fulfill the method naming guidelines: describe the fulfilled behavior or return value clearly, succinctly, and unambiguously; represent the most abstract concept; avoid naming implementation details; adhere to idiomatic Rust naming conventions.
- [ ] **Audit and refactor type names for conceptual abstraction**: Audit and refactor type names (structs, enums, traits) across the codebase to adhere to the type naming guidelines: name types according to their core domain concept and semantic role, not the underlying algorithm or implementation mechanics (for example, refactor `DpOp` to `DiffOp` / `MatchOp`).
- [ ] **Move `self` into `reverse` methods (`reverse(self) -> Self`)**: Update all `reverse(&self) -> Self` methods to take `self` by value and return the new `Self`, entirely avoiding clones inside `reverse` implementations.

## 9. Observability, tracing and logging

- [x] **Tokio tracing & structured logging**: Add Tokio tracing logging (copy the setup from `mirage/logging.rs` and similar). Instrument key methods with Tokio method call tracing (`#[instrument]`). Log method call parameters and return values or errors, skipping large parameters (for example, large diff strings, file contents, whole AST vectors). (See [Tokio Tracing & Structured Logging](plan/done/Tokio%20Tracing%20&%20Structured%20Logging.md))

## 10. Verification, integrity and design by contract (DbC)

- [x] **Design by Contract (DbC) method contracts**: Integrate preconditions (`require!`), postconditions (`ensure!`), and internal state invariants (`invariant!`) across algorithms, AST transforms, streaming channels, syntax classifiers, and TUI coordinate mappers, with zero release overhead. (See [Design by Contract (DbC) Integration](plan/done/Design%20by%20Contract%20(DbC)%20Integration.md))

## 11. LCS engine — future investigations

## 12. Code cleanup

- [x] ~~**Remove unnecessary `split('\n')` on row text in apply module**~~: `apply/mod.rs` splits row text on `\n` before iterating, but rows are always single lines from the diff parser. Using the text directly avoids redundant iterations and allocations.
