//! Header, status bar, and bottom ribbon rendering.

pub mod bar;
pub mod counts;
pub mod status;
pub mod text;

pub use bar::{HeaderFileCount, bottom_ribbon_lines, header_bar_line};
pub use counts::{CountCell, count_cells, count_color, format_counts_plain, format_counts_spans};
pub use status::{StatusBarContext, padded_line, status_bar_line};
#[cfg(test)]
pub(crate) use text::{file_status_tag, stripped_file_path};
pub use text::{truncate_pad, truncate_status_path, visible_width, wrap_text_by_visual_width};

#[cfg(test)]
mod tests;
