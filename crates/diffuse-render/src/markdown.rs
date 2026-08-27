//! GitHub-Flavored Markdown (GFM) Diff Exporter.
//!
//! Generates clean, valid, copy-pasteable Markdown diff representations
//! suitable for pull request descriptions, issue comments, code review bots,
//! and terminal Markdown viewers with optional ANSI colorization.

use crate::{
    core::{
        parse::RowKind,
        wire::{OutputFile, PipelineOutput},
    },
    syntax::classifier::for_each_markdown_token,
    theme::DiffuseTheme,
};

/// Formatting and layout options for Markdown diff export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownOptions {
    /// Whether to generate the overview summary metrics and table at the top.
    pub summary_table: bool,
    /// Wrap each file's diff inside collapsible `<details><summary>` blocks.
    pub collapsible: bool,
    /// Whether `<details>` tags should be initially open (`<details open>`).
    pub default_open: bool,
    /// Render attached review notes as GitHub blockquote callouts (`>
    /// [!NOTE]`).
    pub include_notes: bool,
    /// Whether to colorize the Markdown output with ANSI escape sequences.
    pub colorize: bool,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            summary_table: true,
            collapsible: true,
            default_open: true,
            include_notes: true,
            colorize: false,
        }
    }
}

/// Compute the raw diff deletion and addition line counts for a file.
pub fn file_raw_counts(file: &OutputFile) -> (usize, usize) {
    let mut dels = 0;
    let mut adds = 0;
    for hunk in &file.hunks {
        for row in &hunk.rows {
            match row.kind {
                RowKind::Del => dels += 1,
                RowKind::Add => adds += 1,
                RowKind::Mod => {
                    if row.del_text.is_some() {
                        dels += 1;
                    }
                    adds += 1;
                }
                _ => {}
            }
        }
    }
    if file.hunks.is_empty() {
        dels = file.counts.dels + file.counts.mods;
        adds = file.counts.adds + file.counts.mods;
    }
    (dels, adds)
}

/// Render an entire [`PipelineOutput`] as GitHub-Flavored Markdown.
pub fn render_markdown_output(output: &PipelineOutput, options: &MarkdownOptions) -> String {
    let mut out = String::new();
    let theme = DiffuseTheme::MAGIC;

    if output.files.is_empty() {
        push_md_line(&mut out, "*No changes detected.*", options.colorize, &theme);
        return out;
    }

    // 1. Executive Summary Table
    if options.summary_table {
        let mut total_adds = 0usize;
        let mut total_dels = 0usize;

        for file in &output.files {
            let (dels, adds) = file_raw_counts(file);
            total_dels += dels;
            total_adds += adds;
        }

        let file_count = output.files.len();
        let file_word = if file_count == 1 { "file" } else { "files" };

        push_md_line(&mut out, "### Diff Summary", options.colorize, &theme);
        out.push('\n');
        push_md_line(
            &mut out,
            &format!(
                "**{} {} changed**: **-{}** deletions, **+{}** additions",
                file_count, file_word, total_dels, total_adds
            ),
            options.colorize,
            &theme,
        );
        out.push('\n');

        push_md_line(
            &mut out,
            "| File | Status | -Dels | +Adds |",
            options.colorize,
            &theme,
        );
        push_md_line(&mut out, "|---|---|---|---|", options.colorize, &theme);
        for file in &output.files {
            let (dels, adds) = file_raw_counts(file);
            let row = format!(
                "| `{}` | `{}` | -{} | +{} |",
                file.path.trim(),
                file.status,
                dels,
                adds
            );
            push_md_line(&mut out, &row, options.colorize, &theme);
        }
        out.push('\n');
    }

    // 2. Per-file diff blocks
    for (idx, file) in output.files.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        out.push_str(&render_file_markdown(file, options));
    }

    out
}

/// Render a single [`OutputFile`] as GitHub-Flavored Markdown.
pub fn render_file_markdown(file: &OutputFile, options: &MarkdownOptions) -> String {
    let mut out = String::new();
    let theme = DiffuseTheme::MAGIC;
    let (dels, adds) = file_raw_counts(file);
    let stats = format!("{}: -{} +{}", file.status, dels, adds);

    if options.collapsible {
        let is_open = options.default_open && file.status != "untracked";
        let open_attr = if is_open { " open" } else { "" };
        let open_tag = format!("<details{}>", open_attr);
        let summary_tag = format!(
            "<summary><b>{}</b> <code>({})</code></summary>",
            file.path.trim(),
            stats
        );
        push_md_line(&mut out, &open_tag, options.colorize, &theme);
        push_md_line(&mut out, &summary_tag, options.colorize, &theme);
        out.push('\n');
    } else {
        let header = format!("### {} `({})`", file.path.trim(), stats);
        push_md_line(&mut out, &header, options.colorize, &theme);
        out.push('\n');
    }

    let ext = file.path.rsplit('.').next().unwrap_or("");
    let profile = crate::syntax::registry::lang_profile(ext);
    let ctx = MarkdownRenderCtx {
        colorize: options.colorize,
        theme: &theme,
        profile,
    };

    let mut active_fence_profile = None;

    let hunks = if !file.raw_hunks.is_empty() {
        &file.raw_hunks
    } else {
        &file.hunks
    };

    if hunks.is_empty() {
        push_md_line(&mut out, "```diff", options.colorize, &theme);
        push_diff_line(&mut out, '#', " (No diff content)", options.colorize);
        push_md_line(&mut out, "```", options.colorize, &theme);
    } else {
        push_md_line(&mut out, "```diff", options.colorize, &theme);
        for (h_idx, hunk) in hunks.iter().enumerate() {
            if h_idx > 0 {
                out.push('\n');
            }
            let hunk_header = format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
            );
            push_hunk_header(&mut out, &hunk_header, options.colorize);

            for row in &hunk.rows {
                match row.kind {
                    RowKind::Ctx => {
                        push_diff_row(&mut out, ' ', &row.text, &mut active_fence_profile, &ctx);
                    }
                    RowKind::Add => {
                        push_diff_row(&mut out, '+', &row.text, &mut active_fence_profile, &ctx);
                    }
                    RowKind::Del => {
                        push_diff_row(&mut out, '-', &row.text, &mut active_fence_profile, &ctx);
                    }
                    RowKind::Mod => {
                        if let Some(ref del_text) = row.del_text {
                            let mut del_fence = active_fence_profile;
                            push_diff_row(&mut out, '-', del_text, &mut del_fence, &ctx);
                        }
                        push_diff_row(&mut out, '+', &row.text, &mut active_fence_profile, &ctx);
                    }
                    RowKind::File | RowKind::Sep => {}
                }
            }
        }
        push_md_line(&mut out, "```", options.colorize, &theme);
    }

    // 3. Render Review Notes callouts if present
    if options.include_notes && !file.notes.is_empty() {
        out.push('\n');
        for note in &file.notes {
            let line_info = if note.start == note.end {
                format!("**Line {}**", note.start)
            } else {
                format!("**Lines {}-{}**", note.start, note.end)
            };
            push_md_line(&mut out, "> [!NOTE]", options.colorize, &theme);
            push_md_line(
                &mut out,
                &format!("> 💬 {}: {}", line_info, note.text),
                options.colorize,
                &theme,
            );
            push_md_line(&mut out, ">", options.colorize, &theme);
        }
    }

    if options.collapsible {
        out.push('\n');
        push_md_line(&mut out, "</details>", options.colorize, &theme);
    }

    out
}

struct MarkdownRenderCtx<'a> {
    colorize: bool,
    theme: &'a DiffuseTheme,
    profile: Option<&'a crate::syntax::profile::LangProfile>,
}

fn push_md_line(out: &mut String, line: &str, colorize: bool, theme: &DiffuseTheme) {
    if colorize {
        for_each_markdown_token(line, |tok| {
            theme.write_token(tok.color, tok.text, "ctx", None, tok.sgr, out);
        });
        out.push_str("\x1b[0m\n");
    } else {
        out.push_str(line);
        out.push('\n');
    }
}

fn push_hunk_header(out: &mut String, header: &str, colorize: bool) {
    if colorize {
        out.push_str("\x1b[1m\x1b[38;5;220m");
        out.push_str(header);
        out.push_str("\x1b[0m\n");
    } else {
        out.push_str(header);
        out.push('\n');
    }
}

fn push_diff_line(out: &mut String, prefix: char, text: &str, colorize: bool) {
    if colorize {
        out.push_str("\x1b[1m\x1b[38;5;220m");
        out.push(prefix);
        out.push_str(text);
        out.push_str("\x1b[0m\n");
    } else {
        out.push(prefix);
        out.push_str(text);
        out.push('\n');
    }
}

fn push_diff_row<'a>(
    out: &mut String,
    prefix: char,
    text: &str,
    active_fence_profile: &mut Option<&'a crate::syntax::profile::LangProfile>,
    ctx: &MarkdownRenderCtx<'a>,
) {
    if ctx.colorize {
        match prefix {
            '+' => out.push_str("\x1b[38;5;120m+\x1b[39m"),
            '-' => out.push_str("\x1b[38;5;203m-\x1b[39m"),
            _ => out.push(' '),
        }

        let trimmed = text.trim();
        let is_fence_line = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        let row_profile = if is_fence_line {
            let tag = trimmed.trim_start_matches(['`', '~']).trim();
            if !tag.is_empty() {
                *active_fence_profile = crate::syntax::registry::lang_profile(tag);
            } else {
                *active_fence_profile = None;
            }
            ctx.profile
        } else if let Some(fence_prof) = *active_fence_profile {
            Some(fence_prof)
        } else {
            ctx.profile
        };

        let mut tokens = Vec::new();
        crate::syntax::classifier::for_each_line_classified_token(text, row_profile, None, |t| {
            tokens.push(t)
        });
        if !tokens.is_empty() {
            for tok in tokens {
                ctx.theme
                    .write_token(tok.color, tok.text, "ctx", None, tok.sgr, out);
            }
        } else {
            ctx.theme.write_token("text", text, "ctx", None, None, out);
        }
        out.push_str("\x1b[0m\n");
    } else {
        out.push(prefix);
        out.push_str(text);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        counts::FileCounts,
        merge::RunKind,
        notes::Side,
        parse::Spans,
        wire::{OutputHunk, OutputNote, OutputRow, OutputRun},
    };

    fn sample_output() -> PipelineOutput {
        PipelineOutput {
            files: vec![
                OutputFile {
                    path: "src/main.rs".to_string(),
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
                                text: "fn main() {".to_string(),
                                old_line: Some(1),
                                new_line: Some(1),
                                spans: Spans::default(),
                                runs: vec![OutputRun {
                                    kind: RunKind::Common,
                                    color: Some("syntaxKeyword".into()),
                                    sgr: None,
                                    text: "fn main() {".to_string(),
                                }],
                                del_text: None,
                                wholesale: false,
                                moved: false,
                            },
                            OutputRow {
                                kind: RowKind::Mod,
                                text: "    println!(\"Hello diffuse!\");".to_string(),
                                old_line: Some(2),
                                new_line: Some(2),
                                spans: Spans::default(),
                                runs: vec![OutputRun {
                                    kind: RunKind::Add,
                                    color: Some("syntaxString".into()),
                                    sgr: None,
                                    text: "    println!(\"Hello diffuse!\");".to_string(),
                                }],
                                del_text: Some("    println!(\"Hello World!\");".to_string()),
                                wholesale: false,
                                moved: false,
                            },
                            OutputRow {
                                kind: RowKind::Ctx,
                                text: "}".to_string(),
                                old_line: Some(3),
                                new_line: Some(3),
                                spans: Spans::default(),
                                runs: vec![OutputRun {
                                    kind: RunKind::Common,
                                    color: Some("text".into()),
                                    sgr: None,
                                    text: "}".to_string(),
                                }],
                                del_text: None,
                                wholesale: false,
                                moved: false,
                            },
                        ],
                    }],
                    raw_hunks: vec![],
                    counts: FileCounts {
                        adds: 1,
                        dels: 1,
                        mods: 1,
                        movs: 0,
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
                    }],
                    content_runs: vec![],
                    rows: vec![],
                },
                OutputFile {
                    path: "README.md".to_string(),
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
                                text: "# Diffuse".to_string(),
                                old_line: None,
                                new_line: Some(1),
                                spans: Spans::default(),
                                runs: vec![],
                                del_text: None,
                                wholesale: false,
                                moved: false,
                            },
                            OutputRow {
                                kind: RowKind::Add,
                                text: "Next gen diffs.".to_string(),
                                old_line: None,
                                new_line: Some(2),
                                spans: Spans::default(),
                                runs: vec![],
                                del_text: None,
                                wholesale: false,
                                moved: false,
                            },
                        ],
                    }],
                    raw_hunks: vec![],
                    counts: FileCounts {
                        adds: 2,
                        dels: 0,
                        mods: 0,
                        movs: 0,
                    },
                    fences: vec![],
                    notes: vec![],
                    content_runs: vec![],
                    rows: vec![],
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
        assert!(md.contains("| `src/main.rs` | `modified` | -1 | +1 |"));
        assert!(md.contains("| `README.md` | `added` | -0 | +2 |"));
    }

    #[test]
    fn test_render_markdown_collapsible_details() {
        let output = sample_output();
        let opts = MarkdownOptions::default();
        let md = render_markdown_output(&output, &opts);

        assert!(md.contains("<details open>"));
        assert!(
            md.contains("<summary><b>src/main.rs</b> <code>(modified: -1 +1)</code></summary>")
        );
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
            colorize: false,
        };
        let md1 = render_markdown_output(&output, &opts_no_summary_no_collapse);
        assert!(!md1.contains("### Diff Summary"));
        assert!(!md1.contains("<details"));
        assert!(md1.contains("### src/main.rs `(modified: -1 +1)`"));
        assert!(!md1.contains("> [!NOTE]"));

        // 2. Collapsible with default_open = false
        let opts_closed = MarkdownOptions {
            summary_table: true,
            collapsible: true,
            default_open: false,
            include_notes: true,
            colorize: false,
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
                                text: "file header".to_string(),
                                old_line: None,
                                new_line: None,
                                spans: Spans::default(),
                                runs: vec![],
                                del_text: None,
                                wholesale: false,
                                moved: false,
                            },
                            OutputRow {
                                kind: RowKind::Del,
                                text: "old line".to_string(),
                                old_line: Some(1),
                                new_line: None,
                                spans: Spans::default(),
                                runs: vec![],
                                del_text: None,
                                wholesale: false,
                                moved: false,
                            },
                            OutputRow {
                                kind: RowKind::Sep,
                                text: "---".to_string(),
                                old_line: None,
                                new_line: None,
                                spans: Spans::default(),
                                runs: vec![],
                                del_text: None,
                                wholesale: false,
                                moved: false,
                            },
                        ],
                    }],
                    raw_hunks: vec![],
                    counts: FileCounts {
                        adds: 0,
                        dels: 1,
                        mods: 0,
                        movs: 0,
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
                    }],
                    content_runs: vec![],
                    rows: vec![],
                },
                OutputFile {
                    path: "empty.txt".to_string(),
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
                        text: "pub fn new_algo() -> bool { true }".to_string(),
                        old_line: Some(1),
                        new_line: Some(1),
                        spans: Spans::default(),
                        runs: vec![], // Empty runs triggers syntax profile classify_tokens fallback
                        del_text: Some("pub fn old_algo() -> bool { false }".to_string()),
                        wholesale: false,
                        moved: false,
                    }],
                }],
                raw_hunks: vec![],
                counts: FileCounts {
                    adds: 1,
                    dels: 1,
                    mods: 1,
                    movs: 0,
                },
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
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
                },
                OutputFile {
                    path: "untracked.rs".to_string(),
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
    fn test_render_markdown_fenced_code_blocks_syntax_highlighting() {
        let output = PipelineOutput {
            files: vec![OutputFile {
                path: "README.md".to_string(),
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
                            text: "```rust".to_string(),
                            old_line: Some(1),
                            new_line: None,
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                        OutputRow {
                            kind: RowKind::Del,
                            text: "pub struct Config {".to_string(),
                            old_line: Some(2),
                            new_line: None,
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                        OutputRow {
                            kind: RowKind::Del,
                            text: "    pub enabled: bool,".to_string(),
                            old_line: Some(3),
                            new_line: None,
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                        OutputRow {
                            kind: RowKind::Del,
                            text: "```".to_string(),
                            old_line: Some(4),
                            new_line: None,
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                    ],
                }],
                raw_hunks: vec![],
                counts: FileCounts::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
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
        // "pub" and "struct" must be highlighted as keywords (\x1b[38;5;141m)
        assert!(md.contains("\x1b[38;5;141mpub\x1b[39m"));
        assert!(md.contains("\x1b[38;5;141mstruct\x1b[39m"));
        // "Config" must be highlighted as type (\x1b[38;5;173m)
        assert!(md.contains("\x1b[38;5;173mConfig\x1b[39m"));
    }

    #[test]
    fn test_render_markdown_diff_prefix_resets_fg_color() {
        let output = PipelineOutput {
            files: vec![OutputFile {
                path: "plan.md".to_string(),
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
                        text: "1. Export `pub mod markdown;` from crate.".to_string(),
                        old_line: Some(1),
                        new_line: None,
                        spans: Spans::default(),
                        runs: vec![],
                        del_text: None,
                        wholesale: false,
                        moved: false,
                    }],
                }],
                raw_hunks: vec![],
                counts: FileCounts::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
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
        // The prefix '-' is red and immediately reset with \x1b[39m so "1. Export " is
        // not red
        assert!(md.contains("\x1b[38;5;203m-\x1b[39m1. Export "));
    }

    #[test]
    fn test_render_markdown_raw_hunks_deletions_before_additions() {
        let output = PipelineOutput {
            files: vec![OutputFile {
                path: "src/lib.rs".to_string(),
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
                            text: "old line 1".to_string(),
                            old_line: Some(1),
                            new_line: None,
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                        OutputRow {
                            kind: RowKind::Del,
                            text: "old line 2".to_string(),
                            old_line: Some(2),
                            new_line: None,
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                        OutputRow {
                            kind: RowKind::Add,
                            text: "new line 1".to_string(),
                            old_line: None,
                            new_line: Some(1),
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                        OutputRow {
                            kind: RowKind::Add,
                            text: "new line 2".to_string(),
                            old_line: None,
                            new_line: Some(2),
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                        OutputRow {
                            kind: RowKind::Add,
                            text: "new line 3".to_string(),
                            old_line: None,
                            new_line: Some(3),
                            spans: Spans::default(),
                            runs: vec![],
                            del_text: None,
                            wholesale: false,
                            moved: false,
                        },
                    ],
                }],
                counts: FileCounts::default(),
                fences: vec![],
                notes: vec![],
                content_runs: vec![],
                rows: vec![],
            }],
            branch: "main".to_string(),
            untracked_skipped: vec![],
        };

        let md = render_markdown_output(&output, &MarkdownOptions::default());
        let expected_diff = "```diff\n@@ -1,2 +1,3 @@\n-old line 1\n-old line 2\n+new line 1\n+new line 2\n+new line 3\n```";
        assert!(md.contains(expected_diff));
    }
}
