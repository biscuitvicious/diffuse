//! Diff grid row, hunk divider, and file header line formatting.

pub mod file;
pub mod hunk;

pub use file::{
    FILE_HEADER_INDICATOR_COL, file_header_line, file_header_text, file_header_text_with_prefix,
    render_file_header_segment,
};
pub use hunk::{
    bottom_gap_divider_line, bottom_gap_divider_text, divider_seg_count, hunk_divider_line,
    hunk_divider_text, render_bottom_gap_divider_segment, render_hunk_divider,
    render_hunk_divider_segment, wrapped_glyph_hit,
};

#[cfg(test)]
mod tests;
