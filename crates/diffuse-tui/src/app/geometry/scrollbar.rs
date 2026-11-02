//! Scrollbar track geometry, thumb sizing, subcell eighth-block decomposition,
//! and pointer mapping.

use crate::app::AppState;

impl AppState {
    /// Minimum scrollbar thumb height as a fraction of track height.
    pub const SCROLLBAR_MIN_PERCENT: f64 = 0.12;
    /// Absolute floor for scrollbar thumb height in visual lines.
    pub const SCROLLBAR_MIN_THUMB_LINES: usize = 2;

    /// Scrollbar thumb height for a track of height `track_h` over
    /// `total_vis_lines` visual lines with a `vh`-high viewport, or `None`
    /// when no scrollbar applies. Clamped to at least
    /// [`Self::SCROLLBAR_MIN_PERCENT`] of the track height and at least
    /// [`Self::SCROLLBAR_MIN_THUMB_LINES`].
    pub(crate) fn scrollbar_thumb_size(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
    ) -> Option<usize> {
        if total_vis_lines <= vh {
            return None;
        }
        let max_thumb = track_h.saturating_sub(1);
        if max_thumb < Self::SCROLLBAR_MIN_THUMB_LINES {
            return None;
        }
        let min_thumb = ((track_h as f64 * Self::SCROLLBAR_MIN_PERCENT).round() as usize)
            .clamp(Self::SCROLLBAR_MIN_THUMB_LINES, max_thumb);
        let prop_thumb = ((vh as f64 / total_vis_lines as f64) * track_h as f64).round() as usize;
        Some(prop_thumb.clamp(min_thumb, max_thumb))
    }

    /// Scroll delta per touch / wheel step on the scrollbar track.
    ///
    /// Computes the kinematic slope of the thumb travel range:
    /// each event moves by `max_scroll / span`, so traversing the track
    /// height in finger gestures navigates through the entire file at 1:1
    /// physical speed. Clamped to at least `min_step`.
    pub(crate) fn scrollbar_step(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        min_step: usize,
    ) -> usize {
        let max_scroll = total_vis_lines.saturating_sub(vh);
        if max_scroll == 0 || track_h <= 1 {
            return min_step;
        }
        let span =
            if let Some(thumb_size) = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines) {
                track_h.saturating_sub(thumb_size).max(1)
            } else {
                track_h.max(1)
            };
        ((max_scroll + span / 2) / span).max(min_step)
    }

    /// Thumb `(offset, size)` for a scrollbar track of height `track_h`
    /// over `total_vis_lines` visual lines with a `vh`-high viewport at
    /// the given scroll position, or `None` when no scrollbar applies.
    /// `header_vis` is the first file header's visual height: the thumb
    /// leaves the top row once the header scrolls out. Single source of
    /// truth shared by the renderer and mouse hit-testing so the drawn
    /// thumb and the grab area can never disagree.
    pub(crate) fn scrollbar_thumb_for(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        scroll: usize,
        header_vis: usize,
    ) -> Option<(usize, usize)> {
        let thumb_size = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines)?;
        let max_scroll = total_vis_lines.saturating_sub(vh);
        // The thumb touches the first track row only while the first
        // file's header is in view, and the last track row only while the
        // final line is in view; interior scroll positions stay strictly
        // inside the span.
        let span = track_h.saturating_sub(thumb_size);
        let header_vis = header_vis.max(1);
        let thumb_offset = if max_scroll == 0 || scroll >= max_scroll {
            span
        } else if scroll < header_vis {
            0
        } else if span >= 2 {
            let interior_min = header_vis;
            let interior_max = max_scroll.saturating_sub(1);
            if interior_max > interior_min {
                let t = (scroll.saturating_sub(interior_min) as f64)
                    / ((interior_max - interior_min) as f64);
                let num_buckets = span - 1;
                let bucket = ((t * num_buckets as f64).floor() as usize).min(span - 2);
                1 + bucket
            } else {
                1
            }
        } else {
            0
        }
        .min(span);
        Some((thumb_offset, thumb_size))
    }

    /// Continuous eighth-block subcell thumb geometry over a scrollbar track
    /// of height `track_h` for `scroll` within `total_vis_lines`.
    pub(crate) fn scrollbar_subcell_for(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        scroll: usize,
        _header_vis: usize,
    ) -> Option<ScrollbarSubcellThumb> {
        let thumb_size = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines)?;
        let max_scroll = total_vis_lines.saturating_sub(vh);
        let span = track_h.saturating_sub(thumb_size);
        let span_eighths = span * 8;
        let size_eighths = thumb_size * 8;

        let start_eighths = if max_scroll == 0 || span_eighths == 0 {
            0
        } else if scroll >= max_scroll {
            span_eighths
        } else if scroll == 0 {
            0
        } else if span_eighths >= 2 && max_scroll > 1 {
            let t = (scroll - 1) as f64 / (max_scroll - 1) as f64;
            let num_interior = span_eighths - 2;
            let step = (t * num_interior as f64).round() as usize;
            1 + step
        } else {
            1
        }
        .min(span_eighths);

        Some(ScrollbarSubcellThumb {
            start_eighths,
            size_eighths,
            track_h,
        })
    }

    /// Scroll position for a target thumb offset in `0..=span`.
    pub(crate) fn scrollbar_scroll_for_offset(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        header_vis: usize,
        target_offset: usize,
    ) -> usize {
        let max_scroll = total_vis_lines.saturating_sub(vh);
        if max_scroll == 0 || track_h <= 1 {
            return 0;
        }
        let Some(thumb_size) = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines) else {
            return 0;
        };
        let span = track_h.saturating_sub(thumb_size);
        if span == 0 || target_offset == 0 {
            return 0;
        }
        if target_offset >= span {
            return max_scroll;
        }
        let header_vis = header_vis.max(1);
        let interior_min = header_vis;
        let interior_max = max_scroll.saturating_sub(1);
        if interior_max > interior_min {
            let bucket = target_offset - 1;
            let num_buckets = span - 1;
            let t = (bucket as f64 + 0.5) / num_buckets as f64;
            interior_min + (t * (interior_max - interior_min) as f64).round() as usize
        } else {
            interior_min
        }
    }

    /// Scroll position for a scrollbar pointer at track row `row`: exact
    /// invertible mapping used identically by click-seek and drag.
    ///
    /// Respects the physical travel range `span = track_h - thumb_size`,
    /// ensuring that pointer row 0 yields scroll 0 (top of thumb at row 0),
    /// pointer row 1 yields thumb offset 1, the penultimate row yields
    /// `span - 1` (bottom of thumb at `track_h - 2`), and the last row
    /// yields `max_scroll` (bottom of thumb at `track_h - 1`). The thumb
    /// follows smoothly through [`Self::scrollbar_thumb_for`] with zero
    /// edge snapping.
    pub(crate) fn scrollbar_scroll_for_row(
        track_h: usize,
        vh: usize,
        total_vis_lines: usize,
        header_vis: usize,
        row: usize,
    ) -> usize {
        let max_scroll = total_vis_lines.saturating_sub(vh);
        if max_scroll == 0 || track_h <= 1 {
            return 0;
        }
        if max_scroll == 1 {
            return if row >= track_h / 2 { 1 } else { 0 };
        }
        let Some(thumb_size) = Self::scrollbar_thumb_size(track_h, vh, total_vis_lines) else {
            return 0;
        };
        let span = track_h.saturating_sub(thumb_size);
        if span == 0 {
            return 0;
        }
        let r = row.min(track_h - 1);
        let target_offset = ((r as f64 / (track_h - 1) as f64) * span as f64).round() as usize;
        Self::scrollbar_scroll_for_offset(track_h, vh, total_vis_lines, header_vis, target_offset)
    }
}

/// Standard Unicode lower block character for fraction `eighths` in 1..=8.
#[inline]
pub(crate) fn lower_block_glyph(eighths: u8) -> char {
    match eighths {
        1 => ' ', // U+2581 LOWER ONE EIGHTH BLOCK
        2 => '▂', // U+2582 LOWER ONE QUARTER BLOCK
        3 => '▃', // U+2583 LOWER THREE EIGHTHS BLOCK
        4 => '▄', // U+2584 LOWER HALF BLOCK
        5 => '▅', // U+2585 LOWER FIVE EIGHTHS BLOCK
        6 => '▆', // U+2586 LOWER THREE QUARTERS BLOCK
        7 => '▇', // U+2587 LOWER SEVEN EIGHTHS BLOCK
        8 => '█', // U+2588 FULL BLOCK
        _ => ' ',
    }
}

/// Visual classification and fractional decomposition of a single scrollbar
/// track cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollbarCell {
    /// Empty track cell (no thumb present).
    Track,
    /// Solid full thumb cell (all 8 eighths occupied).
    Full,
    /// Top edge of thumb entering from the bottom of this cell.
    /// `eighths` (1..=7) is the fraction filled by thumb at the cell bottom.
    TopEdge { eighths: u8 },
    /// Bottom edge of thumb exiting at the top of this cell.
    /// `eighths` (1..=7) is the fraction filled by thumb at the cell top.
    BottomEdge { eighths: u8 },
}

impl ScrollbarCell {
    /// Return the primary Unicode glyph representing this cell.
    #[must_use]
    pub(crate) fn glyph(self) -> char {
        match self {
            Self::Track => '│',
            Self::Full => '█',
            Self::TopEdge { eighths } => lower_block_glyph(eighths),
            Self::BottomEdge { eighths } => {
                if eighths == 4 {
                    '▀' // U+2580 UPPER HALF BLOCK
                } else {
                    lower_block_glyph(8 - eighths)
                }
            }
        }
    }

    /// Render this scrollbar cell into an ANSI SGR escape sequence string.
    #[cfg(test)]
    pub(crate) fn render_ansi(self, palette: &crate::render::ThemePalette) -> String {
        use crate::render::ColorExt;
        let thumb_fg = palette.scrollbar_thumb.fg_code();
        let thumb_bg = palette.scrollbar_thumb.bg_code();
        let track_fg = palette.scrollbar_track.fg_code();
        let track_bg = palette.scrollbar_track.bg_code();
        let ch = self.glyph();

        match self {
            Self::Track => format!("{track_fg}{track_bg}{ch}\x1b[0m"),
            Self::Full => format!("{thumb_fg}{thumb_bg}{ch}\x1b[0m"),
            Self::TopEdge { .. } => format!("{thumb_fg}{track_bg}{ch}\x1b[0m"),
            Self::BottomEdge { eighths } => {
                if eighths == 4 {
                    format!("{thumb_fg}{track_bg}{ch}\x1b[0m")
                } else {
                    format!("{track_fg}{thumb_bg}{ch}\x1b[0m")
                }
            }
        }
    }

    /// Convert this cell into a styled Ratatui `Line`.
    pub(crate) fn to_ratatui_line(
        self,
        thumb_color: ratatui::style::Color,
        track_color: ratatui::style::Color,
    ) -> ratatui::text::Line<'static> {
        use ratatui::{
            style::Style,
            text::{Line, Span},
        };
        let ch = self.glyph().to_string();
        match self {
            Self::Track => Line::from(Span::styled(
                ch,
                Style::default().fg(track_color).bg(track_color),
            )),
            Self::Full => Line::from(Span::styled(
                ch,
                Style::default().fg(thumb_color).bg(thumb_color),
            )),
            Self::TopEdge { .. } => Line::from(Span::styled(
                ch,
                Style::default().fg(thumb_color).bg(track_color),
            )),
            Self::BottomEdge { eighths } => {
                if eighths == 4 {
                    Line::from(Span::styled(
                        ch,
                        Style::default().fg(thumb_color).bg(track_color),
                    ))
                } else {
                    Line::from(Span::styled(
                        ch,
                        Style::default().fg(track_color).bg(thumb_color),
                    ))
                }
            }
        }
    }
}

/// Continuous fractional geometry of the scrollbar thumb in eighths of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarSubcellThumb {
    /// Start position in eighths of a row (0 ..= span * 8).
    pub start_eighths: usize,
    /// Size of the thumb in eighths of a row (thumb_size * 8).
    pub size_eighths: usize,
    /// Total track height in terminal rows.
    pub track_h: usize,
}

impl ScrollbarSubcellThumb {
    /// Classify the scrollbar state for track row `r` (0 .. track_h).
    #[must_use]
    pub(crate) fn cell_at(&self, r: usize) -> ScrollbarCell {
        let c_start = r * 8;
        let c_end = (r + 1) * 8;
        let thumb_end = self.start_eighths + self.size_eighths;

        let ov_start = c_start.max(self.start_eighths);
        let ov_end = c_end.min(thumb_end);

        if ov_start >= ov_end {
            ScrollbarCell::Track
        } else if ov_start == c_start && ov_end == c_end {
            ScrollbarCell::Full
        } else if self.start_eighths > c_start {
            // Thumb begins inside this cell: thumb is in bottom fraction
            let frac = (c_end - self.start_eighths) as u8;
            ScrollbarCell::TopEdge { eighths: frac }
        } else {
            // Thumb ends inside this cell: thumb is in top fraction
            let frac = (thumb_end - c_start) as u8;
            ScrollbarCell::BottomEdge { eighths: frac }
        }
    }
}
