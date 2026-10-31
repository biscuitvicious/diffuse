//! Terminal Markdown rendering mode: renders the *styling* of Markdown
//! rather than its source as styled visual lines reflowed to a content
//! width. Note-agnostic — notes are one consumer.
//!
//! Parsing and marker-free classification live in
//! [`crate::syntax::markdown`]; this module owns width reflow, glyph
//! substitution, and ANSI stylization through [`DiffuseTheme`]. No new
//! dependencies.

pub mod flow;
pub mod inline;
pub mod table;
pub mod types;
pub mod wrap;

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use crate::syntax::profile::SGR_BOLD;
#[cfg(test)]
pub(crate) use crate::width::display_width;
#[cfg(test)]
pub(crate) use std::borrow::Cow;

use std::fmt::{self, Write};

pub use types::{MarkdownLine, MarkdownRun};

use crate::{
    row::expand_tabs,
    syntax::{
        markdown::blocks,
        profile::{SGR_DIM, SGR_STRIKE, SGR_UNDERLINE},
    },
    theme::DiffuseTheme,
};

/// Renders Markdown text as styled visual lines reflowed to `width`.
#[must_use]
pub fn markdown_lines(text: &str, width: usize) -> Vec<MarkdownLine> {
    let width = width.max(1);
    // Tabs expanded once upfront; all downstream slicing borrows from the
    // expanded text via `Cow` (content copies to `Owned` only because
    // `OutputRun` requires `'static`).
    let expanded = expand_tabs(text, 4);
    let flow = blocks(&expanded);
    let renderer = flow::Renderer { full_width: width };
    let mut out: Vec<types::VisualLine> = Vec::new();
    let mut pending_blank = false;
    renderer.render_blocks(flow.blocks(), 0, 0, &mut out, &mut pending_blank);
    out.into_iter().map(wrap::flatten_line).collect()
}

/// Writes one rendered line with the TUI prefix applied.
pub fn write_markdown_line<W: Write>(
    out: &mut W,
    line: &MarkdownLine,
    prefix: &str,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> fmt::Result {
    out.write_str(prefix)?;
    for run in &line.runs {
        if !syntax_on {
            out.write_str(&run.text)?;
            continue;
        }
        let color = run.color;
        // Stacked styles (heading titles, dim strikethrough) combine into one
        // escape; single styles keep the legacy single-code path
        // byte-identical.
        let mut codes = [0u32; 4];
        let mut count = 0usize;
        if run.dim && !codes[..count].contains(&SGR_DIM) {
            codes[count] = SGR_DIM;
            count += 1;
        }
        if run.strike && !codes[..count].contains(&SGR_STRIKE) {
            codes[count] = SGR_STRIKE;
            count += 1;
        }
        if let Some(s) = run.sgr
            && !codes[..count].contains(&s)
        {
            codes[count] = s;
            count += 1;
        }
        if run.underline && !codes[..count].contains(&SGR_UNDERLINE) {
            codes[count] = SGR_UNDERLINE;
            count += 1;
        }
        if count <= 1 {
            theme.write_token(
                color,
                &run.text,
                "ctx",
                None,
                codes[..count].first().copied(),
                out,
            );
        } else {
            theme.write_token_stacked(color, &run.text, "ctx", None, &codes[..count], out);
        }
    }
    Ok(())
}

/// `write_markdown_line` into an owned `String`.
#[must_use]
pub fn render_markdown_line(
    line: &MarkdownLine,
    prefix: &str,
    syntax_on: bool,
    theme: &DiffuseTheme,
) -> String {
    let mut out = String::with_capacity(prefix.len() + 64);
    let _ = write_markdown_line(&mut out, line, prefix, syntax_on, theme);
    out
}
