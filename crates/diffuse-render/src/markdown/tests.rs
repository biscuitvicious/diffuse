use diffuse_syntax::color::TokenColor;

use super::*;
use crate::core::{
    counts::FileCounts,
    merge::RunKind,
    notes::{NoteStatus, Side},
    wire::{OutputHunk, OutputNote, OutputRow, OutputRun},
};

#[test]
fn test_markdown_options_default_theme() {
    assert_eq!(MarkdownOptions::default().theme, DiffuseTheme::DEFAULT);
}

fn sample_output() -> PipelineOutput {
    PipelineOutput {
        files: vec![
            OutputFile {
                path: "src/main.rs".to_string(),
                old_path: None,
                old_mode: None,
                new_mode: None,
                status: "modified".to_string(),
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
                    side: Side::New,
                    start: 2,
                    end: 2,
                    text: "Great greeting update!".to_string(),
                    created_at: 123456789,
                    runs: vec![],
                    line_hash: None,
                    status: NoteStatus::Open,
                    source_lines: None,
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
                status: "added".to_string(),
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
        show_source_lines: false,
        colorize: false,
        theme: DiffuseTheme::DEFAULT,
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
        show_source_lines: false,
        colorize: false,
        theme: DiffuseTheme::DEFAULT,
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
                status: "deleted".to_string(),
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
                    side: Side::Old,
                    start: 10,
                    end: 15,
                    text: "Multi-line removal comment".to_string(),
                    created_at: 987654321,
                    runs: vec![],
                    line_hash: None,
                    status: NoteStatus::Open,
                    source_lines: None,
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
                status: "modified".to_string(),
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
            status: "modified".to_string(),
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
                status: "modified".to_string(),
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
                status: "untracked".to_string(),
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
            status: "renamed".to_string(),
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
            status: "renamed".to_string(),
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
            status: "modified".to_string(),
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
            status: "modified".to_string(),
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
    // The prefix '-' is red and immediately reset with \x1b[39m so "1. Export "
    // is not red
    assert!(md.contains("\x1b[38;5;203m-\x1b[39m1. Export "));
}

#[test]
fn test_render_markdown_raw_hunks_deletions_before_additions() {
    let output = PipelineOutput {
        files: vec![OutputFile {
            path: "src/lib.rs".to_string(),
            old_path: None,
            old_mode: None,
            new_mode: None,
            status: "modified".to_string(),
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
                status: "modified".to_string(),
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
                status: "added".to_string(),
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
            side: Side::New,
            start: 2,
            end: 2,
            text: "Open comment".to_string(),
            created_at: 100,
            runs: vec![],
            line_hash: None,
            status: NoteStatus::Open,
            source_lines: None,
        },
        OutputNote {
            id: "n_in_progress".to_string(),
            file: "src/main.rs".to_string(),
            side: Side::New,
            start: 5,
            end: 5,
            text: "In progress comment".to_string(),
            created_at: 200,
            runs: vec![],
            line_hash: None,
            status: NoteStatus::InProgress,
            source_lines: None,
        },
        OutputNote {
            id: "n_resolved".to_string(),
            file: "src/main.rs".to_string(),
            side: Side::New,
            start: 8,
            end: 8,
            text: "Resolved comment".to_string(),
            created_at: 300,
            runs: vec![],
            line_hash: None,
            status: NoteStatus::Resolved,
            source_lines: None,
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
        side: Side::New,
        start: 2,
        end: 2,
        text: "Open comment".to_string(),
        created_at: 100,
        runs: vec![],
        line_hash: None,
        status: NoteStatus::Open,
        source_lines: None,
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
fn test_render_markdown_source_lines_code_block() {
    let mut output = sample_output();
    output.files[0].notes = vec![OutputNote {
        id: "n_src".to_string(),
        file: "src/main.rs".to_string(),
        side: Side::New,
        start: 2,
        end: 3,
        text: "Quoted review note".to_string(),
        created_at: 100,
        runs: vec![],
        line_hash: None,
        status: NoteStatus::Open,
        source_lines: Some("println!(\"Hello diffuse!\");\n}".to_string()),
    }];

    let opts = MarkdownOptions {
        show_source_lines: true,
        ..MarkdownOptions::default()
    };
    let md = render_markdown_output(&output, &opts);
    assert!(md.contains("> 💬 **Lines 2-3**:"));
    assert!(md.contains("> ```rs\n> println!(\"Hello diffuse!\");\n> }\n> ```"));
    assert!(md.contains("> Quoted review note"));

    let opts_no_src = MarkdownOptions {
        show_source_lines: false,
        ..MarkdownOptions::default()
    };
    let md_no_src = render_markdown_output(&output, &opts_no_src);
    assert!(!md_no_src.contains("> ```rs"));
    assert!(md_no_src.contains("> 💬 **Lines 2-3**: Quoted review note"));
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
