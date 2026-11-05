//! Note text and source context formatting for terminal and Markdown output.

use diffuse::prelude::*;

/// The stored original source rows as plain text lines (Markdown fences).
pub fn plain_row_texts(rows: &[OutputRow]) -> String {
    rows.iter()
        .map(OutputRow::text)
        .collect::<Vec<String>>()
        .join("\n")
}

/// Format a note's stored original source rows for CLI display wrapped at
/// terminal width, with standard diff row coloring (or plain text when color is
/// disabled), without extra line-prefix indentation.
pub fn format_rows_with_context(rows: &[OutputRow], use_color: bool, width: usize) -> Vec<String> {
    let theme = DiffuseTheme::dark();
    let rendered = render_source_rows(rows, &theme, width, true);
    rendered
        .into_iter()
        .map(|l| {
            if use_color {
                l
            } else {
                strip_ansi(&l).to_string()
            }
        })
        .collect()
}

/// Print a note's stored original source rows wrapped at terminal width,
/// stripping ANSI when color is disabled.
pub fn print_rows_with_context(rows: &[OutputRow], use_color: bool) {
    let width = detect_terminal_width();
    for l in format_rows_with_context(rows, use_color, width) {
        println!("{l}");
    }
}

/// Formats a note's text lines with markdown syntax highlighting (or plain text
/// when color is disabled), indented by two spaces, wrapped at the given width
/// (or unconstrained when width is 0), with two-space padding on wrapped
/// continuation lines.
pub fn format_note_text(text: &str, use_color: bool, width: usize) -> Vec<String> {
    let theme = DiffuseTheme::dark();
    diffuse::render::render_markdown_lines_with_width(text, &theme, use_color, "  ", width)
}

/// Print a note's text lines with markdown syntax highlighting (or plain text
/// when color is disabled), indented by two spaces and wrapped at terminal
/// width.
pub fn print_note_text(text: &str, use_color: bool) {
    let width = detect_terminal_width();
    for l in format_note_text(text, use_color, width) {
        println!("{l}");
    }
}
