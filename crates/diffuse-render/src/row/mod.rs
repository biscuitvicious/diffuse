//! Row & Segment Renderer with subword styling, background tinting, and column
//! slicing.

pub mod gutter;
pub mod note;
pub mod raw;
pub mod segment;
pub mod text;

#[cfg(test)]
mod tests;

pub use gutter::{
    DEFAULT_PAGER_LINE_NUM_WIDTH, MAX_LINE_NUM_BUF_LEN, MAX_LINE_NUM_WIDTH,
    continuation_prefix_plain_with_mod_side, continuation_prefix_plain_with_mod_side_and_width,
    continuation_prefix_spans, continuation_prefix_spans_with_mod_side,
    continuation_prefix_spans_with_mod_side_and_width, continuation_prefix_spans_with_width,
    format_line_num, prefix_of, prefix_of_with_mod_side, prefix_of_with_mod_side_and_width,
    styled_prefix_spans, styled_prefix_spans_with_width, write_gutter_prefix,
};
pub use note::{note_segment_spans, render_note_segment, write_note_segment};
pub use raw::{raw_row_spans, render_raw_row, write_raw_row};
pub use segment::{
    RowSegmentLine, is_common_only_row, is_pure_deletion_mod, render_row_segment,
    row_segment_spans, write_row_segment,
};
pub use text::{display_text, expand_runs_tabs, expand_tabs, slice_by_visual_width};
