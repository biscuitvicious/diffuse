//! Modal viewport geometry calculations.

use ratatui::layout::Rect;

/// Compute a centered rectangle within an area.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// Compute the centered area for standard dialog modals (file finder, key
/// bindings).
///
/// On large screens, the dialog occupies 50% of width and 60% of height,
/// centered horizontally and vertically. On small screens, it expands up to the
/// full screen dimensions.
#[must_use]
pub fn standard_modal_area(area: Rect) -> Rect {
    let modal_w = ((area.width as u32 * 50) / 100)
        .max(50)
        .min(area.width as u32) as u16;
    let modal_h = ((area.height as u32 * 60) / 100)
        .max(16)
        .min(area.height as u32) as u16;
    centered_rect(modal_w, modal_h, area)
}

/// Compute the centered area for the file finder modal.
#[must_use]
pub fn finder_modal_area(area: Rect) -> Rect {
    standard_modal_area(area)
}

/// Compute the centered area for the key-bindings help modal.
#[must_use]
pub fn help_modal_area(area: Rect) -> Rect {
    standard_modal_area(area)
}
