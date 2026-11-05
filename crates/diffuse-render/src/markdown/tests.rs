use diffuse_syntax::color::TokenColor;

use super::*;
use crate::{
    Cursor, DiffKind, DiffuseTheme,
    ansi::ColorExt,
    core::{
        counts::FileCounts,
        merge::RunKind,
        notes::NoteStatus,
        parse::RowKind,
        wire::{MovSide, OutputFile, OutputHunk, OutputNote, OutputRow, OutputRun, PipelineOutput},
    },
};

#[test]
fn test_markdown_options_default_theme() {
    assert_eq!(MarkdownOptions::default().theme, DiffuseTheme::dark());
}

fn sample_output() -> PipelineOutput {
    PipelineOutput {
        files: vec![
            OutputFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: diffuse_core::FileStatus::Modified,
                new_line_count: 10,
                old_line_count: 10,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![OutputHunk {
                    old_start: 1,
                    old_count: 3,
                    new_start: 1,
                    new_count: 3,
                    rows: vec![
                        OutputRow {
                            kind: RowKind::Ctx,
                            mov_side: None,
                            old_line: Some(1),
                            new_line: Some(1),
                            runs: vec![OutputRun {
                                kind: RunKind::Ctx,
                                color: TokenColor::Keyword,
                                sgr: None,
                                text: "fn main() {".into(),
                            }],
                            wholesale: false,
                            group: None,
                        },
                        OutputRow {
                            kind: RowKind::Mod,
                            mov_side: None,
                            old_line: Some(2),
                            new_line: Some(2),
                            runs: vec![
                                OutputRun {
                                    kind: RunKind::Del,
                                    color: TokenColor::String,
                                    sgr: None,
                                    text: "    println!(\"Hello World!\");".into(),
                                },
                                OutputRun {
                                    kind: RunKind::Add,
                                    color: TokenColor::String,
                                    sgr: None,
                                    text: "    println!(\"Hello diffuse!\");".into(),
                                },
                            ],
                            wholesale: false,
                            group: None,
                        },
                        OutputRow {
                            kind: RowKind::Ctx,
                            mov_side: None,
                            old_line: Some(3),
                            new_line: Some(3),
                            runs: vec![OutputRun {
                                kind: RunKind::Ctx,
                                color: TokenColor::Default,
                                sgr: None,
                                text: "}".into(),
                            }],
                            wholesale: false,
                            group: None,
                        },
                    ],
                }],
                raw_hunks: vec![],
                counts: FileCounts {
                    adds: 1,
                    dels: 1,
                    mods: 1,
                    movs: 0,
                    notes: 0,
                    ..Default::default()
                },
                fences: vec![],
                notes: vec![OutputNote {
                    id: "n1".to_string(),
                    file: "src/main.rs".to_string(),
                    location: diffuse_core::notes::NoteLocation::line(
                        diffuse_core::notes::DiffCoord::new_side(2),
                    ),
                    text: "Great greeting update!".to_string(),
                    created_at: 123456789,
                    runs: vec![],
                    content_hash: None,
                    status: NoteStatus::Open,
                    original_source_rows: Vec::new(),
                }],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: Some(0),
                new_bytes: Some(0),
                raw_source: None,
            },
            OutputFile {
                path: "README.md".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: diffuse_core::FileStatus::Added,
                new_line_count: 5,
                old_line_count: 0,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![OutputHunk {
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 2,
                    rows: vec![
                        OutputRow {
                            kind: RowKind::Add,
                            mov_side: None,
                            old_line: None,
                            new_line: Some(1),
                            runs: vec![OutputRun {
                                kind: RunKind::Add,
                                color: TokenColor::Italic,
                                sgr: Some(14),
                                text: "# Diffuse".into(),
                            }],
                            wholesale: false,
                            group: None,
                        },
                        OutputRow {
                            kind: RowKind::Add,
                            mov_side: None,
                            old_line: None,
                            new_line: Some(2),
                            runs: vec![OutputRun {
                                kind: RunKind::Add,
                                color: TokenColor::Default,
                                sgr: None,
                                text: "Next gen diffs.".into(),
                            }],
                            wholesale: false,
                            group: None,
                        },
                    ],
                }],
                raw_hunks: vec![],
                counts: FileCounts {
                    adds: 2,
                    dels: 0,
                    mods: 0,
                    movs: 0,
                    notes: 0,
                    ..Default::default()
                },
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: None,
                new_bytes: Some(0),
                raw_source: None,
            },
        ],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    }
}

#[test]
fn test_render_markdown_summary_table() {
    let output = sample_output();
    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);

    assert!(md.contains("### Diff Summary"));
    assert!(md.contains("**2 files changed**: **-1** deletions, **+3** additions"));
    assert!(md.contains("| `src/main.rs` | `0B` | -1 | +1 |"));
    assert!(md.contains("| `README.md` | `added, 0B` | -0 | +2 |"));
}

#[test]
fn test_render_markdown_collapsible_details() {
    let output = sample_output();
    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);

    assert!(md.contains("<details open>"));
    assert!(md.contains("<summary><b>src/main.rs</b> <code>(0B: -1 +1)</code></summary>"));
    assert!(md.contains("</details>"));
}

#[test]
fn test_render_markdown_diff_code_blocks() {
    let output = sample_output();
    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);

    assert!(md.contains("```diff\n@@ -1,3 +1,3 @@\n fn main() {\n-    println!(\"Hello World!\");\n+    println!(\"Hello diffuse!\");\n }\n```"));
    assert!(md.contains("```diff\n@@ -0,0 +1,2 @@\n+# Diffuse\n+Next gen diffs.\n```"));
}

#[test]
fn test_render_markdown_review_notes_callouts() {
    let output = sample_output();
    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);

    assert!(md.contains("> [!NOTE]"));
    assert!(md.contains("> 💬 **Line 2**: Great greeting update!"));
}

#[test]
fn test_render_markdown_colorized() {
    let output = sample_output();
    let opts = MarkdownOptions {
        colorize: true,
        ..MarkdownOptions::default()
    };
    let md = render_markdown_output(&output, &opts);

    // Must contain ANSI escape sequences
    assert!(md.contains("\x1b["));
    assert!(md.contains("\x1b[0m"));
    // Must contain the styled markdown summary
    assert!(md.contains("Diff Summary"));
    assert!(md.contains("src/main.rs"));
}

#[test]
fn test_render_markdown_empty_output() {
    let output = PipelineOutput {
        files: vec![],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };
    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);

    assert_eq!(md, "*No changes detected.*\n");
}

#[test]
fn test_render_markdown_options_variations() {
    let output = sample_output();

    // 1. Non-collapsible without summary table
    let opts_no_summary_no_collapse = MarkdownOptions {
        summary_table: false,
        collapsible: false,
        default_open: false,
        include_notes: false,
        show_resolved_notes: false,
        show_source: false,
        colorize: false,
        theme: DiffuseTheme::dark(),
    };
    let md1 = render_markdown_output(&output, &opts_no_summary_no_collapse);
    assert!(!md1.contains("### Diff Summary"));
    assert!(!md1.contains("<details"));
    assert!(md1.contains("### src/main.rs `(0B: -1 +1)`"));
    assert!(!md1.contains("> [!NOTE]"));

    // 2. Collapsible with default_open = false
    let opts_closed = MarkdownOptions {
        summary_table: true,
        collapsible: true,
        default_open: false,
        include_notes: true,
        show_resolved_notes: false,
        show_source: false,
        colorize: false,
        theme: DiffuseTheme::dark(),
    };
    let md2 = render_markdown_output(&output, &opts_closed);
    assert!(md2.contains("<details>"));
    assert!(!md2.contains("<details open>"));
}

#[test]
fn test_render_markdown_single_file_and_multi_line_notes_and_empty_hunks() {
    let output = PipelineOutput {
        files: vec![
            OutputFile {
                path: "src/lib.rs".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: diffuse_core::FileStatus::Deleted,
                new_line_count: 0,
                old_line_count: 3,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![OutputHunk {
                    old_start: 1,
                    old_count: 2,
                    new_start: 0,
                    new_count: 0,
                    rows: vec![
                        OutputRow {
                            kind: RowKind::File,
                            mov_side: None,
                            old_line: None,
                            new_line: None,
                            runs: vec![],
                            wholesale: false,
                            group: None,
                        },
                        OutputRow {
                            kind: RowKind::Del,
                            mov_side: None,
                            old_line: Some(1),
                            new_line: None,
                            runs: vec![OutputRun {
                                kind: RunKind::Del,
                                color: TokenColor::Default,
                                sgr: None,
                                text: "old line".into(),
                            }],
                            wholesale: false,
                            group: None,
                        },
                        OutputRow {
                            kind: RowKind::Sep,
                            mov_side: None,
                            old_line: None,
                            new_line: None,
                            runs: vec![],
                            wholesale: false,
                            group: None,
                        },
                    ],
                }],
                raw_hunks: vec![],
                counts: FileCounts {
                    adds: 0,
                    dels: 1,
                    mods: 0,
                    movs: 0,
                    notes: 0,
                    ..Default::default()
                },
                fences: vec![],
                notes: vec![OutputNote {
                    id: "n2".to_string(),
                    file: "src/lib.rs".to_string(),
                    location: diffuse_core::notes::NoteLocation::range(
                        diffuse_core::notes::DiffCoord::old_side(10),
                        diffuse_core::notes::DiffCoord::old_side(15),
                    ),
                    text: "Multi-line removal comment".to_string(),
                    created_at: 987654321,
                    runs: vec![],
                    content_hash: None,
                    status: NoteStatus::Open,
                    original_source_rows: Vec::new(),
                }],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: Some(0),
                new_bytes: Some(0),
                raw_source: None,
            },
            OutputFile {
                path: "empty.txt".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: diffuse_core::FileStatus::Modified,
                new_line_count: 0,
                old_line_count: 0,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![],
                raw_hunks: vec![],
                counts: FileCounts::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: Some(0),
                new_bytes: Some(0),
                raw_source: None,
            },
        ],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let md = render_markdown_output(&output, &MarkdownOptions::default());
    assert!(md.contains("-old line"));
    assert!(md.contains("> 💬 **Lines 10-15**: Multi-line removal comment"));
    assert!(md.contains("# (No diff content)"));

    // Test single file summary
    let single_file_out = PipelineOutput {
        files: vec![output.files[0].clone()],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };
    let single_md = render_markdown_output(&single_file_out, &MarkdownOptions::default());
    assert!(single_md.contains("**1 file changed**"));

    // Test with colorize
    let color_md = render_markdown_output(
        &output,
        &MarkdownOptions {
            colorize: true,
            ..MarkdownOptions::default()
        },
    );
    assert!(color_md.contains("\x1b["));
}

#[test]
fn test_render_markdown_language_profile_syntax_highlighting() {
    let output = PipelineOutput {
        files: vec![OutputFile {
            path: "src/algo.rs".to_string(),
            old_path: None,
            old_mode: None,
            new_mode: None,
            status: diffuse_core::FileStatus::Modified,
            new_line_count: 5,
            old_line_count: 5,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![OutputHunk {
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                rows: vec![OutputRow {
                    kind: RowKind::Mod,
                    mov_side: None,
                    old_line: Some(1),
                    new_line: Some(1),
                    runs: vec![
                        OutputRun {
                            kind: RunKind::Del,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "pub fn old_algo() -> bool { false }".into(),
                        },
                        OutputRun {
                            kind: RunKind::Add,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "pub fn new_algo() -> bool { true }".into(),
                        },
                    ],
                    wholesale: false,
                    group: None,
                }],
            }],
            raw_hunks: vec![],
            counts: FileCounts {
                adds: 1,
                dels: 1,
                mods: 1,
                movs: 0,
                notes: 0,
                ..Default::default()
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
            is_binary: false,
            old_bytes: Some(0),
            new_bytes: Some(0),
            raw_source: None,
        }],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let md = render_markdown_output(
        &output,
        &MarkdownOptions {
            colorize: true,
            ..MarkdownOptions::default()
        },
    );
    // "pub", "fn", "bool", "true", "false" are colorized with ANSI codes
    assert!(md.contains("\x1b["));
    assert!(md.contains("new_algo"));
    assert!(md.contains("old_algo"));
    assert!(md.contains("bool"));
}

#[test]
fn test_render_markdown_untracked_files_collapsed_by_default() {
    let output = PipelineOutput {
        files: vec![
            OutputFile {
                path: "tracked.rs".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: diffuse_core::FileStatus::Modified,
                new_line_count: 1,
                old_line_count: 1,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![],
                raw_hunks: vec![],
                counts: FileCounts::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: Some(0),
                new_bytes: Some(0),
                raw_source: None,
            },
            OutputFile {
                path: "untracked.rs".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: diffuse_core::FileStatus::Untracked,
                new_line_count: 1,
                old_line_count: 0,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![],
                raw_hunks: vec![],
                counts: FileCounts::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: Some(0),
                new_bytes: Some(0),
                raw_source: None,
            },
        ],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);
    // Tracked file has <details open>
    assert!(md.contains("<details open>\n<summary><b>tracked.rs</b>"));
    // Untracked file has <details> (collapsed by default)
    assert!(md.contains("<details>\n<summary><b>untracked.rs</b>"));
}

#[test]
fn test_render_markdown_renamed_files_show_arrow_and_collapsed_by_default() {
    let output = PipelineOutput {
        files: vec![OutputFile {
            path: "src/new.rs".to_string(),
            old_path: Some("src/old.rs".to_string()),
            old_mode: None,
            new_mode: None,
            status: diffuse_core::FileStatus::Renamed,
            new_line_count: 5,
            old_line_count: 5,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![],
            raw_hunks: vec![],
            counts: FileCounts::default(),
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
            is_binary: false,
            old_bytes: Some(0),
            new_bytes: Some(0),
            raw_source: None,
        }],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);
    // Table shows old -> new path
    assert!(md.contains("| `src/old.rs -> new.rs` | `renamed, 0B` | -0 | +0 |"));
    // Renamed file with 0 changes has <details> (collapsed by default) and
    // summary with old -> new path
    assert!(md.contains("<details>\n<summary><b>src/old.rs -> new.rs</b>"));

    // Renamed file WITH content changes is open by default
    let output_with_changes = PipelineOutput {
        files: vec![OutputFile {
            path: "src/new.rs".to_string(),
            old_path: Some("src/old.rs".to_string()),
            old_mode: None,
            new_mode: None,
            status: diffuse_core::FileStatus::Renamed,
            new_line_count: 5,
            old_line_count: 5,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![OutputHunk {
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                rows: vec![OutputRow {
                    kind: RowKind::Add,
                    mov_side: None,
                    new_line: Some(1),
                    old_line: None,
                    runs: vec![OutputRun {
                        kind: RunKind::Add,
                        color: TokenColor::Default,
                        sgr: None,
                        text: "added".into(),
                    }],
                    wholesale: false,
                    group: None,
                }],
            }],
            raw_hunks: vec![],
            counts: FileCounts {
                adds: 1,
                ..Default::default()
            },
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
            is_binary: false,
            old_bytes: Some(0),
            new_bytes: Some(0),
            raw_source: None,
        }],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };
    let md_changes = render_markdown_output(&output_with_changes, &opts);
    assert!(md_changes.contains("<details open>\n<summary><b>src/old.rs -> new.rs</b>"));
}

#[test]
fn test_render_markdown_fenced_code_blocks_syntax_highlighting() {
    let output = PipelineOutput {
        files: vec![OutputFile {
            path: "README.md".to_string(),
            old_path: None,
            old_mode: None,
            new_mode: None,
            status: diffuse_core::FileStatus::Modified,
            new_line_count: 4,
            old_line_count: 4,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![OutputHunk {
                old_start: 1,
                old_count: 4,
                new_start: 1,
                new_count: 4,
                rows: vec![
                    OutputRow {
                        kind: RowKind::Del,
                        mov_side: None,
                        old_line: Some(1),
                        new_line: None,
                        runs: vec![OutputRun {
                            kind: RunKind::Del,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "```rust".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                    OutputRow {
                        kind: RowKind::Del,
                        mov_side: None,
                        old_line: Some(2),
                        new_line: None,
                        runs: vec![OutputRun {
                            kind: RunKind::Del,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "pub struct Config {".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                    OutputRow {
                        kind: RowKind::Del,
                        mov_side: None,
                        old_line: Some(3),
                        new_line: None,
                        runs: vec![OutputRun {
                            kind: RunKind::Del,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "    pub enabled: bool,".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                    OutputRow {
                        kind: RowKind::Del,
                        mov_side: None,
                        old_line: Some(4),
                        new_line: None,
                        runs: vec![OutputRun {
                            kind: RunKind::Del,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "```".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                ],
            }],
            raw_hunks: vec![],
            counts: FileCounts::default(),
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
            is_binary: false,
            old_bytes: Some(0),
            new_bytes: Some(0),
            raw_source: None,
        }],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let md = render_markdown_output(
        &output,
        &MarkdownOptions {
            colorize: true,
            ..MarkdownOptions::default()
        },
    );
    // "pub" and "struct" must be highlighted as keywords (\x1b[38;5;147m in
    // default theme)
    assert!(md.contains("\x1b[38;5;147mpub\x1b[39m"));
    assert!(md.contains("\x1b[38;5;147mstruct\x1b[39m"));
    // "Config" must be highlighted as type (\x1b[38;5;214m in default theme
    // ctx/mod)
    assert!(md.contains("Config"));
}

#[test]
fn test_render_markdown_diff_prefix_resets_fg_color() {
    let output = PipelineOutput {
        files: vec![OutputFile {
            path: "plan.md".to_string(),
            old_path: None,
            old_mode: None,
            new_mode: None,
            status: diffuse_core::FileStatus::Modified,
            new_line_count: 1,
            old_line_count: 1,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![OutputHunk {
                old_start: 1,
                old_count: 1,
                new_start: 0,
                new_count: 0,
                rows: vec![OutputRow {
                    kind: RowKind::Del,
                    mov_side: None,
                    old_line: Some(1),
                    new_line: None,
                    runs: vec![OutputRun {
                        kind: RunKind::Del,
                        color: TokenColor::Default,
                        sgr: None,
                        text: "1. Export `pub mod markdown;` from crate.".into(),
                    }],
                    wholesale: false,
                    group: None,
                }],
            }],
            raw_hunks: vec![],
            counts: FileCounts::default(),
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
            is_binary: false,
            old_bytes: Some(0),
            new_bytes: Some(0),
            raw_source: None,
        }],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let md = render_markdown_output(
        &output,
        &MarkdownOptions {
            colorize: true,
            ..MarkdownOptions::default()
        },
    );
    // The prefix '-' is red; the following plain Markdown text uses the
    // explicit context foreground rather than the terminal default.
    assert!(md.contains("\x1b[38;5;203m-\x1b[39m\x1b[38;5;231m1. \x1b[39m\x1b[38;5;231mExport "));
}

#[test]
fn test_render_markdown_raw_hunks_deletions_before_additions() {
    let output = PipelineOutput {
        files: vec![OutputFile {
            path: "src/lib.rs".to_string(),
            old_path: None,
            old_mode: None,
            new_mode: None,
            status: diffuse_core::FileStatus::Modified,
            new_line_count: 3,
            old_line_count: 2,
            new_content: vec![],
            old_content: vec![],
            hunks: vec![],
            raw_hunks: vec![OutputHunk {
                old_start: 1,
                old_count: 2,
                new_start: 1,
                new_count: 3,
                rows: vec![
                    OutputRow {
                        kind: RowKind::Del,
                        mov_side: None,
                        old_line: Some(1),
                        new_line: None,
                        runs: vec![OutputRun {
                            kind: RunKind::Del,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "old line 1".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                    OutputRow {
                        kind: RowKind::Del,
                        mov_side: None,
                        old_line: Some(2),
                        new_line: None,
                        runs: vec![OutputRun {
                            kind: RunKind::Del,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "old line 2".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                    OutputRow {
                        kind: RowKind::Add,
                        mov_side: None,
                        old_line: None,
                        new_line: Some(1),
                        runs: vec![OutputRun {
                            kind: RunKind::Add,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "new line 1".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                    OutputRow {
                        kind: RowKind::Add,
                        mov_side: None,
                        old_line: None,
                        new_line: Some(2),
                        runs: vec![OutputRun {
                            kind: RunKind::Add,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "new line 2".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                    OutputRow {
                        kind: RowKind::Add,
                        mov_side: None,
                        old_line: None,
                        new_line: Some(3),
                        runs: vec![OutputRun {
                            kind: RunKind::Add,
                            color: TokenColor::Default,
                            sgr: None,
                            text: "new line 3".into(),
                        }],
                        wholesale: false,
                        group: None,
                    },
                ],
            }],
            counts: FileCounts::default(),
            fences: vec![],
            notes: vec![],
            content_runs: vec![],
            rows: vec![],
            is_binary: false,
            old_bytes: Some(0),
            new_bytes: Some(0),
            raw_source: None,
        }],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let md = render_markdown_output(&output, &MarkdownOptions::default());
    let expected_diff = "```diff\n@@ -1,2 +1,3 @@\n-old line 1\n-old line 2\n+new line 1\n+new line 2\n+new line 3\n```";
    assert!(md.contains(expected_diff));
}

#[test]
fn test_render_markdown_paths_preserve_spaces_and_indicate_tabs() {
    let output = PipelineOutput {
        files: vec![
            OutputFile {
                path: "file with space ".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: diffuse_core::FileStatus::Modified,
                new_line_count: 1,
                old_line_count: 1,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![],
                raw_hunks: vec![],
                counts: FileCounts::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: Some(0),
                new_bytes: Some(0),
                raw_source: None,
            },
            OutputFile {
                path: "file\twith\ttab".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: diffuse_core::FileStatus::Added,
                new_line_count: 1,
                old_line_count: 0,
                new_content: vec![],
                old_content: vec![],
                hunks: vec![],
                raw_hunks: vec![],
                counts: FileCounts::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
                is_binary: false,
                old_bytes: Some(0),
                new_bytes: Some(0),
                raw_source: None,
            },
        ],
        branch: "main".to_string(),
        untracked_skipped: vec![],
    };

    let md = render_markdown_output(&output, &MarkdownOptions::default());
    assert!(md.contains("`file with space `"));
    assert!(md.contains("<summary><b>file with space </b>"));
    assert!(md.contains("`file⇥with⇥tab`"));
    assert!(md.contains("<summary><b>file⇥with⇥tab</b>"));
}

#[test]
fn test_render_markdown_note_statuses() {
    let mut output = sample_output();
    output.files[0].notes = vec![
        OutputNote {
            id: "n_open".to_string(),
            file: "src/main.rs".to_string(),
            location: diffuse_core::notes::NoteLocation::line(
                diffuse_core::notes::DiffCoord::new_side(2),
            ),
            text: "Open comment".to_string(),
            created_at: 100,
            runs: vec![],
            content_hash: None,
            status: NoteStatus::Open,
            original_source_rows: Vec::new(),
        },
        OutputNote {
            id: "n_in_progress".to_string(),
            file: "src/main.rs".to_string(),
            location: diffuse_core::notes::NoteLocation::line(
                diffuse_core::notes::DiffCoord::new_side(5),
            ),
            text: "In progress comment".to_string(),
            created_at: 200,
            runs: vec![],
            content_hash: None,
            status: NoteStatus::InProgress,
            original_source_rows: Vec::new(),
        },
        OutputNote {
            id: "n_resolved".to_string(),
            file: "src/main.rs".to_string(),
            location: diffuse_core::notes::NoteLocation::line(
                diffuse_core::notes::DiffCoord::new_side(8),
            ),
            text: "Resolved comment".to_string(),
            created_at: 300,
            runs: vec![],
            content_hash: None,
            status: NoteStatus::Resolved,
            original_source_rows: Vec::new(),
        },
    ];

    // Default: active only (Open and InProgress)
    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);
    assert!(md.contains("> 💬 **Line 2**: Open comment"));
    assert!(md.contains("> 🔄 **Line 5**: In progress comment"));
    assert!(!md.contains("> ✅ **Line 8**: Resolved comment"));

    // With show_resolved_notes: true
    let opts_all = MarkdownOptions {
        show_resolved_notes: true,
        ..MarkdownOptions::default()
    };
    let md_all = render_markdown_output(&output, &opts_all);
    assert!(md_all.contains("> 💬 **Line 2**: Open comment"));
    assert!(md_all.contains("> 🔄 **Line 5**: In progress comment"));
    assert!(md_all.contains("> ✅ **Line 8**: Resolved comment"));
}

#[test]
fn test_render_markdown_resolved_notes_omitted_summary() {
    let mut output = sample_output();
    output.files[0].counts.resolved_notes = 2;
    output.files[0].notes = vec![OutputNote {
        id: "n_open".to_string(),
        file: "src/main.rs".to_string(),
        location: diffuse_core::notes::NoteLocation::line(
            diffuse_core::notes::DiffCoord::new_side(2),
        ),
        text: "Open comment".to_string(),
        created_at: 100,
        runs: vec![],
        content_hash: None,
        status: NoteStatus::Open,
        original_source_rows: Vec::new(),
    }];

    let opts = MarkdownOptions::default();
    let md = render_markdown_output(&output, &opts);
    assert!(md.contains("> 💬 **Line 2**: Open comment"));
    assert!(md.contains("*✅ 2 resolved notes omitted (use --resolved or --all to show)*"));

    let opts_all = MarkdownOptions {
        show_resolved_notes: true,
        ..MarkdownOptions::default()
    };
    let md_all = render_markdown_output(&output, &opts_all);
    assert!(!md_all.contains("omitted"));
}

#[test]
fn test_render_markdown_multi_line_note_location() {
    let mut output = sample_output();
    output.files[0].notes = vec![OutputNote {
        id: "n_src".to_string(),
        file: "src/main.rs".to_string(),
        location: diffuse_core::notes::NoteLocation::range(
            diffuse_core::notes::DiffCoord::new_side(2),
            diffuse_core::notes::DiffCoord::new_side(3),
        ),
        text: "Quoted review note".to_string(),
        created_at: 100,
        runs: vec![],
        content_hash: None,
        status: NoteStatus::Open,
        original_source_rows: Vec::new(),
    }];

    let md = render_markdown_output(&output, &MarkdownOptions::default());
    assert!(md.contains("> 💬 **Lines 2-3**: Quoted review note"));
    assert!(
        !md.contains("> ```rs"),
        "source-lines code blocks no longer render"
    );
}

#[test]
fn test_render_markdown_mov_sides() {
    let mut output = sample_output();
    output.files[0].notes.clear();
    output.files[0].hunks[0].rows = vec![
        OutputRow {
            kind: RowKind::Mov,
            mov_side: Some(MovSide::Src),
            old_line: Some(1),
            new_line: None,
            runs: vec![OutputRun {
                kind: RunKind::MovedSrc,
                color: TokenColor::Default,
                sgr: None,
                text: "original line".into(),
            }],
            wholesale: false,
            group: None,
        },
        OutputRow {
            kind: RowKind::Mov,
            mov_side: Some(MovSide::Tgt),
            old_line: None,
            new_line: Some(1),
            runs: vec![OutputRun {
                kind: RunKind::MovedTgt,
                color: TokenColor::Default,
                sgr: None,
                text: "relocated line".into(),
            }],
            wholesale: false,
            group: None,
        },
    ];

    let opts = MarkdownOptions {
        collapsible: false,
        colorize: false,
        summary_table: false,
        ..MarkdownOptions::default()
    };
    let md = render_markdown_output(&output, &opts);
    assert!(
        md.contains("-original line\n+relocated line\n"),
        "expected MovSrc to render '-' and MovTgt to render '+', but got:\n{md}"
    );
}

#[test]
#[should_panic(expected = "RowKind::Mov rows must specify mov_side")]
fn test_render_markdown_mov_none_side_panics() {
    let mut output = sample_output();
    output.files[0].notes.clear();
    output.files[0].hunks[0].rows = vec![OutputRow {
        kind: RowKind::Mov,
        mov_side: None,
        old_line: Some(1),
        new_line: Some(1),
        runs: vec![OutputRun {
            kind: RunKind::MovedTgt,
            color: TokenColor::Default,
            sgr: None,
            text: "invalid move".into(),
        }],
        wholesale: false,
        group: None,
    }];

    let opts = MarkdownOptions {
        collapsible: false,
        colorize: false,
        summary_table: false,
        ..MarkdownOptions::default()
    };
    let _ = render_markdown_output(&output, &opts);
}

#[test]
fn test_render_markdown_note_with_nested_source_diff_code_block() {
    let mut output = sample_output();
    output.files[0].notes = vec![OutputNote {
        id: "note-1".to_string(),
        file: "src/main.rs".to_string(),
        location: diffuse_core::notes::NoteLocation::line(
            diffuse_core::notes::DiffCoord::new_side(10),
        ),
        text: "Please refactor this".to_string(),
        created_at: 123456789,
        runs: vec![],
        content_hash: None,
        status: NoteStatus::Open,
        original_source_rows: vec![
            OutputRow {
                kind: RowKind::Del,
                mov_side: None,
                old_line: Some(10),
                new_line: None,
                runs: vec![OutputRun {
                    kind: RunKind::Del,
                    color: TokenColor::Default,
                    sgr: None,
                    text: "old_impl();".into(),
                }],
                wholesale: false,
                group: None,
            },
            OutputRow {
                kind: RowKind::Add,
                mov_side: None,
                old_line: None,
                new_line: Some(10),
                runs: vec![OutputRun {
                    kind: RunKind::Add,
                    color: TokenColor::Default,
                    sgr: None,
                    text: "new_impl();".into(),
                }],
                wholesale: false,
                group: None,
            },
        ],
    }];

    // 1. Without show_source (default), nested code block is absent
    let opts_no_src = MarkdownOptions {
        show_source: false,
        summary_table: false,
        collapsible: false,
        ..MarkdownOptions::default()
    };
    let md_no_src = render_markdown_output(&output, &opts_no_src);
    assert!(md_no_src.contains("> [!NOTE]"));
    assert!(md_no_src.contains("> 💬 **Line 10**: Please refactor this"));
    assert!(!md_no_src.contains("> ```diff"));

    // 2. With show_source = true, nested diff code block is rendered
    let opts_with_src = MarkdownOptions {
        show_source: true,
        summary_table: false,
        collapsible: false,
        ..MarkdownOptions::default()
    };
    let md_with_src = render_markdown_output(&output, &opts_with_src);
    assert!(md_with_src.contains("> [!NOTE]"));
    assert!(md_with_src.contains("> 💬 **Line 10**: Please refactor this"));
    assert!(md_with_src.contains("> ```diff\n> -old_impl();\n> +new_impl();\n> ```"));
}

#[test]
fn test_render_markdown_lines_uncolorized_preserves_text_with_prefix() {
    let md = "# Title\n\nSome **bold** and `code` text.\n- item 1\n- item 2";
    let theme = DiffuseTheme::dark();
    let lines = super::render_markdown_lines(md, &theme, false, "  ");
    assert_eq!(
        lines,
        vec![
            "  # Title",
            "  ",
            "  Some **bold** and `code` text.",
            "  - item 1",
            "  - item 2",
        ]
    );

    let text = super::render_markdown_text(md, &theme, false, "  ");
    assert_eq!(
        text,
        "  # Title\n  \n  Some **bold** and `code` text.\n  - item 1\n  - item 2\n"
    );
}

#[test]
fn test_render_markdown_lines_colorized_applies_markdown_highlighting_and_fences() {
    let md = "### Header\nParagraph with **bold** and `inline`.\n```rust\nlet mut x = 42;\n```\nAfter fence";
    let theme = DiffuseTheme::dark();
    let lines = super::render_markdown_lines(md, &theme, true, "  ");
    assert_eq!(lines.len(), 6);

    // Line 0: Header - starts with indent, contains ANSI bold and Delimiter color,
    // ends with SGR reset
    assert!(lines[0].starts_with("  "));
    assert!(lines[0].ends_with("\x1b[0m"));
    assert!(lines[0].contains("Header"));

    // Line 1: Bold and inline code
    assert!(lines[1].starts_with("  "));
    assert!(lines[1].contains("**"));
    assert!(lines[1].contains("bold"));
    assert!(lines[1].contains("`"));
    assert!(lines[1].contains("inline"));

    // Line 2: Fence opening
    assert!(lines[2].contains("```rust"));

    // Line 3: Fenced Rust line - 'let' and 'mut' must receive keyword coloring
    assert!(lines[3].contains("let"));
    assert!(lines[3].contains("mut"));
    assert!(lines[3].contains("42"));
    // Keyword color for Rust in default theme is non-empty
    let kw_color = theme.palette[Cursor::Rest][DiffKind::Ctx]
        .syntax
        .token_color(TokenColor::Keyword)
        .unwrap();
    let kw_ansi = kw_color.fg_code();
    assert!(
        lines[3].contains(&kw_ansi),
        "fenced rust code must receive keyword styling"
    );

    // Line 4: Fence closing
    assert!(lines[4].contains("```"));

    // Line 5: After fence
    assert!(lines[5].contains("After fence"));
}

#[test]
fn test_render_markdown_lines_table_formatting() {
    let md = "| Col A | Col B |\n|---|---|\n| 1 | 2 |";
    let theme = DiffuseTheme::dark();
    let lines = super::render_markdown_lines(md, &theme, true, "");
    assert_eq!(lines.len(), 3);
    let head_color = theme.palette[Cursor::Rest][DiffKind::Ctx]
        .syntax
        .token_color(TokenColor::TableHeader)
        .unwrap();
    let head_ansi = head_color.fg_code();
    assert!(
        lines[0].contains(&head_ansi),
        "table header row must be highlighted as TableHeader"
    );
}

#[test]
fn test_render_markdown_lines_empty_and_blank() {
    let theme = DiffuseTheme::dark();
    assert!(super::render_markdown_lines("", &theme, false, "  ").is_empty());
    assert!(super::render_markdown_lines("", &theme, true, "  ").is_empty());
    assert_eq!(super::render_markdown_text("", &theme, false, "  "), "");
    assert_eq!(super::render_markdown_text("", &theme, true, "  "), "");
}

#[test]
fn test_render_markdown_lines_wrapping_adds_padding_to_wrapped_lines() {
    let md = "This is a long line of markdown that will wrap into multiple visual lines when wrapped at 40 columns.";
    let theme = DiffuseTheme::dark();
    let lines = super::render_markdown_lines_with_width(md, &theme, false, "  ", 40);
    assert!(lines.len() >= 2, "must wrap into at least 2 lines");
    for line in &lines {
        assert!(
            line.starts_with("  "),
            "every wrapped line must have 2-space padding: {line:?}"
        );
        assert!(
            crate::width::display_width(line) <= 40,
            "wrapped line must not exceed 40 columns: {line:?}"
        );
    }

    let color_lines = super::render_markdown_lines_with_width(md, &theme, true, "  ", 40);
    assert_eq!(color_lines.len(), lines.len());
    for line in &color_lines {
        assert!(
            line.starts_with("  "),
            "every wrapped colorized line must have 2-space padding: {line:?}"
        );
        assert!(line.ends_with("\x1b[0m"));
    }
}
