use std::collections::HashSet;

use super::AppState;

/// Which neighbour row a merged-divider expansion should lock in place.
///
/// `BelowRow`: the row just below the divider (▲▲ fills from the top side,
/// insertions land above the anchor). `AboveRow`: the row just above the
/// divider (▼▼ fills from the bottom side, insertions land below the anchor,
/// so the upper content never moves).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DividerFallback {
    BelowRow,
    AboveRow,
    Coord(CursorAnchor),
}

/// Everything needed to restore the viewport after a hunk-divider expansion:
/// the cursor anchor, the cursor's screen position, the divider's screen
/// position, and both neighbour rows (as coordinates plus their pre-click
/// text screen positions) for merge fallbacks.
pub(crate) struct DividerAnchors {
    pub(crate) is_bottom: bool,
    pub(crate) cursor_screen_y: isize,
    pub(crate) div_screen_y: isize,
    pub(crate) below: Option<CursorAnchor>,
    pub(crate) below_text_screen_y: Option<isize>,
    pub(crate) above: Option<CursorAnchor>,
    pub(crate) above_text_screen_y: Option<isize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CursorAnchor {
    pub(crate) file_idx: usize,
    pub(crate) new_line: Option<u32>,
    pub(crate) old_line: Option<u32>,
}

impl AppState {
    pub(crate) fn capture_divider_anchor(&self, fi: usize, hi: usize) -> DividerAnchors {
        let cursor_screen_y: isize = (self.cursor_visual_line() as isize) - (self.scroll as isize);
        let cw = self.effective_content_width();
        let callouts = self.note_callouts(cw);
        let (starts, counts, seg_counts, _, _) = self.compute_geometry(cw, &callouts);

        // Flat indices that carry leading hunk-divider segments.
        let div_flats: HashSet<usize> = self.hunk_starts.iter().map(|hs| hs.flat).collect();
        // Visual offset of a flat entry's text from its block start (leading
        // divider segments, if any).
        let text_off = |pos: usize| -> usize {
            if div_flats.contains(&pos) {
                counts[pos].saturating_sub(seg_counts[pos])
            } else {
                0
            }
        };
        let coord_of = |pos: usize| -> Option<CursorAnchor> {
            let fr = self.flat.get(pos)?;
            if !fr.is_diff_row() {
                return None;
            }
            let r = self.row_of(fr)?;
            Some(CursorAnchor {
                file_idx: fr.file_idx,
                new_line: r.new_line,
                old_line: r.old_line,
            })
        };

        let is_bottom = self
            .output
            .files
            .get(fi)
            .is_some_and(|f| hi >= f.hunks.len());

        let (div_vis, below, below_text_screen_y, above, above_text_screen_y) = if is_bottom {
            let div_vis = self
                .bottom_divider_vis_line_from_layout(fi, &starts)
                .unwrap_or_else(|| self.cursor_visual_line());
            let bot_row = self
                .flat
                .iter()
                .rposition(|r| r.file_idx == fi && r.is_bottom_gap_divider());
            let last_diff = bot_row
                .and_then(|r| r.checked_sub(1))
                .unwrap_or_else(|| self.last_row_of_file(fi));
            let above = coord_of(last_diff);
            let above_text_screen_y = starts
                .get(last_diff)
                .map(|s| s + text_off(last_diff))
                .map(|vis| (vis as isize) - (self.scroll as isize));
            (div_vis, None, None, above, above_text_screen_y)
        } else {
            let div_start = self
                .hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == hi)
                .map(|hs| hs.flat);
            let div_vis = div_start
                .map(|flat| starts[flat])
                .unwrap_or_else(|| self.cursor_visual_line());
            let below = div_start.and_then(coord_of);
            let below_text_screen_y = div_start
                .map(|flat| starts[flat] + text_off(flat))
                .map(|vis| (vis as isize) - (self.scroll as isize));
            let above = div_start
                .filter(|flat| *flat > 0)
                .and_then(|flat| coord_of(flat - 1));
            let above_text_screen_y = div_start
                .filter(|flat| *flat > 0)
                .map(|flat| starts[flat - 1] + text_off(flat - 1))
                .map(|vis| (vis as isize) - (self.scroll as isize));
            (
                div_vis,
                below,
                below_text_screen_y,
                above,
                above_text_screen_y,
            )
        };
        let div_screen_y = (div_vis as isize) - (self.scroll as isize);

        DividerAnchors {
            is_bottom,
            cursor_screen_y,
            div_screen_y,
            below,
            below_text_screen_y,
            above,
            above_text_screen_y,
        }
    }

    pub(crate) fn restore_divider_pivot(
        &mut self,
        anchors: DividerAnchors,
        fi: usize,
        hi: usize,
        fallback: DividerFallback,
    ) {
        let (flat, hunk_starts) = if self.raw_diff {
            Self::build_raw_flat(&self.output, &self.collapsed_files)
        } else {
            Self::build_visual_flat(&self.output, &self.collapsed_files)
        };
        self.flat = flat;
        self.bump_content();
        self.hunk_starts = hunk_starts;

        let cw = self.effective_content_width();
        let callouts = self.note_callouts(cw);
        let (starts, counts, seg_counts, _nav_counts, total_vis_lines) =
            self.compute_geometry(cw, &callouts);

        let is_bottom = anchors.is_bottom;
        let divider_gone = if is_bottom {
            self.hunk_trailing_gap(fi) == 0
        } else {
            !self
                .hunk_starts
                .iter()
                .any(|hs| hs.file == fi && hs.hunk == hi)
        };

        let (fallback_anchor, fallback_screen_y) = match fallback {
            DividerFallback::BelowRow => (
                anchors.below.or(anchors.above),
                anchors.below_text_screen_y.or(anchors.above_text_screen_y),
            ),
            DividerFallback::AboveRow => (anchors.above, anchors.above_text_screen_y),
            DividerFallback::Coord(coord) => (Some(coord), Some(anchors.div_screen_y)),
        };
        let fallback_pos = fallback_anchor
            .and_then(|a| self.flat_pos_of_coord(a.file_idx, a.new_line, a.old_line));

        // Pivot screen view around the divider line — or, when the merge
        // removed it, around the locked neighbour row.
        let div_flats: HashSet<usize> = self.hunk_starts.iter().map(|hs| hs.flat).collect();
        let text_vis = |pos: usize| -> usize {
            let off = if div_flats.contains(&pos) {
                counts[pos].saturating_sub(seg_counts[pos])
            } else {
                0
            };
            starts[pos] + off
        };
        let div_vis = if is_bottom {
            self.bottom_divider_vis_line_from_layout(fi, &starts)
        } else {
            self.hunk_starts
                .iter()
                .find(|hs| hs.file == fi && hs.hunk == hi)
                .map(|hs| starts[hs.flat])
        };
        let div_vis = div_vis
            .or_else(|| {
                if divider_gone {
                    fallback_pos.map(text_vis)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| self.cursor_visual_line());
        let pivot_screen_y = if divider_gone {
            fallback_screen_y.unwrap_or(anchors.div_screen_y)
        } else {
            anchors.div_screen_y
        };

        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        let new_scroll = ((div_vis as isize) - pivot_screen_y).max(0) as usize;
        let new_scroll = new_scroll.min(max_scroll);
        self.scroll = new_scroll;

        self.retain_cursor_visual_position(anchors.cursor_screen_y);
    }

    /// Retain the cursor's visual position relative to the viewport top
    /// (`self.scroll`), shifting whatever code content is now at that
    /// position underneath the selection. This holds regardless of whether
    /// the position is on screen, above the screen, or below the screen.
    pub(crate) fn retain_cursor_visual_position(&mut self, cursor_screen_y: isize) {
        let cw = self.effective_content_width();
        let callouts = self.note_callouts(cw);
        let (starts, counts, _, nav_counts, total_vis_lines) = self.compute_geometry(cw, &callouts);

        let max_rows = self.viewport_height.max(1);
        let max_scroll = total_vis_lines.saturating_sub(max_rows);
        self.scroll = self.scroll.min(max_scroll);

        if self.flat.is_empty() {
            self.row_idx = 0;
            self.vis_line = 0;
            return;
        }

        let target_vis_signed = (self.scroll as isize) + cursor_screen_y;
        let target_vis =
            target_vis_signed.clamp(0, total_vis_lines.saturating_sub(1) as isize) as usize;

        let mut found_row = self.flat.len().saturating_sub(1);
        let mut found_vis_line = 0;
        for (i, &s) in starts.iter().enumerate() {
            if target_vis >= s && target_vis < s + counts[i] {
                found_row = i;
                found_vis_line = (target_vis - s).min(nav_counts[i].saturating_sub(1));
                break;
            }
        }
        if target_vis >= total_vis_lines && !self.flat.is_empty() {
            found_row = self.flat.len() - 1;
            found_vis_line = nav_counts[found_row].saturating_sub(1);
        } else if target_vis < starts.first().copied().unwrap_or(0) {
            found_row = 0;
            found_vis_line = 0;
        }
        self.row_idx = found_row;
        self.vis_line = found_vis_line;
        self.scroll_pin = Some((self.scroll, self.row_idx));
    }
}
