//! High-level CLI Diff Pretty-Printer.
//!
//! Formats full file diff summaries and colored hunks directly to terminal
//! output.

pub mod header;
pub mod hunk;
pub mod render;
pub mod row;

#[cfg(test)]
mod tests;

pub use header::{
    FileHeaderBanner, FileHeaderSpec, HeaderRole, HeaderSpan, banner_spans, file_header_banner,
    file_header_text, raw_file_header_lines, write_file_header_bar,
    write_file_header_bar_with_width, write_raw_file_header,
};
pub use hunk::{hunk_divider_spans, hunk_divider_text, hunk_leading_gap, render_hunk_divider};
pub use render::{
    render_file_diff, render_file_diff_with_all_options, render_file_diff_with_options,
    render_file_diff_with_width, render_pipeline_output, render_pipeline_output_with_all_options,
    render_pipeline_output_with_options, render_pipeline_output_with_width, render_raw_file_diff,
    render_raw_file_diff_with_width, render_raw_pipeline_output,
    render_raw_pipeline_output_with_width,
};
pub use row::render_source_rows;
