# diffuse — Roadmap

A prioritized roadmap of outstanding features, engine refinements, and ecosystem integrations, ordered by user value and engineering impact.

---

## 1. Interactive TUI and navigation

*Immediate productivity gains for day-to-day code review and diff inspection.*

1. **In-Diff Text Search (`/` and `?`)**
   - Interactive search prompt: `/` searches forward, `?` searches backward, with live match highlighting and `n`/`N` navigation across files and hunks.
2. **File / Status Filtering (`F` / Quick Filter Modal)**
   - Quick-toggle to filter files by status: only Modified `M`, Added `A`, Untracked `?`, or files with comments or notes.
3. **Interactive Line / Hunk Staging (`s` / `u`)** — Done (`s`/`S` stage, `u`/`U` unstage in TUI)
   - Direct staging and unstaging of individual selected lines, selections (`v`), or whole hunks, without dropping to a shell or editor.
4. **Hunk Folding / Unfolding (`z` / `zo` / `zc`)**
   - Collapse or expand individual hunks independently of the entire file header.
5. **Selection Copy to Clipboard (`y` / `c`)**
   - Copy selected diff rows or visual content (clean text without diff markers) directly to the system clipboard via OSC 52 or the platform clipboard API.

---

## 2. Diff engine and subword alignment refinements

*Core algorithm polish to enhance visual readability on complex rewrites.*

1. **Syntax Classification in Markdown Modified Rows (`.md` word-diff)**
   - Extend fence-aware syntax highlighting to `.md` mod/reflow lines. Context and pure lines currently track fences, but subword mod lines use base diff styling.
2. **Additional Language Profiles (`crates/diffuse-syntax`)**
   - Add language profiles, fence boundaries, and token classifier definitions for Kotlin, Swift, Zig, Scala, Ruby, PHP, and SQL.
3. **Word Diff Tokenizer Punctuation Delimiters**
   - Tune language-specific subword boundaries further (for example, `::`, `->`, `=>`, `?.`) to prevent token over-grouping during nested generic and closure refactors.
4. **Split / Side-by-Side View Mode**
   - Add an optional side-by-side dual-pane rendering mode for wide terminal windows (`--side-by-side` / `tab` switch).

---

## 3. Terminal Markdown and note rendering polish

*Rich visual fidelity for comments, code annotations, and `.md` files.*

1. **Blockquote (`> `) and List (`- `, `* `, `1. `) Styling**
   - Render blockquotes with a muted vertical bar and indent alignment. Style bullets and ordered numbering cleanly in notes and Markdown files.
2. **Table Alignment (`| col | col |`)**
   - Align Markdown tables to terminal column boundaries with subtle border styling.
3. **Multi-line / Nested Markdown Span Parsing**
   - Support nested styles (for example, bold inside links `[**label**](url)`, italic code spans) in review note callouts and status prompts.

---

## 4. CLI, export, and ecosystem integrations

*Interoperability and developer toolchain ergonomics.*

1. **Static HTML / Unified Markdown Export (`diffuse export --format html/md`)**
   - Generate standalone, self-contained HTML reports with the *diffuse* color theme, or clean GitHub-flavored Markdown summaries for pull request descriptions.
2. **User Theme Configuration (`~/.config/diffuse/theme.toml`)**
   - Allow custom terminal color definitions (RGB / 256-color) and palette overrides beyond the built-in dark and light presets.
3. **Performance Profiling on Massive Repositories (>10,000 files)** — Done (streaming worker pool / rayon thread pool in `diffuse-core::streaming`)
   - Add an asynchronous streaming worker pool for initial diff parsing and syntax classification on massive monorepos.
