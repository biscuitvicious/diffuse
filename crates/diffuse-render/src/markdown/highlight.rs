use crate::theme::DiffuseTheme;
use diffuse_syntax::color::TokenColor;

/// Write a single line of Markdown text into an output buffer with syntax
/// highlighting using the specified language profile (or the standard Markdown
/// profile) and theme.
pub fn write_markdown_line_highlighted<W: std::fmt::Write>(
    out: &mut W,
    line: &str,
    colorize: bool,
    theme: &DiffuseTheme,
    row_profile: Option<&crate::syntax::profile::LangProfile>,
    next_line: Option<&str>,
) -> std::fmt::Result {
    if colorize {
        let mut tokens = Vec::new();
        crate::syntax::classifier::for_each_line_classified_token_with_next(
            line,
            next_line,
            row_profile,
            None,
            |t| tokens.push(t),
        );
        if !tokens.is_empty() {
            for tok in tokens {
                theme.write_token(tok.color, tok.text, "ctx", None, tok.sgr, out);
            }
            out.write_str("\x1b[0m")?;
        } else if !line.is_empty() {
            theme.write_token(TokenColor::Default, line, "ctx", None, None, out);
            out.write_str("\x1b[0m")?;
        }
    } else {
        out.write_str(line)?;
    }
    Ok(())
}

/// Renders multiline Markdown text line-by-line using the nondestructive
/// Markdown syntax highlighter and the "md" language profile, preserving all
/// source characters, indentation, and formatting.
pub fn render_markdown_lines(
    text: &str,
    theme: &DiffuseTheme,
    colorize: bool,
    prefix: &str,
) -> Vec<String> {
    render_markdown_lines_with_width(text, theme, colorize, prefix, 0)
}

/// Renders multiline Markdown text line-by-line using the nondestructive
/// Markdown syntax highlighter and the "md" language profile, with line
/// wrapping at the specified width and prefix padding on all continuation
/// lines. When width is 0, lines are not wrapped.
pub fn render_markdown_lines_with_width(
    text: &str,
    theme: &DiffuseTheme,
    colorize: bool,
    prefix: &str,
    width: usize,
) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    let md_profile = crate::syntax::registry::lang_profile("md");
    let mut active_fence_profile: Option<&crate::syntax::profile::LangProfile> = None;
    let prefix_w = crate::width::display_width(prefix);

    for (i, line) in lines.iter().enumerate() {
        let next_line = lines.get(i + 1).copied();
        let trimmed = line.trim();
        let is_fence_line = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        let (row_profile, close_fence_after) = if is_fence_line {
            let tag = trimmed.trim_start_matches(['`', '~']).trim();
            if !tag.is_empty() {
                active_fence_profile = crate::syntax::registry::lang_profile(tag);
                (md_profile, false)
            } else if active_fence_profile.is_some() {
                (md_profile, true)
            } else {
                active_fence_profile = None;
                (md_profile, false)
            }
        } else if let Some(fence_prof) = active_fence_profile {
            (Some(fence_prof), false)
        } else {
            (md_profile, false)
        };

        let mut tokens = Vec::new();
        crate::syntax::classifier::for_each_line_classified_token_with_next(
            line,
            next_line,
            row_profile,
            None,
            |t| tokens.push(t),
        );
        let runs: Vec<crate::core::wire::OutputRun> = tokens
            .into_iter()
            .map(|t| crate::core::wire::OutputRun {
                kind: crate::core::merge::RunKind::Ctx,
                color: t.color,
                sgr: t.sgr,
                text: std::borrow::Cow::Owned(t.text.to_string()),
            })
            .collect();

        let exp_line = crate::row::expand_tabs(line, 4);
        let line_len = crate::width::display_width(&exp_line);

        let (seg_count, text_cols) = match width {
            w if w > prefix_w => {
                let cols = w - prefix_w;
                let count = if line_len == 0 {
                    1
                } else {
                    line_len.div_ceil(cols)
                };
                (count, cols)
            }
            _ => (1, usize::MAX),
        };

        for s_idx in 0..seg_count {
            let seg_start = s_idx * text_cols;
            let seg_end = seg_start.saturating_add(text_cols);
            let mut line_buf = String::with_capacity(prefix.len() + text_cols.min(line_len) + 32);
            let _ = crate::row::write_note_segment(
                &mut line_buf,
                line,
                &runs,
                seg_start,
                seg_end,
                prefix,
                colorize,
                theme,
            );
            if colorize && !line.is_empty() {
                line_buf.push_str("\x1b[0m");
            }
            out.push(line_buf);
        }

        if close_fence_after {
            active_fence_profile = None;
        }
    }

    out
}

/// Renders multiline Markdown text into a single String using the
/// nondestructive Markdown syntax highlighter and the "md" language profile.
pub fn render_markdown_text(
    text: &str,
    theme: &DiffuseTheme,
    colorize: bool,
    prefix: &str,
) -> String {
    render_markdown_text_with_width(text, theme, colorize, prefix, 0)
}

/// Renders multiline Markdown text into a single String using the
/// nondestructive Markdown syntax highlighter and the "md" language profile,
/// with wrapping at the specified width and prefix padding on all continuation
/// lines.
pub fn render_markdown_text_with_width(
    text: &str,
    theme: &DiffuseTheme,
    colorize: bool,
    prefix: &str,
    width: usize,
) -> String {
    let lines = render_markdown_lines_with_width(text, theme, colorize, prefix, width);
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len() + lines.len() * (prefix.len() + 32));
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    out
}
