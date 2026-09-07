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
    StatusBarContext, bottom_ribbon_lines, header_bar_line, header_bar_line_themed,
    render_bottom_ribbon, render_header_bar, render_status_bar, status_bar_line, truncate_pad,
    visible_width, wrap_text_by_visual_width,
};
pub use modals::{
    ansi_to_ratatui_line, render_finder_modal, render_finder_modal_widget, render_help_modal,
    render_help_modal_widget, render_note_modal_widget,
};

use ratatui::style::Color;

/// Convert a [`yansi::Color`] (or [`crate::render::AnsiColor`]) to a Ratatui
/// [`Color`].
#[inline]
#[must_use]
pub fn to_color(c: impl Into<yansi::Color>) -> Color {
    match c.into() {
        yansi::Color::Primary => Color::Reset,
        yansi::Color::Fixed(i) => Color::Indexed(i),
        yansi::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
        yansi::Color::Black => Color::Indexed(0),
        yansi::Color::Red => Color::Indexed(1),
        yansi::Color::Green => Color::Indexed(2),
        yansi::Color::Yellow => Color::Indexed(3),
        yansi::Color::Blue => Color::Indexed(4),
        yansi::Color::Magenta => Color::Indexed(5),
        yansi::Color::Cyan => Color::Indexed(6),
        yansi::Color::White => Color::Indexed(7),
        yansi::Color::BrightBlack => Color::Indexed(8),
        yansi::Color::BrightRed => Color::Indexed(9),
        yansi::Color::BrightGreen => Color::Indexed(10),
        yansi::Color::BrightYellow => Color::Indexed(11),
        yansi::Color::BrightBlue => Color::Indexed(12),
        yansi::Color::BrightMagenta => Color::Indexed(13),
        yansi::Color::BrightCyan => Color::Indexed(14),
        yansi::Color::BrightWhite => Color::Indexed(15),
    }
}

/// Convert a [`yansi::Style`] to a Ratatui [`Style`].
#[inline]
#[must_use]
pub fn to_style(s: yansi::Style) -> ratatui::style::Style {
    let mut style = ratatui::style::Style::default();
    if let Some(fg) = s.foreground {
        style = style.fg(to_color(fg));
    }
    if let Some(bg) = s.background {
        style = style.bg(to_color(bg));
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_color_mapping() {
        assert_eq!(to_color(yansi::Color::Primary), Color::Reset);
        assert_eq!(to_color(yansi::Color::Fixed(120)), Color::Indexed(120));
        assert_eq!(
            to_color(yansi::Color::Rgb(10, 20, 30)),
            Color::Rgb(10, 20, 30)
        );
        assert_eq!(to_color(yansi::Color::Red), Color::Indexed(1));
        assert_eq!(to_color(yansi::Color::BrightWhite), Color::Indexed(15));
    }

    #[test]
    fn test_to_style_mapping() {
        let ys = yansi::Style::new()
            .fg(yansi::Color::Fixed(120))
            .bg(yansi::Color::Rgb(1, 2, 3));
        let rs = to_style(ys);
        assert_eq!(rs.fg, Some(Color::Indexed(120)));
        assert_eq!(rs.bg, Some(Color::Rgb(1, 2, 3)));
    }
}
