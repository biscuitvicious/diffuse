//! TUI View Subsystem — frame rendering, header bars, diff grid, and modal
//! overlays.

pub mod diff_grid;
pub mod header;
pub mod modals;

pub use diff_grid::{
    bottom_gap_divider_text, file_header_text, hunk_divider_text, render_bottom_gap_divider,
    render_bottom_gap_divider_segment, render_file_header, render_file_header_segment,
    render_hunk_divider, render_hunk_divider_segment,
};
pub use header::{
    StatusBarContext, render_bottom_ribbon, render_header_bar, render_status_bar, truncate_pad,
    visible_width, wrap_text_by_visual_width,
};
pub use modals::{render_finder_modal, render_help_modal};
