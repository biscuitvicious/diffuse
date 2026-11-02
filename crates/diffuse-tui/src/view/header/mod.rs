//! Header, status bar, and bottom ribbon rendering.

pub mod bar;
pub mod counts;
pub mod status;
pub mod text;

pub use bar::{bottom_ribbon_lines, header_bar_line, render_bottom_ribbon, render_header_bar};
pub use counts::{
    CountCell, count_cells, count_color, format_counts_ansi, format_counts_plain,
    format_counts_spans,
};
pub use status::{StatusBarContext, padded_line, render_status_bar, status_bar_line};
pub use text::{
    clamp_or_pad_ansi, truncate_pad, truncate_status_path, visible_width, wrap_text_by_visual_width,
};
#[cfg(test)]
pub(crate) use text::{file_status_tag, stripped_file_path};

#[cfg(test)]
mod tests;
