# Review Notes CLI, Status Lifecycle & Source Snapshots

- [x] Global TODO
  - [x] **Phase 1 — Core Data Model, Persistence & Quoted Replies**
    - [x] Add `NoteStatus` enum (`Open`, `InProgress`, `Resolved`) with `#[serde(rename_all = "camelCase")]` and `#[default] Open`
    - [x] Add `status: NoteStatus` (`#[serde(default, skip_serializing_if = "NoteStatus::is_open")]`) to `ReviewNote` in `crates/diffuse-core/src/notes.rs`
    - [x] Add `source_lines: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`) to `ReviewNote`
    - [x] Implement `reply_to_note_text(original: &str, reply: &str) -> String` blockquoting existing lines
    - [x] Implement `set_note_status_by_id`, `resolve_note_by_id`, `reopen_note_by_id`, and `reply_to_note_by_id` database operations in `crates/diffuse-core/src/notes.rs`
    - [x] Add `in_progress_notes: usize` and `resolved_notes: usize` to `FileCounts` in `crates/diffuse-core/src/wire.rs`
    - [x] Extend stdin JSON protocol in `crates/diffuse/src/main.rs` to support `setNoteStatus`, `resolveNote`, `reopenNote`, and `replyNote`
    - [x] Author unit tests in `diffuse-core` covering serialization, backward compatibility, 3-state transitions, and quoted replying
  - [x] **Phase 2 — CLI `diffuse notes` Subcommand Suite**
    - [x] Add `Notes` subcommand to clap CLI in `crates/diffuse/src/main.rs`
    - [x] Implement `diffuse notes [list]` defaulting to active open notes (`Open` and `InProgress`) with `✅ M resolved` summary footer
    - [x] Support `--resolved`, `--in-progress`, and `--all` filters in `diffuse notes list`
    - [x] Support `--show-source` flag to print original captured source lines alongside note text
    - [x] Support `--json` and `-m` flags on `diffuse notes`
    - [x] Implement `diffuse notes show <id>` showing note metadata, target coordinate, status glyph (`💬`, `🔄`, `✅`), and source lines
    - [x] Implement `diffuse notes reply <id> [--status <open|in-progress|resolved>] [--resolve] "<text>"`
    - [x] Implement `diffuse notes progress <id>` (mark in-progress) and `diffuse notes resolve <id>` (mark resolved)
    - [x] Implement `diffuse notes open <id>` / `diffuse notes reopen <id>` (reset in-progress or resolved note back to open)
    - [x] Implement `diffuse notes status <id> <open|in-progress|resolved>` (explicit status update)
    - [x] Implement `diffuse notes add` / `create` command supporting flexible coordinate parsing (`file:start-end`, `file:line`, `file start [end]`, `--line/--end`), text (`-m`, positional, stdin), and source lines (explicit `--source`, synthesized disk/git, or omitted `--no-source`)
    - [x] Anchor `line_hash` to the *last* line of the source range (`end`), matching the visual attachment point above the callout
    - [x] Author CLI unit and integration tests verifying all command verbs and output formats
  - [x] **Phase 3 — Markdown & JSON Filtering with Statuses & Source Lines**
    - [x] Update `crates/diffuse-render/src/markdown.rs` to default to active notes (`Open` and `InProgress`), emitting `💬 N open (✅ M resolved)` summary
    - [x] Render in-progress notes with `🔄` glyph and resolved notes (when enabled) with `✅`
    - [x] Add `show_source_lines` and `note_filter` options to `MarkdownOptions`
    - [x] Render `source_lines` as a code block within markdown callouts when enabled
    - [x] Include resolved notes in markdown only when `--resolved` or `--all` is set
    - [x] Update wire/pipeline output generation in `crates/diffuse-core/src/pipeline.rs` to track `in_progress_notes` and `resolved_notes` in `FileCounts`
    - [x] Filter `OutputFile.notes` by default to active notes unless `--resolved` or `--all` is specified
    - [x] Author tests in `diffuse-render` and `diffuse-core` validating markdown and JSON output
  - [x] **Phase 4 — TUI Source Capture, 3-State Styling & Keybindings**
    - [x] In `crates/diffuse-tui/src/app/notes.rs`, capture exact selected source lines into `note.source_lines` on note creation
    - [x] Anchor note callout and hash verification to the last line (`sel.end`) of the selection range
    - [x] Render notes by status:
      - `Open`: Amber stripe + `💬` speech bubble
      - `InProgress`: Cyan/blue stripe + `🔄` in-progress glyph
      - `Resolved`: Muted green stripe + `✅` green-on-white checkmark
    - [x] Add `x` keybinding to cycle status on focused note: `Open` $\rightarrow$ `InProgress` $\rightarrow$ `Resolved` $\rightarrow$ `Open`
    - [x] Add `z` keybinding to toggle displaying resolved notes inline in the diff view (hidden by default)
    - [x] Add `S` keybinding / option to toggle displaying original source lines above note text
    - [x] Update file header badges in `crates/diffuse-tui/src/view/header.rs` and `diff_grid.rs` to show `💬N` (open), `🔄P` (in progress), and `✅M` (resolved)
    - [x] Author TUI tests in `diffuse-tui` for status cycling, source capture, and badge formatting
  - [x] **Phase 5 — Full Verification & Golden Invariance**
    - [x] Execute `scripts/check.sh` ensuring zero warnings across fmt, clippy, cargo test, and binary install
    - [x] Verify golden acceptance test suite invariance (`cargo test --test golden`)

---

## Executive Summary

This engineering plan introduces a streamlined, CLI-driven review notes workflow in `diffuse` with support for full task lifecycle tracking:
1. **3-State Note Lifecycle**: Notes carry a `NoteStatus` enum:
   - `Open` (`💬`): standard pending feedback.
   - `InProgress` (`🔄`): feedback currently being addressed (e.g. for long-running changes). Counts as open in diff totals and displays alongside open notes.
   - `Resolved` (`✅`): completed feedback using the green-on-white checkmark matching `TaskState::Checked`.
2. **Default Filtering**: The system defaults to showing active notes (`Open` and `InProgress`) across TUI, CLI, Markdown, and JSON, with summary counts of resolved notes (`✅ M resolved`). Resolved notes are only shown when explicitly requested (`--resolved`, `--all`, or TUI toggle `z`).
3. **Original Source Lines Snapshot**: On note creation in the TUI, the exact source lines under the selection are captured in an optional `source_lines` field. An option `--show-source` (and TUI toggle `S`) displays this original code snippet alongside the note, preserving context even when code changes around it.
4. **Quoted Replying**: Replying to a note prepends the reply and blockquotes the previous note text, naturally leveraging diffuse's existing Markdown renderer.
5. **Dedicated CLI (`diffuse notes`)**: Direct command-line and JSON API inspection, status transitions, and quoting.

---

## Table of Contents

1. [Phase 1 — Core Data Model, Persistence & Quoted Replies](#1-phase-1--core-data-model-persistence--quoted-replies)
2. [Phase 2 — CLI `diffuse notes` Subcommand Suite](#2-phase-2--cli-diffuse-notes-subcommand-suite)
3. [Phase 3 — Markdown & JSON Filtering with Statuses & Source Lines](#3-phase-3--markdown--json-filtering-with-statuses--source-lines)
4. [Phase 4 — TUI Source Capture, 3-State Styling & Keybindings](#4-phase-4--tui-source-capture-3-state-styling--keybindings)
5. [Phase 5 — Full Verification & Golden Invariance](#5-phase-5--full-verification--golden-invariance)

---

## 1. Phase 1 — Core Data Model, Persistence & Quoted Replies

### Data Model Updates in `crates/diffuse-core/src/notes.rs`
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NoteStatus {
    #[default]
    Open,
    InProgress,
    Resolved,
}

impl NoteStatus {
    pub const fn is_open(&self) -> bool {
        matches!(self, Self::Open)
    }

    pub const fn is_in_progress(&self) -> bool {
        matches!(self, Self::InProgress)
    }

    pub const fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved)
    }

    /// Whether this status counts as active/open (Open or InProgress).
    pub const fn counts_as_open(&self) -> bool {
        matches!(self, Self::Open | Self::InProgress)
    }

    /// Next status in the cyclic toggle: Open -> InProgress -> Resolved -> Open.
    pub const fn cycle(&self) -> Self {
        match self {
            Self::Open => Self::InProgress,
            Self::InProgress => Self::Resolved,
            Self::Resolved => Self::Open,
        }
    }

    pub const fn glyph(&self) -> &'static str {
        match self {
            Self::Open => "💬",
            Self::InProgress => "🔄",
            Self::Resolved => "✅",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewNote {
    pub id: String,
    pub file: String,
    pub side: Side,
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_hash: Option<LineHash>,

    /// Note lifecycle status: Open (default), InProgress, or Resolved.
    #[serde(default, skip_serializing_if = "NoteStatus::is_open")]
    pub status: NoteStatus,

    /// Exact original source lines selected when the note was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_lines: Option<String>,
}
```

### Invariants & Backward Compatibility
- Existing entries in `notes.json` deserialize with `status: NoteStatus::Open` and `source_lines: None`.
- Serializing a note with `status: NoteStatus::Open` omits `status`, keeping disk files compact and fully backward-compatible.

### Quoted Reply Function
```rust
pub fn reply_to_note_text(original: &str, reply: &str) -> String {
    let quoted = original
        .lines()
        .map(|l| format!("> {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{reply}\n\n{quoted}")
}
```

### Database Operations
- `set_note_status_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str, status: NoteStatus) -> bool`
- `resolve_note_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str) -> bool`
- `reopen_note_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str) -> bool`
- `reply_to_note_by_id(db: &mut NotesDB, repo: &str, file: &str, note_id: &str, reply: &str, status: Option<NoteStatus>) -> bool`

### Wire Metrics in `crates/diffuse-core/src/wire.rs`
- Add `pub in_progress_notes: usize` and `pub resolved_notes: usize` to `FileCounts`.

---

## 2. Phase 2 — CLI `diffuse notes` Subcommand Suite

### Clap Subcommand in `crates/diffuse/src/main.rs`
```rust
#[derive(Subcommand, Debug)]
enum SubCommand {
    Review { ... },
    Parse { ... },
    /// Query, inspect, reply to, and update review notes.
    Notes(NotesArgs),
}

#[derive(Args, Debug)]
struct NotesArgs {
    #[command(subcommand)]
    action: Option<NotesAction>,

    /// Filter notes: 'open' (default: Open + InProgress), 'in-progress', 'resolved', or 'all'.
    #[arg(long, default_value = "open")]
    status: NotesFilter,

    /// Show original source lines captured when notes were created.
    #[arg(long)]
    show_source: bool,

    /// Emit notes as JSON.
    #[arg(long)]
    json: bool,

    /// Emit notes as Markdown.
    #[arg(short = 'm', long)]
    markdown: bool,

    /// Optional target file filter.
    #[arg(long)]
    file: Option<String>,
}

#[derive(Subcommand, Debug)]
enum NotesAction {
    /// List review notes (default action).
    List,
    /// Create a new review note.
    Add {
        target: Option<String>,
        line: Option<u32>,
        end: Option<u32>,
        pos2: Option<String>,
        pos3: Option<String>,
        file: Option<String>,
        side: String,
        old: bool,
        message: Option<String>,
        text: Option<String>,
        source: Option<String>,
        no_source: bool,
        status: String,
        json: bool,
    },
    /// Inspect details and original source context for a specific note.
    Show { note_id: String },
    /// Prepend a reply and blockquote previous text on a note.
    Reply {
        note_id: String,
        text: String,
        #[arg(long)]
        status: Option<NoteStatus>,
        #[arg(long)]
        resolve: bool,
    },
    /// Mark a note as in progress.
    Progress { note_id: String },
    /// Mark a note as resolved.
    Resolve { note_id: String },
    /// Reset an in-progress or resolved note back to open.
    Open { note_id: String },
    /// Alias for Open.
    Reopen { note_id: String },
    /// Set a note's lifecycle status explicitly.
    Status {
        note_id: String,
        status: NoteStatus,
    },
}
```

### Output Formatting
- Human-readable default lists active notes:
  ```text
  [open] 💬 shift-semantic-boundary-1 · crates/diffuse-core/src/shift.rs:21 (new)
  Semantic boundary scoring provides natural breaking heuristic for hunk splits.

  [in-progress] 🔄 shift-normalize-context-1 · crates/diffuse-core/src/shift.rs:84 (new)
  Context normalization also merges adjacent synthetic hunks created by review notes.

  2 active notes (1 open, 1 in progress 🔄) · ✅ 1 resolved (use --all to show)
  ```
- When `--show-source` is supplied:
  ```text
  [open] 💬 shift-semantic-boundary-1 · crates/diffuse-core/src/shift.rs:21 (new)
    Source:
      pub fn semantic_boundary_score(
    Note:
      Semantic boundary scoring provides natural breaking heuristic for hunk splits.
  ```

---

## 3. Phase 3 — Markdown & JSON Filtering with Statuses & Source Lines

### Markdown Formatting in `crates/diffuse-render/src/markdown.rs`
- In `render_file_markdown`, active notes (`Open` and `InProgress`) render by default.
- For each note:
  ```markdown
  > [!NOTE]
  > 💬 **Line 21**: Semantic boundary scoring provides natural breaking heuristic.
  ```
  Or for in-progress:
  ```markdown
  > [!NOTE]
  > 🔄 **Line 84** (in progress): Refactoring context normalization.
  ```
  If `options.show_source_lines` is true and `note.source_lines` is present:
  ```markdown
  > [!NOTE]
  > 💬 **Line 21**:
  > ```rust
  > pub fn semantic_boundary_score(
  > ```
  > Semantic boundary scoring provides natural breaking heuristic.
  ```
- If resolved notes exist, summary note badge is appended:
  ```markdown
  > ✅ 1 resolved note omitted (use --all to show)
  ```

### JSON Stream Wire Representation
- `OutputFile.notes` contains active notes by default (`Open` and `InProgress`).
- `OutputFile.counts` includes:
  - `notes`: total active notes (`Open` + `InProgress`).
  - `in_progress_notes`: count of in-progress notes.
  - `resolved_notes`: count of resolved notes.

---

## 4. Phase 4 — TUI Source Capture, 3-State Styling & Keybindings

### Source Line Capture in `crates/diffuse-tui/src/app/notes.rs`
In `commit_note_draft()`:
- When a selection or row coordinate is active:
  - Extract the exact lines from `file.new_content` or `file.old_content` spanning `start..=end`.
  - Populate `note.source_lines = Some(selected_text)`.

### 3-State Presentation
- `Open`:
  - Stripe: amber (`Rgb(250, 75, 40)`).
  - Prefix glyph: `💬`.
- `InProgress`:
  - Stripe: cyan (`Rgb(60, 180, 240)`).
  - Prefix glyph: `🔄`.
- `Resolved`:
  - Stripe: muted dark green (`Rgb(40, 140, 60)`).
  - Prefix glyph: `✅`.

### TUI Keybindings & State
- `x`: Cycle status: `Open` $\rightarrow$ `InProgress` $\rightarrow$ `Resolved` $\rightarrow$ `Open` on focused note; immediate persistence to `notes.json`.
- `z`: Toggle visibility of resolved notes in the diff view (`self.show_resolved_notes: bool`, default `false`).
- `S`: Toggle inline display of original source lines within note callouts (`self.show_source_lines: bool`, default `false`).
- Badges: Header bars display `💬N` (open), `🔄P` (in progress), and `✅M` (resolved).

---

## 5. Phase 5 — Full Verification & Golden Invariance

### Verification Discipline
1. Unit tests across `crates/diffuse-core`, `crates/diffuse-render`, `crates/diffuse-tui`, and `crates/diffuse`.
2. 100% branch coverage on all new match arms and status filters.
3. Verification gate: `./scripts/check.sh` must complete cleanly with zero warnings.
4. Golden test suite: `cargo test --test golden` must remain 100% byte-identical.

---

## Execution Status & Verification Checklist

- [x] **Phase 1: Core Data Model & Persistence**
  - [x] `NoteStatus` (`Open`, `InProgress`, `Resolved`) enum with serde compatibility (`skip_serializing_if = "NoteStatus::is_open"`).
  - [x] `ReviewNote.source_lines: Option<String>` with serde compatibility (`skip_serializing_if = "Option::is_none"`).
  - [x] `FileCounts.in_progress_notes` and `FileCounts.resolved_notes` tracking.
  - [x] `reply_to_note_text` prepends reply and blockquotes previous text (`> ...`).
  - [x] DB helpers: `set_note_status_by_id`, `resolve_note_by_id`, `reopen_note_by_id`.
  - [x] Stdin JSON protocol commands: `setNoteStatus`, `resolveNote`, `reopenNote`, `replyNote`.
- [x] **Phase 2: CLI `diffuse notes` Subcommand Suite**
  - [x] `list`: Active notes default, `--resolved`, `--in-progress`, `--all` / `-a`, `--show-source`, `--json`, `-m`, summary badge with resolved count.
  - [x] `add` / `create`: Note creation with flexible coordinates (`file:line[-end]`, `file line [end]`, `--line`), text (`-m`, positional, stdin), `--side` (`new` / `old`), `--source` / synthesized source lines / `--no-source`, `--status`, `--json`.
  - [x] `show <id>`: Note details with `--show-source`, `--json`, `-m`.
  - [x] `reply <id>`: Trailing argument or stdin message with `--status` or `--resolve`.
  - [x] `progress <id>`, `resolve <id>`, `open <id>`, `reopen <id>`, `status <id> <status>`.
- [x] **Phase 3: Wire Protocol, Pipeline & Markdown Exporter**
  - [x] Wire `OutputNote` contains `status` and `source_lines`.
  - [x] `PipelineOptions.with_show_resolved`: Filters resolved notes by default while maintaining accurate counts.
  - [x] Markdown exporter formats status glyphs (`💬`, `🔄`, `✅`), fenced source snippets, and omitted summary notices.
- [x] **Phase 4: TUI 3-State Styling, Filtering & Keybindings**
  - [x] `AppState.show_resolved_notes` and `AppState.show_source_lines` toggles.
  - [x] Status-specific stripe and glyph colors (Amber `💬`, Cyan `🔄`, Muted Green `✅`).
  - [x] Source code block rendering when enabled.
  - [x] `x`: Cycle note status (`Open` $\rightarrow$ `InProgress` $\rightarrow$ `Resolved` $\rightarrow$ `Open`).
  - [x] `z`: Toggle showing resolved notes.
  - [x] `S`: Toggle showing source lines.
  - [x] Help modal keybinding documentation.
- [x] **Phase 5: CLI Top-Level Flags & Full Verification Gate**
  - [x] `--resolved`, `--all` / `-a` / `--all-notes`, `--show-source` flags on `Args` and `Parse`.
  - [x] `./scripts/check.sh` verification gate passed cleanly with zero warnings.
  - [x] Golden tests (`cargo test --test golden`): all 23 acceptance baselines passed byte-for-byte.
