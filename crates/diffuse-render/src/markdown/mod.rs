//! GitHub-Flavored Markdown (GFM) Diff Exporter.
//!
//! Generates clean, valid, copy-pasteable Markdown diff representations
//! suitable for pull request descriptions, issue comments, code review bots,
//! and terminal Markdown viewers with optional ANSI colorization.

pub mod diff;
pub mod highlight;
pub mod options;
pub mod terminal;

pub use diff::{file_raw_counts, render_file_markdown, render_markdown_output};
pub use highlight::{
    render_markdown_lines, render_markdown_lines_with_width, render_markdown_text,
    render_markdown_text_with_width, write_markdown_line_highlighted,
};
pub use options::MarkdownOptions;
pub use terminal::{
    MarkdownLine, MarkdownRun, markdown_lines, render_markdown_line, write_markdown_line,
};

#[cfg(test)]
mod tests;
