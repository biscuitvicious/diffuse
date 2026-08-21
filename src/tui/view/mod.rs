//! TUI View Subsystem — frame rendering, header bars, diff grid, and modal
//! overlays.

pub mod diff_grid;
pub mod header;
pub mod modals;

pub use diff_grid::{render_file_header, render_hunk_divider};
pub use header::{
    StatusBarContext, render_bottom_ribbon, render_header_bar, render_status_bar, truncate_pad,
    visible_width,
};
pub use modals::{render_finder_modal, render_help_modal};
