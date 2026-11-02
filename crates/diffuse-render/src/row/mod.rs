//! Row & Segment Renderer with subword styling, background tinting, and column
//! slicing.

pub mod gutter;
pub mod note;
pub mod raw;
pub mod segment;
pub mod text;

#[cfg(test)]
mod tests;

pub use gutter::{pad4, pad4_bytes, prefix_of, styled_prefix};
pub use note::{render_note_segment, write_note_segment};
pub use raw::{render_raw_row, write_raw_row};
pub use segment::{
    is_common_only_row, is_pure_deletion_mod, render_row_segment, write_row_segment,
};
pub use text::{display_text, expand_runs_tabs, expand_tabs, slice_by_visual_width};
