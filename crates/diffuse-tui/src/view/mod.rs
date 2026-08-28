//! TUI View Subsystem — frame rendering, header bars, diff grid, and modal
//! overlays.

pub mod diff_grid;
pub mod header;
pub mod modals;

pub use diff_grid::{
    bottom_gap_divider_line, bottom_gap_divider_text, file_header_line, file_header_text,
    hunk_divider_line, hunk_divider_text, render_bottom_gap_divider,
    render_bottom_gap_divider_segment, render_file_header, render_file_header_segment,
    render_hunk_divider, render_hunk_divider_segment,
};
pub use header::{
    StatusBarContext, bottom_ribbon_lines, header_bar_line, render_bottom_ribbon,
    render_header_bar, render_status_bar, status_bar_line, truncate_pad, visible_width,
    wrap_text_by_visual_width,
};
pub use modals::{
    ansi_to_ratatui_line, render_finder_modal, render_finder_modal_widget, render_help_modal,
    render_help_modal_widget, render_note_modal_widget,
};
