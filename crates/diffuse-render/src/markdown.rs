//! GitHub-Flavored Markdown (GFM) Diff Exporter.
//!
//! Generates clean, valid, copy-pasteable Markdown diff representations
//! suitable for pull request descriptions, issue comments, code review bots,
//! and terminal Markdown viewers with optional ANSI colorization.

use std::borrow::Cow;

use crate::{
    core::{
        parse::RowKind,
        wire::{OutputFile, PipelineOutput},
    },
    syntax::classifier::for_each_markdown_token,
    theme::DiffuseTheme,
};
use diffuse_syntax::color::TokenColor;

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
    /// Theme used for token colors when `colorize` is enabled.
    pub theme: DiffuseTheme,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            summary_table: true,
            collapsible: true,
            default_open: true,
            include_notes: true,
            colorize: false,
            theme: DiffuseTheme::DEFAULT,
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
                    if row.del_text().is_some() {
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

fn format_md_path(path: &str) -> Cow<'_, str> {
    if path.contains('\t') {
        Cow::Owned(path.replace('\t', "⇥"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Render an entire [`PipelineOutput`] as GitHub-Flavored Markdown.
pub fn render_markdown_output(output: &PipelineOutput, options: &MarkdownOptions) -> String {
    let mut out = String::new();
    let theme = options.theme;

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
            let path_display = if let Some(ref old_path) = file.old_path {
                if old_path != &file.path {
                    crate::core::wire::format_rename_path(
                        &format_md_path(old_path),
                        &format_md_path(&file.path),
                    )
                } else {
                    format_md_path(&file.path).into_owned()
                }
            } else {
                format_md_path(&file.path).into_owned()
            };
            let row = format!(
                "| `{}` | `{}` | -{} | +{} |",
                path_display,
                file.display_status(),
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
    let theme = options.theme;
    let (dels, adds) = file_raw_counts(file);
    let stats = format!("{}: -{} +{}", file.display_status(), dels, adds);
    let md_path = if let Some(ref old_path) = file.old_path {
        if old_path != &file.path {
            crate::core::wire::format_rename_path(
                &format_md_path(old_path),
                &format_md_path(&file.path),
            )
        } else {
            format_md_path(&file.path).into_owned()
        }
    } else {
        format_md_path(&file.path).into_owned()
    };

    if options.collapsible {
        let has_content_changes = dels > 0 || adds > 0;
        let is_open = options.default_open
            && !file.is_binary
            && file.status != "untracked"
            && (file.status != "renamed" || has_content_changes)
            && (file.status != "mode changed" || has_content_changes);
        let open_attr = if is_open { " open" } else { "" };
        let open_tag = format!("<details{}>", open_attr);
        let summary_tag = format!(
            "<summary><b>{}</b> <code>({})</code></summary>",
            md_path, stats
        );
        push_md_line(&mut out, &open_tag, options.colorize, &theme);
        push_md_line(&mut out, &summary_tag, options.colorize, &theme);
        out.push('\n');
    } else {
        let header = format!("### {} `({})`", md_path, stats);
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

    if file.is_binary {
        push_md_line(&mut out, "```diff", options.colorize, &theme);
        push_diff_line(&mut out, '#', " Binary files differ", options.colorize);
        push_md_line(&mut out, "```", options.colorize, &theme);
    } else if hunks.is_empty() {
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
                        let text = row.new_text();
                        push_diff_row(&mut out, ' ', &text, &mut active_fence_profile, &ctx);
                    }
                    RowKind::Add => {
                        let text = row.new_text();
                        push_diff_row(&mut out, '+', &text, &mut active_fence_profile, &ctx);
                    }
                    RowKind::Del => {
                        let text = row.old_text();
                        push_diff_row(&mut out, '-', &text, &mut active_fence_profile, &ctx);
                    }
                    RowKind::Mod | RowKind::Mov => {
                        if let Some(del_text) = row.del_text() {
                            let mut del_fence = active_fence_profile;
                            push_diff_row(&mut out, '-', &del_text, &mut del_fence, &ctx);
                        }
                        let text = row.new_text();
                        push_diff_row(&mut out, '+', &text, &mut active_fence_profile, &ctx);
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
            ctx.theme
                .write_token(TokenColor::Default, text, "ctx", None, None, out);
        }
        out.push_str("\x1b[0m\n");
    } else {
        out.push(prefix);
        out.push_str(text);
        out.push('\n');
    }
}

pub mod terminal;

pub use terminal::{MarkdownLine, markdown_lines, render_markdown_line, write_markdown_line};

#[cfg(test)]
mod tests;
